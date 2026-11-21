import { Fastboot, FastbootError } from "./lib/fastboot.js";
import { DEVICES } from "./lib/devices.js";
import { bootImage } from "./lib/build.js";

const $ = (id) => document.getElementById(id);
const state = { device: null, fb: null, info: null, imageGz: null };

const logEl = $("log");
function log(line) {
  logEl.textContent += line + "\n";
  logEl.scrollTop = logEl.scrollHeight;
}

// ------------------------------------------------------------ devices

const unlockTag = { easy: ["good", "easy unlock"], medium: ["mid", "unlock code needed"], hard: ["bad", "hard to unlock"] };

function renderDevices() {
  const box = $("devices");
  for (const d of DEVICES) {
    const b = document.createElement("button");
    b.className = "device";
    b.setAttribute("role", "radio");
    b.setAttribute("aria-checked", "false");
    const [cls, txt] = unlockTag[d.unlock];
    b.innerHTML = `<span class="name"></span><span class="meta"></span><span class="tag ${cls}">${txt}</span> <span class="tag mid">untested</span>`;
    b.querySelector(".name").textContent = d.name;
    b.querySelector(".meta").textContent = `${d.soc} · ${d.year} · ${d.price}`;
    b.addEventListener("click", () => selectDevice(d, b));
    box.appendChild(b);
  }
  const saved = (() => {
    try {
      return localStorage.getItem("tq-device");
    } catch {
      return null;
    }
  })();
  const i = DEVICES.findIndex((d) => d.id === saved);
  if (i >= 0) selectDevice(DEVICES[i], box.children[i]);
}

function selectDevice(d, el) {
  state.device = d;
  for (const c of $("devices").children) c.setAttribute("aria-checked", String(c === el));
  $("keys").textContent = d.fastbootKeys;
  $("unlock-note").textContent = d.unlockNote;
  try {
    localStorage.setItem("tq-device", d.id);
  } catch {
    /* private mode */
  }
  refresh();
}

// ------------------------------------------------------------ image

async function kernelImage() {
  if (state.imageGz) return state.imageGz;
  log("Fetching kernel image…");
  const r = await fetch("tranquility-Image.gz");
  if (!r.ok) throw new Error(`couldn't fetch the kernel image (HTTP ${r.status}). Build the site with "make web".`);
  state.imageGz = new Uint8Array(await r.arrayBuffer());
  log(`Kernel image: ${state.imageGz.length} bytes`);
  return state.imageGz;
}

async function buildImage() {
  const img = await bootImage(state.device, await kernelImage());
  log(`Built boot.img for ${state.device.name}: ${img.length} bytes`);
  return img;
}

// ------------------------------------------------------------ connection

function refresh() {
  const connected = !!state.fb;
  const unlocked = state.info?.unlocked === "yes";
  const ready = connected && unlocked && !!state.device;
  $("try").disabled = !ready;
  $("install").disabled = !ready || $("install-word").value.trim() !== "INSTALL";
  $("restore").disabled = !connected || !unlocked || !$("stock").files.length;
  $("reboot").disabled = !connected;
  $("download").disabled = !state.device;
  const showUnlock = connected && !unlocked;
  $("step-unlock").hidden = !showUnlock;
  $("unlock").disabled = !showUnlock || !$("unlock-ok").checked || !state.device?.unlockCommand;
}

function showInfo(info) {
  const d = state.device;
  const rows = [
    ["Phone reports", info.product ?? "?"],
    ["Bootloader", info.unlocked === "yes" ? "unlocked ✓" : "locked"],
    ["Active slot", info["current-slot"] ?? "—"],
    ["Bootloader version", info["version-bootloader"] ?? "?"],
    ["Max download", info["max-download-size"] ?? "?"],
  ];
  const t = document.createElement("table");
  for (const [k, v] of rows) {
    const tr = t.insertRow();
    tr.insertCell().textContent = k;
    tr.insertCell().textContent = v;
  }
  const box = $("info");
  box.replaceChildren(t);
  if (d && info.product && !d.products.some((p) => p.toLowerCase() === info.product.toLowerCase())) {
    const w = document.createElement("p");
    w.className = "bad-text";
    w.textContent = `This phone says it is "${info.product}", which doesn't match ${d.name}. Pick the right model above; booting an image for another phone won't work.`;
    box.appendChild(w);
  }
  box.hidden = false;
}

async function connect() {
  try {
    if (state.fb) await state.fb.close();
    state.fb = await Fastboot.request();
    state.fb.log = log;
    state.fb.device.addEventListener?.("disconnect", () => {});
    log(`Connected: ${state.fb.device.productName ?? "fastboot device"}`);
    state.info = await state.fb.info();
    showInfo(state.info);
  } catch (e) {
    if (e?.name === "NotFoundError") log("No phone selected.");
    else log(`Connection failed: ${e.message ?? e}`);
    state.fb = null;
  }
  refresh();
}

navigator.usb?.addEventListener("disconnect", (ev) => {
  if (state.fb && ev.device === state.fb.device) {
    log("Phone disconnected.");
    state.fb = null;
    state.info = null;
    $("info").hidden = true;
    refresh();
  }
});

async function guarded(label, fn) {
  const buttons = ["try", "install", "restore", "reboot", "unlock", "connect"].map($);
  buttons.forEach((b) => (b.disabled = true));
  try {
    log(`— ${label}`);
    await fn();
  } catch (e) {
    log(`${label} failed: ${e instanceof FastbootError ? e.message : (e.message ?? e)}`);
  } finally {
    $("connect").disabled = false;
    $("progress").hidden = true;
    refresh();
  }
}

const progress = (p) => {
  $("progress").hidden = false;
  $("progress").value = p;
};

function afterReboot() {
  // the phone leaves fastboot; the USB device goes away
  state.fb = null;
  state.info = null;
  $("info").hidden = true;
}

// ------------------------------------------------------------ actions

$("connect").addEventListener("click", connect);
$("unlock-ok").addEventListener("change", refresh);
$("install-word").addEventListener("input", refresh);
$("stock").addEventListener("change", refresh);

$("unlock").addEventListener("click", () =>
  guarded("Unlock", async () => {
    log("Confirm on the phone: use the volume keys to select “Unlock”, then press power.");
    await state.fb.command(state.device.unlockCommand);
    log("Done. The phone wipes itself and reboots; when it's back in fastboot mode, connect again.");
    afterReboot();
  }),
);

$("try").addEventListener("click", () =>
  guarded("Try it", async () => {
    const img = await buildImage();
    await state.fb.boot(img, progress);
    log("Booting Tranquility from RAM. Hold power ~10 s to return to Android.");
    afterReboot();
  }),
);

$("install").addEventListener("click", () =>
  guarded("Install", async () => {
    if (!confirm(`Replace the boot partition of this ${state.device.name}? Android won't start until you restore it.`)) return;
    const img = await buildImage();
    await state.fb.flash("boot", img, progress);
    log("Installed. Rebooting…");
    await state.fb.reboot();
    $("install-word").value = "";
    afterReboot();
  }),
);

$("restore").addEventListener("click", () =>
  guarded("Restore", async () => {
    const f = $("stock").files[0];
    const img = new Uint8Array(await f.arrayBuffer());
    if (new TextDecoder().decode(img.subarray(0, 8)) !== "ANDROID!") throw new Error(`${f.name} isn't an Android boot image`);
    await state.fb.flash("boot", img, progress);
    log("Stock boot image restored. Rebooting…");
    await state.fb.reboot();
    afterReboot();
  }),
);

$("reboot").addEventListener("click", () =>
  guarded("Reboot", async () => {
    await state.fb.reboot();
    afterReboot();
  }),
);

$("download").addEventListener("click", async () => {
  try {
    const img = await buildImage();
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([img], { type: "application/octet-stream" }));
    a.download = `tranquility-${state.device.id}.img`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
  } catch (e) {
    log(`Download failed: ${e.message ?? e}`);
  }
});

// ------------------------------------------------------------ start

if (!Fastboot.supported()) {
  $("nousb").hidden = false;
  $("connect").disabled = true;
}
if (!window.isSecureContext) log("Note: WebUSB needs https:// or http://localhost.");
renderDevices();
refresh();
