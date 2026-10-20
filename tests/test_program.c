/* Host-side tests for real kernel parsers; no application executes. */
#include <indianos/program.h>
#include <indianos/syscall_abi.h>
#include <assert.h>
#include <stdio.h>
#include <string.h>

static void w16(unsigned char *p, unsigned v) { p[0]=(unsigned char)v; p[1]=(unsigned char)(v>>8); }
static void w32(unsigned char *p, unsigned v) { for (unsigned i=0;i<4;++i) p[i]=(unsigned char)(v>>(i*8)); }
static void w64(unsigned char *p, uint64_t v) { for (unsigned i=0;i<8;++i) p[i]=(unsigned char)(v>>(i*8)); }
static void good_elf(unsigned char *elf, size_t size) {
    memset(elf,0,size); elf[0]=0x7f; elf[1]='E'; elf[2]='L'; elf[3]='F'; elf[4]=2; elf[5]=1; elf[6]=1;
    w16(elf+16,2); w16(elf+18,62); w32(elf+20,1); w64(elf+24,0x400100); w64(elf+32,64);
    w16(elf+52,64); w16(elf+54,56); w16(elf+56,1);
    w32(elf+64,1); w32(elf+68,5); w64(elf+72,0x100); w64(elf+80,0x400100);
    w64(elf+96,1); w64(elf+104,4096); w64(elf+112,4096); elf[0x100]=0xc3;
}
#define REJECT(change) do {good_elf(binary, sizeof binary); change; assert(!indianos_program_plan_elf64(binary,sizeof binary,&plan));} while (0)
int main(void) {
    unsigned char binary[0x200]; struct indianos_program_plan plan={0};
    assert(indianos_program_self_test()); good_elf(binary,sizeof binary);
    assert(indianos_program_plan_elf64(binary,sizeof binary,&plan));
    assert(plan.segment_count==1 && plan.entry==0x400100);
    assert(plan.segments[0].page_begin==0x400000 && plan.segments[0].page_end==0x402000);
    assert(plan.stack_end-plan.stack_begin == INDIANOS_USER_STACK_BYTES);
    assert(plan.guard_begin+INDIANOS_USER_PAGE_BYTES==plan.stack_begin);
    assert(indianos_program_user_range(&plan, 0x400100, 4096,false));
    assert(!indianos_program_user_range(&plan, 0x400100, 1,true));
    assert(indianos_program_user_range(&plan, plan.stack_begin, 256,true));
    assert(!indianos_program_user_range(&plan, plan.guard_begin, 1,false));
    assert(!indianos_program_user_range(&plan, plan.stack_end-10,11,true));
    assert(!indianos_program_plan_elf64(NULL, sizeof binary,&plan));
    assert(!indianos_program_plan_elf64(binary,63,&plan));
    assert(!indianos_program_plan_elf64(binary,sizeof binary,NULL));
    REJECT(binary[0]=0);
    REJECT(binary[4]=1);
    REJECT(binary[5]=2);
    REJECT(binary[7]=3);
    REJECT(w16(binary+16,3)); /* dynamic executable */
    REJECT(w16(binary+18,183)); /* ARM64 */
    REJECT(w16(binary+54,0));
    REJECT(w16(binary+56,0));
    REJECT(w16(binary+56,33));
    REJECT(w64(binary+32,UINT64_MAX));
    REJECT(w64(binary+24,0x400000)); /* entry outside executable bytes */
    REJECT(w64(binary+24,INDIANOS_USER_TOP));
    REJECT(w32(binary+68,7)); /* W+X */
    REJECT(w32(binary+68,1)); /* executable, unreadable */
    REJECT(w32(binary+64,3)); /* interpreter */
    REJECT(w32(binary+64,2)); /* dynamic */
    REJECT(w32(binary+64,7)); /* TLS */
    REJECT(w64(binary+96,500)); /* p_filesz out of image */
    REJECT(w64(binary+104,0));
    REJECT(w64(binary+104,0xffffffffffffffffull));
    REJECT(w64(binary+96,4097));
    REJECT(w64(binary+80,0x400000)); /* misaligned vs file */
    REJECT(w64(binary+104,INDIANOS_USER_TOP));
    REJECT(w64(binary+112,3)); /* not power of 2 */
    REJECT(w64(binary+72,0x200)); /* offset at EOF */
    REJECT(w64(binary+80,plan.guard_begin));
    REJECT((w16(binary+56,2),w32(binary+120,1),w32(binary+124,6),w64(binary+136,0x400200),w64(binary+128,0x200),w64(binary+160,0x1000),w64(binary+152,0))); /* invalid second */
    good_elf(binary,sizeof binary);
    assert(indianos_program_plan_elf64(binary,sizeof binary,&plan));
    assert(indianos_syscall_validate(&plan,(struct indianos_syscall_request){0,0,0}));
    assert(indianos_syscall_validate(&plan,(struct indianos_syscall_request){1,0x400100,1}));
    assert(!indianos_syscall_validate(&plan,(struct indianos_syscall_request){1,0x400100,257}));
    assert(!indianos_syscall_validate(&plan,(struct indianos_syscall_request){1,plan.guard_begin,1}));
    assert(!indianos_syscall_validate(&plan,(struct indianos_syscall_request){1,UINT64_MAX,2}));
    assert(indianos_syscall_validate(&plan,(struct indianos_syscall_request){2,42,0}));
    assert(!indianos_syscall_validate(&plan,(struct indianos_syscall_request){333,0,0}));
    assert(!indianos_syscall_validate(NULL,(struct indianos_syscall_request){0,0,0}));
    puts("IndianOS M005 ELF64 and syscall ABI host tests passed");
    return 0;
}
