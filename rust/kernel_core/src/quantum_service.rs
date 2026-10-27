//! R017.5: ownership-aware quantum service facade.
//! Kernel-side policy building block, NOT a ring-3 syscall implementation.
//! Callers must supply an authenticated nonzero process identity from the
//! kernel process table; never trust an owner ID copied from a user request.
use crate::quantum_queue::{QuantumQueue,QueueError,JobStatus,RESULT_BYTES,MAX_JOBS};
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum ServiceError{InvalidOwner,NotOwner,Queue(QueueError)}
impl From<QueueError> for ServiceError{
    fn from(e:QueueError)->Self{Self::Queue(e)}
}
#[derive(Clone,Copy)]
struct Ownership{job:u64,owner:u32,generation:u64}
impl Ownership{const fn empty()->Self{Self{job:0,owner:0,generation:0}}}
pub struct QuantumService{
    queue:QuantumQueue,
    owners:[Ownership;MAX_JOBS],
}
impl Default for QuantumService{fn default()->Self{Self::new()}}
impl QuantumService{
    pub const fn new()->Self{
        Self{queue:QuantumQueue::new(),owners:[Ownership::empty();MAX_JOBS]}
    }
    fn check_owner(&self,owner:u32,id:u64)->Result<(),ServiceError>{
        if owner==0{return Err(ServiceError::InvalidOwner);}
        if self.owners.iter().any(|x|x.job==id && id!=0 && x.owner==owner && x.generation==0){
            Ok(())
        }else{Err(ServiceError::NotOwner)}
    }
    pub fn submit(&mut self,owner:u32,request:&[u8])->Result<u64,ServiceError>{
        if owner==0{return Err(ServiceError::InvalidOwner);}
        // No ID is consumed unless the queue accepts the request.
        let id=self.queue.submit(request)?;
        let entry=self.owners.iter_mut().find(|x|x.job==0)
            .expect("ownership slots track queue slots");
        *entry=Ownership{job:id,owner,generation:0};
        Ok(id)
    }
    pub fn status(&self,owner:u32,id:u64)->Result<JobStatus,ServiceError>{
        self.check_owner(owner,id)?;
        Ok(self.queue.status(id)?)
    }
    pub fn result(&self,owner:u32,id:u64,out:&mut [u8;RESULT_BYTES])->Result<(),ServiceError>{
        self.check_owner(owner,id)?;
        Ok(self.queue.result(id,out)?)
    }
    pub fn release(&mut self,owner:u32,id:u64)->Result<(),ServiceError>{
        self.check_owner(owner,id)?;
        self.queue.release(id)?;
        let entry=self.owners.iter_mut().find(|x|x.job==id)
            .expect("owned job must have ownership entry");
        *entry=Ownership::empty();
        Ok(())
    }
    /// Scheduler-authenticated generation-scoped job submission. PIDs may
    /// be recycled; old job handles must never become the next process's.
    pub fn submit_scoped(&mut self,actor:Identity,request:&[u8])
        ->Result<u64,ServiceError>{
        if actor.pid==0||actor.generation==0{return Err(ServiceError::InvalidOwner);}
        let id=self.queue.submit(request)?;
        let entry=self.owners.iter_mut().find(|x|x.job==0)
            .expect("ownership slots track queue slots");
        *entry=Ownership{job:id,owner:actor.pid,generation:actor.generation};
        Ok(id)
    }
    fn check_scoped(&self,actor:Identity,id:u64)->Result<(),ServiceError>{
        if actor.pid==0||actor.generation==0{return Err(ServiceError::InvalidOwner);}
        if self.owners.iter().any(|x|
            x.job==id&&id!=0&&x.owner==actor.pid&&x.generation==actor.generation){
            Ok(())
        }else{Err(ServiceError::NotOwner)}
    }
    pub fn status_scoped(&self,actor:Identity,id:u64)
        ->Result<JobStatus,ServiceError>{
        self.check_scoped(actor,id)?;
        self.queue.status(id).map_err(ServiceError::Queue)
    }
    pub fn result_scoped(&self,actor:Identity,id:u64,out:&mut[u8;RESULT_BYTES])
        ->Result<(),ServiceError>{
        self.check_scoped(actor,id)?;
        self.queue.result(id,out).map_err(ServiceError::Queue)
    }
    pub fn release_scoped(&mut self,actor:Identity,id:u64)
        ->Result<(),ServiceError>{
        self.check_scoped(actor,id)?;
        self.queue.release(id)?;
        let entry=self.owners.iter_mut().find(|x|x.job==id)
            .expect("authorized job has slot");
        *entry=Ownership::empty();
        Ok(())
    }
    /// Trusted reaper cancels ONLY jobs of the old PID generation.
    pub fn process_terminated_scoped(&mut self,actor:Identity)
        ->Result<usize,ServiceError>{
        if actor.pid==0||actor.generation==0{return Err(ServiceError::InvalidOwner);}
        let mut count=0;
        for entry in &mut self.owners{
            if entry.job!=0&&entry.owner==actor.pid&&entry.generation==actor.generation{
                self.queue.release(entry.job)?;
                *entry=Ownership::empty();count+=1;
            }
        }
        Ok(count)
    }
    /// Called by a trusted process lifecycle owner on exit or fault.
    /// Jobs belonging to other process identities remain available.
    pub fn process_terminated(&mut self, owner:u32)->Result<usize,ServiceError>{
        self.release_owner(owner)
    }
    /// Kernel worker explicitly drives execution. No background execution.
    pub fn run_next(&mut self)->Option<u64>{self.queue.run_next()}
    /// Reclaim all jobs owned by an exiting process, including queued jobs.
    pub fn release_owner(&mut self,owner:u32)->Result<usize,ServiceError>{
        if owner==0{return Err(ServiceError::InvalidOwner);}
        let mut removed=0;
        for entry in &mut self.owners{
            if entry.job!=0 && entry.owner==owner{
                self.queue.release(entry.job)?;
                *entry=Ownership::empty();
                removed+=1;
            }
        }
        Ok(removed)
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    const BELL:&[u8]=&[1,2,2,0,1,0,0,4,0,1];
    #[test] fn isolates_processes_and_cleans_up(){
        let mut service=QuantumService::new();
        assert_eq!(service.submit(0,BELL),Err(ServiceError::InvalidOwner));
        let a=service.submit(101,BELL).unwrap();
        let b=service.submit(202,BELL).unwrap();
        assert_eq!(service.status(202,a),Err(ServiceError::NotOwner));
        let mut output=[0x55u8;RESULT_BYTES];
        assert_eq!(service.result(202,a,&mut output),Err(ServiceError::NotOwner));
        assert!(output.iter().all(|x|*x==0x55));
        assert_eq!(service.release(202,a),Err(ServiceError::NotOwner));
        assert_eq!(service.run_next(),Some(a));
        assert_eq!(service.status(101,a),Ok(JobStatus::Completed));
        service.result(101,a,&mut output).unwrap();
        assert_eq!(service.release_owner(101),Ok(1));
        assert_eq!(service.status(101,a),Err(ServiceError::NotOwner));
        assert_eq!(service.run_next(),Some(b));
        assert_eq!(service.release_owner(202),Ok(1));
        assert_eq!(service.run_next(),None);
    }
    #[test] fn termination_only_cleans_up_owner_jobs(){
        let mut service=QuantumService::new();
        let first=service.submit(11,BELL).unwrap();
        let second=service.submit(22,BELL).unwrap();
        assert_eq!(service.process_terminated(11),Ok(1));
        assert_eq!(service.status(11,first),Err(ServiceError::NotOwner));
        assert_eq!(service.status(22,second),Ok(JobStatus::Queued));
        assert_eq!(service.run_next(),Some(second));
        assert_eq!(service.process_terminated(22),Ok(1));
        assert_eq!(service.run_next(),None);
    }
    #[test] fn cancellation_and_slot_reuse(){
        let mut service=QuantumService::new();
        let a=service.submit(1,BELL).unwrap();
        service.release(1,a).unwrap();
        let b=service.submit(2,BELL).unwrap();
        assert_ne!(a,b);
        assert_eq!(service.status(1,a),Err(ServiceError::NotOwner));
        assert_eq!(service.status(1,b),Err(ServiceError::NotOwner));
        assert_eq!(service.run_next(),Some(b));
    }

    #[test]fn scoped_jobs_cannot_be_stolen_when_pid_is_reused(){
        let mut svc=QuantumService::new();
        let old=Identity{pid:17,generation:8};
        let next=Identity{pid:17,generation:9};
        let job=svc.submit_scoped(old,BELL).unwrap();
        assert_eq!(svc.status_scoped(next,job),Err(ServiceError::NotOwner));
        assert_eq!(svc.status(17,job),Err(ServiceError::NotOwner));
        assert_eq!(svc.release_scoped(next,job),Err(ServiceError::NotOwner));
        assert_eq!(svc.run_next(),Some(job));
        let mut out=[0x55;RESULT_BYTES];
        assert_eq!(svc.result_scoped(next,job,&mut out),Err(ServiceError::NotOwner));
        assert!(out.iter().all(|b|*b==0x55));
        assert_eq!(svc.result_scoped(old,job,&mut out),Ok(()));
    }
    #[test]fn scoped_reaper_leaves_new_pid_generation_jobs_untouched(){
        let mut svc=QuantumService::new();
        let old=Identity{pid:17,generation:8};
        let next=Identity{pid:17,generation:9};
        let first=svc.submit_scoped(old,BELL).unwrap();
        let second=svc.submit_scoped(next,BELL).unwrap();
        assert_eq!(svc.process_terminated_scoped(old),Ok(1));
        assert_eq!(svc.status_scoped(old,first),Err(ServiceError::NotOwner));
        assert_eq!(svc.status_scoped(next,second),Ok(JobStatus::Queued));
        assert_eq!(svc.run_next(),Some(second));
        assert_eq!(svc.process_terminated_scoped(next),Ok(1));
    }
    #[test]fn zero_generation_is_not_an_authenticated_compute_owner(){
        let mut svc=QuantumService::new();
        let invalid=Identity{pid:17,generation:0};
        assert_eq!(svc.submit_scoped(invalid,BELL),Err(ServiceError::InvalidOwner));
        assert_eq!(svc.process_terminated_scoped(invalid),Err(ServiceError::InvalidOwner));
    }

}


/// R006: bounded batch operations for the authenticated quantum simulation
/// service. These are CPU simulator jobs; neither QPU nor GPU hardware is
/// configured or assumed to exist.
impl QuantumService{
 /// Validate the complete batch and available queue capacity first; if an
 /// unexpectedly exhausted job ID fails a later submission, cancel all
 /// already-published jobs from this batch before returning an error.
 pub fn submit_batch_scoped(&mut self,actor:Identity,
  requests:&[&[u8]],out:&mut[u64])->Result<usize,ServiceError>{
  if actor.pid==0||actor.generation==0{return Err(ServiceError::InvalidOwner);}
  if requests.is_empty()||requests.len()>MAX_JOBS||out.len()<requests.len(){
   return Err(ServiceError::Queue(QueueError::Full));
  }
  let free=self.owners.iter().filter(|x|x.job==0).count();
  if free<requests.len(){return Err(ServiceError::Queue(QueueError::Full));}
  for request in requests{
   crate::quantum_protocol::decode(request)
    .map_err(|e|ServiceError::Queue(QueueError::InvalidRequest(e)))?;
  }
  let mut ids=[0u64;MAX_JOBS];let mut created=0usize;
  for request in requests{
   match self.submit_scoped(actor,request){
    Ok(id)=>{ids[created]=id;created+=1;},
    Err(error)=>{
     for &id in &ids[..created]{
      let _=self.release_scoped(actor,id);
     }
     return Err(error);
    }
   }
  }
  out[..created].copy_from_slice(&ids[..created]);Ok(created)
 }
 /// Transactional cross-job status view. Rejecting even one foreign/stale
 /// ID leaves the destination untouched.
 pub fn status_batch_scoped(&self,actor:Identity,ids:&[u64],
  out:&mut[JobStatus])->Result<usize,ServiceError>{
  if actor.pid==0||actor.generation==0{return Err(ServiceError::InvalidOwner);}
  if ids.is_empty()||ids.len()>MAX_JOBS||out.len()<ids.len(){
   return Err(ServiceError::Queue(QueueError::Full));
  }
  let mut staged=[JobStatus::Queued;MAX_JOBS];
  for (i,&id) in ids.iter().enumerate(){
   if ids[..i].contains(&id){return Err(ServiceError::NotOwner);}
   staged[i]=self.status_scoped(actor,id)?;
  }
  out[..ids.len()].copy_from_slice(&staged[..ids.len()]);
  Ok(ids.len())
 }
 /// Process teardown or user cancellation of a list of known jobs.
 /// Validate ownership and duplicates BEFORE releasing a single job.
 pub fn cancel_batch_scoped(&mut self,actor:Identity,ids:&[u64])
  ->Result<usize,ServiceError>{
  if actor.pid==0||actor.generation==0{return Err(ServiceError::InvalidOwner);}
  if ids.is_empty()||ids.len()>MAX_JOBS{
   return Err(ServiceError::Queue(QueueError::Full));
  }
  for (i,&id) in ids.iter().enumerate(){
   if ids[..i].contains(&id){return Err(ServiceError::NotOwner);}
   self.check_scoped(actor,id)?;
  }
  for &id in ids{self.release_scoped(actor,id)?;}
  Ok(ids.len())
 }
 /// A capacity-bounded owner-only inventory, never disclosing other
 /// processes' request bytes or their quantum result amplitudes.
 pub fn active_job_ids_scoped(&self,actor:Identity,out:&mut[u64])
  ->Result<usize,ServiceError>{
  if actor.pid==0||actor.generation==0{return Err(ServiceError::InvalidOwner);}
  let total=self.owners.iter().filter(|x|
   x.job!=0&&x.owner==actor.pid&&x.generation==actor.generation).count();
  if total>out.len(){return Err(ServiceError::Queue(QueueError::Full));}
  let mut n=0;
  for item in &self.owners{
   if item.job!=0&&item.owner==actor.pid&&item.generation==actor.generation{
    out[n]=item.job;n+=1;
   }
  }
  Ok(n)
 }
}
#[cfg(test)]
mod r006_quantum_batch_tests{
 use super::*;
 const BELL:[u8;10]=[1,2,2,0,1,0,0,4,0,1];
 fn owner()->Identity{Identity{pid:60,generation:8}}
 #[test]fn submit_and_status_multiple_jobs_without_escaping_owner(){
  let mut svc=QuantumService::new();let mut ids=[0u64;2];
  assert_eq!(svc.submit_batch_scoped(owner(),&[&BELL,&BELL],&mut ids),Ok(2));
  assert!(ids[0]>0&&ids[1]>ids[0]);
  let mut observed=[JobStatus::Queued;2];
  assert_eq!(svc.status_batch_scoped(owner(),&ids,&mut observed),Ok(2));
  assert_eq!(observed,[JobStatus::Queued;2]);
  assert_eq!(svc.run_next(),Some(ids[0]));
  assert_eq!(svc.run_next(),Some(ids[1]));
  assert_eq!(svc.status_batch_scoped(owner(),&ids,&mut observed),Ok(2));
  assert_eq!(observed,[JobStatus::Completed;2]);
 }
 #[test]fn bad_batch_does_not_publish_any_jobs_or_partial_ids(){
  let mut svc=QuantumService::new();let mut ids=[88u64;2];
  assert!(svc.submit_batch_scoped(owner(),&[&BELL,&[0xff]],&mut ids).is_err());
  assert_eq!(ids,[88;2]);
  let mut visible=[0u64;MAX_JOBS];
  assert_eq!(svc.active_job_ids_scoped(owner(),&mut visible),Ok(0));
 }
 #[test]fn foreign_and_stale_generations_cannot_cancel_peer_jobs(){
  let mut svc=QuantumService::new();let mut ids=[0u64;2];
  svc.submit_batch_scoped(owner(),&[&BELL,&BELL],&mut ids).unwrap();
  let stale=Identity{generation:9,..owner()};
  assert_eq!(svc.cancel_batch_scoped(stale,&ids),Err(ServiceError::NotOwner));
  assert_eq!(svc.cancel_batch_scoped(owner(),&[ids[0],ids[0]]),
   Err(ServiceError::NotOwner));
  assert_eq!(svc.cancel_batch_scoped(owner(),&ids),Ok(2));
  assert_eq!(svc.active_job_ids_scoped(owner(),&mut ids),Ok(0));
 }
}
