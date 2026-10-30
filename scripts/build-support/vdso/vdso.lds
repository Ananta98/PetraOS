/* SPDX-License-Identifier: GPL-2.0 */
OUTPUT_FORMAT("elf64-x86-64", "elf64-x86-64", "elf64-x86-64")
OUTPUT_ARCH(i386:x86-64)

PHDRS {
    text PT_LOAD FILEHDR PHDRS FLAGS(5); /* PF_R | PF_X */
    dynamic PT_DYNAMIC FLAGS(4);         /* PF_R */
    eh_frame_hdr PT_GNU_EH_FRAME FLAGS(4); /* PF_R */
}

SECTIONS {
    . = SIZEOF_HEADERS;

    .hash : { *(.hash) } :text
    .gnu.hash : { *(.gnu.hash) } :text
    .dynsym : { *(.dynsym) } :text
    .dynstr : { *(.dynstr) } :text
    .gnu.version : { *(.gnu.version) } :text
    .gnu.version_d : { *(.gnu.version_d) } :text
    .gnu.version_r : { *(.gnu.version_r) } :text

    .eh_frame_hdr : { *(.eh_frame_hdr) } :text :eh_frame_hdr
    .eh_frame : { KEEP (*(.eh_frame)) } :text

    .dynamic : { *(.dynamic) } :text :dynamic

    .rodata : { *(.rodata*) } :text
    .text : { *(.text*) } :text

    /DISCARD/ : {
        *(.note*)
        *(.comment*)
        *(.data*)
        *(.bss*)
    }
}
