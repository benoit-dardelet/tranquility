//! Entry point.
//!
//! The kernel is an arm64 Linux-style `Image`: a 64-byte header lets
//! QEMU and Android bootloaders (ABL, U-Boot…) load it like Linux, place
//! it anywhere, and pass a device tree in x0. The image is linked at 0 as
//! a position-independent executable and applies its own
//! R_AARCH64_RELATIVE relocations before touching any absolute address.
//!
//! Boot: park secondary cores → drop EL2→EL1 if needed → relocate →
//! stack, FP, BSS → identity-map all 512 GiB as device memory (XN) with the
//! kernel's own GiB as normal cacheable memory → MMU on → `kmain(dtb)`.
use core::arch::global_asm;

global_asm!(
    r#"
.section .text.boot, "ax"
.global _start
_start:
    b       __real_start            // code0
    .long   0                       // code1
    .quad   0x80000                 // text_offset (traditional; we run anywhere)
    .quad   __image_size            // effective image size incl. BSS + stack
    .quad   0xA                     // flags: LE, 4K pages, place anywhere
    .quad   0
    .quad   0
    .quad   0
    .ascii  "ARM\x64"               // magic
    .long   0

__real_start:
    mov     x21, x0                 // device tree blob
    mrs     x0, mpidr_el1
    and     x0, x0, #0xFF
    cbz     x0, 2f
1:  wfe
    b       1b
2:
    mrs     x0, CurrentEL
    lsr     x0, x0, #2
    cmp     x0, #2
    b.ne    3f
    // EL2 -> EL1 (AArch64), give EL1 the timers
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
    // ---- self-relocation (image linked at 0)
    adr     x22, _start
    adrp    x1, __rela_start
    add     x1, x1, :lo12:__rela_start
    adrp    x2, __rela_end
    add     x2, x2, :lo12:__rela_end
4:  cmp     x1, x2
    b.hs    5f
    ldp     x3, x4, [x1]            // r_offset, r_info
    ldr     x5, [x1, #16]           // r_addend
    add     x1, x1, #24
    and     x4, x4, #0xFFFFFFFF
    cmp     x4, #1027               // R_AARCH64_RELATIVE
    b.ne    4b
    add     x5, x5, x22
    str     x5, [x22, x3]
    b       4b
5:
    adrp    x0, __stack_top
    add     x0, x0, :lo12:__stack_top
    mov     sp, x0
    mov     x0, #(3 << 20)          // FP/SIMD on
    msr     cpacr_el1, x0
    isb
    adrp    x0, __bss_start
    add     x0, x0, :lo12:__bss_start
    adrp    x1, __bss_end
    add     x1, x1, :lo12:__bss_end
6:  cmp     x0, x1
    b.hs    7f
    str     xzr, [x0], #8
    b       6b
7:
    adrp    x0, __vectors
    add     x0, x0, :lo12:__vectors
    msr     vbar_el1, x0

    // ---- MMU: 512 x 1 GiB L1 blocks (4K granule, 39-bit VA), identity.
    adrp    x0, __l1_table
    add     x0, x0, :lo12:__l1_table
    ldr     x2, =0x0060000000000401     // device nGnRnE, AF, PXN|UXN
    mov     x1, #0
8:  lsl     x3, x1, #30
    orr     x3, x3, x2
    str     x3, [x0, x1, lsl #3]
    add     x1, x1, #1
    cmp     x1, #512
    b.lo    8b
    // the GiB holding the kernel: normal write-back, inner shareable
    lsr     x1, x22, #30
    lsl     x3, x1, #30
    mov     x2, #0x705
    orr     x3, x3, x2
    str     x3, [x0, x1, lsl #3]
    // the GiB holding the DTB too (it's in RAM)
    lsr     x1, x21, #30
    lsl     x3, x1, #30
    orr     x3, x3, x2
    cbz     x21, 9f
    str     x3, [x0, x1, lsl #3]
9:
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
    orr     x1, x1, #(1 << 0)
    orr     x1, x1, #(1 << 2)
    orr     x1, x1, #(1 << 12)
    bic     x1, x1, #(1 << 1)
    bic     x1, x1, #(1 << 19)          // no WXN
    msr     sctlr_el1, x1
    isb

    mov     x0, x21
    mov     x1, x22
    bl      kmain
10: wfe
    b       10b

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
    VENT    __exc_sync
    VENT    __exc_irq
    VENT    __exc_other
    VENT    __exc_other
    VENT    __exc_sync
    VENT    __exc_irq
    VENT    __exc_other
    VENT    __exc_other
    VENT    __exc_other
    VENT    __exc_other
    VENT    __exc_other
    VENT    __exc_other
    VENT    __exc_other
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
