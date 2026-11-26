//! Isolated two-vCPU acceptance for polled, acknowledged page invalidation.
//! Static backing stays owned until both CPUs halt, before PMM startup.
//! This is not a normal-boot AP scheduler, IPI handler or frame recycler.
use core::{ptr,sync::atomic::{AtomicBool,AtomicU64,Ordering}};
const PHYS:u64=0x000f_ffff_ffff_f000;
const FLAGS:u64=3|(1<<63);
const A:u64=0x132a_132a_132a_132a;
const B:u64=0x132b_132b_132b_132b;
#[repr(C,align(4096))]
struct Frame([u64;512]);
static mut TABLES:[Frame;3]=[const{Frame([0;512])};3];
static mut DATA:[Frame;2]=[Frame([A;512]),Frame([B;512])];
#[derive(Clone,Copy)]
struct Probe{root:u64,alias:u64,leaf:*mut u64,pages:[u64;2]}
static mut PROBE:Probe=Probe{root:0,alias:0,leaf:ptr::null_mut(),pages:[0;2]};
static READY:AtomicBool=AtomicBool::new(false);
static CHANGED:AtomicU64=AtomicU64::new(0);
static STALE:AtomicBool=AtomicBool::new(false);
pub(crate) fn stale_observed()->bool{STALE.load(Ordering::Acquire)}

unsafe fn physical(root:u64,address:*mut u64)->Option<u64>{
 let (page,flags)=unsafe{crate::paging_cpu::mapped_physical(root,address as u64)}?;
 (page!=0&&page&!PHYS==0&&flags&7==3).then_some(page)
}
fn expected(sequence:u64)->u64{if sequence&1==0{A}else{B}}

/// BSP only; before any AP begins, boot mappings and all static pages live.
pub(crate) unsafe fn prepare()->bool{
 if READY.load(Ordering::Acquire){return false;}
 let mut cpu=crate::paging::CpuState::default();let mut offset=0;
 if !unsafe{crate::paging_cpu::x86_64_paging_detect(&mut cpu)}||
    !unsafe{crate::paging::paging_preflight(&cpu)}||
    !unsafe{crate::boot::boot_hhdm_offset(&mut offset)}{return false;}
 let root=cpu.cr3&PHYS;
 let Some(root_table)=crate::boot::direct_map_frame(offset,root)else{return false;};
 let root_table=root_table.cast::<u64>();
 let Some(slot)=(256..512).rev().find(|&i|unsafe{ptr::read_volatile(root_table.add(i))}==0)else{return false;};
 let mut tables=[ptr::null_mut();3];let mut frames=[0u64;5];
 for i in 0..3{
  tables[i]=unsafe{ptr::addr_of_mut!(TABLES).cast::<Frame>().add(i).cast::<u64>()};
  let Some(frame)=(unsafe{physical(root,tables[i])})else{return false;};frames[i]=frame;
 }
 for i in 0..2{
  let data=unsafe{ptr::addr_of_mut!(DATA).cast::<Frame>().add(i).cast::<u64>()};
  let Some(frame)=(unsafe{physical(root,data)})else{return false;};frames[i+3]=frame;
 }
 if frames.iter().enumerate().any(|(i,f)|*f==root||frames[..i].contains(f)){return false;}
 for table in tables{for i in 0..512{unsafe{ptr::write_volatile(table.add(i),0)}}}
 unsafe{
  ptr::write_volatile(tables[0],frames[1]|3);
  ptr::write_volatile(tables[1],frames[2]|3);
  ptr::write_volatile(tables[2],frames[3]|FLAGS);
 }
 let alias=0xffff_0000_0000_0000|((slot as u64)<<39);
 if unsafe{ptr::read_volatile(root_table.add(slot))}!=0{return false;}
 unsafe{
  ptr::write_volatile(root_table.add(slot),frames[0]|3);
  ptr::write(ptr::addr_of_mut!(PROBE),Probe{root,alias,leaf:tables[2],pages:[frames[3],frames[4]]});
 }
 if !unsafe{crate::tlb::invalidate_page(root,alias)}{return false;}
 READY.store(true,Ordering::Release);true
}
fn probe()->Option<Probe>{
 READY.load(Ordering::Acquire).then(||unsafe{ptr::read(ptr::addr_of!(PROBE))})
}
/// AP primes a real cached translation before the first BSP remap.
pub(crate) unsafe fn prime()->bool{
 let Some(p)=probe()else{return false;};
 unsafe{crate::tlb::invalidate_page(p.root,p.alias)&&ptr::read_volatile(p.alias as *const u64)==A}
}
/// BSP only, after the previous lifecycle receipt has been acknowledged.
pub(crate) unsafe fn remap(sequence:u64)->bool{
 if !(1..=64).contains(&sequence)||CHANGED.load(Ordering::Acquire)!=sequence-1{return false;}
 let Some(p)=probe()else{return false;};
 unsafe{ptr::write_volatile(p.leaf,p.pages[(sequence&1) as usize]|FLAGS)};
 CHANGED.store(sequence,Ordering::Release);true
}
/// AP local work, before the lifecycle fence publishes acknowledgement.
pub(crate) unsafe fn observe(sequence:u64)->bool{
 let Some(p)=probe()else{return false;};
 if CHANGED.load(Ordering::Acquire)!=sequence{return false;}
 #[cfg(not(feature="test-r132-tlb-stale"))]
 if !unsafe{crate::tlb::invalidate_page(p.root,p.alias)}{return false;}
 // The negative fixture alone deliberately omits INVLPG. A stale read must
 // refuse acknowledgement; the BSP cannot publish its success receipt.
 let value=unsafe{ptr::read_volatile(p.alias as *const u64)};
 if sequence!=0&&value==expected(sequence-1){STALE.store(true,Ordering::Release);}
 value==expected(sequence)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn alternating_remaps_never_expect_the_previous_page(){
  assert_eq!(expected(0),A);
  for sequence in 1..=64{assert_ne!(expected(sequence),expected(sequence-1));}
 }
 #[test]fn proof_backing_is_page_aligned_and_exactly_one_page_per_frame(){
  assert_eq!(core::mem::size_of::<Frame>(),4096);
  assert_eq!(core::mem::align_of::<Frame>(),4096);
 }
}
