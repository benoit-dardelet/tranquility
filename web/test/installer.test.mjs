// node --test web/test/*.test.mjs   (cross-checks need mkbootimg, unpack_bootimg, dtc on PATH)
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { gunzipSync } from "node:zlib";
import { buildBootImg, emptyCpio, parseBootImg } from "../lib/bootimg.js";
import { DtNode, buildDtb } from "../lib/dtb.js";
import { Fastboot } from "../lib/fastboot.js";
import { DEVICES, NOT_YET, QEMU_PHONE } from "../lib/devices.js";
import { bootImage, deviceTree, mergeRegions } from "../lib/build.js";

const here = dirname(fileURLToPath(import.meta.url));
const tmp = mkdtempSync(join(tmpdir(), "tq-web-"));
const have = (bin) => {
  try {
    execFileSync("which", [bin]);
    return true;
  } catch {
    return false;
  }
};
const rand = (n, seed) => {
  const b = new Uint8Array(n);
  let x = seed;
  for (let i = 0; i < n; i++) b[i] = (x = (x * 1103515245 + 12345) >>> 0) >>> 24;
  return b;
};

for (const v of [3, 4]) {
  test(`boot image v${v} is byte-identical to AOSP mkbootimg`, { skip: !have("mkbootimg") }, async () => {
    const kernel = rand(222223, 6), ramdisk = rand(5001, 7);
    writeFileSync(join(tmp, "k3"), kernel);
    writeFileSync(join(tmp, "r3"), ramdisk);
    mkdirSync(join(tmp, "stub/gki"), { recursive: true });
    writeFileSync(join(tmp, "stub/gki/__init__.py"), "");
    writeFileSync(join(tmp, "stub/gki/generate_gki_certificate.py"), "def generate_gki_certificate(*a, **k):\n    raise SystemExit('unused')\n");
    execFileSync("mkbootimg", ["--kernel", join(tmp, "k3"), "--ramdisk", join(tmp, "r3"), "--cmdline", "tq.fb=0xfac00000,1080,2400",
      "--header_version", String(v), "-o", join(tmp, `ref${v}.img`)], { env: { ...process.env, PYTHONPATH: join(tmp, "stub") } });
    const ref = new Uint8Array(readFileSync(join(tmp, `ref${v}.img`)));
    const ours = await buildBootImg({ kernel, ramdisk, headerVersion: v, cmdline: "tq.fb=0xfac00000,1080,2400" });
    assert.equal(ours.length, ref.length);
    assert.deepEqual(ours, ref);
  });
}

for (const v of [0, 1, 2]) {
  test(`boot image v${v} is byte-identical to AOSP mkbootimg`, { skip: !have("mkbootimg") }, async () => {
    const kernel = rand(123457, 1), ramdisk = rand(4099, 2), dtb = rand(777, 3);
    for (const [n, d] of [["k", kernel], ["r", ramdisk], ["d", dtb]]) writeFileSync(join(tmp, n), d);
    const args = ["--kernel", join(tmp, "k"), "--ramdisk", join(tmp, "r"), "--pagesize", "4096", "--base", "0x0",
      "--kernel_offset", "0x8000", "--ramdisk_offset", "0x1000000", "--second_offset", "0xf00000", "--tags_offset", "0x100",
      "--cmdline", "console=null " + "x".repeat(600), "--header_version", String(v), "-o", join(tmp, `ref${v}.img`)];
    if (v === 2) args.push("--dtb", join(tmp, "d"), "--dtb_offset", "0x1f00000");
    // Debian's mkbootimg imports a `gki` module (only used for v4 signing)
    // that the package doesn't ship: give it a stub.
    mkdirSync(join(tmp, "stub/gki"), { recursive: true });
    writeFileSync(join(tmp, "stub/gki/__init__.py"), "");
    writeFileSync(join(tmp, "stub/gki/generate_gki_certificate.py"), "def generate_gki_certificate(*a, **k):\n    raise SystemExit('unused')\n");
    execFileSync("mkbootimg", args, { env: { ...process.env, PYTHONPATH: join(tmp, "stub") } });
    const ref = new Uint8Array(readFileSync(join(tmp, `ref${v}.img`)));
    const ours = await buildBootImg({ kernel, ramdisk, dtb: v === 2 ? dtb : undefined, headerVersion: v, pagesize: 4096, base: 0,
      kernelOffset: 0x8000, ramdiskOffset: 0x1000000, secondOffset: 0xf00000, tagsOffset: 0x100, dtbOffset: 0x1f00000,
      cmdline: "console=null " + "x".repeat(600) });
    assert.equal(ours.length, ref.length);
    assert.deepEqual(ours, ref);
  });
}

test("DTB writer output is accepted by dtc", { skip: !have("dtc") }, () => {
  const root = new DtNode("");
  root.prop("#address-cells", [2]).prop("#size-cells", [2]).prop("compatible", ["a,b", "c,d"]).prop("model", "X");
  root.child("chosen").prop("bootargs", "hello").prop("empty", true);
  writeFileSync(join(tmp, "t.dtb"), buildDtb(root, { memReserve: [[0x80000000, 0x1000]] }));
  const dts = execFileSync("dtc", ["-I", "dtb", "-O", "dts", join(tmp, "t.dtb")], { stdio: ["ignore", "pipe", "ignore"] }).toString();
  assert.match(dts, /\/memreserve\/\s+0x0*80000000 0x0*1000;/);
  assert.match(dts, /compatible = "a,b\\0c,d";/);
  assert.match(dts, /bootargs = "hello";/);
  assert.match(dts, /empty;/);
});

test("every device gets a valid DTB and boot image", async () => {
  const gz = existsSync(join(here, "../../build/Image.gz")) ? new Uint8Array(readFileSync(join(here, "../../build/Image.gz"))) : rand(5000, 9);
  for (const d of [...DEVICES, QEMU_PHONE]) {
    if (!d.dt) {
      // Tensor Pixels: v4 image, raw kernel, display address on the command line
      const img = await bootImage(d, gz);
      const h = parseBootImg(img);
      assert.equal(h.headerVersion, 4, d.id);
      assert.match(h.cmdline, /^tq\.fb=0x[0-9a-f]+,\d+,\d+$/, d.id);
      if (gz[0] === 0x1f) assert.equal(new TextDecoder().decode(img.subarray(4096 + 56, 4096 + 60)), "ARM\x64", "uncompressed Image");
      if (have("unpack_bootimg")) {
        writeFileSync(join(tmp, `${d.id}.img`), img);
        const out = execFileSync("unpack_bootimg", ["--boot_img", join(tmp, `${d.id}.img`), "--out", join(tmp, d.id)]).toString();
        assert.match(out, /boot image header version: 4/);
      }
      continue;
    }
    const dtb = deviceTree(d);
    if (have("dtc")) {
      writeFileSync(join(tmp, `${d.id}.dtb`), dtb);
      execFileSync("dtc", ["-I", "dtb", "-O", "dts", "-o", "/dev/null", join(tmp, `${d.id}.dtb`)], { stdio: "pipe" });
    }
    const img = await bootImage(d, gz);
    const h = parseBootImg(img);
    assert.equal(h.headerVersion, 0);
    assert.equal(h.pagesize, 4096);
    assert.equal(h.kernelAddr, 0x8000);
    assert.equal(h.kernelSize, gz.length + dtb.length, "DTB appended to Image.gz");
    // the appended kernel still gunzips, and the DTB follows the gzip stream
    const kernel = img.subarray(4096, 4096 + h.kernelSize);
    if (gz[0] === 0x1f) {
      const raw = gunzipSync(kernel.subarray(0, gz.length));
      assert.equal(new TextDecoder().decode(raw.subarray(56, 60)), "ARM\x64");
    }
    assert.deepEqual([...kernel.subarray(gz.length, gz.length + 4)], [0xd0, 0x0d, 0xfe, 0xed]);
    if (have("unpack_bootimg")) {
      writeFileSync(join(tmp, `${d.id}.img`), img);
      const out = execFileSync("unpack_bootimg", ["--boot_img", join(tmp, `${d.id}.img`), "--out", join(tmp, d.id)]).toString();
      assert.match(out, /boot image header version: 0/);
    }
  }
});

test("reserved regions are merged, framebuffer stays outside them", () => {
  assert.deepEqual(mergeRegions([[0x10, 0x10], [0x18, 0x10], [0x40, 4]]), [[0x10, 0x18], [0x40, 4]]);
  for (const d of DEVICES.filter((d) => d.dt)) {
    const rs = mergeRegions(d.dt.reserved);
    for (let i = 1; i < rs.length; i++) assert.ok(rs[i][0] >= rs[i - 1][0] + rs[i - 1][1], d.id);
    // our heap goes after the kernel at the start of DRAM: make sure there's room there
    const firstReserved = rs[0][0];
    assert.ok(firstReserved - 0x80000000 >= 64 * 1024 * 1024, d.id);
  }
});

test("empty initramfs is a valid newc archive", () => {
  const c = emptyCpio();
  assert.equal(new TextDecoder().decode(c.subarray(0, 6)), "070701");
  assert.equal(c.length % 512, 0);
  assert.match(new TextDecoder().decode(c), /TRAILER!!!/);
});

// ------------------------------------------------------------------ fastboot with a mock USB device

class MockPhone {
  constructor({ product = "sargo", slots = 2, unlocked = "yes" } = {}) {
    this.opened = false;
    this.configuration = null;
    this.vars = { product, unlocked, "slot-count": String(slots), "current-slot": "a", "max-download-size": "0x10000000", "has-slot:boot": slots > 1 ? "yes" : "no" };
    this.queue = [];
    this.expect = 0;
    this.buffer = [];
    this.flashed = {};
    this.booted = null;
    this.downloaded = null;
  }
  async open() {
    this.opened = true;
  }
  async selectConfiguration() {
    this.configuration = {
      interfaces: [{ interfaceNumber: 0, alternates: [{ interfaceClass: 0xff, interfaceSubclass: 0x42, interfaceProtocol: 3,
        endpoints: [{ type: "bulk", direction: "in", endpointNumber: 1 }, { type: "bulk", direction: "out", endpointNumber: 2 }] }] }],
    };
  }
  async claimInterface() {}
  async releaseInterface() {}
  async close() {}
  reply(s) {
    this.queue.push(new TextEncoder().encode(s));
  }
  async transferOut(ep, data) {
    assert.equal(ep, 2);
    if (this.expect > 0) {
      this.buffer.push(...data);
      this.expect -= data.length;
      if (this.expect === 0) {
        this.downloaded = new Uint8Array(this.buffer);
        this.buffer = [];
        this.reply("OKAY");
      }
      return { status: "ok" };
    }
    const cmd = new TextDecoder().decode(data);
    if (cmd.startsWith("getvar:")) {
      const v = this.vars[cmd.slice(7)];
      this.reply(v === undefined ? "FAILunknown variable" : "OKAY" + v);
    } else if (cmd.startsWith("download:")) {
      this.expect = parseInt(cmd.slice(9), 16);
      this.reply(`DATA${cmd.slice(9)}`);
    } else if (cmd === "boot") {
      if (this.vars.unlocked !== "yes") return this.reply("FAILlocked"), { status: "ok" };
      this.booted = this.downloaded;
      this.reply("OKAY");
    } else if (cmd.startsWith("flash:")) {
      this.reply("INFOwriting");
      this.flashed[cmd.slice(6)] = this.downloaded;
      this.reply("OKAY");
    } else if (cmd === "reboot") this.reply("OKAY");
    else this.reply("FAILunknown command");
    return { status: "ok" };
  }
  async transferIn(ep) {
    assert.equal(ep, 1);
    const d = this.queue.shift();
    assert.ok(d, "host read with nothing queued");
    return { data: new DataView(d.buffer) };
  }
}

test("fastboot: info, RAM boot and A/B-aware flash", async () => {
  const phone = new MockPhone();
  const fb = new Fastboot(phone);
  await fb.open();
  const info = await fb.info();
  assert.equal(info.product, "sargo");
  assert.equal(info["version-bootloader"], null);
  const img = rand(100_000, 4);
  let last = 0;
  await fb.boot(img, (p) => (last = p));
  assert.equal(last, 1);
  assert.deepEqual(phone.booted, img);
  await fb.flash("boot", img);
  assert.deepEqual(Object.keys(phone.flashed), ["boot_a"]);
  const legacy = new MockPhone({ slots: 1, product: "beryllium" });
  const fb2 = new Fastboot(legacy);
  await fb2.open();
  await fb2.flash("boot", img);
  assert.deepEqual(Object.keys(legacy.flashed), ["boot"]);
});

test("fastboot: a locked bootloader refuses to boot", async () => {
  const fb = new Fastboot(new MockPhone({ unlocked: "no" }));
  await fb.open();
  await assert.rejects(fb.boot(rand(1000, 5)), /FAIL: locked/);
});

test("only Google Pixels are offered", () => {
  for (const d of DEVICES) assert.match(d.name, /^Pixel /, d.id);
  assert.ok(NOT_YET.every(([n]) => n.startsWith("Pixel")));
  const ids = DEVICES.map((d) => d.id);
  assert.deepEqual(ids, ["google-blueline", "google-crosshatch", "google-sargo", "google-bonito", "google-oriole", "google-raven", "google-bluejay"]);
});
