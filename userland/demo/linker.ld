OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)
PHDRS { text PT_LOAD FLAGS(0x05); }
SECTIONS {
    . = 0x0000000000400000;
    .text : { *(.text .text.*) } :text
    .rodata : { *(.rodata .rodata.*) } :text
    /DISCARD/ : { *(.eh_frame*) *(.note*) *(.comment*) *(.interp) *(.dynamic) }
}
