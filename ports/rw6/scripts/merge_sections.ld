SECTIONS
{
  . = 0x0;
  .text : {
    *(.text)
    *(.text.*)
    *(.glue_7)
    *(.glue_7t)
    *(.vfp11_veneer)
    *(.text._ZN*)
  }
  /* 2026-08-22: rodata 对齐 8 对齐 Canopus（其 .rodata align=8） */
  . = ALIGN(8);
  .rodata : {
    *(.rodata)
    *(.rodata.*)
    *(.rodata.str1.1)
    *(.rodata.cst8)
  }
  . = ALIGN(4);
  .data : {
    *(.data)
    *(.data.*)
  }
  . = ALIGN(4);
  .bss : {
    *(.bss)
    *(.bss.*)
    *(COMMON)
  }
  . = ALIGN(4);
  .init_array : {
    *(.init_array)
    *(.init_array.*)
  }
  . = ALIGN(4);
  .fini_array : {
    *(.fini_array)
    *(.fini_array.*)
  }
  .ARM.exidx : {
    *(.ARM.exidx)
    *(.ARM.exidx.*)
  }
  .ARM.extab : {
    *(.ARM.extab)
    *(.ARM.extab.*)
  }
  .note.GNU-stack : {
    *(.note.GNU-stack)
  }
  /DISCARD/ : {
    *(.debug*)
    *(.llvmbc)
    *(.llvmcmd)
    *(.comment)
  }
}