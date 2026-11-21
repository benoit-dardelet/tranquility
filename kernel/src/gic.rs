//! ARM Generic Interrupt Controller: GICv2 (QEMU virt default) and GICv3
//! (QEMU gic-version=3, Qualcomm SoCs). Addresses come from the DTB.
use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy, Debug)]
pub enum Gic {
    V2 { dist: usize, cpu: usize },
    V3 { dist: usize, redist: usize },
}

static KIND: AtomicUsize = AtomicUsize::new(0); // 0 none, 2, 3
static DIST: AtomicUsize = AtomicUsize::new(0);
static CPU: AtomicUsize = AtomicUsize::new(0); // v2 cpu iface / v3 our redistributor

unsafe fn w(a: usize, v: u32) {
    write_volatile(a as *mut u32, v)
}
unsafe fn r(a: usize) -> u32 {
    read_volatile(a as *const u32)
}

pub fn init(g: Gic) -> bool {
    unsafe {
        match g {
            Gic::V2 { dist, cpu } => {
                w(dist, 0);
                let lines = ((r(dist + 0x4) & 0x1F) + 1) * 32;
                for i in (32..lines).step_by(32) {
                    w(dist + 0x180 + (i / 8) as usize, 0xFFFF_FFFF);
                }
                for i in (0..lines).step_by(4) {
                    w(dist + 0x400 + i as usize, 0xA0A0_A0A0);
                }
                for i in (32..lines).step_by(4) {
                    w(dist + 0x800 + i as usize, 0x0101_0101);
                }
                w(dist, 1);
                w(cpu + 0x4, 0xF0);
                w(cpu, 1);
                DIST.store(dist, Ordering::Relaxed);
                CPU.store(cpu, Ordering::Relaxed);
                KIND.store(2, Ordering::Relaxed);
                true
            }
            Gic::V3 { dist, redist } => {
                // find the redistributor frame whose affinity matches this CPU
                let mpidr: u64;
                asm!("mrs {}, mpidr_el1", out(reg) mpidr);
                // GICR_TYPER[63:32] = Aff3.Aff2.Aff1.Aff0
                let aff = (((mpidr >> 32) & 0xFF) << 24 | (mpidr & 0xFF_FFFF)) as u32;
                let mut rd = redist;
                let mut found = None;
                for _ in 0..128 {
                    let typer = read_volatile((rd + 0x8) as *const u64);
                    if (typer >> 32) as u32 == aff {
                        found = Some(rd);
                        break;
                    }
                    if typer & (1 << 4) != 0 {
                        break; // Last
                    }
                    rd += if typer & 1 != 0 { 0x40000 } else { 0x20000 }; // VLPIS frames
                }
                let rd = match found {
                    Some(x) => x,
                    None => return false,
                };
                // distributor: affinity routing, group 1
                w(dist, 0);
                while r(dist) & (1 << 31) != 0 {}
                let lines = ((r(dist + 0x4) & 0x1F) + 1) * 32;
                for i in (32..lines).step_by(32) {
                    w(dist + 0x180 + (i / 8) as usize, 0xFFFF_FFFF); // disable
                    w(dist + 0x80 + (i / 8) as usize, 0xFFFF_FFFF); // group 1
                }
                for i in (32..lines).step_by(4) {
                    w(dist + 0x400 + i as usize, 0xA0A0_A0A0);
                }
                w(dist, (1 << 4) | (1 << 1) | 1); // ARE_NS | EnableGrp1A | EnableGrp1
                while r(dist) & (1 << 31) != 0 {}
                // wake our redistributor
                w(rd + 0x14, r(rd + 0x14) & !(1 << 1));
                let mut n = 0;
                while r(rd + 0x14) & (1 << 2) != 0 && n < 1_000_000 {
                    n += 1;
                }
                let sgi = rd + 0x10000;
                w(sgi + 0x80, 0xFFFF_FFFF); // PPIs/SGIs group 1
                for i in (0..32).step_by(4) {
                    w(sgi + 0x400 + i, 0xA0A0_A0A0);
                }
                // CPU interface via system registers
                let mut sre: u64;
                asm!("mrs {}, s3_0_c12_c12_5", out(reg) sre); // ICC_SRE_EL1
                sre |= 1;
                asm!("msr s3_0_c12_c12_5, {}", "isb", in(reg) sre);
                asm!("msr s3_0_c4_c6_0, {}", in(reg) 0xF0u64); // ICC_PMR_EL1
                asm!("msr s3_0_c12_c12_3, {}", in(reg) 0u64); // ICC_BPR1_EL1
                asm!("msr s3_0_c12_c12_7, {}", "isb", in(reg) 1u64); // ICC_IGRPEN1_EL1
                DIST.store(dist, Ordering::Relaxed);
                CPU.store(rd, Ordering::Relaxed);
                KIND.store(3, Ordering::Relaxed);
                true
            }
        }
    }
}

pub fn enable(irq: u32) {
    let (d, c) = (DIST.load(Ordering::Relaxed), CPU.load(Ordering::Relaxed));
    unsafe {
        match KIND.load(Ordering::Relaxed) {
            2 => w(d + 0x100 + (irq / 32 * 4) as usize, 1 << (irq % 32)),
            3 if irq < 32 => w(c + 0x10000 + 0x100, 1 << irq),
            3 => {
                write_volatile((d + 0x6000 + irq as usize * 8) as *mut u64, 0); // route to aff 0.0.0.0
                w(d + 0x100 + (irq / 32 * 4) as usize, 1 << (irq % 32));
            }
            _ => {}
        }
    }
}

pub fn ack() -> u32 {
    unsafe {
        match KIND.load(Ordering::Relaxed) {
            2 => r(CPU.load(Ordering::Relaxed) + 0xC),
            3 => {
                let v: u64;
                asm!("mrs {}, s3_0_c12_c12_0", out(reg) v); // ICC_IAR1_EL1
                v as u32
            }
            _ => 1023,
        }
    }
}

pub fn eoi(iar: u32) {
    unsafe {
        match KIND.load(Ordering::Relaxed) {
            2 => w(CPU.load(Ordering::Relaxed) + 0x10, iar),
            3 => asm!("msr s3_0_c12_c12_1, {}", in(reg) iar as u64), // ICC_EOIR1_EL1
            _ => {}
        }
    }
}
