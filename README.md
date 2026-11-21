# Tranquility

A dumbphone OS written from scratch in Rust: a small bare-metal aarch64
kernel plus a `no_std` phone stack covering 4G/5G, voice, SMS, MMS, RCS chat
and the usual feature-phone apps. It doesn't use Linux, Android or any
third-party crates.

```
┌────────────────────────┐
│5G▂▄▆█ Chat   100% 12:00│
│      Alice · Chat      │
├────────────────────────┤
│      Me 12:00 Chat Read│
│ hello                  │
│Alice 12:00 Chat        │
│ Hey! How is life on the│
│ dumbphone? 👍          │
│                        │
├────────────────────────┤
│Options  Write      Back│
└────────────────────────┘
```

## Quick start

Requires Rust (stable) and `qemu-system-aarch64`.

```bash
make run      # build the kernel and boot it in QEMU (Ctrl-A X to quit)
make test     # run the phone-stack tests on the host
```

The SIM PIN is **1234**. The terminal is the phone. The serial console works
as both the screen and the keypad:

| Key            | Phone button                   |
|----------------|--------------------------------|
| `0`-`9` `*` `#`| keypad (multi-tap text entry, `#` changes case or mode) |
| arrows         | d-pad                          |
| Enter          | OK / centre key                |
| Backspace      | Back / clear                   |
| `[` or `]`     | left soft key (Options)        |
| Tab            | green Call key                 |
| `` ` `` or Esc | red End key                    |
| letters        | typed straight into text fields |

From the idle screen: digits open the dialer, Enter opens the menu, Left opens
Messages, Right opens Contacts, Up opens the Clock and Tab opens the Call log.

## Architecture

```
kernel/            bare-metal aarch64 (QEMU virt), ~730 lines
  boot.rs          reset, EL2→EL1 drop, FP enable, BSS, identity MMU, vectors
  gic.rs           GICv2
  timer.rs         generic timer tick (100 Hz) + monotonic clock
  uart.rs          PL011: polled TX, IRQ-driven RX ring
  heap.rs          size-class global allocator
  keys.rs          terminal escape sequences → keypad events
  main.rs          init + event loop: keys → phone, step, redraw, WFI

phone/             no_std + alloc, host-testable
  at.rs            AT command driver (27.007 / 27.005 + TCP socket AT cmds)
  gsm7.rs sms.rs   GSM 03.38 alphabet, TS 23.040 SUBMIT/DELIVER/STATUS-REPORT,
                   UDH concatenation + port addressing, reassembly
  cb.rs            TS 23.041 cell broadcast (emergency alerts)
  wsp.rs mms.rs    WAP-230 WSP encoding, OMA MMS 1.3 PDUs, WAP Push
  msg.rs sip.rs    HTTP/SIP message framing, SIP helpers, HTTP digest (md5.rs)
  cpim.rs          Message/CPIM, IMDN receipts, isComposing
  bmp.rs           1-bpp BMP pictures (MMS attachments drawn as block art)
  services/        registration, telephony, SMS, sockets, MMS agent, RCS client
  ui/              screen stack, T9 input, all the apps
  sim/             simulated baseband + network (see below)
```

There is a single address space and a single core, and the kernel runs one
cooperative event loop. Interrupts only push timer ticks and received
bytes; everything else runs in the main loop. That's how dumbphone RTOSes
usually work, and it keeps the kernel minimal.

### Features

- **Network**: 4G (E-UTRAN) and 5G (NR) registration through `+CEREG` and
  `+C5GREG`, operator name, signal bars, battery, network time (`+CCLK`),
  network-mode selection (`AT+WS46`: auto, 4G only, 5G only), airplane mode,
  and automatic recovery after losing signal.
- **SIM**: PIN unlock with a retry counter, plus IMSI, ICCID and own-number
  queries.
- **Voice**: dial, answer, reject and hang up, call states polled with
  `AT+CLCC`, caller ID, DTMF, mute and speaker. Shows HD VoLTE or VoNR. Busy
  and no-answer are handled, calls go into the call log (dialed, received,
  missed), and emergency numbers can be called while the SIM is locked or in
  airplane mode.
- **SMS**: PDU mode with GSM-7 and UCS-2 encoding, long-message
  concatenation, delivery reports, and a live character/segment counter.
- **MMS**: m-send-req sent by HTTP POST to the MMSC. Incoming messages arrive
  as an m-notification-ind over WAP Push SMS, are fetched by HTTP GET, and
  are acknowledged with m-notifyresp-ind. Text, pictures and vCards are
  supported.
- **RCS chat** (Universal Profile, pager-mode CPM): IMS REGISTER with digest
  auth and re-registration, capability discovery with SIP OPTIONS,
  `MESSAGE` with CPIM, delivered and read receipts (IMDN), and typing
  indicators. Messages fall back to SMS automatically when the peer can't
  chat or RCS is down.
- **USSD** (`*100#` and others) and the local `*#06#` IMEI code.
- **Cell broadcast**: presidential, extreme, severe and AMBER alerts show
  full screen.
- **Apps**: messages (threads mixing SMS, MMS and chat), contacts, call log,
  clock with alarm and snooze, stopwatch, calculator, settings, phone
  status, and an AT command log.

### The simulated baseband

QEMU has no cellular modem, so `phone::sim::SimModem` plays one. It
implements the `Link` trait and behaves like a UART-attached LTE/NR module:
it parses AT commands, emits URCs, and answers with `> ` prompts and PDUs.
Behind it is a small network: registration, an SMSC, a USSD gateway, cell
broadcast, packet data with TCP sockets, an **MMSC** (HTTP), and an **IMS
core with an RCS application server** (SIP). The phone talks to these
servers with its real HTTP, SIP and MMS stacks. Four simulated contacts
reply, answer calls and send receipts:

| Contact | Number    | Behaviour                              |
|---------|-----------|----------------------------------------|
| Alice   | +15550101 | RCS, replies, answers calls            |
| Bob     | +15550102 | SMS only, replies (MMS too), hangs up after 15 s |
| Carol   | +15550103 | RCS, doesn't answer calls              |
| Dave    | +15550104 | SMS only, never replies                |
| —       | +15550199 | always busy                            |

The **Network simulator** menu entry (vendor command `AT+SIMEV`) triggers
incoming calls, SMS, chats, MMS, an emergency alert, signal loss or a long
concatenated SMS.

To run on real hardware, write a `Link` for the board's modem UART and pass
it to `Phone::new` instead of `SimModem`. The AT driver, the protocols and the
UI don't change.

## Tests

`make test` runs the protocol, service and UI tests on the host. The
protocol tests use known vectors (GSM-7 packing, a real-world SMS PDU, RFC
2617 digest, MD5). The service tests run end to end against the simulated
network (SMS with reports, RCS chat with receipts and fallback, MMS both
ways, calls, USSD, alerts, airplane mode and RAT changes). The UI tests drive
the phone with key presses.
