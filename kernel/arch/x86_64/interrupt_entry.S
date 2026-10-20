/* Minimal terminal CPU exception entry stubs; no user mode or interrupt returns.
 * CPU pushes error code for #DF, #TS, #NP, #SS, #GP, #PF, #AC, #CP,
 * #VC and #SX. Every stub produces [vector,error,rip,cs,rflags]. */
.text
.extern x86_64_exception_dispatch
.extern x86_64_load_gdt

.global x86_64_load_gdt
.type x86_64_load_gdt, @function
x86_64_load_gdt:
    lgdt (%rdi)
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    pushq $0x08
    leaq 1f(%rip), %rax
    pushq %rax
    lretq
1:
    retq
.size x86_64_load_gdt, . - x86_64_load_gdt

.type x86_64_isr_common, @function
x86_64_isr_common:
    cli
    movq %rsp, %rdi
    andq $-16, %rsp
    call x86_64_exception_dispatch
2:
    cli
    hlt
    jmp 2b
.size x86_64_isr_common, . - x86_64_isr_common

.global x86_64_isr_0
.type x86_64_isr_0, @function
x86_64_isr_0:
    pushq $0
    pushq $0
    jmp x86_64_isr_common
.size x86_64_isr_0, . - x86_64_isr_0

.global x86_64_isr_1
.type x86_64_isr_1, @function
x86_64_isr_1:
    pushq $0
    pushq $1
    jmp x86_64_isr_common
.size x86_64_isr_1, . - x86_64_isr_1

.global x86_64_isr_2
.type x86_64_isr_2, @function
x86_64_isr_2:
    pushq $0
    pushq $2
    jmp x86_64_isr_common
.size x86_64_isr_2, . - x86_64_isr_2

.global x86_64_isr_3
.type x86_64_isr_3, @function
x86_64_isr_3:
    pushq $0
    pushq $3
    jmp x86_64_isr_common
.size x86_64_isr_3, . - x86_64_isr_3

.global x86_64_isr_4
.type x86_64_isr_4, @function
x86_64_isr_4:
    pushq $0
    pushq $4
    jmp x86_64_isr_common
.size x86_64_isr_4, . - x86_64_isr_4

.global x86_64_isr_5
.type x86_64_isr_5, @function
x86_64_isr_5:
    pushq $0
    pushq $5
    jmp x86_64_isr_common
.size x86_64_isr_5, . - x86_64_isr_5

.global x86_64_isr_6
.type x86_64_isr_6, @function
x86_64_isr_6:
    pushq $0
    pushq $6
    jmp x86_64_isr_common
.size x86_64_isr_6, . - x86_64_isr_6

.global x86_64_isr_7
.type x86_64_isr_7, @function
x86_64_isr_7:
    pushq $0
    pushq $7
    jmp x86_64_isr_common
.size x86_64_isr_7, . - x86_64_isr_7

.global x86_64_isr_8
.type x86_64_isr_8, @function
x86_64_isr_8:
    pushq $8
    jmp x86_64_isr_common
.size x86_64_isr_8, . - x86_64_isr_8

.global x86_64_isr_9
.type x86_64_isr_9, @function
x86_64_isr_9:
    pushq $0
    pushq $9
    jmp x86_64_isr_common
.size x86_64_isr_9, . - x86_64_isr_9

.global x86_64_isr_10
.type x86_64_isr_10, @function
x86_64_isr_10:
    pushq $10
    jmp x86_64_isr_common
.size x86_64_isr_10, . - x86_64_isr_10

.global x86_64_isr_11
.type x86_64_isr_11, @function
x86_64_isr_11:
    pushq $11
    jmp x86_64_isr_common
.size x86_64_isr_11, . - x86_64_isr_11

.global x86_64_isr_12
.type x86_64_isr_12, @function
x86_64_isr_12:
    pushq $12
    jmp x86_64_isr_common
.size x86_64_isr_12, . - x86_64_isr_12

.global x86_64_isr_13
.type x86_64_isr_13, @function
x86_64_isr_13:
    pushq $13
    jmp x86_64_isr_common
.size x86_64_isr_13, . - x86_64_isr_13

.global x86_64_isr_14
.type x86_64_isr_14, @function
x86_64_isr_14:
    pushq $14
    jmp x86_64_isr_common
.size x86_64_isr_14, . - x86_64_isr_14

.global x86_64_isr_15
.type x86_64_isr_15, @function
x86_64_isr_15:
    pushq $0
    pushq $15
    jmp x86_64_isr_common
.size x86_64_isr_15, . - x86_64_isr_15

.global x86_64_isr_16
.type x86_64_isr_16, @function
x86_64_isr_16:
    pushq $0
    pushq $16
    jmp x86_64_isr_common
.size x86_64_isr_16, . - x86_64_isr_16

.global x86_64_isr_17
.type x86_64_isr_17, @function
x86_64_isr_17:
    pushq $17
    jmp x86_64_isr_common
.size x86_64_isr_17, . - x86_64_isr_17

.global x86_64_isr_18
.type x86_64_isr_18, @function
x86_64_isr_18:
    pushq $0
    pushq $18
    jmp x86_64_isr_common
.size x86_64_isr_18, . - x86_64_isr_18

.global x86_64_isr_19
.type x86_64_isr_19, @function
x86_64_isr_19:
    pushq $0
    pushq $19
    jmp x86_64_isr_common
.size x86_64_isr_19, . - x86_64_isr_19

.global x86_64_isr_20
.type x86_64_isr_20, @function
x86_64_isr_20:
    pushq $0
    pushq $20
    jmp x86_64_isr_common
.size x86_64_isr_20, . - x86_64_isr_20

.global x86_64_isr_21
.type x86_64_isr_21, @function
x86_64_isr_21:
    pushq $21
    jmp x86_64_isr_common
.size x86_64_isr_21, . - x86_64_isr_21

.global x86_64_isr_22
.type x86_64_isr_22, @function
x86_64_isr_22:
    pushq $0
    pushq $22
    jmp x86_64_isr_common
.size x86_64_isr_22, . - x86_64_isr_22

.global x86_64_isr_23
.type x86_64_isr_23, @function
x86_64_isr_23:
    pushq $0
    pushq $23
    jmp x86_64_isr_common
.size x86_64_isr_23, . - x86_64_isr_23

.global x86_64_isr_24
.type x86_64_isr_24, @function
x86_64_isr_24:
    pushq $0
    pushq $24
    jmp x86_64_isr_common
.size x86_64_isr_24, . - x86_64_isr_24

.global x86_64_isr_25
.type x86_64_isr_25, @function
x86_64_isr_25:
    pushq $0
    pushq $25
    jmp x86_64_isr_common
.size x86_64_isr_25, . - x86_64_isr_25

.global x86_64_isr_26
.type x86_64_isr_26, @function
x86_64_isr_26:
    pushq $0
    pushq $26
    jmp x86_64_isr_common
.size x86_64_isr_26, . - x86_64_isr_26

.global x86_64_isr_27
.type x86_64_isr_27, @function
x86_64_isr_27:
    pushq $0
    pushq $27
    jmp x86_64_isr_common
.size x86_64_isr_27, . - x86_64_isr_27

.global x86_64_isr_28
.type x86_64_isr_28, @function
x86_64_isr_28:
    pushq $0
    pushq $28
    jmp x86_64_isr_common
.size x86_64_isr_28, . - x86_64_isr_28

.global x86_64_isr_29
.type x86_64_isr_29, @function
x86_64_isr_29:
    pushq $29
    jmp x86_64_isr_common
.size x86_64_isr_29, . - x86_64_isr_29

.global x86_64_isr_30
.type x86_64_isr_30, @function
x86_64_isr_30:
    pushq $30
    jmp x86_64_isr_common
.size x86_64_isr_30, . - x86_64_isr_30

.global x86_64_isr_31
.type x86_64_isr_31, @function
x86_64_isr_31:
    pushq $0
    pushq $31
    jmp x86_64_isr_common
.size x86_64_isr_31, . - x86_64_isr_31

/* Should never fire in M002: external IRQs remain masked/disabled. */
.global x86_64_unexpected_interrupt
.type x86_64_unexpected_interrupt, @function
x86_64_unexpected_interrupt:
    pushq $0
    pushq $255
    jmp x86_64_isr_common
.size x86_64_unexpected_interrupt, . - x86_64_unexpected_interrupt

.section .rodata, "a", @progbits
.align 8
.global x86_64_exception_stubs
x86_64_exception_stubs:
    .quad x86_64_isr_0
    .quad x86_64_isr_1
    .quad x86_64_isr_2
    .quad x86_64_isr_3
    .quad x86_64_isr_4
    .quad x86_64_isr_5
    .quad x86_64_isr_6
    .quad x86_64_isr_7
    .quad x86_64_isr_8
    .quad x86_64_isr_9
    .quad x86_64_isr_10
    .quad x86_64_isr_11
    .quad x86_64_isr_12
    .quad x86_64_isr_13
    .quad x86_64_isr_14
    .quad x86_64_isr_15
    .quad x86_64_isr_16
    .quad x86_64_isr_17
    .quad x86_64_isr_18
    .quad x86_64_isr_19
    .quad x86_64_isr_20
    .quad x86_64_isr_21
    .quad x86_64_isr_22
    .quad x86_64_isr_23
    .quad x86_64_isr_24
    .quad x86_64_isr_25
    .quad x86_64_isr_26
    .quad x86_64_isr_27
    .quad x86_64_isr_28
    .quad x86_64_isr_29
    .quad x86_64_isr_30
    .quad x86_64_isr_31

.section .note.GNU-stack,"",@progbits
