//! Opt-in guest acceptance fixture; excluded from ordinary desktop images.
use core::arch::global_asm;
global_asm!(r#"
.text
.global desktop_fpu_probe
.type desktop_fpu_probe,@function
desktop_fpu_probe:
    subq $72, %rsp
    fnstcw 0(%rsp)
    stmxcsr 4(%rsp)
    movq %rdi, 8(%rsp)
    movl %edi, %eax
    andl $3, %eax
    shll $10, %eax
    orl $0x37f, %eax
    movw %ax, 24(%rsp)
    movl %edi, %eax
    andl $3, %eax
    shll $13, %eax
    orl $0x1f80, %eax
    movl %eax, 28(%rsp)
    fninit
    fldcw 24(%rsp)
    ldmxcsr 28(%rsp)
    fildq 8(%rsp)
    movq %rdi, %xmm0
    movq %rdi, %xmm15
    /* Two actual absent-stack faults while x87/XMM state is live. */
    movabsq $0x00007fffefff0000, %rax
    cmpq $0, 4096(%rax)
    jne 4f
    movq %rdi, 4096(%rax)  /* protection fault must split the shared zero */
    cmpq %rdi, 4096(%rax)
    jne 4f
    cmpq $0, 8192(%rax)    /* another shared alias must remain immutable */
    jne 4f
    movq %rdi, (%rax)
    cmpq %rdi, (%rax)
    jne 4f
    movl $4, 32(%rsp)
2:
    movl $3, %eax
    xorl %edi, %edi
    xorl %esi, %esi
    int $0x80
    testq %rax, %rax
    jnz 4f
    movabsq $0x00007fffefff0000, %rax
    movq 8(%rsp), %rcx
    cmpq %rcx, 4096(%rax)
    jne 4f
    cmpq $0, 8192(%rax)
    jne 4f
    movq %xmm0, %rax
    cmpq 8(%rsp), %rax
    jne 4f
    movq %xmm15, %rax
    cmpq 8(%rsp), %rax
    jne 4f
    fistpq 16(%rsp)
    fildq 16(%rsp)
    movq 16(%rsp), %rax
    cmpq 8(%rsp), %rax
    jne 4f
    fnstcw 48(%rsp)
    movzwl 48(%rsp), %eax
    cmpw 24(%rsp), %ax
    jne 4f
    stmxcsr 52(%rsp)
    movl 52(%rsp), %eax
    cmpl 28(%rsp), %eax
    jne 4f
    decl 32(%rsp)
    jnz 2b
    movl $1, %eax
    jmp 5f
4:  xorl %eax, %eax
5:  fninit
    fldcw 0(%rsp)
    ldmxcsr 4(%rsp)
    addq $72, %rsp
    retq
.size desktop_fpu_probe, .-desktop_fpu_probe
"#,options(att_syntax));
unsafe extern "C" {fn desktop_fpu_probe(seed:u64)->u64;}
pub(super) fn verify(){
 let pid=super::call(4,0,0);
 if !(1..=4).contains(&pid)||unsafe{desktop_fpu_probe(pid)}!=1{super::exit(124);}
 super::write(match pid{
  1=>b"desktop PID1 x87 SSE context retained across yields",
  2=>b"desktop PID2 x87 SSE context retained across yields",
  3=>b"desktop PID3 x87 SSE context retained across yields",
  _=>b"desktop PID4 x87 SSE context retained across yields",
 });
}
