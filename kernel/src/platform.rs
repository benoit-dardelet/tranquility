//! Platform discovery from the device tree, and choice of heap memory.
use crate::fb::Fb;
use crate::fdt::Fdt;
use crate::gic::Gic;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Default)]
pub struct Platform {
    pub model: String,
    pub ram: Vec<(u64, u64)>,
    pub reserved: Vec<(u64, u64)>,
    pub uart: Option<(usize, u32)>,
    pub gic: Option<Gic>,
    pub timer_irq: u32,
    pub fb: Option<Fb>,
    pub virtio: Vec<usize>,
    pub fwcfg: Option<usize>,
    /// MMIO windows that must stay device memory
    pub mmio: Vec<(u64, u64)>,
    /// RAM that survives a warm restart (a "ramoops" reserved region)
    pub persist: Option<(u64, u64)>,
    pub psci: Option<crate::psci::Conduit>,
}

pub fn discover(f: &Fdt, dtb: usize) -> Platform {
    let mut p = Platform { timer_irq: 27, ..Default::default() };
    p.model = f.str(0, "model").unwrap_or("unknown").into();
    for n in 0..f.nodes.len() {
        let is_mem = f.nodes[n].name.split('@').next() == Some("memory") && f.nodes[n].parent == Some(0) || f.str(n, "device_type") == Some("memory");
        if is_mem {
            p.ram.extend(f.reg(n).into_iter().filter(|r| r.1 > 0));
        }
    }
    if let Some(rm) = f.by_path("/reserved-memory") {
        for c in f.children(rm).collect::<Vec<_>>() {
            p.reserved.extend(f.reg(c));
        }
    }
    // /memreserve/ entries from the header
    unsafe {
        let hdr = core::slice::from_raw_parts(dtb as *const u8, 40);
        let off = u32::from_be_bytes(hdr[16..20].try_into().unwrap()) as usize;
        for i in 0..64 {
            let e = core::slice::from_raw_parts((dtb + off + i * 16) as *const u8, 16);
            let (a, s) = (u64::from_be_bytes(e[0..8].try_into().unwrap()), u64::from_be_bytes(e[8..16].try_into().unwrap()));
            if a == 0 && s == 0 {
                break;
            }
            p.reserved.push((a, s));
        }
    }
    if let Some(&n) = f.find_compatible(&["arm,pl011"]).first() {
        if let Some(&(a, s)) = f.reg(n).first() {
            p.uart = Some((a as usize, f.irqs(n).first().copied().unwrap_or(33)));
            p.mmio.push((a, s.max(0x1000)));
        }
    }
    if let Some(&n) = f.find_compatible(&["arm,gic-v3"]).first() {
        let r = f.reg(n);
        if r.len() >= 2 {
            p.gic = Some(Gic::V3 { dist: r[0].0 as usize, redist: r[1].0 as usize });
            p.mmio.extend(r.iter().take(2));
        }
    } else if let Some(&n) = f.find_compatible(&["arm,cortex-a15-gic", "arm,gic-400", "arm,cortex-a9-gic", "arm,arm11mp-gic"]).first() {
        let r = f.reg(n);
        if r.len() >= 2 {
            p.gic = Some(Gic::V2 { dist: r[0].0 as usize, cpu: r[1].0 as usize });
            p.mmio.extend(r.iter().take(2));
        }
    }
    if let Some(&n) = f.find_compatible(&["arm,armv8-timer", "arm,armv7-timer"]).first() {
        if let Some(&v) = f.irqs(n).get(2) {
            p.timer_irq = v;
        }
    }
    if let Some(&n) = f.find_compatible(&["simple-framebuffer"]).first() {
        let base = f.reg(n).first().map(|r| r.0).or_else(|| {
            // memory-region = <&phandle>
            let ph = f.u32(n, "memory-region")?;
            let t = (0..f.nodes.len()).find(|&i| f.u32(i, "phandle") == Some(ph))?;
            f.reg(t).first().map(|r| r.0)
        });
        let (w, h) = (f.u32(n, "width").unwrap_or(0) as usize, f.u32(n, "height").unwrap_or(0) as usize);
        let stride = f.u32(n, "stride").map(|s| s as usize).unwrap_or(w * 4);
        let fmt = f.str(n, "format").unwrap_or("a8r8g8b8");
        if let (Some(b), true, true) = (base, w > 0 && h > 0, fmt.len() == 8 && fmt.ends_with('8')) {
            p.fb = Some(Fb { base: b as usize, width: w, height: h, stride, bgr: fmt.contains("b8g8r8"), clean: true });
            p.reserved.push((b, (stride * h) as u64));
        }
    }
    for n in f.find_compatible(&["virtio,mmio"]) {
        if let Some(&(a, s)) = f.reg(n).first() {
            p.virtio.push(a as usize);
            p.mmio.push((a, s.max(0x200)));
        }
    }
    if let Some(&n) = f.find_compatible(&["ramoops"]).first() {
        if let Some(&(a, s)) = f.reg(n).first() {
            if s >= 64 * 1024 {
                p.persist = Some((a, s));
                p.reserved.push((a, s));
            }
        }
    }
    if let Some(&n) = f.find_compatible(&["arm,psci-1.0", "arm,psci-0.2", "arm,psci"]).first() {
        p.psci = match f.str(n, "method") {
            Some("smc") => Some(crate::psci::Conduit::Smc),
            Some("hvc") => Some(crate::psci::Conduit::Hvc),
            _ => None,
        };
    }
    if let Some(&n) = f.find_compatible(&["qemu,fw-cfg-mmio"]).first() {
        if let Some(&(a, s)) = f.reg(n).first() {
            p.fwcfg = Some(a as usize);
            p.mmio.push((a, s));
        }
    }
    p
}

/// Pick a heap region: free RAM (not reserved, not the kernel image, the
/// DTB or the framebuffer), preferring memory right after the kernel.
pub fn pick_heap(p: &Platform, kernel: (u64, u64), dtb: (u64, u64), cap: u64) -> Option<(u64, u64)> {
    let mut busy: Vec<(u64, u64)> = p.reserved.iter().map(|&(a, s)| (a, a + s)).collect();
    busy.push((kernel.0, kernel.1));
    busy.push((dtb.0, dtb.0 + dtb.1));
    busy.sort();
    let mut free = Vec::new();
    for &(base, size) in &p.ram {
        let mut cur = base;
        let end = base + size;
        for &(b, e) in &busy {
            if e <= cur || b >= end {
                continue;
            }
            if b > cur {
                free.push((cur, b));
            }
            cur = cur.max(e);
        }
        if cur < end {
            free.push((cur, end));
        }
    }
    let align = |x: u64| (x + 0xFFFF) & !0xFFFF;
    let usable = |&(s, e): &(u64, u64)| (align(s), e);
    // first choice: the free range starting where the kernel ends
    if let Some(r) = free.iter().map(usable).find(|&(s, e)| s >= kernel.1 && s < kernel.1 + 0x20_0000 && e > s + 8 * 1024 * 1024) {
        return Some((r.0, r.1.min(r.0 + cap)));
    }
    free.iter().map(usable).filter(|&(s, e)| e > s).max_by_key(|&(s, e)| e - s).map(|(s, e)| (s, e.min(s + cap)))
}
