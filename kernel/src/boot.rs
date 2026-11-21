//! Reset entry, EL2→EL1 drop, FP enable, BSS clear, identity-mapped MMU
//! and the exception vector table.
use core::arch::global_asm;

global_asm!(
    r#"
.section .text.boot, "ax"
.global _start
_start:
    // park secondary cores
    mrs     x0, mpidr_el1
    and     x0, x0, #3
    cbz     x0, 2f
1:  wfe
    b       1b
2:
    mrs     x0, CurrentEL
    lsr     x0, x0, #2
    cmp     x0, #2
    b.ne    3f
    // running at EL2: configure EL1 as AArch64 and drop to it
    mov     x0, #(1 << 31)
    msr     hcr_el2, x0
    mov     x0, #3
    msr     cnthctl_el2, x0
    msr     cntvoff_el2, xzr
    mov     x0, #0x3c5
    msr     spsr_el2, x0
    adr     x0, 3f
    msr     elr_el2, x0
    eret
3:
    ldr     x0, =__stack_top
    mov     sp, x0
    // FP/SIMD on (CPACR_EL1.FPEN = 0b11)
    mov     x0, #(3 << 20)
    msr     cpacr_el1, x0
    isb
    // clear .bss
    ldr     x0, =__bss_start
    ldr     x1, =__bss_end
4:  cmp     x0, x1
    b.hs    5f
    str     xzr, [x0], #8
    b       4b
5:
    ldr     x0, =__vectors
    msr     vbar_el1, x0

    // ---- MMU: identity map with 1 GiB L1 blocks (4K granule, 39-bit VA)
    //  [0, 1G)  device nGnRnE  (flash, GIC, UART, virtio …)
    //  [1G, 2G) normal write-back, inner shareable (RAM)
    ldr     x0, =__l1_table
    ldr     x1, =0x0000000000000401
    str     x1, [x0]
    ldr     x1, =0x0000000040000705
    str     x1, [x0, #8]
    ldr     x1, =0xFF00                 // MAIR: attr0 device, attr1 normal WB
    msr     mair_el1, x1
    ldr     x1, =0x200803519            // T0SZ=25 WBWA IS 4K EPD1 IPS=40bit
    msr     tcr_el1, x1
    msr     ttbr0_el1, x0
    dsb     ish
    tlbi    vmalle1
    dsb     ish
    isb
    mrs     x1, sctlr_el1
    orr     x1, x1, #(1 << 0)           // M
    orr     x1, x1, #(1 << 2)           // C
    orr     x1, x1, #(1 << 12)          // I
    bic     x1, x1, #(1 << 1)           // no alignment faults
    msr     sctlr_el1, x1
    isb

    bl      kmain
6:  wfe
    b       6b

.section .bss.pagetable, "aw", @nobits
.balign 4096
.global __l1_table
__l1_table:
    .space 4096

// ------------------------------------------------------------------ vectors
.section .text
.macro VENT target
    .balign 0x80
    b       \target
.endm

.balign 0x800
.global __vectors
__vectors:
    VENT    __exc_sync      // current EL, SP0
    VENT    __exc_irq
    VENT    __exc_other
    VENT    __exc_other
    VENT    __exc_sync      // current EL, SPx
    VENT    __exc_irq
    VENT    __exc_other
    VENT    __exc_other
    VENT    __exc_other     // lower EL, AArch64
    VENT    __exc_other
    VENT    __exc_other
    VENT    __exc_other
    VENT    __exc_other     // lower EL, AArch32
    VENT    __exc_other
    VENT    __exc_other
    VENT    __exc_other

// frame: x0..x30 @0, elr @248, spsr @256, q0..q31 @272 — 784 bytes
.macro SAVE
    sub     sp, sp, #784
    stp     x0,  x1,  [sp, #0]
    stp     x2,  x3,  [sp, #16]
    stp     x4,  x5,  [sp, #32]
    stp     x6,  x7,  [sp, #48]
    stp     x8,  x9,  [sp, #64]
    stp     x10, x11, [sp, #80]
    stp     x12, x13, [sp, #96]
    stp     x14, x15, [sp, #112]
    stp     x16, x17, [sp, #128]
    stp     x18, x19, [sp, #144]
    stp     x20, x21, [sp, #160]
    stp     x22, x23, [sp, #176]
    stp     x24, x25, [sp, #192]
    stp     x26, x27, [sp, #208]
    stp     x28, x29, [sp, #224]
    str     x30,      [sp, #240]
    mrs     x0, elr_el1
    mrs     x1, spsr_el1
    stp     x0, x1,   [sp, #248]
    add     x0, sp, #272
    stp     q0,  q1,  [x0, #0]
    stp     q2,  q3,  [x0, #32]
    stp     q4,  q5,  [x0, #64]
    stp     q6,  q7,  [x0, #96]
    stp     q8,  q9,  [x0, #128]
    stp     q10, q11, [x0, #160]
    stp     q12, q13, [x0, #192]
    stp     q14, q15, [x0, #224]
    stp     q16, q17, [x0, #256]
    stp     q18, q19, [x0, #288]
    stp     q20, q21, [x0, #320]
    stp     q22, q23, [x0, #352]
    stp     q24, q25, [x0, #384]
    stp     q26, q27, [x0, #416]
    stp     q28, q29, [x0, #448]
    stp     q30, q31, [x0, #480]
.endm

.macro RESTORE
    add     x0, sp, #272
    ldp     q0,  q1,  [x0, #0]
    ldp     q2,  q3,  [x0, #32]
    ldp     q4,  q5,  [x0, #64]
    ldp     q6,  q7,  [x0, #96]
    ldp     q8,  q9,  [x0, #128]
    ldp     q10, q11, [x0, #160]
    ldp     q12, q13, [x0, #192]
    ldp     q14, q15, [x0, #224]
    ldp     q16, q17, [x0, #256]
    ldp     q18, q19, [x0, #288]
    ldp     q20, q21, [x0, #320]
    ldp     q22, q23, [x0, #352]
    ldp     q24, q25, [x0, #384]
    ldp     q26, q27, [x0, #416]
    ldp     q28, q29, [x0, #448]
    ldp     q30, q31, [x0, #480]
    ldp     x0, x1,   [sp, #248]
    msr     elr_el1, x0
    msr     spsr_el1, x1
    ldp     x0,  x1,  [sp, #0]
    ldp     x2,  x3,  [sp, #16]
    ldp     x4,  x5,  [sp, #32]
    ldp     x6,  x7,  [sp, #48]
    ldp     x8,  x9,  [sp, #64]
    ldp     x10, x11, [sp, #80]
    ldp     x12, x13, [sp, #96]
    ldp     x14, x15, [sp, #112]
    ldp     x16, x17, [sp, #128]
    ldp     x18, x19, [sp, #144]
    ldp     x20, x21, [sp, #160]
    ldp     x22, x23, [sp, #176]
    ldp     x24, x25, [sp, #192]
    ldp     x26, x27, [sp, #208]
    ldp     x28, x29, [sp, #224]
    ldr     x30,      [sp, #240]
    add     sp, sp, #784
.endm

__exc_irq:
    SAVE
    bl      irq_handler
    RESTORE
    eret

__exc_sync:
    SAVE
    mov     x0, sp
    bl      sync_handler
    RESTORE
    eret

__exc_other:
    SAVE
    mov     x0, sp
    bl      other_handler
    RESTORE
    eret
"#
);
