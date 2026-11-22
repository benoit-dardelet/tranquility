// Google Pixel phones.
//
// Data sources, per phone:
//  * boot parameters: postmarketOS `deviceinfo` (pmaports) where it exists;
//  * display address, board IDs, interrupt controller, reserved memory:
//    mainline Linux device trees (arch/arm64/boot/dts/qcom/sdm670-google-*,
//    sdm845-google-*, exynos/google/gs101-*).
// Pixels without that data are listed as not supported yet instead of
// guessing. None of these has been tested with Tranquility on real hardware.

const QCOM_GIC = { dist: 0x17a00000, redist: 0x17a60000, redistSize: 0x100000 };
// Qualcomm wires the arch timer to PPIs 1 (secure), 2 (phys), 3 (virt), 0 (hyp).
const QCOM_TIMER = [1, 2, 3, 0];
// postmarketOS deviceinfo for google-sargo / bonito / blueline
const PM_OFFSETS = { base: 0x00000000, kernelOffset: 0x00008000, ramdiskOffset: 0x01000000, secondOffset: 0x00000000, tagsOffset: 0x00000100, pagesize: 4096, headerVersion: 0, appendDtb: true };

export const RESERVED = {
  "sdm670-google": [[0x85700000, 0x600000], [0x85e00000, 0x100000], [0x85fc0000, 0x20000], [0x85fe0000, 0x20000], [0x86000000, 0x200000], [0x86200000, 0x2d00000], [0x8ab00000, 0x500000], [0x8b000000, 0x7e00000], [0x8b000000, 0x9800000], [0x92e00000, 0x500000], [0x93300000, 0x100000], [0x93400000, 0x800000], [0x93c00000, 0x200000], [0x93e00000, 0x1e00000], [0x94800000, 0x500000], [0x94d00000, 0x100000], [0x94e00000, 0x800000], [0x95600000, 0x200000], [0x95800000, 0x2200000], [0x95c00000, 0x10000], [0x95c10000, 0x5000], [0x95c15000, 0x2000], [0x97a00000, 0x10000], [0x97a10000, 0x5000], [0x97a15000, 0x2000], [0x97b00000, 0x100000], [0x9c000000, 0x2400000], [0x9e400000, 0x1400000], [0xa1800000, 0x411000]],
  "sdm845-google": [[0x85700000, 0x600000], [0x85e00000, 0x100000], [0x85fc0000, 0x20000], [0x85fe0000, 0x20000], [0x86000000, 0x200000], [0x86200000, 0x2d00000], [0x88f00000, 0x200000], [0x8ab00000, 0x1400000], [0x8bf00000, 0x500000], [0x8c400000, 0x10000], [0x8c410000, 0x5000], [0x8c415000, 0x2000], [0x8c500000, 0x1a00000], [0x8df00000, 0x100000], [0x8e000000, 0x9800000], [0x95800000, 0x500000], [0x95d00000, 0x800000], [0x96500000, 0x200000], [0x96700000, 0x1400000], [0x97800000, 0x500000], [0x97b00000, 0x100000], [0x97d00000, 0x800000], [0x98500000, 0x200000], [0x98700000, 0x1400000], [0x99b00000, 0x100000], [0x9d400000, 0x2400000], [0xf2700000, 0x202000]],
};

const PIXEL_UNLOCK = "Settings → System → Developer options → OEM unlocking, then “Unlock bootloader” here. Carrier-locked phones (e.g. Verizon models) can’t be unlocked.";
const PIXEL_KEYS = "Power off, then hold Power + Volume Down.";

function qcomPixel(id, name, soc, year, price, products, model, compatible, fb, reserved, extra = {}) {
  return {
    id, name, soc, year, price, supported: true,
    unlock: "easy", unlockNote: PIXEL_UNLOCK, unlockCommand: "flashing unlock", fastbootKeys: PIXEL_KEYS,
    products, ab: true, tryIt: true,
    boot: { ...PM_OFFSETS },
    dt: { model, compatible, fb, gic: QCOM_GIC, timer: QCOM_TIMER, ram: 0x80000000, reserved, psci: "smc", ...extra },
  };
}

/** Tensor Pixels boot with header v4: the bootloader always passes the
 *  phone's own device tree (from vendor_boot), so we don't send one; the
 *  display address goes on the kernel command line instead. */
function tensorPixel(id, name, year, price, products, fb, note) {
  return {
    id, name, soc: "Google Tensor", year, price, supported: true, note,
    unlock: "easy", unlockNote: PIXEL_UNLOCK, unlockCommand: "flashing unlock", fastbootKeys: PIXEL_KEYS,
    products, ab: true,
    // `fastboot boot` may be refused on Tensor Pixels: Install is then the only way
    tryIt: "maybe",
    boot: { headerVersion: 4, rawKernel: true, cmdline: `tq.fb=0x${fb.base.toString(16)},${fb.width},${fb.height}` },
    dt: null,
  };
}

export const DEVICES = [
  qcomPixel("google-blueline", "Pixel 3", "Snapdragon 845", 2018, "≈ $40–60 used", ["blueline"], "Google Pixel 3", ["google,blueline", "qcom,sdm845"],
    { base: 0x9d400000, size: 0x2400000, width: 1080, height: 2160, format: "a8r8g8b8" }, RESERVED["sdm845-google"],
    { msmId: [321, 0x20001], boardId: [0x00021505, 0] }),
  qcomPixel("google-crosshatch", "Pixel 3 XL", "Snapdragon 845", 2018, "≈ $50–70 used", ["crosshatch"], "Google Pixel 3 XL", ["google,crosshatch", "qcom,sdm845"],
    { base: 0x9d400000, size: 0x2400000, width: 1440, height: 2960, format: "a8r8g8b8" }, RESERVED["sdm845-google"],
    { msmId: [321, 0x20001], boardId: [0x00021505, 0] }),
  qcomPixel("google-sargo", "Pixel 3a", "Snapdragon 670", 2019, "≈ $40–70 used", ["sargo"], "Google Pixel 3a", ["google,sargo", "qcom,sdm670"],
    { base: 0x9c000000, size: 0x2400000, width: 1080, height: 2220, format: "a8r8g8b8" }, RESERVED["sdm670-google"]),
  qcomPixel("google-bonito", "Pixel 3a XL", "Snapdragon 670", 2019, "≈ $50–80 used", ["bonito"], "Google Pixel 3a XL", ["google,bonito", "qcom,sdm670"],
    { base: 0x9c000000, size: 0x2400000, width: 1080, height: 2160, format: "a8r8g8b8" }, RESERVED["sdm670-google"]),
  tensorPixel("google-oriole", "Pixel 6", 2021, "≈ $120–180 used", ["oriole"], { base: 0xfac00000, width: 1080, height: 2400 }),
  tensorPixel("google-raven", "Pixel 6 Pro", 2021, "≈ $150–220 used", ["raven"], { base: 0xfac00000, width: 1440, height: 3120 }),
  tensorPixel("google-bluejay", "Pixel 6a", 2022, "≈ $100–150 used", ["bluejay"], { base: 0xfac00000, width: 1080, height: 2400 },
    "Display address assumed to match the Pixel 6 (same chip); mainline Linux has no Pixel 6a description yet."),
];

/** The rest of the line-up: no open hardware description yet (display
 *  address, boot layout), so no image can be built safely. */
export const NOT_YET = [
  ["Pixel / Pixel XL", 2016], ["Pixel 2 / 2 XL", 2017], ["Pixel 4 / 4 XL", 2019], ["Pixel 4a", 2020], ["Pixel 4a (5G)", 2020],
  ["Pixel 5", 2020], ["Pixel 5a", 2021], ["Pixel 7 / 7 Pro", 2022], ["Pixel 7a", 2023], ["Pixel Fold", 2023], ["Pixel Tablet", 2023],
  ["Pixel 8 / 8 Pro", 2023], ["Pixel 8a", 2024], ["Pixel 9 / 9 Pro / 9 Pro XL", 2024], ["Pixel 9 Pro Fold", 2024], ["Pixel 9a", 2025],
  ["Pixel 10 series", 2025],
];

/** A synthetic "phone" for testing the real-device boot path in QEMU
 *  (GICv3, no UART, no virtio: framebuffer only, like a real phone). */
export const QEMU_PHONE = {
  id: "qemu-phone",
  name: "QEMU phone-like test target",
  ab: false,
  boot: { ...PM_OFFSETS },
  dt: {
    model: "QEMU phone-like test target",
    compatible: ["tranquility,qemu-phone"],
    fb: { base: 0x4e000000, size: 720 * 1440 * 4, width: 720, height: 1440, format: "a8r8g8b8" },
    gic: { dist: 0x08000000, redist: 0x080a0000, redistSize: 0xf60000 },
    timer: [13, 14, 11, 10],
    ram: 0x40000000,
    ramSize: 0x10000000,
    reserved: [[0x4f000000, 0x100000]],
    psci: "hvc", ramoops: [0x4f400000, 0x400000],
  },
};
