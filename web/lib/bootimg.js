// Android boot image writer (AOSP boot_img_hdr v0, v1, v2) — byte-compatible
// with AOSP `mkbootimg` for the same inputs.

const enc = new TextEncoder();

async function sha1(parts) {
  const total = parts.reduce((a, p) => a + p.length, 0);
  const buf = new Uint8Array(total);
  let o = 0;
  for (const p of parts) {
    buf.set(p, o);
    o += p.length;
  }
  return new Uint8Array(await crypto.subtle.digest("SHA-1", buf));
}

const le32 = (x) => {
  const b = new Uint8Array(4);
  new DataView(b.buffer).setUint32(0, x >>> 0, true);
  return b;
};

/**
 * @param {object} o
 * @param {Uint8Array} o.kernel
 * @param {Uint8Array} [o.ramdisk]
 * @param {Uint8Array} [o.second]
 * @param {Uint8Array} [o.dtb]          header v2 only
 * @param {number} [o.headerVersion=0]
 * @param {number} [o.pagesize=4096]
 * @param {number} [o.base=0x10000000]  mkbootimg defaults
 * @param {number} [o.kernelOffset=0x8000]
 * @param {number} [o.ramdiskOffset=0x01000000]
 * @param {number} [o.secondOffset=0x00f00000]
 * @param {number} [o.tagsOffset=0x100]
 * @param {number} [o.dtbOffset=0x01f00000]
 * @param {string} [o.cmdline=""]
 * @param {string} [o.board=""]
 */
export async function buildBootImg(o) {
  const v = o.headerVersion ?? 0;
  if (v > 2) throw new Error("header v3+ not supported");
  const page = o.pagesize ?? 4096;
  const base = o.base ?? 0x10000000;
  const kernel = o.kernel;
  const ramdisk = o.ramdisk ?? new Uint8Array(0);
  const second = o.second ?? new Uint8Array(0);
  const dtb = v >= 2 ? (o.dtb ?? new Uint8Array(0)) : new Uint8Array(0);
  const hdrSize = [1632, 1648, 1660][v];
  const hdr = new Uint8Array(hdrSize);
  const dv = new DataView(hdr.buffer);
  hdr.set(enc.encode("ANDROID!"), 0);
  const w32 = (off, x) => dv.setUint32(off, x >>> 0, true);
  w32(8, kernel.length);
  w32(12, (base + (o.kernelOffset ?? 0x8000)) >>> 0);
  w32(16, ramdisk.length);
  w32(20, (base + (o.ramdiskOffset ?? 0x01000000)) >>> 0);
  w32(24, second.length);
  w32(28, second.length ? (base + (o.secondOffset ?? 0x00f00000)) >>> 0 : 0); // AOSP: 0 when absent
  w32(32, (base + (o.tagsOffset ?? 0x100)) >>> 0);
  w32(36, page);
  w32(40, v);
  w32(44, 0); // os_version
  const board = enc.encode(o.board ?? "");
  if (board.length > 15) throw new Error("board name too long");
  hdr.set(board, 48);
  const cmd = enc.encode(o.cmdline ?? "");
  if (cmd.length > 511 + 1023) throw new Error("cmdline too long");
  // AOSP keeps a NUL in the 512-byte field: 511 bytes there, the rest in extra_cmdline
  hdr.set(cmd.subarray(0, 511), 64);
  hdr.set(cmd.subarray(511), 608);
  const shaParts = [kernel, le32(kernel.length), ramdisk, le32(ramdisk.length), second, le32(second.length)];
  if (v >= 1) shaParts.push(le32(0)); // recovery dtbo
  if (v >= 2) shaParts.push(dtb, le32(dtb.length));
  hdr.set(await sha1(shaParts), 576);
  if (v >= 1) {
    w32(1632, 0); // recovery_dtbo_size
    dv.setBigUint64(1636, 0n, true);
    w32(1644, hdrSize);
  }
  if (v >= 2) {
    w32(1648, dtb.length);
    dv.setBigUint64(1652, BigInt((base + (o.dtbOffset ?? 0x01f00000)) >>> 0), true);
  }
  const pad = (n) => Math.ceil(n / page) * page;
  const sections = [hdr, kernel, ramdisk, second];
  if (v >= 2) sections.push(dtb);
  const total = sections.reduce((a, s) => a + pad(s.length), 0);
  const out = new Uint8Array(total);
  let off = 0;
  for (const s of sections) {
    out.set(s, off);
    off += pad(s.length);
  }
  return out;
}

/** Minimal valid newc cpio archive (just the trailer): an empty initramfs. */
export function emptyCpio() {
  const name = "TRAILER!!!\0";
  const fields = ["070701", ...[0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, name.length, 0].map((x) => x.toString(16).toUpperCase().padStart(8, "0"))];
  let s = fields.join("") + name;
  while (s.length % 4) s += "\0";
  while (s.length % 512) s += "\0";
  return enc.encode(s);
}

/** Parse the fields we care about back out of a boot image (for checks). */
export function parseBootImg(b) {
  const dv = new DataView(b.buffer, b.byteOffset, b.byteLength);
  const magic = new TextDecoder().decode(b.subarray(0, 8));
  if (magic !== "ANDROID!") throw new Error("not an Android boot image");
  const r = (o) => dv.getUint32(o, true);
  return { kernelSize: r(8), kernelAddr: r(12), ramdiskSize: r(16), pagesize: r(36), headerVersion: r(40) };
}
