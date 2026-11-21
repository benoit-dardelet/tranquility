// Build a flashable/bootable boot.img for a device from the kernel Image.gz.
import { DtNode, buildDtb, cells64 } from "./dtb.js";
import { buildBootImg, emptyCpio } from "./bootimg.js";

const hex = (x) => x.toString(16);

/** Merge overlapping/adjacent [base, size] regions (keeps the union: conservative). */
export function mergeRegions(regions) {
  const r = regions.map(([a, s]) => [a, a + s]).sort((x, y) => x[0] - y[0]);
  const out = [];
  for (const [a, e] of r) {
    const last = out[out.length - 1];
    if (last && a <= last[1]) last[1] = Math.max(last[1], e);
    else out.push([a, e]);
  }
  return out.map(([a, e]) => [a, e - a]);
}

/** Device tree describing what the Tranquility kernel needs on this phone. */
export function deviceTree(dev) {
  const d = dev.dt;
  const root = new DtNode("");
  root.prop("#address-cells", [2]).prop("#size-cells", [2]).prop("model", d.model).prop("compatible", d.compatible);
  if (d.msmId) root.prop("qcom,msm-id", d.msmId);
  if (d.boardId) root.prop("qcom,board-id", d.boardId);
  const chosen = root.child("chosen");
  chosen.prop("#address-cells", [2]).prop("#size-cells", [2]).prop("ranges", true);
  const fb = chosen.child(`framebuffer@${hex(d.fb.base)}`);
  fb.prop("compatible", "simple-framebuffer")
    .prop("reg", [...cells64(d.fb.base), ...cells64(d.fb.size)])
    .prop("width", [d.fb.width])
    .prop("height", [d.fb.height])
    .prop("stride", [d.fb.width * 4])
    .prop("format", d.fb.format);
  // The bootloader fills in the real DRAM layout.
  root.child(`memory@${hex(d.ram)}`).prop("device_type", "memory").prop("reg", [...cells64(d.ram), ...cells64(d.ramSize ?? 0)]);
  const rm = root.child("reserved-memory");
  rm.prop("#address-cells", [2]).prop("#size-cells", [2]).prop("ranges", true);
  for (const [a, s] of mergeRegions(d.reserved)) rm.child(`memory@${hex(a)}`).prop("reg", [...cells64(a), ...cells64(s)]).prop("no-map", true);
  const gic = root.child(`interrupt-controller@${hex(d.gic.dist)}`);
  gic.prop("compatible", "arm,gic-v3")
    .prop("#interrupt-cells", [3])
    .prop("interrupt-controller", true)
    .prop("reg", [...cells64(d.gic.dist), ...cells64(0x10000), ...cells64(d.gic.redist), ...cells64(d.gic.redistSize)])
    .prop("phandle", [1]);
  root.prop("interrupt-parent", [1]);
  root.child("timer").prop("compatible", "arm,armv8-timer").prop("interrupts", d.timer.flatMap((p) => [1, p, 0xf08]));
  return buildDtb(root);
}

/** Kernel + appended DTB (what these bootloaders expect), wrapped in a v0 boot image. */
export async function bootImage(dev, imageGz) {
  const dtb = deviceTree(dev);
  const b = dev.boot;
  let kernel = imageGz;
  if (b.appendDtb) {
    kernel = new Uint8Array(imageGz.length + dtb.length);
    kernel.set(imageGz);
    kernel.set(dtb, imageGz.length);
  }
  return buildBootImg({
    kernel,
    ramdisk: emptyCpio(),
    dtb: b.headerVersion >= 2 ? dtb : undefined,
    headerVersion: b.headerVersion,
    pagesize: b.pagesize,
    base: b.base,
    kernelOffset: b.kernelOffset,
    ramdiskOffset: b.ramdiskOffset,
    secondOffset: b.secondOffset,
    tagsOffset: b.tagsOffset,
    cmdline: "tranquility",
  });
}
