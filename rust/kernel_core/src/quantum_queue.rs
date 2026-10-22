//! R017.4 bounded cooperative quantum job queue.
//! No allocator, threads, interrupts or privileged user pointer access.
//! The caller owns exclusive mutable access and invokes run_next explicitly.
use crate::quantum::AMPLITUDES;
use crate::quantum_protocol::{decode,execute_probabilities,ProtocolError,MAX_REQUEST_BYTES};
pub const MAX_JOBS:usize=4;
pub const RESULT_BYTES:usize=AMPLITUDES*8;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum JobStatus{Queued,Completed,Failed(ProtocolError)}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum QueueError{Full,InvalidRequest(ProtocolError),NotFound,IdExhausted}
#[derive(Clone,Copy)]
struct Job {
    id:u64,
    len:usize,
    request:[u8;MAX_REQUEST_BYTES],
    result:[u8;RESULT_BYTES],
    status:JobStatus,
}
impl Job {
    const fn empty()->Self{
        Self{id:0,len:0,request:[0;MAX_REQUEST_BYTES],
             result:[0;RESULT_BYTES],status:JobStatus::Queued}
    }
}
pub struct QuantumQueue{jobs:[Job;MAX_JOBS],next_id:u64}
impl Default for QuantumQueue{fn default()->Self{Self::new()}}
impl QuantumQueue {
    pub const fn new()->Self{Self{jobs:[Job::empty();MAX_JOBS],next_id:1}}
    /// Validate before taking a slot. Caller bytes are copied, not retained.
    pub fn submit(&mut self,request:&[u8])->Result<u64,QueueError>{
        decode(request).map_err(QueueError::InvalidRequest)?;
        if self.next_id==0{return Err(QueueError::IdExhausted);}
        let slot=self.jobs.iter_mut().find(|j|j.id==0).ok_or(QueueError::Full)?;
        let id=self.next_id;
        self.next_id=self.next_id.checked_add(1).unwrap_or(0);
        *slot=Job::empty();
        slot.id=id;
        slot.len=request.len();
        slot.request[..request.len()].copy_from_slice(request);
        Ok(id)
    }
    /// Executes one queued job in FIFO submission order.
    /// Returns None when no jobs remain queued.
    pub fn run_next(&mut self)->Option<u64>{
        let slot=self.jobs.iter_mut().filter(|j|j.id!=0 && j.status==JobStatus::Queued)
            .min_by_key(|j|j.id)?;
        let id=slot.id;
        slot.status=match execute_probabilities(&slot.request[..slot.len],&mut slot.result){
            Ok(_)=>JobStatus::Completed,
            Err(e)=>JobStatus::Failed(e),
        };
        Some(id)
    }
    pub fn status(&self,id:u64)->Result<JobStatus,QueueError>{
        self.jobs.iter().find(|j|id!=0 && j.id==id)
            .map(|j|j.status).ok_or(QueueError::NotFound)
    }
    /// Copy a completed result into caller-owned memory.
    /// An incomplete job is not readable.
    pub fn result(&self,id:u64,out:&mut [u8;RESULT_BYTES])->Result<(),QueueError>{
        let slot=self.jobs.iter().find(|j|id!=0 && j.id==id).ok_or(QueueError::NotFound)?;
        if slot.status!=JobStatus::Completed{return Err(QueueError::NotFound);}
        *out=slot.result;
        Ok(())
    }
    /// Reclaim a completed/failed job or cancel one that is still queued.
    pub fn release(&mut self,id:u64)->Result<(),QueueError>{
        let slot=self.jobs.iter_mut().find(|j|id!=0 && j.id==id).ok_or(QueueError::NotFound)?;
        *slot=Job::empty();
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn prob(data:&[u8],basis:usize)->f64{
        f64::from_le_bytes(data[basis*8..basis*8+8].try_into().unwrap())
    }
    #[test] fn fifo_and_bell_results(){
        let mut q=QuantumQueue::new();
        let first=q.submit(&[1,2,2,0,1,0,0,4,0,1]).unwrap();
        let second=q.submit(&[1,1,1,0,2,0,0]).unwrap();
        assert_eq!(q.status(first),Ok(JobStatus::Queued));
        assert_eq!(q.run_next(),Some(first));
        let mut out=[0u8;RESULT_BYTES];
        q.result(first,&mut out).unwrap();
        assert!((prob(&out,0)-0.5).abs()<1e-12);
        assert!((prob(&out,3)-0.5).abs()<1e-12);
        assert_eq!(q.run_next(),Some(second));
        assert_eq!(q.run_next(),None);
        q.release(first).unwrap();
        assert_eq!(q.status(first),Err(QueueError::NotFound));
    }
    #[test] fn full_invalid_and_cancel(){
        let mut q=QuantumQueue::new();
        assert_eq!(q.submit(&[1,2,1,0,4,0,0]),
            Err(QueueError::InvalidRequest(ProtocolError::InvalidGate)));
        let mut ids=[0;MAX_JOBS];
        for id in &mut ids{*id=q.submit(&[1,1,0,0]).unwrap();}
        assert_eq!(q.submit(&[1,1,0,0]),Err(QueueError::Full));
        q.release(ids[1]).unwrap();
        assert_eq!(q.status(ids[1]),Err(QueueError::NotFound));
        let replacement=q.submit(&[1,1,0,0]).unwrap();
        assert!(replacement>ids[3]);
        assert_eq!(q.run_next(),Some(ids[0]));
        assert_eq!(q.run_next(),Some(ids[2]));
        assert_eq!(q.run_next(),Some(ids[3]));
        assert_eq!(q.run_next(),Some(replacement));
    }
}
