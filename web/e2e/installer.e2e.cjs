// End-to-end test of the installer in headless Chromium with a fake fastboot
// phone behind navigator.usb.   node web/e2e/installer.e2e.cjs [screenshot.png]
const { chromium } = require("playwright");
const http = require("http");
const fs = require("fs");
const path = require("path");

const root = path.join(__dirname, "..", "dist");
const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".gz": "application/octet-stream" };

const FAKE_USB = `
(() => {
  const enc = new TextEncoder(), dec = new TextDecoder();
  class FakePhone {
    constructor() {
      this.productName = "Fake Pixel 3a"; this.opened = false; this.configuration = null;
      this.vars = { product: "sargo", unlocked: "yes", secure: "yes", "current-slot": "b", "slot-count": "2",
        "max-download-size": "0x10000000", "version-bootloader": "b4s4-0.4", serialno: "FAKE123", "has-slot:boot": "yes" };
      this.q = []; this.expect = 0; this.buf = []; window.__phone = this;
      this.flashed = {}; this.booted = null; this.commands = [];
    }
    async open() { this.opened = true; }
    async selectConfiguration() { this.configuration = { interfaces: [{ interfaceNumber: 0, alternates: [{ interfaceClass: 255, interfaceSubclass: 66, interfaceProtocol: 3,
      endpoints: [{ type: "bulk", direction: "in", endpointNumber: 1 }, { type: "bulk", direction: "out", endpointNumber: 1 }] }] }] }; }
    async claimInterface() {} async releaseInterface() {} async close() {}
    say(s) { this.q.push(enc.encode(s)); }
    async transferOut(ep, data) {
      if (this.expect > 0) {
        for (const b of data) this.buf.push(b);
        this.expect -= data.length;
        if (this.expect === 0) { this.dl = new Uint8Array(this.buf); this.buf = []; this.say("OKAY"); }
        return { status: "ok", bytesWritten: data.length };
      }
      const c = dec.decode(data); this.commands.push(c);
      if (c.startsWith("getvar:")) { const v = this.vars[c.slice(7)]; this.say(v === undefined ? "FAILunknown" : "OKAY" + v); }
      else if (c.startsWith("download:")) { this.expect = parseInt(c.slice(9), 16); this.say("DATA" + c.slice(9)); }
      else if (c === "boot") { this.booted = this.dl; this.say("OKAY"); }
      else if (c.startsWith("flash:")) { this.flashed[c.slice(6)] = this.dl.length; this.say("INFOwriting"); this.say("OKAY"); }
      else if (c === "reboot") this.say("OKAY");
      else this.say("FAILunknown command");
      return { status: "ok", bytesWritten: data.length };
    }
    async transferIn() { const d = this.q.shift(); return { status: "ok", data: new DataView(d.buffer) }; }
  }
  const usb = new EventTarget();
  usb.requestDevice = async () => new FakePhone();
  usb.getDevices = async () => [];
  Object.defineProperty(navigator, "usb", { value: usb });
})();`;

(async () => {
  const server = http.createServer((req, res) => {
    const f = path.join(root, decodeURIComponent(req.url.split("?")[0]).replace(/\/$/, "/index.html"));
    if (!f.startsWith(root) || !fs.existsSync(f)) return res.writeHead(404).end();
    res.writeHead(200, { "content-type": types[path.extname(f)] ?? "application/octet-stream" }).end(fs.readFileSync(f));
  }).listen(0);
  const url = `http://localhost:${server.address().port}/`;
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 900, height: 1500 } });
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.addInitScript(FAKE_USB);
  await page.goto(url);
  await page.getByRole("radio", { name: /Pixel 3a\b(?! XL)/ }).click();
  await page.click("#connect");
  await page.waitForSelector("#info table");
  const info = await page.textContent("#info");
  if (!info.includes("unlocked ✓") || !info.includes("sargo")) throw new Error("device info not shown: " + info);
  await page.click("#try");
  await page.waitForFunction(() => window.__phone && window.__phone.booted, null, { timeout: 15000 });
  const booted = await page.evaluate(() => {
    const b = window.__phone.booted;
    const dv = new DataView(b.buffer);
    return { magic: new TextDecoder().decode(b.subarray(0, 8)), size: b.length, kernel: dv.getUint32(8, true), page: dv.getUint32(36, true), cmds: window.__phone.commands };
  });
  if (booted.magic !== "ANDROID!" || booted.page !== 4096) throw new Error("bad boot image " + JSON.stringify(booted));
  // reconnect and install (flash goes to the active slot)
  await page.click("#connect");
  await page.waitForSelector("#info table");
  await page.fill("#install-word", "INSTALL");
  page.once("dialog", (d) => d.accept());
  await page.click("#install");
  await page.waitForFunction(() => window.__phone && window.__phone.flashed["boot_b"], null, { timeout: 15000 });
  const log = await page.textContent("#log");
  if (errors.length) throw new Error("page errors: " + errors.join("; "));
  console.log("boot.img:", booted.size, "bytes, kernel", booted.kernel, "bytes");
  console.log("fastboot commands:", booted.cmds.join(" | "));
  console.log(log.trim().split("\n").slice(-3).join("\n"));
  if (process.argv[2]) {
    await page.click("#connect");
    await page.waitForSelector("#info table");
    await page.screenshot({ path: process.argv[2], fullPage: true });
  }
  await browser.close();
  server.close();
  console.log("installer e2e: OK");
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
