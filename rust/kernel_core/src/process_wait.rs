//! Versioned supervised-child query and bounded cooperative wait continuations.
//! The live runtime authenticates parent/child membership and copies user data.
use crate::Process;
pub(crate) const BYTES:usize=64;
pub(crate) const MAX_ROUNDS:u8=32;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum Error{Invalid,Stale,Consumed,Copy}
#[derive(Clone,Copy)]
pub(crate) struct Request{pub pid:u32,pub generation:u64,pub rounds:u64,pub address:u64}
impl Request{
 pub fn decode(operation:u64,bytes:&[u8;BYTES])->Result<Self,Error>{
  if operation>1||bytes[..4]!=[1,0,64,0]||bytes[32..].iter().any(|&b|b!=0){return Err(Error::Invalid);}
  let result=Self{pid:u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
   generation:u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
   rounds:u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
   address:u64::from_le_bytes(bytes[24..32].try_into().unwrap())};
  if result.pid==0||result.address==0||
     (operation==0&&result.rounds!=0)||
     (operation==1&&(result.generation==0||!(1..=MAX_ROUNDS as u64).contains(&result.rounds))){return Err(Error::Invalid);}
  Ok(result)
 }
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) struct Status{pub pid:u32,pub generation:u64,state:i32,reclaimed:bool,consumed:bool,status:u64,vector:u64}
impl Status{
 pub fn capture(process:&Process,generation:u64,consumed:bool)->Result<Self,Error>{
  let terminal=matches!(process.state,4|5);
  if process.pid==0||generation==0||!(1..=5).contains(&process.state)||
     (process.resources_reclaimed&&(!terminal||process.address_space_root!=0))||
     (consumed&&!process.resources_reclaimed){return Err(Error::Invalid);}
  Ok(Self{pid:process.pid,generation,state:process.state,reclaimed:process.resources_reclaimed,
   consumed,status:if terminal&&process.resources_reclaimed{process.exit_status}else{0},
   vector:if process.state==5&&process.resources_reclaimed{process.fault_vector}else{0}})
 }
 pub fn complete(&self)->bool{self.reclaimed&&matches!(self.state,4|5)}
 pub fn available(&self)->Result<bool,Error>{if self.consumed{Err(Error::Consumed)}else{Ok(self.complete())}}
 pub fn wire(&self)->[u8;BYTES]{
  let mut bytes=[0;BYTES];bytes[..4].copy_from_slice(&[1,0,64,0]);
  bytes[4..8].copy_from_slice(&self.pid.to_le_bytes());
  bytes[8..16].copy_from_slice(&self.generation.to_le_bytes());
  bytes[16..20].copy_from_slice(&self.state.to_le_bytes());
  bytes[20..24].copy_from_slice(&(u32::from(self.reclaimed)|(u32::from(self.consumed)<<1)).to_le_bytes());
  bytes[24..32].copy_from_slice(&self.status.to_le_bytes());bytes[32..40].copy_from_slice(&self.vector.to_le_bytes());bytes
 }
}
#[derive(Clone,Copy)]
pub(crate) struct Wait{owner:u64,pid:u32,generation:u64,address:u64,remaining:u8,fresh:bool}
pub(crate) struct OwnerState{pub incarnation:u64,pub wait:Wait,pub consumed:bool}
impl OwnerState{
 pub const fn new()->Self{Self{incarnation:0,wait:Wait::empty(),consumed:false}}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum Poll{Pending,TimedOut,Delivered,Failed(Error)}
impl Wait{
 pub const fn empty()->Self{Self{owner:0,pid:0,generation:0,address:0,remaining:0,fresh:false}}
 pub fn begin(owner:u64,request:Request)->Result<Self,Error>{
  if owner==0||request.pid==0||request.generation==0||request.address==0||
     !(1..=MAX_ROUNDS as u64).contains(&request.rounds){return Err(Error::Invalid);}
  Ok(Self{owner,pid:request.pid,generation:request.generation,address:request.address,
   remaining:request.rounds as u8,fresh:true})
 }
 pub fn active(&self)->bool{self.owner!=0}
 pub fn target(&self)->u32{self.pid}
 pub fn address(&self)->u64{self.address}
 pub fn cancel(&mut self){*self=Self::empty();}
 pub fn poll(&mut self,owner:u64,status:Result<Status,Error>,deliver:impl FnOnce(&[u8;BYTES])->bool)->Poll{
  let result=(||{
   if !self.active()||owner!=self.owner{return Poll::Failed(Error::Stale);}
   let status=match status{Ok(s)=>s,Err(e)=>return Poll::Failed(e)};
   if status.pid!=self.pid||status.generation!=self.generation{return Poll::Failed(Error::Stale);}
   match status.available(){
    Err(e)=>Poll::Failed(e),
    Ok(true)=>if deliver(&status.wire()){Poll::Delivered}else{Poll::Failed(Error::Copy)},
    Ok(false)=>{
     if self.fresh{self.fresh=false;}else{self.remaining=self.remaining.saturating_sub(1);}
     if self.remaining==0{Poll::TimedOut}else{Poll::Pending}
    }
   }
  })();
  if result!=Poll::Pending{self.cancel();}result
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn request()->Request{Request{pid:2,generation:9,rounds:1,address:0x400000}}
 fn status(reclaimed:bool)->Status{
  Status::capture(&Process{pid:2,state:if reclaimed{4}else{2},
   address_space_root:if reclaimed{0}else{0x1000},exit_status:7,
   resources_reclaimed:reclaimed,..Process::empty()},9,false).unwrap()
 }
 #[test]fn completion_waits_for_reclamation_and_copies_no_kernel_addresses(){
  let mut wait=Wait::begin(4,request()).unwrap();
  assert_eq!(wait.poll(4,Ok(status(false)),|_|panic!("premature delivery")),Poll::Pending);
  assert_eq!(wait.poll(4,Ok(status(true)),|b|{
   assert_eq!(&b[..4],&[1,0,64,0]);assert_eq!(&b[24..32],&7u64.to_le_bytes());
   assert!(b[40..].iter().all(|&v|v==0));true
  }),Poll::Delivered);assert!(!wait.active());
 }
 #[test]fn timeout_permits_a_peer_round_and_does_not_deliver(){
  let mut wait=Wait::begin(4,request()).unwrap();
  assert_eq!(wait.poll(4,Ok(status(false)),|_|false),Poll::Pending);
  assert_eq!(wait.poll(4,Ok(status(false)),|_|panic!("timeout copied")),Poll::TimedOut);
  assert!(!wait.active());
 }
 #[test]fn stale_parent_child_and_consumed_completion_fail_without_copy(){
  for mode in 0..3{
   let mut wait=Wait::begin(4,request()).unwrap();let mut status=status(true);
   if mode==1{status.generation+=1;}if mode==2{status.consumed=true;}
   let result=wait.poll(if mode==0{5}else{4},Ok(status),|_|panic!("foreign delivery"));
   assert!(matches!(result,Poll::Failed(Error::Stale|Error::Consumed)));assert!(!wait.active());
  }
 }
 #[test]fn failed_copy_does_not_consume_status_and_can_be_retried(){
  let status=status(true);let mut wait=Wait::begin(4,request()).unwrap();
  assert_eq!(wait.poll(4,Ok(status),|_|false),Poll::Failed(Error::Copy));
  let mut retry=Wait::begin(4,request()).unwrap();
  assert_eq!(retry.poll(4,Ok(status),|_|true),Poll::Delivered);
 }
 #[test]fn malformed_descriptors_and_unbounded_waits_are_refused(){
  let mut bytes=[0u8;64];bytes[..4].copy_from_slice(&[1,0,64,0]);
  bytes[4..8].copy_from_slice(&2u32.to_le_bytes());bytes[24..32].copy_from_slice(&0x400000u64.to_le_bytes());
  assert!(Request::decode(0,&bytes).is_ok());assert!(Request::decode(1,&bytes).is_err());
  bytes[8..16].copy_from_slice(&9u64.to_le_bytes());bytes[16..24].copy_from_slice(&1u64.to_le_bytes());
  assert!(Request::decode(1,&bytes).is_ok());assert!(Request::decode(0,&bytes).is_err());
  for index in [0,2,32,63]{let mut bad=bytes;bad[index]^=1;assert!(Request::decode(1,&bad).is_err());}
  bytes[16..24].copy_from_slice(&33u64.to_le_bytes());assert!(Request::decode(1,&bytes).is_err());
 }
 #[test]fn inconsistent_reclaimed_state_cannot_be_reported_as_exit(){
  for (state,root) in [(2,0),(4,0x1000)]{
   let p=Process{pid:2,state,address_space_root:root,resources_reclaimed:true,..Process::empty()};
   assert_eq!(Status::capture(&p,9,false),Err(Error::Invalid));
  }
 }
 #[test]fn fault_status_is_published_after_cleanup_without_fault_address_or_root(){
  let process=Process{pid:2,state:5,exit_status:0x10e,fault_vector:14,
   fault_error:6,fault_address:0xffff800000001000,resources_reclaimed:true,..Process::empty()};
  let status=Status::capture(&process,9,false).unwrap();assert!(status.complete());
  let bytes=status.wire();assert_eq!(&bytes[24..32],&0x10eu64.to_le_bytes());
  assert_eq!(&bytes[32..40],&14u64.to_le_bytes());assert!(bytes[40..].iter().all(|&v|v==0));
 }
}
