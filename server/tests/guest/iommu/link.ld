OUTPUT_ARCH(riscv)
ENTRY(_start)
SECTIONS {
  . = 0x80000000;
  .text : { KEEP(*(.text.entry)) *(.text .text.*) }
  .rodata : ALIGN(4096) { *(.rodata .rodata.*) }
  .data : ALIGN(4096) { *(.data .data.*) *(.sdata .sdata.*) }
  PROVIDE(__global_pointer$ = . + 0x800);
  .bss : ALIGN(4096) { *(.bss .bss.*) *(.sbss .sbss.*) *(COMMON) }
  . = ALIGN(16);
  . += 65536;
  _stack_top = .;
  /DISCARD/ : { *(.eh_frame*) }
}
