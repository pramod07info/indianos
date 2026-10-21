//! Rust-owned x86-64 paging hardware inspection and opt-in shadow CR3 test.
//! BSP/ring0 only; calls are forbidden from concurrent/interrupt contexts.
use core::{arch::{asm,x86_64::__cpuid_count},ffi::c_void,ptr};
use crate::boot::Region;
use crate::paging::{Backend,CloneResult,CpuState,paging_preflight,paging_clone_root,paging_clone_release,paging_lookup};
const PHYS:u64=0x000f_ffff_ffff_f000;
const PAGE:u64=4096;
unsafe extern "C" {
 fn boot_memory_region_count()->u64;
 fn boot_memory_region_at(i:u64,out:*mut Region,context:*mut c_void)->bool;
 fn boot_hhdm_offset(out:*mut u64)->bool;
 fn pmm_allocate_frame(out:*mut u64)->bool;
 fn pmm_release_frame(frame:u64)->bool;
 fn pmm_free_frames()->u64;
}
#[no_mangle]
pub unsafe extern "C" fn x86_64_paging_detect(out:*mut CpuState)->bool {
 if out.is_null(){return false;}
 if unsafe{__cpuid_count(0,0).eax}<1{return false;}
 let standard=unsafe{__cpuid_count(1,0).edx};
 let msr=standard&(1<<5)!=0;
 let pae=standard&(1<<6)!=0;
 if !msr || unsafe{__cpuid_count(0x8000_0000,0).eax}<0x8000_0008{return false;}
 let extended=unsafe{__cpuid_count(0x8000_0001,0).edx};
 let bits=(unsafe{__cpuid_count(0x8000_0008,0).eax}&0xff) as u8;
 let cr0:u64;let cr3:u64;let cr4:u64;let lo:u32;let hi:u32;
 unsafe{
  asm!("mov {}, cr0",out(reg)cr0,options(nostack,nomem,preserves_flags));
  asm!("mov {}, cr3",out(reg)cr3,options(nostack,nomem,preserves_flags));
  asm!("mov {}, cr4",out(reg)cr4,options(nostack,nomem,preserves_flags));
  asm!("rdmsr",in("ecx")0xc000_0080u32,out("eax")lo,out("edx")hi,
      options(nostack,preserves_flags));
 }
 let efer=((hi as u64)<<32)|(lo as u64);
 unsafe{ptr::write(out,CpuState{
  long_mode:extended&(1<<29)!=0 && efer&(1<<10)!=0,
  nx:extended&(1<<20)!=0,msr,pae,
  paging_enabled:cr0&(1<<31)!=0,pae_enabled:cr4&(1<<5)!=0,
  la57_enabled:cr4&(1<<12)!=0,nxe_enabled:efer&(1<<11)!=0,
  write_protect_enabled:cr0&(1<<16)!=0,pcid_enabled:cr4&(1<<17)!=0,
  physical_bits:bits,cr3,
 });}
 true
}
#[repr(C)]struct DirectMap{offset:u64}
fn in_region(frame:u64,r:&Region)->Option<bool>{
 let end=r.base.checked_add(r.length)?;
 Some(frame>=r.base&&frame.checked_add(PAGE).is_some_and(|e|e<=end)
      &&matches!(r.kind,0|5|6|8))
}
unsafe fn valid_frame(frame:u64)->bool{
 if frame==0||frame&(PAGE-1)!=0||frame.checked_add(PAGE).is_none(){return false;}
 let count=unsafe{boot_memory_region_count()};
 if count==0||count>4096{return false;}
 for i in 0..count{
  let mut r=Region{base:0,length:0,kind:0};
  if !unsafe{boot_memory_region_at(i,&mut r,ptr::null_mut())}{return false;}
  let Some(allowed)=in_region(frame,&r) else{return false};
  let Some(end)=r.base.checked_add(r.length) else{return false};
  if frame>=r.base&&frame.checked_add(PAGE).is_some_and(|e|e<=end){return allowed;}
 }
 false
}
unsafe extern "C" fn allocate(p:*mut u64,_:*mut c_void)->bool{
 unsafe{pmm_allocate_frame(p)}
}
unsafe extern "C" fn release(p:u64,_:*mut c_void)->bool{
 unsafe{pmm_release_frame(p)}
}
unsafe extern "C" fn access(p:u64,context:*mut c_void)->*mut u64{
 let Some(m)=(unsafe{(context as *const DirectMap).as_ref()})else{return ptr::null_mut()};
 if !unsafe{valid_frame(p)}{return ptr::null_mut();}
 let Some(addr)=p.checked_add(m.offset)else{return ptr::null_mut()};
 if addr>>48!=0xffff||addr&(1u64<<47)==0{return ptr::null_mut()};
 addr as usize as *mut u64
}
fn same(b:&Backend,old:u64,new:u64,va:u64)->bool{
 let(mut a,mut z,mut af,mut zf)=(0,0,0,0);
 unsafe{paging_lookup(b,old,va,&mut a,&mut af)&&
 paging_lookup(b,new,va,&mut z,&mut zf)&&a==z&&af==zf}
}
/// # Safety
/// QEMU-only: BSP ring0, masked IRQs, no SMP CR3 users; caller keeps
/// Limine root/HHDM alive. Original CR3 restored before freeing clone.
#[no_mangle]
pub unsafe extern "C" fn x86_64_paging_shadow_switch_test()->bool{
 let mut cpu=CpuState::default();let mut offset=0;
 if !unsafe{x86_64_paging_detect(&mut cpu)}
    ||!unsafe{paging_preflight(&cpu)}
    ||!unsafe{boot_hhdm_offset(&mut offset)}
    ||offset==0||cpu.pcid_enabled{return false;}
 let original=cpu.cr3&PHYS;
 let before=unsafe{pmm_free_frames()};
 let mut map=DirectMap{offset};
 let backend=Backend{
  allocate:Some(allocate),release:Some(release),access:Some(access),
  context:(&mut map as *mut DirectMap).cast(),
 };
 let mut copy=CloneResult{root:0,table_count:0};
 if !unsafe{paging_clone_root(&backend,original,&mut copy)}{return false;}
 let stack:u64;
 unsafe{asm!("mov {}, rsp",out(reg)stack,options(nomem,nostack,preserves_flags));}
 let safe=copy.root.checked_add(offset).is_some_and(|mapped|{
  same(&backend,original,copy.root,x86_64_paging_shadow_switch_test as usize as u64)&&
  same(&backend,original,copy.root,stack)&&
  same(&backend,original,copy.root,(&cpu as *const CpuState) as usize as u64)&&
  same(&backend,original,copy.root,mapped)
 });
 if !safe{
  let _=unsafe{paging_clone_release(&backend,&mut copy)};
  return false;
 }
 let new_cr3=(cpu.cr3&0xfff)|copy.root;
 unsafe{
  asm!("cli; mov cr3, {}",in(reg)new_cr3,options(nostack));
  asm!("mov cr3, {}",in(reg)(cpu.cr3&!(1u64<<63)),options(nostack));
 }
 unsafe{paging_clone_release(&backend,&mut copy)} && unsafe{pmm_free_frames()}==before
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn rejects_bad_frames_and_overflow(){
  let r=Region{base:4096,length:8192,kind:0};
  assert_eq!(in_region(4096,&r),Some(true));
  assert_eq!(in_region(12288,&r),Some(false));
  assert_eq!(in_region(4096,&Region{kind:1,..r}),Some(false));
  assert_eq!(in_region(4096,&Region{base:u64::MAX,..r}),None);
 }
}
