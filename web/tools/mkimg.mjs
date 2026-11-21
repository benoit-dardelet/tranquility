#!/usr/bin/env node
// Build boot images and device trees outside the browser (same code as the site).
//   node web/tools/mkimg.mjs <device-id|qemu-phone|all> <Image.gz> <out-dir>
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { DEVICES, QEMU_PHONE } from "../lib/devices.js";
import { bootImage, deviceTree } from "../lib/build.js";

const [which, image, outDir] = process.argv.slice(2);
if (!which || !image || !outDir) {
  console.error("usage: mkimg.mjs <device-id|qemu-phone|all> <Image.gz> <out-dir>");
  process.exit(2);
}
mkdirSync(outDir, { recursive: true });
const gz = new Uint8Array(readFileSync(image));
const all = [...DEVICES, QEMU_PHONE];
const targets = which === "all" ? all : all.filter((d) => d.id === which);
if (!targets.length) {
  console.error(`unknown device "${which}". Known: ${all.map((d) => d.id).join(", ")}`);
  process.exit(2);
}
for (const d of targets) {
  writeFileSync(`${outDir}/${d.id}.dtb`, deviceTree(d));
  const img = await bootImage(d, gz);
  writeFileSync(`${outDir}/tranquility-${d.id}.img`, img);
  console.log(`${d.id}: boot.img ${img.length} bytes`);
}
