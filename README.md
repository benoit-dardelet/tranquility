# Tranquility

A dumbphone OS written from scratch in Rust: a small bare-metal aarch64
kernel plus a `no_std` phone stack covering 4G/5G, voice with call audio,
SMS, MMS, RCS chat (1:1 and group, with file transfer) and the usual
feature-phone apps, in six languages. Data is saved to disk. A web
installer puts it on Google Pixel phones over WebUSB. It doesn't use Linux,
Android or any third-party crates.

![Tranquility on a framebuffer](docs/screen.png)

## Quick start

Requires Rust (stable; `rust-toolchain.toml` pulls the aarch64 target) and
`qemu-system-aarch64` (8.2 or newer for virtio-sound).

```bash
make run          # phone on the serial console (Ctrl-A X to quit)
make run-gui      # phone in a window: keyboard = keypad, sound, saved data
make test         # phone-stack, installer and browser end-to-end tests
make serve        # installer site on http://localhost:8000
```

The SIM PIN is **1234**. Data persists in `build/data.img`. If your machine
has no PulseAudio/PipeWire, use `make run AUDIO=none`.

| Key            | Phone button                   |
|----------------|--------------------------------|
| `0`-`9` `*` `#`| keypad (multi-tap text, `#` changes case/mode) |
| arrows         | d-pad                          |
| Enter          | OK / centre key                |
| Backspace      | Back / clear (deletes in text fields) |
| Esc            | leave the screen, even with text in it |
| `[` or `]`     | left soft key (Options)        |
| Tab            | green Call key                 |
| `` ` `` or End | red End key (back to the idle screen) |
| letters        | typed straight into text fields |

## Installing on a Pixel

`make web` builds a static site in `web/dist`. Deploy it to any HTTPS host,
or use the included GitHub Actions workflow, which publishes it to GitHub
Pages from the default branch. Enable *Settings → Pages → Source: GitHub
Actions*; private repositories need a paid GitHub plan for Pages.
Open it in Chrome, Edge or another Chromium browser. The site walks you
through choosing the Pixel, preparing it and connecting over USB, then
offers four actions:

- **Unlock** — `fastboot flashing unlock` (wipes the phone).
- **Try it** — `fastboot boot`: runs from RAM and writes nothing.
- **Install** — flashes the boot partition of the active slot.
- **Restore** — flashes your stock `boot.img` back.

Each boot image is built in the browser for the selected model.

| Pixel | Chip | How it boots |
|---|---|---|
| Pixel 3 / 3 XL | Snapdragon 845 | header v0, kernel + generated device tree |
| Pixel 3a / 3a XL | Snapdragon 670 | header v0, kernel + generated device tree |
| Pixel 6 / 6 Pro | Google Tensor | header v4; uses the phone's own device tree, display address on the command line |
| Pixel 6a | Google Tensor | as Pixel 6; display address assumed from the Pixel 6 |

The rest of the line-up (Pixel 1, 2, 4, 4a, 5, 5a, 7, 8, 9 and 10
families, Fold, Tablet) is listed on the site as **not supported yet**.
Neither mainline Linux nor postmarketOS describes their display and boot
layout, and the installer doesn't guess.

Data sources:

- **Boot parameters:** postmarketOS `deviceinfo`.
- **Display, GIC, board IDs and reserved memory:** the mainline Linux device
  trees (`sdm845-google-*`, `sdm670-google-*`, `gs101-*`).

Carrier-locked Pixels (e.g. Verizon models) can't be unlocked. Tensor
Pixels may refuse *Try it*; the site then explains that only Install works.

### What works on a real Pixel today — read this

**None of this has been run on real hardware yet.** On a Pixel, the kernel
is designed to:

- boot through the stock bootloader;
- find the display the bootloader left on, from the device tree or from
  `tq.fb=` on the command line;
- keep its memory out of every reserved firmware region;
- run the interface. With no input driver, it plays a short scripted demo:
  unlocks the SIM, receives a chat, answers a call.

Pixel hardware drivers don't exist yet: no modem, touchscreen, buttons,
audio codec or UFS/eMMC flash storage. Until they do, the phone side is a
demo: it can't place real calls or send real texts, and **nothing is kept
across a restart**. The kernel can keep data in a restart-surviving
"ramoops" RAM region when the device tree describes one, but mainline gives
none for these Pixels. Whether a Tensor Pixel's own device tree has one is
unknown. The modem is the hardest part: Qualcomm and Samsung basebands need
firmware loading and QMI or IPC, not AT commands over a UART.

Some bootloaders may also stop refreshing the display after handing over.
If *Try it* shows only the boot logo, that's why.

What is tested, in QEMU and in tests:

- **Boot path.** QEMU boots with the exact device tree the site generates:
  GICv3, no serial port, no virtio, display through `simple-framebuffer`
  only. The Pixel 6 path (no framebuffer in the device tree, `tq.fb=` on the
  command line) is tested too.
- **Boot images.** The site's boot images are byte-identical to AOSP
  `mkbootimg` output for header v0 to v4.
- **Device trees.** Every generated device tree passes `dtc`.

## Architecture

```
kernel/            bare-metal aarch64, ~2,200 lines
  boot.rs          arm64 Image header, PIE self-relocation, EL2→EL1, MMU, vectors
  fdt.rs           device-tree parser; platform.rs: discovery + heap placement
  gic.rs           GICv2 and GICv3;  timer.rs: virtual timer tick
  mmu.rs heap.rs   RAM mapping, cache maintenance, size-class allocator
  uart.rs keys.rs  PL011 console (QEMU), terminal → keypad
  fb.rs fwcfg.rs   framebuffer: bootloader simple-framebuffer or QEMU ramfb
  virtio.rs        virtio-mmio v2 + split queues
  vblk.rs vsnd.rs vinput.rs   disk, sound, keyboard
  ramstore.rs psci.rs   restart-surviving RAM storage, restart / power off
  main.rs          init, event loop, demo autopilot, on-screen panics

phone/             no_std + alloc, host-testable, ~10,700 lines
  at.rs            AT command driver (27.007 / 27.005 + TCP socket commands)
  gsm7.rs sms.rs   GSM 03.38, TS 23.040 PDUs, concatenation, ports, reports
  cb.rs            TS 23.041 cell broadcast (emergency alerts)
  wsp.rs mms.rs    WSP encoding, OMA MMS 1.3, WAP Push
  msg.rs sip.rs    HTTP/SIP framing, SIP helpers, HTTP digest (md5.rs)
  cpim.rs mime.rs  CPIM, IMDN, isComposing, multipart, RFC 5365 lists
  audio.rs         tones, ringtones, DTMF, mixer, simulated far-end voice
  store.rs         snapshot format, A/B CRC-protected block storage
  services/        network, calls, SMS, sockets, HTTP, MMS, RCS, file transfer
  ui/              screens, T9 input, translations (i18n.rs), ANSI and
                   framebuffer renderers, font
  sim/             simulated baseband + network (SMSC, MMSC, IMS, FT server)

web/               installer, ~1,100 lines, no dependencies or build step
  lib/fastboot.js  fastboot over WebUSB
  lib/bootimg.js   Android boot image writer (v0–v2)
  lib/dtb.js       device-tree writer
  lib/devices.js   phone profiles;  lib/build.js: DTB + boot.img per phone
  app.js           the page;  test/: node tests;  e2e/: headless Chromium
```

### Features

- **Network.** 4G/5G registration, operator name, signal, battery, network
  time, mode selection (auto, 4G or 5G only), airplane mode, and recovery
  after signal loss.
- **SIM.** PIN unlock with a retry counter.
- **Calls.** Dial, answer, reject, DTMF, mute, speaker, call log, emergency
  calls while locked or in airplane mode. **Audio** includes ringtone,
  North American ringback, busy tone, keypad DTMF tones and the far end's
  voice from the baseband.
- **SMS.** GSM-7 and UCS-2 encoding, long-message concatenation, delivery
  reports.
- **MMS.** Send and receive, including group MMS with reply-all threading.
  Pictures and vCards are supported.
- **RCS chat.** IMS registration with digest auth, capability discovery,
  read receipts and typing indicators. **Group chat** uses pager-mode
  multi-recipient MESSAGE (RFC 5365) with Conversation-ID. **File
  transfer** goes over HTTP: upload, then send the file-info. Chat falls
  back automatically to SMS, MMS or group MMS.
- **Saved data.** Contacts, conversations (attachments included), call log
  and settings, written every 2 s when changed. Two alternating slots with
  CRCs survive a power cut mid-write. In QEMU it goes on a virtio disk. On
  phones it goes in a restart-surviving RAM region. Settings → Restart and
  Power off use PSCI.
- **Languages.** English, Français, Español, Deutsch, Italiano and
  Português (Settings → Language, saved with your data). This covers every
  screen, the dates, and keypad letters with accents (e.g. 3 → d e f é è ê ë
  in French).
- **Other.** USSD, `*#06#`, cell-broadcast alerts, clock with alarm and
  snooze, stopwatch, calculator, settings, phone status, AT log.

### The simulated network

QEMU has no cellular modem, so `phone::sim::SimModem` plays a UART-attached
LTE/NR module behind the AT interface. Behind it are an SMSC, USSD, cell
broadcast, TCP sockets, an MMSC, an IMS core with an RCS server and
conference factory, and an RCS file-transfer server. Simulated contacts
answer:

| Contact | Number    | Behaviour |
|---------|-----------|-----------|
| Alice   | +15550101 | RCS, replies, sends pictures back, answers calls |
| Bob     | +15550102 | SMS/MMS only, replies, hangs up after 15 s |
| Carol   | +15550103 | RCS, replies, doesn't answer calls |
| Dave    | +15550104 | SMS only, never replies |
| —       | +15550199 | always busy |

**Menu → Network simulator** triggers an incoming call, SMS, chat, MMS,
group chat, emergency alert, signal loss or a long SMS.

### Limits

- **Pager-mode groups only.** Group chat is pager mode. Session-based UP
  group chat (MSRP, conference event package) isn't implemented, and pager
  groups don't carry typing indicators.
- **No TLS.** The phone has no TLS stack, so file transfer and MMS use plain
  HTTP. Real operators require HTTPS for file transfer.
- **Downlink audio only.** No microphone path yet.
- **Simulated network only.** Nothing talks to a real network until there's
  a modem driver.

## Credits

The 8x16 font comes from Terminus Font by Dimitar Zhekov (SIL Open Font
License 1.1); `tools/genfont.py` generates `phone/src/ui/font.rs`.
Device data comes from postmarketOS and the Linux kernel device trees.
