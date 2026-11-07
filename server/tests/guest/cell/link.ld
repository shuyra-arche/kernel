OUTPUT_ARCH(riscv)
ENTRY(_start)
PHDRS { code PT_LOAD FLAGS(5); data PT_LOAD FLAGS(6); }
SECTIONS {
    . = 0x10000;
    .text : { KEEP(*(.text.entry)) *(.text .text.*) *(.rodata .rodata.* .srodata .srodata.*) } :code
    . = ALIGN(4096);
    .data : { *(.data .data.* .sdata .sdata.*) } :data
    .bss : { *(.bss .bss.* .sbss .sbss.*) *(COMMON) } :data
    /DISCARD/ : { *(.eh_frame*) }
}
