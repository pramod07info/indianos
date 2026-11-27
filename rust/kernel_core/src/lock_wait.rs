//! Bounded scheduler continuation for existing authenticated range locks.
//! No separate lock table, file lookup, device probe or user pointer is stored.
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
struct Pending{owner:Identity,receipt:LockReceipt,budget:RoundBudget}
#[derive(Clone,Copy)]
pub(crate) struct Wait{active:bool,pending:MaybeUninit<Pending>}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub(crate) enum Poll{Pending,Acquired,TimedOut,Failed(FsError)}
impl Wait{
 pub const fn empty()->Self{Self{active:false,pending:MaybeUninit::uninit()}}
 pub fn begin(owner:Identity,receipt:LockReceipt,rounds:u64)->Option<Self>{
  if owner.pid==0||owner.generation==0||!receipt.owned_by(owner){return None;}
  let budget=RoundBudget::new(rounds)?;
  Some(Self{active:true,pending:MaybeUninit::new(Pending{owner,receipt,budget})})
 }
 pub fn active(&self)->bool{self.active}
 pub fn cancel(&mut self){*self=Self::empty();}
 pub fn poll(&mut self,owner:Identity,files:&mut FileHandles)->Poll{
  if !self.active{return Poll::Failed(FsError::InvalidName);}
  let pending=unsafe{self.pending.assume_init_mut()};
  let result=if pending.owner!=owner{Poll::Failed(FsError::InvalidName)}else{
   match files.retry_lock_wait(pending.receipt){
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
 #[test]fn descriptor_version_reserved_fields_deadline_and_zero_handle_are_checked(){
  let mut bytes=[0u8;64];bytes[..4].copy_from_slice(&[1,0,64,0]);bytes[8]=7;bytes[24]=1;
  assert!(Request::decode(35,&bytes).is_some());
  for i in [0,2,4,32,63]{let mut bad=bytes;bad[i]^=1;assert!(Request::decode(35,&bad).is_none());}
  assert!(Request::decode(36,&bytes).is_none());bytes[24]=33;assert!(Request::decode(35,&bytes).is_none());
  bytes[24]=1;bytes[8]=0;assert!(Request::decode(35,&bytes).is_none());
 }
}
