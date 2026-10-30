/* tun_module.ld — 把 clang 的 section 名归一成 NuttX modlib 期望的名字。
 *
 * 与 Shell++ 的 Shellpp-ii-build/shellpp_ii.ld 同构，差别只有一处：
 * .ARM.exidx 直接丢进 /DISCARD/（模块用 -fno-unwind-tables 编译，
 * 正常不会生成；万一生成也不让它进模块，省 ~1.4 KB 并避免
 * loader 找不到 unwind personality 符号）。
 *
 * .init_array 必须 KEEP —— 构造函数靠它被 modlib 调用。
 */

SECTIONS
{
    .text : ALIGN(4)
    {
        *(.text)
        *(.text.*)
    }

    .rodata : ALIGN(4)
    {
        *(.rodata)
        *(.rodata.*)
    }

    .data : ALIGN(4)
    {
        *(.data)
        *(.data.*)
    }

    .bss : ALIGN(4)
    {
        *(.bss)
        *(.bss.*)
        *(COMMON)
    }

    .init_array : ALIGN(4)
    {
        KEEP(*(.init_array))
        KEEP(*(.init_array.*))
    }

    .fini_array : ALIGN(4)
    {
        KEEP(*(.fini_array))
        KEEP(*(.fini_array.*))
    }

    /DISCARD/ :
    {
        *(.ARM.exidx*)
        *(.ARM.extab*)
        *(.comment)
        *(.llvm_addrsig)
        *(.note.GNU-stack)
    }
}
