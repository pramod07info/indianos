//! Bounded scheduler continuation for existing authenticated range locks.
//! No separate lock table, file lookup, device probe or user pointer is stored.
//! The serialized coordinator supplies ticket order and conflicting-predecessor
//! admission. Deadlines keep advancing while queued; current locks are preserved.
use core::mem::MaybeUninit;
use crate::{batch5_session::Identity,durablefs::FsError,
 fs_handles::{FileHandles,LockReceipt},process_wait::RoundBudget};
pub(crate) const BYTES:usize=64;
#[derive(Clone,Copy)]
pub(crate) struct Request{pub handle:u64,pub range:u64,pub rounds:u64}
impl Request{
 pub fn decode(operation:u64,bytes:&[u8;BYTES])->Option<Self>{
  if !matches!(operation,34|35)||bytes[..8]!=[1,0,64,0,0,0,0,0]||
     bytes[32..].iter().any(|&b|b!=0){return None;}
  let result=Self{handle:u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
   range:u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
   rounds:u64::from_le_bytes(bytes[24..32].try_into().unwrap())};
  (result.handle!=0&&RoundBudget::new(result.rounds).is_some()).then_some(result)
 }
}
#[derive(Clone,Copy)]
struct Pending{owner:Identity,receipt:LockReceipt,budget:RoundBudget,ticket:u64}
#[derive(Clone,Copy)]
pub(crate) struct Wait{active:bool,pending:MaybeUninit<Pending>}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub(crate) enum Poll{Pending,Acquired,TimedOut,Failed(FsError)}
impl Wait{
 pub const fn empty()->Self{Self{active:false,pending:MaybeUninit::uninit()}}
 pub fn begin(owner:Identity,receipt:LockReceipt,rounds:u64)->Option<Self>{
  Self::begin_ordered(owner,receipt,rounds,1)
 }
 /// The BSP coordinator issues monotonically increasing nonzero tickets.
 /// The continuation does not mint authority or own a second lock table.
 pub fn begin_ordered(owner:Identity,receipt:LockReceipt,rounds:u64,ticket:u64)->Option<Self>{
  if owner.pid==0||owner.generation==0||ticket==0||!receipt.owned_by(owner){return None;}
  let budget=RoundBudget::new(rounds)?;
  Some(Self{active:true,pending:MaybeUninit::new(Pending{owner,receipt,budget,ticket})})
 }
 pub fn active(&self)->bool{self.active}
 pub fn ticket(&self)->Option<u64>{
  self.active.then(||unsafe{self.pending.assume_init_ref().ticket})
 }
 pub fn receipt(&self)->Option<LockReceipt>{
  self.active.then(||unsafe{self.pending.assume_init_ref().receipt})
 }
 pub fn cancel(&mut self){*self=Self::empty();}
 pub fn poll(&mut self,owner:Identity,files:&mut FileHandles)->Poll{
  self.poll_behind(owner,files,false)
 }
 /// A current older conflicting waiter prevents acquisition, not validation
 /// or deadline expiry. Retired receipts cannot retain a place in the queue.
 pub fn poll_behind(&mut self,owner:Identity,files:&mut FileHandles,behind:bool)->Poll{
  if !self.active{return Poll::Failed(FsError::InvalidName);}
  let pending=unsafe{self.pending.assume_init_mut()};
  let result=if pending.owner!=owner{Poll::Failed(FsError::InvalidName)}else{
   let attempt=if behind{
    if files.lock_wait_is_current(pending.receipt){Ok(1)}else{Err(FsError::InvalidName)}
   }else{files.retry_lock_wait(pending.receipt)};
   match attempt{
    Ok(0)=>Poll::Acquired,
    Ok(1)=>if pending.budget.expired_after_pending(){Poll::TimedOut}else{Poll::Pending},
    Ok(_)=>Poll::Failed(FsError::InvalidName),Err(error)=>Poll::Failed(error),
   }
  };
  if result!=Poll::Pending{self.cancel();}result
 }
}
#[cfg(test)]mod tests{
 use super::*;
 use crate::{block::{BlockDevice,BlockError,SECTOR_BYTES},hier_vfs::HierVfs,fs_handles::{READ,WRITE}};
 struct Disk([[u8;SECTOR_BYTES];100]);
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,l:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.0.get(l as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,l:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.0.get_mut(l as usize).ok_or(BlockError::OutOfRange)?=*data;Ok(())
  }
  fn sync(&mut self)->Result<(),BlockError>{Ok(())}
 }
 fn fixture()->(FileHandles,HierVfs<Disk>,Identity,Identity,u64,u64){
  let mut fs=HierVfs::format_blank(Disk([[0;SECTOR_BYTES];100])).unwrap();
  fs.mkdir("/shared").unwrap();fs.create("/shared/note",b"data").unwrap();
  let mut files=FileHandles::new();let a=files.bind_process(1).unwrap();let b=files.bind_process(2).unwrap();
  let writer=files.dispatch_authenticated(1,a,0,READ|WRITE,"/shared/note",&mut[],&mut fs).unwrap();
  let reader=files.dispatch_authenticated(1,b,0,READ,"/shared/note",&mut[],&mut fs).unwrap();
  assert_eq!(files.dispatch_authenticated(34,b,reader,0,"",&mut[],&mut fs),Ok(0));
  (files,fs,a,b,writer,reader)
 }
 #[test]fn pending_writer_acquires_after_peer_unlock_without_another_lock_table(){
  let (mut files,mut fs,a,b,writer,reader)=fixture();
  let receipt=files.lock_wait_receipt(a,writer,35,0).unwrap();
  let mut wait=Wait::begin(a,receipt,1).unwrap();
  assert_eq!(wait.poll(a,&mut files),Poll::Pending);
  assert_eq!(files.dispatch_authenticated(36,b,reader,0,"",&mut[],&mut fs),Ok(0));
  assert_eq!(wait.poll(a,&mut files),Poll::Acquired);assert!(!wait.active());
  assert_eq!(files.dispatch_authenticated(34,b,reader,0,"",&mut[],&mut fs),Ok(1));
 }
 #[test]fn timeout_preserves_prior_read_lock_and_foreign_holder(){
  let (mut files,mut fs,a,b,writer,reader)=fixture();
  assert_eq!(files.dispatch_authenticated(34,a,writer,0,"",&mut[],&mut fs),Ok(0));
  let receipt=files.lock_wait_receipt(a,writer,35,0).unwrap();let mut wait=Wait::begin(a,receipt,1).unwrap();
  let peer_receipt=files.lock_wait_receipt(b,reader,34,0).unwrap();
  assert_eq!(wait.poll(a,&mut files),Poll::Pending);assert_eq!(wait.poll(a,&mut files),Poll::TimedOut);
  assert!(files.lock_wait_is_current(receipt));assert!(files.lock_wait_is_current(peer_receipt));
  assert_eq!(files.dispatch_authenticated(35,a,writer,0,"",&mut[],&mut fs),Ok(1));
  assert_eq!(files.dispatch_authenticated(36,b,reader,0,"",&mut[],&mut fs),Ok(0));
  // The caller's old shared lock remains until it explicitly unlocks.
  assert_eq!(files.dispatch_authenticated(36,a,writer,0,"",&mut[],&mut fs),Ok(0));
 }
 #[test]fn closed_descriptor_namespace_change_and_foreign_receipt_cannot_resume(){
  let (mut files,mut fs,a,b,writer,_)=fixture();let receipt=files.lock_wait_receipt(a,writer,35,0).unwrap();
  assert!(Wait::begin(b,receipt,1).is_none());
  let mut wait=Wait::begin(a,receipt,2).unwrap();
  files.dispatch_authenticated(5,a,writer,0,"",&mut[],&mut fs).unwrap();
  assert!(matches!(wait.poll(a,&mut files),Poll::Failed(_)));
  let new=files.dispatch_authenticated(1,a,0,READ|WRITE,"/shared/note",&mut[],&mut fs).unwrap();assert_ne!(new,writer);
  let mut wait=Wait::begin(a,files.lock_wait_receipt(a,new,35,0).unwrap(),2).unwrap();
  files.narrow_root_kernel(a,"/shared").unwrap();
  assert!(matches!(wait.poll(a,&mut files),Poll::Failed(_)));
 }
 #[test]fn cancellation_and_rebound_owner_never_acquire(){
  let (mut files,mut fs,a,_,writer,_)=fixture();let receipt=files.lock_wait_receipt(a,writer,35,0).unwrap();
  let mut wait=Wait::begin(a,receipt,2).unwrap();wait.cancel();
  assert!(matches!(wait.poll(a,&mut files),Poll::Failed(_)));
  assert_eq!(files.dispatch_authenticated(35,a,writer,0,"",&mut[],&mut fs),Ok(1));
  let mut wait=Wait::begin(a,receipt,2).unwrap();let replacement=files.bind_process(1).unwrap();
  assert!(matches!(wait.poll(replacement,&mut files),Poll::Failed(_)));
 }
 #[test]fn ordered_receipts_require_nonzero_tickets_and_cancel_without_stale_queue_state(){
  let (files,_,a,_,writer,_)=fixture();let receipt=files.lock_wait_receipt(a,writer,35,0).unwrap();
  assert!(Wait::begin_ordered(a,receipt,1,0).is_none());
  let mut wait=Wait::begin_ordered(a,receipt,1,u64::MAX).unwrap();
  assert_eq!(wait.ticket(),Some(u64::MAX));assert!(wait.receipt().unwrap().owned_by(a));
  wait.cancel();assert_eq!(wait.ticket(),None);assert!(wait.receipt().is_none());
 }
 #[test]fn younger_reader_cannot_barge_or_replace_its_prior_lock_while_writer_waits(){
  let (mut files,mut fs,a,b,writer,reader)=fixture();
  let older=files.lock_wait_receipt(a,writer,35,0).unwrap();
  let receipt=files.lock_wait_receipt(b,reader,34,0).unwrap();
  assert!(older.conflicts(receipt));assert!(receipt.conflicts(older));
  let mut writer_wait=Wait::begin_ordered(a,older,32,1).unwrap();
  let mut reader_wait=Wait::begin_ordered(b,receipt,1,2).unwrap();
  assert!(writer_wait.ticket()<reader_wait.ticket());
  assert_eq!(reader_wait.poll_behind(b,&mut files,true),Poll::Pending);
  assert_eq!(reader_wait.poll_behind(b,&mut files,true),Poll::TimedOut);
  assert!(files.lock_wait_is_current(receipt));assert!(files.lock_wait_is_current(older));
  assert_eq!(writer_wait.poll(a,&mut files),Poll::Pending);
  files.dispatch_authenticated(36,b,reader,0,"",&mut[],&mut fs).unwrap();
  assert_eq!(writer_wait.poll(a,&mut files),Poll::Acquired);
 }
 #[test]fn blocked_stale_or_foreign_receipt_fails_without_waiting_out_its_deadline(){
  let (mut files,mut fs,a,b,writer,reader)=fixture();
  let receipt=files.lock_wait_receipt(b,reader,34,0).unwrap();
  let mut wait=Wait::begin_ordered(b,receipt,32,2).unwrap();
  files.dispatch_authenticated(5,b,reader,0,"",&mut[],&mut fs).unwrap();
  assert_eq!(wait.poll_behind(b,&mut files,true),Poll::Failed(FsError::InvalidName));
  assert!(!wait.active());assert_eq!(wait.ticket(),None);
  let receipt=files.lock_wait_receipt(a,writer,35,0).unwrap();
  let mut wait=Wait::begin_ordered(a,receipt,32,3).unwrap();
  assert_eq!(wait.poll_behind(b,&mut files,true),Poll::Failed(FsError::InvalidName));
  assert!(!wait.active());
 }
 #[test]fn ordered_waiter_handoff_never_acquires_while_predecessor_is_pending_or_holding(){
  let (mut files,mut fs,a,b,writer,reader)=fixture();
  let candidate=files.dispatch_authenticated(1,b,0,READ,"/shared/note",&mut[],&mut fs).unwrap();
  let older=files.lock_wait_receipt(a,writer,35,0).unwrap();
  let younger=files.lock_wait_receipt(b,candidate,34,0).unwrap();
  assert!(older.conflicts(younger));
  let mut first=Wait::begin_ordered(a,older,32,1).unwrap();
  let mut second=Wait::begin_ordered(b,younger,32,2).unwrap();
  assert_eq!(first.poll(a,&mut files),Poll::Pending);
  files.dispatch_authenticated(36,b,reader,0,"",&mut[],&mut fs).unwrap();
  assert_eq!(second.poll_behind(b,&mut files,true),Poll::Pending);
  assert!(files.lock_wait_is_current(younger));
  assert_eq!(first.poll(a,&mut files),Poll::Acquired);
  assert_eq!(second.poll(b,&mut files),Poll::Pending);
  files.dispatch_authenticated(36,a,writer,0,"",&mut[],&mut fs).unwrap();
  assert_eq!(second.poll(b,&mut files),Poll::Acquired);
 }
 #[test]fn descriptor_version_reserved_fields_deadline_and_zero_handle_are_checked(){
  let mut bytes=[0u8;64];bytes[..4].copy_from_slice(&[1,0,64,0]);bytes[8]=7;bytes[24]=1;
  assert!(Request::decode(35,&bytes).is_some());
  for i in [0,2,4,32,63]{let mut bad=bytes;bad[i]^=1;assert!(Request::decode(35,&bad).is_none());}
  assert!(Request::decode(36,&bytes).is_none());bytes[24]=33;assert!(Request::decode(35,&bytes).is_none());
  bytes[24]=1;bytes[8]=0;assert!(Request::decode(35,&bytes).is_none());
 }
}
