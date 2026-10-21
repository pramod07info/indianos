OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)
/* Keep executable text and mutable Rust statics in distinct page-aligned
   PT_LOAD segments. The kernel ELF loader rejects writable+executable pages. */
PHDRS {
    text PT_LOAD FLAGS(0x05);
    data PT_LOAD FLAGS(0x06);
}
SECTIONS {
    . = 0x0000000000400000;
    .text : { *(.text .text.*) } :text
    .rodata : { *(.rodata .rodata.*) } :text
    . = ALIGN(0x1000);
    .data : { *(.data .data.*) } :data
    .bss (NOLOAD) : { *(.bss .bss.*) *(COMMON) } :data
    /DISCARD/ : { *(.eh_frame*) *(.note*) *(.comment*) *(.interp) *(.dynamic) }
}
