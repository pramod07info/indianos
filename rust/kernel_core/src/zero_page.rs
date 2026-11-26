//! One immutable, kernel-owned zero page for demand-zero COW mappings.
//! Ownership lasts until boot ends; image teardown never releases this frame.
use core::ptr;
fn valid(frame:u64)->bool{frame!=0&&frame&!0x000f_ffff_ffff_f000==0}
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
 struct Restore(u64);impl Drop for Restore{fn drop(&mut self){TEST_FRAME.with(|cell|cell.set(self.0));}}
 let _restore=Restore(TEST_FRAME.with(|cell|cell.replace(frame)));body()
}
#[cfg(test)]mod tests{
 use super::*;
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
