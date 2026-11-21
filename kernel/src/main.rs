//! Tranquility kernel — a minimal aarch64 kernel for a dumbphone.
//!
//! Single core, single address space, event driven. Everything about the
//! machine is discovered from the device tree the bootloader passes in:
//!   * boot.rs      — Image header, self-relocation, MMU, vectors
//!   * fdt.rs       — device tree parser; platform.rs — discovery, heap choice
//!   * gic.rs       — GICv2 / GICv3;  timer.rs — virtual timer tick
//!   * uart.rs      — PL011 console (QEMU)
//!   * fb.rs        — framebuffer (bootloader simple-framebuffer, or QEMU ramfb via fwcfg.rs)
//!   * virtio.rs    — virtio-mmio: vblk.rs (storage), vsnd.rs (audio), vinput.rs (keyboard)
//!   * heap.rs, mmu.rs, keys.rs
//!
//! The phone stack (`phone` crate) is the only task: the loop feeds it keys
//! and time, lets it poll the baseband, pumps audio, redraws the display
//! and sleeps in WFI until the next interrupt.
#![no_std]
#![no_main]

extern crate alloc;

#[macro_use]
mod uart;
mod boot;
mod fb;
mod fdt;
mod fwcfg;
mod gic;
mod heap;
mod keys;
mod mmu;
mod platform;
mod psci;
mod ramstore;
mod timer;
mod vblk;
mod virtio;
mod vinput;
mod vsnd;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::arch::asm;
use core::panic::PanicInfo;
use phone::sim::SimModem;
use phone::store::BlockStore;
use phone::ui::raster::Raster;
use phone::ui::{Key, Phone};

extern "C" {
    static __kernel_end: u8;
}

/// Small bootstrap heap for parsing the device tree before we know where RAM is.
static mut BOOT_HEAP: [u8; 1 << 20] = [0; 1 << 20];

static mut PANIC_FB: Option<fb::Fb> = None;

#[no_mangle]
pub extern "C" fn kmain(dtb: usize, load: usize) -> ! {
    let bh = core::ptr::addr_of_mut!(BOOT_HEAP) as usize;
    heap::init(bh, bh + (1 << 20));
    let kend = core::ptr::addr_of!(__kernel_end) as u64;

    let fdt = unsafe { fdt::Fdt::from_ptr(dtb) };
    let plat = match &fdt {
        Some(f) => platform::discover(f, dtb),
        None => platform::Platform { timer_irq: 27, ..Default::default() },
    };
    for &(b, s) in &plat.ram {
        mmu::map_normal(b, s, &plat.mmio);
    }
    let dtb_size = fdt.as_ref().map(|f| f.total_size as u64).unwrap_or(0);
    let cap = if plat.fwcfg.is_some() { 96 << 20 } else { 48 << 20 };
    let heap_range = platform::pick_heap(&plat, (load as u64, kend), (dtb as u64, dtb_size), cap);
    if let Some((s, e)) = heap_range {
        heap::init(s as usize, e as usize);
    }

    if let Some((base, irq)) = plat.uart {
        uart::init(base, irq);
    }
    uart::write("\x1b[2J\x1b[H");
    kprintln!("Tranquility kernel 0.2 (aarch64)");
    kprintln!("  machine: {}", plat.model);
    kprintln!("  loaded at {:#x}, EL{}, timer {} Hz (irq {})", load, current_el(), timer::freq(), plat.timer_irq);
    for &(b, s) in &plat.ram {
        kprintln!("  ram {:#x}+{} MiB", b, s >> 20);
    }
    let (hs, he) = heap::range();
    kprintln!("  heap {:#x}..{:#x} ({} MiB), {} reserved regions", hs, he, (he - hs) >> 20, plat.reserved.len());

    let irqs = match plat.gic {
        Some(g) => gic::init(g),
        None => false,
    };
    timer::init(plat.timer_irq);
    if irqs {
        gic::enable(plat.timer_irq);
        if uart::present() {
            gic::enable(uart::IRQ.load(core::sync::atomic::Ordering::Relaxed));
            uart::enable_rx_irq();
        }
        unsafe { asm!("msr daifclr, #2") };
    }
    kprintln!("  interrupts: {}", match plat.gic {
        Some(gic::Gic::V2 { .. }) if irqs => "GICv2",
        Some(gic::Gic::V3 { .. }) if irqs => "GICv3",
        _ => "none (polling)",
    });

    // display
    let mut display = plat.fb;
    if display.is_none() {
        if let Some(fw) = plat.fwcfg {
            let (w, h) = (720usize, 1440usize);
            let mem = virtio::dma(w * h * 4) as usize;
            if fwcfg::setup_ramfb(fw, mem, w as u32, h as u32) {
                display = Some(fb::Fb { base: mem, width: w, height: h, stride: w * 4, bgr: false, clean: false });
            }
        }
    }
    unsafe { PANIC_FB = display };
    if let Some(d) = &display {
        kprintln!("  display {}x{} @ {:#x}", d.width, d.height, d.base);
    }

    // virtio devices
    let (mut disk, mut sound, mut kbd) = (None, None, None);
    for &base in &plat.virtio {
        if let Some((m, id)) = virtio::Mmio::probe(base) {
            match id {
                virtio::DEV_BLOCK if disk.is_none() => disk = vblk::VirtioBlk::new(m),
                virtio::DEV_SOUND if sound.is_none() => sound = vsnd::VirtioSnd::new(m),
                virtio::DEV_INPUT if kbd.is_none() => kbd = vinput::VirtioInput::new(m),
                _ => {}
            }
        }
    }
    let storage = match (&disk, plat.persist) {
        (Some(_), _) => String::from("disk"),
        (None, Some((_, s))) => alloc::format!("RAM {} KiB (kept on restart)", s >> 10),
        (None, None) => String::from("not saved"),
    };
    kprintln!("  storage: {}", storage);
    kprintln!("  audio: {}", if sound.is_some() { "virtio-snd 48 kHz" } else { "none" });
    kprintln!("  keypad: {}", if kbd.is_some() { "virtio keyboard" } else if uart::present() { "serial console" } else { "none: demo mode" });
    kprintln!("  baseband: simulated TQ-5G1 over AT link");
    kprintln!("");
    kprintln!("  SIM PIN is 1234");
    let t0 = timer::now_ms();
    while timer::now_ms() - t0 < 1200 {
        idle(irqs);
    }

    let mut phone = Phone::new(Box::new(SimModem::new()), timer::now_ms);
    phone.core.storage = storage;
    if let Some(d) = disk {
        phone.set_storage(Box::new(BlockStore::new(d)));
    } else if let Some((b, s)) = plat.persist {
        phone.set_storage(Box::new(BlockStore::new(ramstore::RamStore::new(b as usize, s as usize))));
    }
    phone.boot();
    let mut autopilot = (!uart::present() && kbd.is_none()).then(Autopilot::new);
    let mut raster = display.as_ref().map(|d| Raster::new(d.width, d.height));
    let mut dec = keys::Decoder::default();
    let mut shown = String::new();
    let mut last_draw = 0u64;
    let mut pending: Vec<Key> = Vec::new();
    uart::write("\x1b[2J\x1b[?25l");
    loop {
        let now = timer::now_ms();
        while let Some(b) = uart::getc() {
            if let Some(k) = dec.feed(b, now) {
                pending.push(k);
            }
        }
        if let Some(k) = dec.flush(now) {
            pending.push(k);
        }
        if let Some(kb) = &mut kbd {
            kb.poll(&mut pending);
        }
        if let Some(a) = &mut autopilot {
            a.step(now, &mut phone, &mut pending);
        }
        let pressed = !pending.is_empty();
        for k in pending.drain(..) {
            phone.key(k);
        }
        phone.step(now);
        if let Some(req) = phone.take_power_request() {
            power(req, plat.psci, &mut phone, &mut raster, &mut display);
        }
        if let Some(s) = &mut sound {
            s.pump(&mut |buf| phone.audio(buf));
        }
        if pressed || now.saturating_sub(last_draw) >= 100 {
            last_draw = now;
            let frame = phone.render();
            if let (Some(r), Some(d)) = (&mut raster, &mut display) {
                r.draw(&frame, d);
            }
            if uart::present() {
                let s = phone::ui::ansi(&frame);
                if s != shown {
                    uart::write(&s);
                    shown = s;
                }
            }
        }
        idle(irqs);
    }
}

/// Restart or power off through PSCI (data was saved by the phone first).
fn power(req: phone::services::Power, conduit: Option<psci::Conduit>, phone: &mut Phone, raster: &mut Option<Raster>, display: &mut Option<fb::Fb>) {
    use phone::services::Power;
    let (msg, func) = match req {
        Power::Restart => ("Restarting…", psci::SYSTEM_RESET),
        Power::Off => ("Goodbye", psci::SYSTEM_OFF),
    };
    kprintln!("\r\n{}", msg);
    if let (Some(r), Some(d)) = (raster.as_mut(), display.as_mut()) {
        let mut f = phone.render();
        f.lines = alloc::vec![String::new(); phone::ui::ROWS];
        f.lines[4] = phone::util::center(msg, phone::ui::W);
        f.soft = Default::default();
        r.draw(&f, d);
    }
    match conduit {
        Some(c) => {
            psci::call(c, func);
            phone.core.toast("Restart not supported by firmware");
        }
        None => phone.core.toast("Restart not available: hold the power button"),
    }
}

fn idle(irqs: bool) {
    if irqs {
        unsafe { asm!("wfi") };
    } else {
        timer::delay_ms(2);
    }
}

fn current_el() -> u64 {
    let el: u64;
    unsafe { asm!("mrs {}, CurrentEL", out(reg) el) };
    el >> 2
}

/// Scripted demo for devices without a keypad driver yet: proves the
/// system is alive on first boot of a real phone.
struct Autopilot {
    at: u64,
    i: usize,
}

enum Act {
    K(Key),
    S(u32),
    T(&'static str),
    W(u64),
}

const SCRIPT: &[Act] = &[
    Act::W(3000),
    Act::T("1234"),
    Act::K(Key::Ok),
    Act::W(5000),
    // ---- loop starts here (index 4)
    Act::S(3),
    Act::W(4000),
    Act::K(Key::Left),
    Act::W(1200),
    Act::K(Key::Down), // past "New message" and "New group"
    Act::K(Key::Down),
    Act::K(Key::Ok),
    Act::W(3000),
    Act::K(Key::Ok),
    Act::T("Hello from my new phone"),
    Act::W(1000),
    Act::K(Key::Ok),
    Act::W(7000),
    Act::K(Key::End),
    Act::S(1),
    Act::W(3000),
    Act::K(Key::Call),
    Act::W(6000),
    Act::K(Key::End),
    Act::W(2000),
    Act::S(8),
    Act::W(6000),
    Act::K(Key::Left),
    Act::W(1000),
    Act::K(Key::Down),
    Act::K(Key::Down),
    Act::K(Key::Ok),
    Act::W(5000),
    Act::K(Key::End),
    Act::K(Key::Up),
    Act::W(4000),
    Act::K(Key::End),
    Act::W(3000),
];

impl Autopilot {
    fn new() -> Autopilot {
        Autopilot { at: 0, i: 0 }
    }
    fn step(&mut self, now: u64, phone: &mut Phone, keys: &mut Vec<Key>) {
        while now >= self.at {
            if self.i >= SCRIPT.len() {
                self.i = 4;
            }
            match &SCRIPT[self.i] {
                Act::K(k) => keys.push(*k),
                Act::S(n) => phone.core.simulate(*n),
                Act::T(t) => keys.extend(t.chars().map(|c| if c.is_ascii_digit() { Key::Num(c) } else { Key::Char(c) })),
                Act::W(ms) => {
                    self.at = now + ms;
                    self.i += 1;
                    return;
                }
            }
            self.i += 1;
            self.at = now + 150;
            return;
        }
    }
}

#[no_mangle]
extern "C" fn irq_handler() {
    let iar = gic::ack();
    let id = iar & 0x3FF;
    if id >= 1020 {
        return; // spurious
    }
    if id == timer::IRQ.load(core::sync::atomic::Ordering::Relaxed) {
        timer::on_irq();
    } else if id == uart::IRQ.load(core::sync::atomic::Ordering::Relaxed) {
        uart::on_irq();
    }
    gic::eoi(iar);
}

fn sysreg_dump(kind: &str) -> ! {
    let (esr, elr, far): (u64, u64, u64);
    unsafe {
        asm!("mrs {}, esr_el1", out(reg) esr);
        asm!("mrs {}, elr_el1", out(reg) elr);
        asm!("mrs {}, far_el1", out(reg) far);
    }
    panic!("{} exception: ESR={:#x} (EC={:#x}) ELR={:#x} FAR={:#x}", kind, esr, esr >> 26, elr, far);
}

#[no_mangle]
extern "C" fn sync_handler(_frame: *mut u64) {
    sysreg_dump("synchronous");
}

#[no_mangle]
extern "C" fn other_handler(_frame: *mut u64) {
    sysreg_dump("unexpected");
}

struct Buf {
    s: [u8; 512],
    n: usize,
}

impl core::fmt::Write for Buf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &b in s.as_bytes() {
            if self.n < self.s.len() {
                self.s[self.n] = b;
                self.n += 1;
            }
        }
        Ok(())
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe { asm!("msr daifset, #2") };
    uart::write("\x1b[?25h\r\n\r\n*** KERNEL PANIC ***\r\n");
    kprintln!("{}", info);
    kprintln!("ticks={} heap in use={} KiB", timer::ticks(), heap::IN_USE.load(core::sync::atomic::Ordering::Relaxed) >> 10);
    // also show it on screen: real phones have no serial port
    if let Some(mut d) = unsafe { PANIC_FB } {
        use core::fmt::Write;
        let mut b = Buf { s: [0; 512], n: 0 };
        let _ = write!(b, "{}", info);
        let msg = core::str::from_utf8(&b.s[..b.n]).unwrap_or("panic");
        let mut r = Raster::new(d.width, d.height);
        r.text(&mut d, 1, 1, "KERNEL PANIC", true);
        for (i, chunk) in msg.as_bytes().chunks(phone::ui::raster::COLS - 2).enumerate().take(14) {
            r.text(&mut d, 1, 3 + i, core::str::from_utf8(chunk).unwrap_or("?"), false);
        }
    }
    loop {
        unsafe { asm!("wfe") };
    }
}
