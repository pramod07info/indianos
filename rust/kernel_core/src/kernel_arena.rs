//! BSP-only virtual backing for kernel buffers. All normal roots borrow the
//! direct-map subtree. Infrastructure is reserved once before session baselines;
//! data pages are independently owned PMM frames, not contiguous physical runs.
use core::ptr;
use crate::kernel_buffer::{Backing,Error};
const PAGE:u64=4096;
const PHYS:u64=0x000f_ffff_ffff_f000;
const FLAGS:u64=3|(1<<63); // present, writable, supervisor, NX, non-global
const VOLATILE:u64=(1<<5)|(1<<6);
const STRIDE:u64=16*1024*1024;
const PT_PER_SLOT:usize=8;
const MAX_PT:usize=128;
const MAX_TABLES:usize=MAX_PT+1;
fn frame_ok(frame:u64)->bool{frame!=0&&frame&4095==0&&frame&!PHYS==0}
/// # Safety: fresh frames are unique; table() maps a whole frame stably while
/// owned. Source root/direct-map tables stay live. Caller serializes all calls.
unsafe trait Memory{
 fn allocate(&mut self)->Option<u64>;
 fn release(&mut self,frame:u64)->bool;
 fn table(&mut self,frame:u64)->*mut u64;
 fn invalidate(&mut self,address:u64);
}
// Volatile stores keep zeroization observable before a physical frame is
// returned to another owner, including under whole-program optimization.
unsafe fn zero_frame(data:*mut u64){
 for word in 0..512{unsafe{ptr::write_volatile(data.add(word),0)};}
}
#[derive(Clone,Copy)]
struct Slot{key:u64,requested:usize,allocated:usize,mapped:usize,loaned:bool,scrubbed:bool}
impl Slot{const fn new()->Self{Self{key:0,requested:0,allocated:0,mapped:0,loaned:false,scrubbed:false}}}
struct Arena<const S:usize,const P:usize>{
 ready:bool,base:u64,parent:*mut u64,pd:*mut u64,
 tables:[u64;MAX_TABLES],table_count:usize,leaves:[*mut u64;MAX_PT],
 slots:[Slot;S],frames:[[u64;P];S],next_key:u64,
}
impl<const S:usize,const P:usize>Arena<S,P>{
 const fn new()->Self{Self{ready:false,base:0,parent:ptr::null_mut(),pd:ptr::null_mut(),
  tables:[0;MAX_TABLES],table_count:0,leaves:[ptr::null_mut();MAX_PT],
  slots:[const{Slot::new()};S],frames:[[0;P];S],next_key:1}}
 fn pending_pages(&self)->usize{
  let data=self.slots.iter().enumerate().filter(|(_,s)|!s.loaned&&s.key!=0)
   .map(|(i,_)|self.frames[i].iter().filter(|&&f|f!=0).count()).sum::<usize>();
  data+if self.ready{0}else{self.tables.iter().filter(|&&f|f!=0).count()}
 }
 fn infrastructure_ok(&self)->bool{
  if !self.ready||self.parent.is_null()||self.pd.is_null()||self.table_count!=1+S*PT_PER_SLOT{return false;}
  if (unsafe{ptr::read_volatile(self.parent)}&!VOLATILE)!=(self.tables[0]|FLAGS){return false;}
  for i in 0..S*PT_PER_SLOT{
   if self.leaves[i].is_null()||(unsafe{ptr::read_volatile(self.pd.add(i))}&!VOLATILE)!=(self.tables[i+1]|FLAGS){return false;}
  }
  true
 }
 fn leaf(&self,slot:usize,page:usize)->Option<*mut u64>{
  if slot>=S||page>=4096{return None;}
  let table=*self.leaves.get(slot*PT_PER_SLOT+page/512)?;
  if table.is_null(){None}else{Some(unsafe{table.add(page%512)})}
 }
 fn validate_slot(&self,index:usize,slot:Slot)->bool{
  if !self.infrastructure_ok()||slot.requested==0||slot.requested>P||
     slot.allocated>slot.requested||slot.mapped>slot.allocated{return false;}
  if self.frames[index][slot.allocated..].iter().any(|&f|f!=0){return false;}
  for page in 0..4096{
   let Some(leaf)=self.leaf(index,page)else{return false;};
   let expected=if page<slot.mapped{
    let frame=self.frames[index][page];if !frame_ok(frame){return false;}frame|FLAGS
   }else{0};
   if unsafe{ptr::read_volatile(leaf)}&!VOLATILE!=expected{return false;}
  }
  true
 }
 fn release_slot(&mut self,memory:&mut impl Memory,index:usize,slot:&mut Slot)->Result<(),Error>{
  if !self.validate_slot(index,*slot){return Err(Error::Integrity);}
  if !slot.scrubbed{
   for page in 0..slot.allocated{
    let frame=self.frames[index][page];if frame==0{continue;}
    let data=memory.table(frame);
    if data.is_null(){
     if slot.loaned||page<slot.mapped{return Err(Error::Mapping);}
    }else{unsafe{zero_frame(data)};}
   }
   slot.scrubbed=true;
  }
  while slot.mapped>0{
   let page=slot.mapped-1;let leaf=self.leaf(index,page).ok_or(Error::Mapping)?;
   unsafe{ptr::write_volatile(leaf,0)};
   memory.invalidate(self.base+index as u64*STRIDE+page as u64*PAGE);slot.mapped-=1;
  }
  let mut complete=true;
  for frame in &mut self.frames[index][..slot.allocated]{
   if *frame==0{continue;}
   if memory.release(*frame){*frame=0;}else{complete=false;}
  }
  if complete{*slot=Slot::new();Ok(())}else{Err(Error::Release)}
 }
 fn reclaim_pending(&mut self,memory:&mut impl Memory)->u64{
  let before=self.pending_pages();
  if !self.ready{
   for frame in self.tables.iter_mut().rev().filter(|f|**f!=0){if memory.release(*frame){*frame=0;}}
   if self.tables.iter().all(|&f|f==0){self.table_count=0;self.leaves.fill(ptr::null_mut());self.pd=ptr::null_mut();}
  }else{
   for index in 0..S{
    let mut slot=self.slots[index];
    if slot.key!=0&&!slot.loaned{let _=self.release_slot(memory,index,&mut slot);self.slots[index]=slot;}
   }
  }
  before.saturating_sub(self.pending_pages()) as u64*PAGE
 }
 fn new_table(&mut self,memory:&mut impl Memory)->Result<*mut u64,Error>{
  if self.table_count==self.tables.len(){return Err(Error::Metadata);}
  let frame=memory.allocate().ok_or(Error::OutOfMemory)?;
  if !frame_ok(frame){return Err(Error::Mapping);}
  self.tables[self.table_count]=frame;self.table_count+=1;
  let table=memory.table(frame);if table.is_null(){return Err(Error::Mapping);}
  unsafe{zero_frame(table)};Ok(table)
 }
 fn initialize(&mut self,memory:&mut impl Memory,root:u64,offset:u64,nx:bool)->Result<(),Error>{
  if self.ready{return if self.infrastructure_ok(){Ok(())}else{Err(Error::Integrity)};}
  if S==0||S>16||P==0||P>4096||!nx||!frame_ok(root)||offset&4095!=0||offset>>48!=0xffff||offset&(1<<47)==0{return Err(Error::Geometry);}
  self.reclaim_pending(memory);if self.pending_pages()!=0{return Err(Error::Release);}
  let root_table=memory.table(root);if root_table.is_null(){return Err(Error::Mapping);}
  let top=((offset>>39)&511) as usize;
  let entry=unsafe{ptr::read_volatile(root_table.add(top))};
  if entry&1==0||entry&0x80!=0||!frame_ok(entry&PHYS){return Err(Error::Mapping);}
  let pdpt=memory.table(entry&PHYS);if pdpt.is_null(){return Err(Error::Mapping);}
  // Reserve one otherwise absent 1-GiB directory in the shared HHDM slot.
  let directory=(0..512).rev().find(|&i|unsafe{ptr::read_volatile(pdpt.add(i))}==0).ok_or(Error::Mapping)?;
  self.base=(offset&!((1u64<<39)-1))|((directory as u64)<<30);
  self.parent=unsafe{pdpt.add(directory)};
  let built=(||{
   self.pd=self.new_table(memory)?;
   for i in 0..S*PT_PER_SLOT{
    self.leaves[i]=self.new_table(memory)?;
    unsafe{ptr::write_volatile(self.pd.add(i),self.tables[i+1]|FLAGS)};
   }
   Ok::<(),Error>(())
  })();
  if let Err(error)=built{self.reclaim_pending(memory);return Err(if self.pending_pages()==0{error}else{Error::Release});}
  // Publish only a fully initialized subtree. No existing entry is replaced.
  if unsafe{ptr::read_volatile(self.parent)}!=0{self.reclaim_pending(memory);return Err(Error::Mapping);}
  unsafe{ptr::write_volatile(self.parent,self.tables[0]|FLAGS)};self.ready=true;Ok(())
 }
 fn allocate(&mut self,memory:&mut impl Memory,pages:u32)->Result<Backing,Error>{
  let requested=pages as usize;
  if requested==0||requested>P{return Err(Error::Geometry);}
  if !self.infrastructure_ok(){return Err(Error::Mapping);}
  self.reclaim_pending(memory);if self.pending_pages()!=0{return Err(Error::Release);}
  let index=self.slots.iter().enumerate().position(|(i,s)|s.key==0&&self.frames[i].iter().all(|&f|f==0)).ok_or(Error::Metadata)?;
  let key=self.next_key;let next=key.checked_add(1).ok_or(Error::Metadata)?;
  let mut slot=Slot{key,requested,allocated:0,mapped:0,loaned:false,scrubbed:false};
  if !self.validate_slot(index,slot){return Err(Error::Integrity);}
  self.next_key=next;
  let built=(||{
   for page in 0..requested{
    let frame=memory.allocate().ok_or(Error::OutOfMemory)?;
    if !frame_ok(frame){return Err(Error::Mapping);}
    self.frames[index][page]=frame;slot.allocated+=1;
    let data=memory.table(frame);if data.is_null(){return Err(Error::Mapping);}
    unsafe{zero_frame(data)};
    let leaf=self.leaf(index,page).ok_or(Error::Mapping)?;
    unsafe{ptr::write_volatile(leaf,frame|FLAGS)};
    memory.invalidate(self.base+index as u64*STRIDE+page as u64*PAGE);slot.mapped+=1;
   }
   Ok::<(),Error>(())
  })();
  if let Err(error)=built{
   let released=self.release_slot(memory,index,&mut slot);self.slots[index]=slot;
   return Err(if released.is_ok(){error}else{Error::Release});
  }
  slot.loaned=true;self.slots[index]=slot;
  Ok(Backing{key,physical:None,pointer:(self.base+index as u64*STRIDE) as *mut u8})
 }
 fn release(&mut self,memory:&mut impl Memory,key:u64,pages:u32,pointer:Option<*mut u8>)->bool{
  let Some(index)=self.slots.iter().position(|s|s.key==key&&s.loaned&&s.requested==pages as usize)else{return false;};
  if pointer.is_some_and(|p|p as u64!=self.base+index as u64*STRIDE){return false;}
  let mut slot=self.slots[index];let result=self.release_slot(memory,index,&mut slot);self.slots[index]=slot;result.is_ok()
 }
}

#[cfg(not(test))]
static mut ARENA:Arena<16,2500>=Arena::new();
#[cfg(not(test))]
struct Physical{offset:u64}
#[cfg(not(test))]
unsafe impl Memory for Physical{
 fn allocate(&mut self)->Option<u64>{let mut frame=0;unsafe{crate::pmm::pmm_allocate_frame(&mut frame)}.then_some(frame)}
 fn release(&mut self,frame:u64)->bool{unsafe{crate::pmm::pmm_release_frame(frame)}}
 fn table(&mut self,frame:u64)->*mut u64{crate::boot::direct_map_frame(self.offset,frame).unwrap_or(ptr::null_mut()).cast()}
 fn invalidate(&mut self,address:u64){unsafe{core::arch::asm!("invlpg [{}]",in(reg)address,options(nostack,preserves_flags));}}
}
#[cfg(not(test))]
fn memory()->Option<Physical>{let mut offset=0;unsafe{crate::boot::boot_hhdm_offset(&mut offset)}.then_some(Physical{offset})}
/// All production entry points are called under the heap's exact BSP permit.
#[cfg(not(test))]
pub(crate) fn initialize()->bool{
 let Some(mut memory)=memory()else{return false;};let mut cpu=crate::paging::CpuState::default();
 if !unsafe{crate::paging_cpu::x86_64_paging_detect(&mut cpu)}||!cpu.nxe_enabled||!unsafe{crate::paging::paging_preflight(&cpu)}{return false;}
 let offset=memory.offset;
 unsafe{(&mut *ptr::addr_of_mut!(ARENA)).initialize(&mut memory,cpu.cr3&PHYS,offset,true).is_ok()}
}
#[cfg(not(test))]
pub(crate) fn allocate(pages:u32)->Result<Backing,Error>{let mut m=memory().ok_or(Error::Mapping)?;unsafe{(&mut *ptr::addr_of_mut!(ARENA)).allocate(&mut m,pages)}}
#[cfg(not(test))]
pub(crate) fn release(key:u64,pages:u32)->bool{let Some(mut m)=memory()else{return false;};unsafe{(&mut *ptr::addr_of_mut!(ARENA)).release(&mut m,key,pages,None)}}
#[cfg(not(test))]
pub(crate) fn pending()->bool{unsafe{(&*ptr::addr_of!(ARENA)).pending_pages()!=0}}
#[cfg(not(test))]
pub(crate) fn reclaim()->u64{let Some(mut m)=memory()else{return 0;};unsafe{(&mut *ptr::addr_of_mut!(ARENA)).reclaim_pending(&mut m)}}

#[cfg(not(test))]
pub(crate) fn discard(key:u64,pages:u32,pointer:*mut u8)->bool{
 let Some(mut m)=memory()else{return false;};
 unsafe{(&mut *ptr::addr_of_mut!(ARENA)).release(&mut m,key,pages,Some(pointer))}
}

#[cfg(test)]
mod tests{
 use super::*;
 use std::{boxed::Box,collections::BTreeMap};
 const HHDM:u64=0xffff800000000000;
 struct Ram{frames:BTreeMap<u64,Box<[u64;512]>>,next:u64,allocations:usize,
  fail_at:usize,refuse_release:bool,invalidations:usize,deny:u64}
 impl Ram{
  fn new()->Self{
   let mut frames=BTreeMap::new();frames.insert(0x1000,Box::new([0;512]));
   frames.insert(0x2000,Box::new([0;512]));frames.get_mut(&0x1000).unwrap()[256]=0x2000|3;
   Self{frames,next:0x5000,allocations:0,fail_at:usize::MAX,refuse_release:false,invalidations:0,deny:0}
  }
 }
 unsafe impl Memory for Ram{
  fn allocate(&mut self)->Option<u64>{
   self.allocations+=1;if self.allocations==self.fail_at{return None;}
   let frame=self.next;self.next+=8192;self.frames.insert(frame,Box::new([0xa5;512]));Some(frame)
  }
  fn release(&mut self,frame:u64)->bool{
   assert!(frame>=0x5000,"borrowed source frame released");
   if self.refuse_release{return false;}self.frames.remove(&frame).is_some()
  }
  fn table(&mut self,frame:u64)->*mut u64{
   if frame==self.deny{return ptr::null_mut();}
   self.frames.get_mut(&frame).map_or(ptr::null_mut(),|table|table.as_mut_ptr())
  }
  fn invalidate(&mut self,_:u64){self.invalidations+=1;}
 }
 #[test]fn fragmented_physical_pages_form_one_supervisor_nx_virtual_span(){
  let mut ram=Ram::new();let mut arena=Arena::<1,4>::new();
  arena.initialize(&mut ram,0x1000,HHDM,true).unwrap();let baseline=ram.frames.len();
  let backing=arena.allocate(&mut ram,3).unwrap();assert_eq!(backing.physical,None);
  assert_eq!(backing.pointer as u64,arena.base);
  assert_eq!(arena.frames[0][1]-arena.frames[0][0],8192);
  for page in 0..3{
   let value=unsafe{ptr::read(arena.leaf(0,page).unwrap())};
   assert_eq!(value&PHYS,arena.frames[0][page]);assert_eq!(value&!PHYS,FLAGS);
  }
  assert!(arena.release(&mut ram,backing.key,3,Some(backing.pointer)));
  assert_eq!(ram.frames.len(),baseline);assert_eq!(ram.invalidations,6);
  assert!(!arena.release(&mut ram,backing.key,3,Some(backing.pointer)));
 }
 #[test]fn partial_allocation_refusal_keeps_orphans_until_reclaimed(){
  let mut ram=Ram::new();let mut arena=Arena::<1,4>::new();
  arena.initialize(&mut ram,0x1000,HHDM,true).unwrap();let baseline=ram.frames.len();
  ram.fail_at=ram.allocations+2;ram.refuse_release=true;
  assert_eq!(arena.allocate(&mut ram,3),Err(Error::Release));assert_eq!(arena.pending_pages(),1);
  assert_eq!(unsafe{ptr::read(arena.leaf(0,0).unwrap())},0);
  assert_eq!(arena.reclaim_pending(&mut ram),0);ram.refuse_release=false;
  assert_eq!(arena.reclaim_pending(&mut ram),4096);assert_eq!(ram.frames.len(),baseline);
  let next=arena.allocate(&mut ram,3).unwrap();
  assert!(arena.release(&mut ram,next.key,3,Some(next.pointer)));assert_eq!(ram.frames.len(),baseline);
 }
 #[test]fn partial_release_retry_never_uses_unmapped_buffer_pointer(){
  let mut ram=Ram::new();let mut arena=Arena::<1,4>::new();
  arena.initialize(&mut ram,0x1000,HHDM,true).unwrap();let baseline=ram.frames.len();
  let backing=arena.allocate(&mut ram,2).unwrap();ram.refuse_release=true;
  assert!(!arena.release(&mut ram,backing.key,2,Some(backing.pointer)));
  assert_eq!(arena.slots[0].mapped,0);assert!(arena.slots[0].scrubbed);
  assert_eq!(arena.pending_pages(),0); // Loaned backing is owned by Buffer, not an orphan.
  ram.refuse_release=false;
  assert!(arena.release(&mut ram,backing.key,2,Some(backing.pointer)));
  assert_eq!(ram.frames.len(),baseline);
 }
 #[test]fn foreign_mapping_or_pointer_cannot_authorize_release(){
  let mut ram=Ram::new();let mut arena=Arena::<1,4>::new();
  arena.initialize(&mut ram,0x1000,HHDM,true).unwrap();let backing=arena.allocate(&mut ram,2).unwrap();
  let frame=arena.frames[0][0];ram.frames.get_mut(&frame).unwrap()[0]=0xbeef;
  let leaf=arena.leaf(0,0).unwrap();let original=unsafe{ptr::read(leaf)};
  unsafe{ptr::write(leaf,0x2000|FLAGS)};
  assert!(!arena.release(&mut ram,backing.key,2,Some(backing.pointer)));
  assert_eq!(ram.frames[&frame][0],0xbeef);unsafe{ptr::write(leaf,original)};
  assert!(!arena.release(&mut ram,backing.key,2,Some(backing.pointer.wrapping_add(4))));
  assert!(arena.release(&mut ram,backing.key,2,Some(backing.pointer)));
 }
 #[test]fn initialization_failure_never_publishes_and_retains_failed_table_releases(){
  let mut ram=Ram::new();let mut arena=Arena::<1,4>::new();ram.fail_at=3;ram.refuse_release=true;
  assert_eq!(arena.initialize(&mut ram,0x1000,HHDM,true),Err(Error::Release));
  assert_eq!(ram.frames[&0x2000][511],0);assert_eq!(arena.pending_pages(),2);
  ram.refuse_release=false;assert_eq!(arena.reclaim_pending(&mut ram),8192);
  assert_eq!(ram.frames.len(),2);arena.initialize(&mut ram,0x1000,HHDM,true).unwrap();
 }
 #[test]fn arena_bounds_stale_keys_and_failed_access_preserve_ownership(){
  let mut ram=Ram::new();let mut arena=Arena::<1,4>::new();
  assert_eq!(arena.initialize(&mut ram,0x1000,HHDM,false),Err(Error::Geometry));
  arena.initialize(&mut ram,0x1000,HHDM,true).unwrap();let calls=ram.allocations;
  assert_eq!(arena.allocate(&mut ram,0),Err(Error::Geometry));assert_eq!(arena.allocate(&mut ram,5),Err(Error::Geometry));
  assert_eq!(ram.allocations,calls);
  let first=arena.allocate(&mut ram,1).unwrap();assert!(arena.release(&mut ram,first.key,1,Some(first.pointer)));
  let second=arena.allocate(&mut ram,1).unwrap();assert_ne!(second.key,first.key);
  assert!(!arena.release(&mut ram,first.key,1,Some(first.pointer)));
  assert!(arena.release(&mut ram,second.key,1,Some(second.pointer)));
  ram.deny=ram.next;ram.refuse_release=true;
  assert_eq!(arena.allocate(&mut ram,1),Err(Error::Release));assert_eq!(arena.pending_pages(),1);
  ram.refuse_release=false;assert_eq!(arena.reclaim_pending(&mut ram),4096);
  arena.next_key=u64::MAX;let calls=ram.allocations;
  assert_eq!(arena.allocate(&mut ram,1),Err(Error::Metadata));assert_eq!(ram.allocations,calls);
 }
}

#[cfg(not(test))]
pub(crate) fn pending_bytes()->u64{unsafe{(&*ptr::addr_of!(ARENA)).pending_pages() as u64*PAGE}}
