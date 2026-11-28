//! One immutable, kernel-owned zero page for demand-zero COW mappings.
//! Ownership lasts until boot ends; image teardown never releases this frame.
use core::{ptr,sync::atomic::{AtomicU64,Ordering}};
fn valid(frame:u64)->bool{frame!=0&&frame&!0x000f_ffff_ffff_f000==0}
/// Mapping references are separate from the boot-owned physical reservation.
/// They count shared PTE ownership, never authorize recycling the zero frame.
struct MappingReferences{count:AtomicU64}
impl MappingReferences{
 const fn new()->Self{Self{count:AtomicU64::new(0)}}
 fn acquire(&self)->bool{
  self.count.fetch_update(Ordering::AcqRel,Ordering::Acquire,|count|count.checked_add(1)).is_ok()
 }
 fn release(&self)->bool{
  self.count.fetch_update(Ordering::AcqRel,Ordering::Acquire,|count|count.checked_sub(1)).is_ok()
 }
 fn get(&self)->u64{self.count.load(Ordering::Acquire)}
}
#[cfg(not(test))]
static REFERENCES:MappingReferences=MappingReferences::new();
#[cfg(test)]
std::thread_local!{static TEST_REFERENCES:MappingReferences=const{MappingReferences::new()};}
/// Reserve one alias before installing a shared-zero leaf. The image owns the
/// reservation only after its matching page record is published. Caller must
/// serialize image PTE changes; atomic arithmetic does not make VMM SMP-safe.
pub(crate) fn retain_mapping(frame:u64)->bool{
 if physical()!=Some(frame){return false;}
 #[cfg(not(test))]{REFERENCES.acquire()}
 #[cfg(test)]{TEST_REFERENCES.with(MappingReferences::acquire)}
}
/// Drop one owned alias after unmap or after a successful private replacement.
/// This never releases the permanently reserved physical frame to PMM.
pub(crate) fn release_mapping(frame:u64)->bool{
 if physical()!=Some(frame){return false;}
 #[cfg(not(test))]{REFERENCES.release()}
 #[cfg(test)]{TEST_REFERENCES.with(MappingReferences::release)}
}
pub(crate) fn mapping_references()->u64{
 #[cfg(not(test))]{REFERENCES.get()}
 #[cfg(test)]{TEST_REFERENCES.with(MappingReferences::get)}
}
struct OwnedZero{frame:u64,ready:bool}
impl OwnedZero{
 const fn new()->Self{Self{frame:0,ready:false}}
 /// Callbacks supply a fresh frame and its exclusive full-page supervisor
 /// mapping. Failed access retains the allocation instead of losing it.
 unsafe fn ensure(&mut self,allocate:impl FnOnce()->Option<u64>,access:impl FnOnce(u64)->*mut u64)->bool{
  if self.ready{return true;}
  if self.frame==0{let Some(frame)=allocate()else{return false;};self.frame=frame;}
  if !valid(self.frame){return false;}
  let data=access(self.frame);if data.is_null()||data as usize%8!=0{return false;}
  for word in 0..512{unsafe{ptr::write_volatile(data.add(word),0)}}
  self.ready=true;true
 }
}
#[cfg(not(test))]
static mut OWNED:OwnedZero=OwnedZero::new();
#[cfg(not(test))]
static PUBLISHED:core::sync::atomic::AtomicU64=core::sync::atomic::AtomicU64::new(0);
/// Called once under the existing heap BSP/IRQ admission before session
/// PMM baselines. No user mappings exist until successful publication.
#[cfg(not(test))]
pub(crate) fn initialize()->bool{
 let mut offset=0;if !unsafe{crate::boot::boot_hhdm_offset(&mut offset)}{return false;}
 let owned=unsafe{&mut *ptr::addr_of_mut!(OWNED)};
 let ready=unsafe{owned.ensure(||{
  let mut frame=0;crate::pmm::pmm_allocate_frame(&mut frame).then_some(frame)
 },|frame|crate::boot::direct_map_frame(offset,frame).unwrap_or(ptr::null_mut()).cast())};
 if ready{PUBLISHED.store(owned.frame,core::sync::atomic::Ordering::Release);}ready
}
#[cfg(not(test))]
pub(crate) fn physical()->Option<u64>{
 let frame=PUBLISHED.load(core::sync::atomic::Ordering::Acquire);valid(frame).then_some(frame)
}
#[cfg(test)]
std::thread_local!{static TEST_FRAME:core::cell::Cell<u64>=const{core::cell::Cell::new(0)};}
#[cfg(test)]
pub(crate) fn physical()->Option<u64>{TEST_FRAME.with(|cell|{let f=cell.get();valid(f).then_some(f)})}
#[cfg(test)]
pub(crate) fn with_test_frame<R>(frame:u64,body:impl FnOnce()->R)->R{
 assert!(valid(frame));
 struct Restore(u64,u64);impl Drop for Restore{fn drop(&mut self){
  TEST_FRAME.with(|cell|cell.set(self.0));
  TEST_REFERENCES.with(|references|references.count.store(self.1,Ordering::Release));
 }}
 let old=TEST_FRAME.with(|cell|cell.replace(frame));
 let references=TEST_REFERENCES.with(|references|references.count.swap(0,Ordering::AcqRel));
 let _restore=Restore(old,references);body()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn mapping_count_rejects_underflow_overflow_and_wrong_frame(){
  let references=MappingReferences::new();assert!(!references.release());
  assert_eq!(references.get(),0);assert!(references.acquire());assert!(references.release());
  references.count.store(u64::MAX,Ordering::Release);
  assert!(!references.acquire());assert_eq!(references.get(),u64::MAX);
  with_test_frame(0x1000,||{
   assert!(!retain_mapping(0x2000));assert!(!release_mapping(0x2000));
   assert!(!release_mapping(0x1000));assert_eq!(mapping_references(),0);
   assert!(retain_mapping(0x1000));assert_eq!(mapping_references(),1);
   assert!(release_mapping(0x1000));assert_eq!(mapping_references(),0);
  });
 }
 #[test]fn alias_reference_arithmetic_is_conserved_across_observers(){
  let references=std::sync::Arc::new(MappingReferences::new());let mut threads=std::vec::Vec::new();
  for _ in 0..4{
   let references=references.clone();threads.push(std::thread::spawn(move||{
    for _ in 0..1000{assert!(references.acquire());assert!(references.release());}
   }));
  }
  for thread in threads{thread.join().unwrap();}assert_eq!(references.get(),0);
 }
 #[test]fn page_is_zeroed_before_one_time_publication(){
  let mut owned=OwnedZero::new();let mut bytes=[u64::MAX;512];
  assert!(unsafe{owned.ensure(||Some(0x1000),|_|bytes.as_mut_ptr())});
  assert!(bytes.iter().all(|&b|b==0));
  assert!(unsafe{owned.ensure(||panic!("second allocation"),|_|panic!("second initialization"))});
 }
 #[test]fn refused_access_retains_frame_for_retry_without_reallocation(){
  let mut owned=OwnedZero::new();
  assert!(!unsafe{owned.ensure(||Some(0x1000),|_|ptr::null_mut())});
  assert_eq!(owned.frame,0x1000);assert!(!owned.ready);
  let mut bytes=[1u64;512];
  assert!(unsafe{owned.ensure(||panic!("lost ownership"),|_|bytes.as_mut_ptr())});
  assert!(bytes.iter().all(|&b|b==0));
 }
 #[test]fn malformed_allocator_frame_never_reaches_mapping_callback(){
  for frame in [0,1,1<<52]{
   let mut owned=OwnedZero::new();
   assert!(!unsafe{owned.ensure(||Some(frame),|_|panic!("invalid physical access"))});
   assert!(!owned.ready);
  }
 }
}
