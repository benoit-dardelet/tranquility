// Supported phones. Boot parameters come from postmarketOS `deviceinfo`
// (pmaports); display, interrupt controller, board IDs and reserved memory
// from the mainline Linux device trees (arch/arm64/boot/dts/qcom).
//
// None of these has been tested with Tranquility on real hardware yet.
//
// `ramoops` is the RAM region mainline Linux reserves for crash logs that
// survive a restart; Tranquility keeps the user's data there. Phones
// without one in mainline don't keep data.

const SDM845_GIC = { dist: 0x17a00000, redist: 0x17a60000, redistSize: 0x100000 };
// Qualcomm wires the arch timer to PPIs 1 (secure), 2 (phys), 3 (virt), 0 (hyp).
const QCOM_TIMER = [1, 2, 3, 0];
// postmarketOS deviceinfo flash offsets shared by all of these
const PM_OFFSETS = { base: 0x00000000, kernelOffset: 0x00008000, ramdiskOffset: 0x01000000, tagsOffset: 0x00000100, pagesize: 4096, headerVersion: 0 };

export const RESERVED = {
  'sdm845-oneplus': [[0x85700000, 0x600000], [0x85e00000, 0x100000], [0x85fc0000, 0x20000], [0x85fe0000, 0x20000], [0x86000000, 0x200000], [0x86200000, 0x2d00000], [0x88f00000, 0x200000], [0x88f00000, 0x1c00000], [0x8ab00000, 0x1400000], [0x8bf00000, 0x500000], [0x8c400000, 0x10000], [0x8c410000, 0x5000], [0x8c415000, 0x2000], [0x8c500000, 0x1a00000], [0x8df00000, 0x100000], [0x8e000000, 0x7800000], [0x95800000, 0x500000], [0x95d00000, 0x800000], [0x96500000, 0x200000], [0x96700000, 0x1400000], [0x97b00000, 0x100000], [0x9d400000, 0x2400000], [0xac300000, 0x400000], [0xf5b00000, 0x202000]],
  'sdm845-beryllium': [[0x85700000, 0x600000], [0x85d00000, 0x200000], [0x85e00000, 0x100000], [0x85fc0000, 0x20000], [0x85fe0000, 0x20000], [0x86000000, 0x200000], [0x86200000, 0x2d00000], [0x86200000, 0x4900000], [0x88f00000, 0x200000], [0x8ab00000, 0x1400000], [0x8bf00000, 0x500000], [0x8c400000, 0x10000], [0x8c410000, 0x5000], [0x8c415000, 0x2000], [0x8c500000, 0x1a00000], [0x8c500000, 0x1e00000], [0x8df00000, 0x100000], [0x8e000000, 0x7800000], [0x8e300000, 0x100000], [0x8e400000, 0x7800000], [0x95800000, 0x500000], [0x95c00000, 0x500000], [0x95d00000, 0x800000], [0x96100000, 0x800000], [0x96500000, 0x200000], [0x96700000, 0x1400000], [0x96900000, 0x200000], [0x96b00000, 0x1400000], [0x97b00000, 0x100000], [0x97f00000, 0x100000], [0x9d400000, 0x2400000], [0xf6301000, 0x200000]],
  'sdm845-axolotl': [[0x85700000, 0x600000], [0x85e00000, 0x100000], [0x85fc0000, 0x20000], [0x85fe0000, 0x20000], [0x86000000, 0x200000], [0x86200000, 0x2d00000], [0x88f00000, 0x200000], [0x8ab00000, 0x1400000], [0x8bf00000, 0x500000], [0x8c400000, 0x10000], [0x8c410000, 0x5000], [0x8c415000, 0x2000], [0x8c500000, 0x1a00000], [0x8df00000, 0x100000], [0x8e000000, 0x7800000], [0x95800000, 0x500000], [0x95d00000, 0x800000], [0x96500000, 0x200000], [0x96700000, 0x1400000], [0x97b00000, 0x100000], [0xb0000000, 0x400000]],
  'sdm670-google': [[0x85700000, 0x600000], [0x85e00000, 0x100000], [0x85fc0000, 0x20000], [0x85fe0000, 0x20000], [0x86000000, 0x200000], [0x86200000, 0x2d00000], [0x8ab00000, 0x500000], [0x8b000000, 0x7e00000], [0x8b000000, 0x9800000], [0x92e00000, 0x500000], [0x93300000, 0x100000], [0x93400000, 0x800000], [0x93c00000, 0x200000], [0x93e00000, 0x1e00000], [0x94800000, 0x500000], [0x94d00000, 0x100000], [0x94e00000, 0x800000], [0x95600000, 0x200000], [0x95800000, 0x2200000], [0x95c00000, 0x10000], [0x95c10000, 0x5000], [0x95c15000, 0x2000], [0x97a00000, 0x10000], [0x97a10000, 0x5000], [0x97a15000, 0x2000], [0x97b00000, 0x100000], [0x9c000000, 0x2400000], [0x9e400000, 0x1400000], [0xa1800000, 0x411000]],
  'sm7225-fp4': [[0x80000000, 0x600000], [0x80700000, 0x160000], [0x80860000, 0x20000], [0x808ff000, 0x1000], [0x80900000, 0x200000], [0x80b00000, 0x1e00000], [0x86000000, 0x500000], [0x86500000, 0x500000], [0x86a00000, 0x500000], [0x86f00000, 0x1e00000], [0x88d00000, 0x2800000], [0x8b500000, 0x200000], [0x8b700000, 0x10000], [0x8b710000, 0x5400], [0x8b800000, 0xf800000], [0xa0000000, 0x2300000], [0xa2300000, 0x100000], [0xc0000000, 0x3900000], [0xefe01000, 0x600000], [0xf0d00000, 0x1000], [0xffb00000, 0xc0000], [0xffbc0000, 0x40000], [0xffc00000, 0x100000], [0xffd00000, 0x1000]],
};

export const DEVICES = [
  {
    id: "google-sargo",
    name: "Google Pixel 3a",
    soc: "Snapdragon 670",
    year: 2019,
    price: "≈ $40–70 used",
    unlock: "easy",
    unlockNote: "Settings → System → Developer options → OEM unlocking, then “Unlock bootloader” here. Verizon-branded units can’t be unlocked.",
    unlockCommand: "flashing unlock",
    fastbootKeys: "Power off, then hold Power + Volume Down.",
    products: ["sargo"],
    ab: true,
    boot: { ...PM_OFFSETS, secondOffset: 0x00000000, appendDtb: true },
    dt: {
      model: "Google Pixel 3a",
      compatible: ["google,sargo", "qcom,sdm670"],
      fb: { base: 0x9c000000, size: 0x2400000, width: 1080, height: 2220, format: "a8r8g8b8" },
      gic: SDM845_GIC, timer: QCOM_TIMER, ram: 0x80000000, reserved: RESERVED["sdm670-google"],
      psci: "smc",
    },
  },
  {
    id: "google-bonito",
    name: "Google Pixel 3a XL",
    soc: "Snapdragon 670",
    year: 2019,
    price: "≈ $50–80 used",
    unlock: "easy",
    unlockNote: "Settings → System → Developer options → OEM unlocking, then “Unlock bootloader” here. Verizon-branded units can’t be unlocked.",
    unlockCommand: "flashing unlock",
    fastbootKeys: "Power off, then hold Power + Volume Down.",
    products: ["bonito"],
    ab: true,
    boot: { ...PM_OFFSETS, secondOffset: 0x00000000, appendDtb: true },
    dt: {
      model: "Google Pixel 3a XL",
      compatible: ["google,bonito", "qcom,sdm670"],
      fb: { base: 0x9c000000, size: 0x2400000, width: 1080, height: 2160, format: "a8r8g8b8" },
      gic: SDM845_GIC, timer: QCOM_TIMER, ram: 0x80000000, reserved: RESERVED["sdm670-google"],
      psci: "smc",
    },
  },
  {
    id: "oneplus-enchilada",
    name: "OnePlus 6",
    soc: "Snapdragon 845",
    year: 2018,
    price: "≈ $60–100 used",
    unlock: "easy",
    unlockNote: "Settings → System → Developer options → OEM unlocking, then “Unlock bootloader” here and confirm on the phone.",
    unlockCommand: "oem unlock",
    fastbootKeys: "Power off, then hold Power + Volume Up.",
    products: ["sdm845", "enchilada", "OnePlus6"],
    ab: true,
    boot: { ...PM_OFFSETS, secondOffset: 0x00f00000, appendDtb: true },
    dt: {
      model: "OnePlus 6",
      compatible: ["oneplus,enchilada", "qcom,sdm845"],
      msmId: [321, 0x20001], boardId: [8, 0, 17819, 22],
      fb: { base: 0x9d400000, size: 0x2400000, width: 1080, height: 2280, format: "a8r8g8b8" },
      gic: SDM845_GIC, timer: QCOM_TIMER, ram: 0x80000000, reserved: RESERVED["sdm845-oneplus"],
      psci: "smc", ramoops: [0xac300000, 0x400000],
    },
  },
  {
    id: "oneplus-fajita",
    name: "OnePlus 6T",
    soc: "Snapdragon 845",
    year: 2018,
    price: "≈ $70–110 used",
    unlock: "easy",
    unlockNote: "Settings → System → Developer options → OEM unlocking, then “Unlock bootloader” here and confirm on the phone.",
    unlockCommand: "oem unlock",
    fastbootKeys: "Power off, then hold Power + Volume Up.",
    products: ["sdm845", "fajita", "OnePlus6T"],
    ab: true,
    boot: { ...PM_OFFSETS, secondOffset: 0x00f00000, appendDtb: true },
    dt: {
      model: "OnePlus 6T",
      compatible: ["oneplus,fajita", "qcom,sdm845"],
      msmId: [321, 0x20001], boardId: [8, 0, 18801, 41],
      fb: { base: 0x9d400000, size: 0x2400000, width: 1080, height: 2340, format: "a8r8g8b8" },
      gic: SDM845_GIC, timer: QCOM_TIMER, ram: 0x80000000, reserved: RESERVED["sdm845-oneplus"],
      psci: "smc", ramoops: [0xac300000, 0x400000],
    },
  },
  {
    id: "shift-axolotl",
    name: "SHIFT6mq",
    soc: "Snapdragon 845",
    year: 2020,
    price: "≈ $100–150 used",
    unlock: "easy",
    unlockNote: "Developer options → OEM unlocking, then “Unlock bootloader” here.",
    unlockCommand: "flashing unlock",
    fastbootKeys: "Power off, then hold Power + Volume Down (or use adb reboot bootloader).",
    products: ["axolotl", "SHIFT6mq"],
    ab: true,
    boot: { ...PM_OFFSETS, secondOffset: 0x00f00000, appendDtb: true },
    dt: {
      model: "SHIFT SHIFT6mq",
      compatible: ["shift,axolotl", "qcom,sdm845"],
      msmId: [321, 0x20001], boardId: [11, 0],
      fb: { base: 0x9d400000, size: 0x2400000, width: 1080, height: 2160, format: "a8r8g8b8" },
      gic: SDM845_GIC, timer: QCOM_TIMER, ram: 0x80000000, reserved: RESERVED["sdm845-axolotl"],
      psci: "smc", ramoops: [0xb0000000, 0x400000],
    },
  },
  {
    id: "xiaomi-beryllium",
    name: "Xiaomi Poco F1",
    soc: "Snapdragon 845",
    year: 2018,
    price: "≈ $50–80 used",
    unlock: "hard",
    unlockNote: "Needs Xiaomi’s Mi Unlock tool (Windows), a Mi account and a waiting period of 7+ days. This site can’t do that step; come back once it’s unlocked.",
    unlockCommand: null,
    fastbootKeys: "Power off, then hold Power + Volume Down.",
    products: ["beryllium"],
    ab: false,
    boot: { ...PM_OFFSETS, secondOffset: 0x00f00000, appendDtb: true },
    dt: {
      model: "Xiaomi Pocophone F1",
      compatible: ["xiaomi,beryllium", "qcom,sdm845"],
      msmId: [321, 0x20001], boardId: [69, 0],
      fb: { base: 0x9d400000, size: 0x2400000, width: 1080, height: 2246, format: "a8r8g8b8" },
      gic: SDM845_GIC, timer: QCOM_TIMER, ram: 0x80000000, reserved: RESERVED["sdm845-beryllium"],
      psci: "smc",
    },
  },
  {
    id: "fairphone-fp4",
    name: "Fairphone 4",
    soc: "Snapdragon 750G",
    year: 2021,
    price: "≈ $150–250 used",
    unlock: "medium",
    unlockNote: "Get an unlock code from fairphone.com, enter it under Developer options → OEM unlocking, then “Unlock bootloader” here (the phone also asks for unlock_critical).",
    unlockCommand: "flashing unlock",
    fastbootKeys: "Power off, then hold Volume Down and plug in the USB cable.",
    products: ["FP4"],
    ab: true,
    boot: { ...PM_OFFSETS, secondOffset: 0x00000000, appendDtb: true },
    dt: {
      model: "Fairphone 4",
      compatible: ["fairphone,fp4", "qcom,sm7225"],
      msmId: [459, 0x10000], boardId: [8, 32],
      fb: { base: 0xa0000000, size: 2340 * 1080 * 4, width: 1080, height: 2340, format: "a8r8g8b8" },
      gic: SDM845_GIC, timer: QCOM_TIMER, ram: 0x80000000, reserved: RESERVED["sm7225-fp4"],
      psci: "smc", ramoops: [0xffc00000, 0x100000],
    },
  },
];

/** A synthetic "phone" for testing the real-device boot path in QEMU
 *  (GICv3, no UART, no virtio: framebuffer only, like a real phone). */
export const QEMU_PHONE = {
  id: "qemu-phone",
  name: "QEMU phone-like test target",
  ab: false,
  boot: { ...PM_OFFSETS, secondOffset: 0, appendDtb: true },
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
