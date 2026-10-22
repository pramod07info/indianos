//! R017.5: ownership-aware quantum service facade.
//! Kernel-side policy building block, NOT a ring-3 syscall implementation.
//! Callers must supply an authenticated nonzero process identity from the
//! kernel process table; never trust an owner ID copied from a user request.
use crate::quantum_queue::{QuantumQueue,QueueError,JobStatus,RESULT_BYTES,MAX_JOBS};
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum ServiceError{InvalidOwner,NotOwner,Queue(QueueError)}
impl From<QueueError> for ServiceError{
    fn from(e:QueueError)->Self{Self::Queue(e)}
}
#[derive(Clone,Copy)]
struct Ownership{job:u64,owner:u32}
impl Ownership{const fn empty()->Self{Self{job:0,owner:0}}}
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
        if self.owners.iter().any(|x|x.job==id && id!=0 && x.owner==owner){
            Ok(())
        }else{Err(ServiceError::NotOwner)}
    }
    pub fn submit(&mut self,owner:u32,request:&[u8])->Result<u64,ServiceError>{
        if owner==0{return Err(ServiceError::InvalidOwner);}
        // No ID is consumed unless the queue accepts the request.
        let id=self.queue.submit(request)?;
        let entry=self.owners.iter_mut().find(|x|x.job==0)
            .expect("ownership slots track queue slots");
        *entry=Ownership{job:id,owner};
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
}
