//! Local page invalidation under an exact address-space/IRQ ownership contract.
//! Remote completion must additionally be acknowledged by every participating CPU.
const PHYS:u64=0x000f_ffff_ffff_f000;
fn admitted(root:u64,current:u64,flags:u64,address:u64)->bool{
 root!=0&&root&!PHYS==0&&current&PHYS==root&&flags&(1<<9)==0&&
 address&4095==0&&if address&(1<<47)==0{address>>48==0}else{address>>48==0xffff}
}
/// # Safety: CPL0; caller owns the mapping and serializes page-table writes.
/// Invalidates this CPU/current PCID only. It is not a remote shootdown or
/// permission to recycle frames still referenced by another CPU.
pub(crate) unsafe fn invalidate_page(root:u64,address:u64)->bool{
 let current:u64;let flags:u64;
 unsafe{
  core::arch::asm!("mov {}, cr3",out(reg)current,options(nostack,nomem,preserves_flags));
  core::arch::asm!("pushfq","pop {}",out(reg)flags,options(preserves_flags));
 }
 if !admitted(root,current,flags,address){return false;}
 unsafe{core::arch::asm!("invlpg [{}]",in(reg)address,options(nostack,preserves_flags));}
 true
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn invalidation_admission_requires_current_root_and_masked_interrupts(){
  assert!(admitted(0x1000,0x1000,2,0x400000));
  assert!(admitted(0x1000,0x1007,2,0xffff800000000000));
  for (root,current,flags) in [(0,0,2),(0x1001,0x1000,2),(0x1000,0x2000,2),
    (1<<52,1<<52,2),(0x1000,0x1000,0x202)]{
   assert!(!admitted(root,current,flags,0x400000));
  }
 }
 #[test]fn unaligned_and_noncanonical_aliases_are_refused(){
  for address in [1,0x400001,0x0000800000000000,0xffff000000000000]{
   assert!(!admitted(0x1000,0x1000,2,address));
  }
 }
}
