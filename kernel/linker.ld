/* Position-independent arm64 Image, linked at 0 and relocated at boot. */
ENTRY(_start)

SECTIONS
{
    . = 0;
    .text : {
        KEEP(*(.text.boot))
        *(.text .text.*)
    }
    .rodata : ALIGN(16) { *(.rodata .rodata.*) }
    .data.rel.ro : ALIGN(16) { *(.data.rel.ro .data.rel.ro.*) }
    .dynamic : { *(.dynamic) }
    .got : ALIGN(8) { *(.got .got.*) }
    .data : ALIGN(16) { *(.data .data.*) }
    .rela.dyn : ALIGN(8) {
        __rela_start = .;
        *(.rela.dyn .rela.*)
        __rela_end = .;
    }
    .dynsym : { *(.dynsym) }
    .dynstr : { *(.dynstr) }
    .hash : { *(.hash) }
    .gnu.hash : { *(.gnu.hash) }

    . = ALIGN(4096);
    __file_end = .;

    .bss (NOLOAD) : ALIGN(4096) {
        __bss_start = .;
        *(.bss.pagetable)
        *(.bss .bss.* COMMON)
        . = ALIGN(16);
        __bss_end = .;
    }

    . = ALIGN(16);
    . = . + 0x100000;          /* 1 MiB kernel stack */
    __stack_top = .;
    . = ALIGN(4096);
    __kernel_end = .;
    __image_size = __kernel_end - _start;

    /DISCARD/ : {
        *(.comment)
        *(.eh_frame*)
        *(.interp)
    }
}
