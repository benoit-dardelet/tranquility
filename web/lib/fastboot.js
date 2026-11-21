// Fastboot protocol over WebUSB (Chromium: desktop, and Android via OTG).
// Protocol: https://android.googlesource.com/platform/system/core/+/main/fastboot/README.md

export const FASTBOOT_FILTER = { classCode: 0xff, subclassCode: 0x42, protocolCode: 0x03 };
const CHUNK = 16384;

export class FastbootError extends Error {
  constructor(status, message) {
    super(`${status}: ${message}`);
    this.status = status;
  }
}

const enc = new TextEncoder();
const dec = new TextDecoder();

export class Fastboot {
  /** @param {USBDevice} device */
  constructor(device) {
    this.device = device;
    this.iface = null;
    this.epIn = null;
    this.epOut = null;
    this.log = () => {};
  }

  static supported(nav = globalThis.navigator) {
    return !!(nav && nav.usb);
  }

  /** Ask the user to pick a phone in fastboot mode. */
  static async request(nav = globalThis.navigator) {
    const dev = await nav.usb.requestDevice({ filters: [FASTBOOT_FILTER] });
    const fb = new Fastboot(dev);
    await fb.open();
    return fb;
  }

  async open() {
    const d = this.device;
    if (!d.opened) await d.open();
    if (d.configuration === null) await d.selectConfiguration(1);
    for (const iface of d.configuration.interfaces) {
      for (const alt of iface.alternates) {
        if (alt.interfaceClass === 0xff && alt.interfaceSubclass === 0x42 && alt.interfaceProtocol === 0x03) {
          this.iface = iface.interfaceNumber;
          for (const ep of alt.endpoints) {
            if (ep.type !== "bulk") continue;
            if (ep.direction === "in") this.epIn = ep.endpointNumber;
            else this.epOut = ep.endpointNumber;
          }
        }
      }
    }
    if (this.iface === null || this.epIn === null || this.epOut === null) throw new Error("No fastboot interface on this device");
    await d.claimInterface(this.iface);
  }

  async close() {
    try {
      await this.device.releaseInterface(this.iface);
      await this.device.close();
    } catch (_) {
      /* already gone (e.g. after reboot) */
    }
  }

  async readResponse(onInfo) {
    for (;;) {
      const r = await this.device.transferIn(this.epIn, 256);
      const text = dec.decode(r.data);
      const status = text.slice(0, 4);
      const msg = text.slice(4);
      if (status === "INFO" || status === "TEXT") {
        (onInfo ?? this.log)(msg);
        continue;
      }
      if (status === "OKAY") return { status, msg };
      if (status === "DATA") return { status, msg, size: parseInt(msg, 16) };
      if (status === "FAIL") throw new FastbootError("FAIL", msg);
      throw new FastbootError("PROTO", `unexpected response "${text}"`);
    }
  }

  async command(cmd, onInfo) {
    if (cmd.length > 4096) throw new Error("command too long");
    this.log(`> ${cmd}`);
    await this.device.transferOut(this.epOut, enc.encode(cmd));
    const r = await this.readResponse(onInfo);
    this.log(`< ${r.status} ${r.msg}`);
    return r;
  }

  async getVar(name) {
    try {
      return (await this.command(`getvar:${name}`)).msg.trim();
    } catch (e) {
      if (e instanceof FastbootError && e.status === "FAIL") return null;
      throw e;
    }
  }

  async info() {
    const vars = ["product", "unlocked", "secure", "current-slot", "slot-count", "max-download-size", "version-bootloader", "serialno"];
    const out = {};
    for (const v of vars) out[v] = await this.getVar(v);
    return out;
  }

  /** Upload a blob into the device's download buffer. */
  async download(data, onProgress) {
    const mv = await this.getVar("max-download-size");
    const max = mv ? Number(mv) || parseInt(mv, 16) || 0 : 0; // "0x10000000" or "268435456"
    if (max && data.length > max) throw new Error(`image (${data.length} B) exceeds max-download-size (${max} B)`);
    const r = await this.command(`download:${data.length.toString(16).padStart(8, "0")}`);
    if (r.status !== "DATA" || r.size !== data.length) throw new FastbootError("PROTO", "device refused download");
    for (let off = 0; off < data.length; off += CHUNK) {
      await this.device.transferOut(this.epOut, data.subarray(off, Math.min(off + CHUNK, data.length)));
      onProgress?.(Math.min(off + CHUNK, data.length) / data.length);
    }
    return this.readResponse();
  }

  /** RAM-boot an image: nothing is written to the phone's storage. */
  async boot(img, onProgress) {
    await this.download(img, onProgress);
    return this.command("boot");
  }

  /** Flash a partition (resolving the A/B slot if needed). */
  async flash(partition, img, onProgress) {
    let part = partition;
    const slots = parseInt((await this.getVar("slot-count")) ?? "0", 10);
    const hasSlot = await this.getVar(`has-slot:${partition}`);
    if (slots > 1 && hasSlot === "yes") {
      const cur = await this.getVar("current-slot");
      if (cur) part = `${partition}_${cur.replace(/^_/, "")}`;
    }
    await this.download(img, onProgress);
    return this.command(`flash:${part}`);
  }

  reboot(target = "") {
    return this.command(target ? `reboot-${target}` : "reboot");
  }
}
