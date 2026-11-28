//! Versioned supervised-child query and bounded cooperative wait continuations.
//! The live runtime authenticates parent/child membership and copies user data.
use crate::Process;
pub(crate) const BYTES:usize=64;
pub(crate) const MAX_ROUNDS:u8=32;
/// Shared cooperative deadline. Completion is checked before charging a
/// pending round; the first pass always leaves a peer a chance to execute.
#[derive(Clone,Copy)]
pub(crate) struct RoundBudget{remaining:u8,fresh:bool}
impl RoundBudget{
 pub const fn empty()->Self{Self{remaining:0,fresh:false}}
 pub fn new(rounds:u64)->Option<Self>{
  (1..=MAX_ROUNDS as u64).contains(&rounds).then_some(Self{remaining:rounds as u8,fresh:true})
 }
 pub fn expired_after_pending(&mut self)->bool{
  if self.fresh{self.fresh=false;}else{self.remaining=self.remaining.saturating_sub(1);}
  self.remaining==0
 }
}
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
  // Fail closed on a contradictory PCB instead of turning a lost root into
  // an indefinitely pending child, or publishing a fabricated completion.
  // NEW/READY may be unbound OR own prepared mappings: the R026 launch
  // gate deliberately holds a fully staged worker in NEW until PID1 starts
  // it. State controls dispatch eligibility, not whether those pages exist.
  // A running/terminal process cannot lose its owned root before the
  // explicit reclamation transition; every retained root stays aligned.
  let root=process.address_space_root;
  let fault=process.fault_vector!=0||process.fault_error!=0||process.fault_address!=0;
  if process.pid==0||generation==0||!(1..=5).contains(&process.state)||
     (root!=0&&root&4095!=0)||
     (process.state==3&&root==0)||
     (terminal&&((root==0)!=process.resources_reclaimed))||
     (process.resources_reclaimed&&!terminal)||
     (!terminal&&(process.exit_status!=0||fault))||
     (process.state==4&&fault)||(process.state==5&&process.fault_vector>31)||
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
pub(crate) struct Wait{owner:u64,pid:u32,generation:u64,address:u64,budget:RoundBudget}
pub(crate) struct OwnerState{pub incarnation:u64,pub wait:Wait,pub consumed:bool}
impl OwnerState{
 pub const fn new()->Self{Self{incarnation:0,wait:Wait::empty(),consumed:false}}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum Poll{Pending,TimedOut,Delivered,Failed(Error)}
impl Wait{
 pub const fn empty()->Self{Self{owner:0,pid:0,generation:0,address:0,budget:RoundBudget::empty()}}
 pub fn begin(owner:u64,request:Request)->Result<Self,Error>{
  if owner==0||request.pid==0||request.generation==0||request.address==0||
     !(1..=MAX_ROUNDS as u64).contains(&request.rounds){return Err(Error::Invalid);}
  let Some(budget)=RoundBudget::new(request.rounds)else{return Err(Error::Invalid);};
  Ok(Self{owner,pid:request.pid,generation:request.generation,address:request.address,budget})
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
     if self.budget.expired_after_pending(){Poll::TimedOut}else{Poll::Pending}
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
   address_space_root:if reclaimed{0}else{0x1000},exit_status:if reclaimed{7}else{0},
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

#[cfg(test)]mod lifecycle_integrity_tests{
 use super::*;
 #[test]fn live_pcb_transitions_capture_only_after_real_ownership_changes(){
  let mut process=Process{pid:2,state:1,..Process::empty()};
  assert!(Status::capture(&process,9,false).is_ok());
  assert!(unsafe{crate::indianos_process_ready(&mut process)});
  assert!(Status::capture(&process,9,false).is_ok());
  assert!(unsafe{crate::indianos_process_bind(&mut process,0x1000)});
  assert!(Status::capture(&process,9,false).is_ok());
  assert!(unsafe{crate::indianos_process_start(&mut process,0x1000)});
  assert!(Status::capture(&process,9,false).is_ok());
  assert!(unsafe{crate::indianos_process_exit(&mut process,7)});
  assert!(!Status::capture(&process,9,false).unwrap().complete());
  assert!(unsafe{crate::indianos_process_reap(&mut process)});
  let status=Status::capture(&process,9,false).unwrap();
  assert!(status.complete());assert_eq!(status.available(),Ok(true));
  assert_eq!(Status::capture(&process,9,true).unwrap().available(),Err(Error::Consumed));
 }
 #[test]fn staged_new_worker_can_be_queried_and_waited_without_launch_authority(){
  let mut process=Process{pid:2,state:1,..Process::empty()};
  assert!(unsafe{crate::indianos_process_ready(&mut process)});
  assert!(unsafe{crate::indianos_process_bind(&mut process,0x1000)});
  // Match the actual R026/test-r031-vfs hold after image preparation: it
  // retains its CR3 while NEW prevents scheduler_next_matching dispatch.
  process.state=1;
  let staged=Status::capture(&process,9,false).unwrap();
  assert!(!staged.complete());assert_eq!(staged.available(),Ok(false));
  let request=Request{pid:2,generation:9,rounds:1,address:0x400000};
  let mut wait=Wait::begin(4,request).unwrap();
  assert_eq!(wait.poll(4,Ok(staged),|_|panic!("staged worker delivered")),Poll::Pending);
  assert_eq!(wait.poll(4,Ok(staged),|_|panic!("staged worker delivered")),Poll::TimedOut);
  assert_eq!(process.state,1);assert_eq!(process.address_space_root,0x1000);
  // PID1's separate authenticated launch changes eligibility. A wait alone
  // must neither perform that transition nor consume an eventual completion.
  assert!(unsafe{crate::indianos_process_ready(&mut process)});
  assert!(Status::capture(&process,9,false).is_ok());
  assert!(unsafe{crate::indianos_process_start(&mut process,0x1000)});
  assert!(Status::capture(&process,9,false).is_ok());
  assert!(unsafe{crate::indianos_process_exit(&mut process,0)});
  assert!(unsafe{crate::indianos_process_reap(&mut process)});
  let mut wait=Wait::begin(4,request).unwrap();
  assert_eq!(wait.poll(4,Status::capture(&process,9,false),|_|true),Poll::Delivered);
 }
 #[test]fn lost_unaligned_or_premature_roots_never_become_waitable_status(){
  for (state,root,reclaimed) in [(1,0x1001,false),(2,0x1001,false),
     (3,0,false),(3,0x1001,false),(4,0,false),(5,0,false),
     (2,0,true),(4,0x1000,true),(5,0x1000,true)]{
   let process=Process{pid:2,state,address_space_root:root,resources_reclaimed:reclaimed,
    fault_vector:if state==5{14}else{0},..Process::empty()};
   assert_eq!(Status::capture(&process,9,false),Err(Error::Invalid));
  }
 }
 #[test]fn stale_exit_and_fault_payloads_cannot_hide_in_nonterminal_or_normal_exit(){
  for state in 1..=4{
   for field in 0..4{
    let mut process=Process{pid:2,state,address_space_root:if state==1{0}else{0x1000},
      ..Process::empty()};
    match field{0=>process.exit_status=7,1=>process.fault_vector=14,
      2=>process.fault_error=6,_=>process.fault_address=0xffff800000001000}
    if state==4&&field==0{assert!(Status::capture(&process,9,false).is_ok());}
    else{assert_eq!(Status::capture(&process,9,false),Err(Error::Invalid));}
   }
  }
  let fault=Process{pid:2,state:5,address_space_root:0x1000,fault_vector:32,..Process::empty()};
  assert_eq!(Status::capture(&fault,9,false),Err(Error::Invalid));
 }
 #[test]fn divide_error_is_a_real_fault_and_interrupt_vector_cannot_retire_process(){
  let mut process=Process{pid:2,state:1,..Process::empty()};
  assert!(unsafe{crate::indianos_process_ready(&mut process)});
  assert!(unsafe{crate::indianos_process_bind(&mut process,0x1000)});
  assert!(unsafe{crate::indianos_process_start(&mut process,0x1000)});
  assert!(!unsafe{crate::indianos_process_fault(&mut process,0x120,32,0,0)});
  assert_eq!((process.state,process.address_space_root,process.exit_status),(3,0x1000,0));
  assert_eq!((process.fault_vector,process.fault_error,process.fault_address),(0,0,0));
  assert!(unsafe{crate::indianos_process_fault(&mut process,0x100,0,0,0)});
  let pending=Status::capture(&process,9,false).unwrap();
  assert!(!pending.complete());
  assert!(unsafe{crate::indianos_process_reap(&mut process)});
  let status=Status::capture(&process,9,false).unwrap();assert!(status.complete());
  let bytes=status.wire();assert_eq!(&bytes[16..20],&5i32.to_le_bytes());
  assert_eq!(&bytes[24..32],&0x100u64.to_le_bytes());
  assert_eq!(&bytes[32..40],&0u64.to_le_bytes());
  assert!(!unsafe{crate::indianos_process_success(&process)});
 }
 #[test]fn invalid_snapshot_retires_wait_without_copy_or_completion_consumption(){
  let request=Request{pid:2,generation:9,rounds:32,address:0x400000};
  let mut wait=Wait::begin(4,request).unwrap();
  let lost=Process{pid:2,state:4,exit_status:7,..Process::empty()};
  assert_eq!(wait.poll(4,Status::capture(&lost,9,false),|_|panic!("invalid PCB copied")),
     Poll::Failed(Error::Invalid));
  assert!(!wait.active());
  // Repairing the kernel-owned lifecycle snapshot permits a fresh wait;
  // the rejected continuation never consumes the child's eventual status.
  let cleaned=Process{resources_reclaimed:true,..lost};
  let mut retry=Wait::begin(4,request).unwrap();
  assert_eq!(retry.poll(4,Status::capture(&cleaned,9,false),|_|true),Poll::Delivered);
 }
}
