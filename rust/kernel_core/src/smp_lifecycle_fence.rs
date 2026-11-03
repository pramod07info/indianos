//! R131: bounded single-BSP to one-secondary-CPU lifecycle rendezvous.
//!
//! Genuine inter-CPU atomic release/acquire ordering on two QEMU vCPUs is
//! exercised by the opt-in R118 AP fixture. This is a *protocol primitive*,
//! not APIC interrupt routing, TLB shootdown, SMP scheduler or IOMMU support.
//! A CPU is identified by its trusted boot local-APIC ID, not user input.
//! BSP alone publishes monotonically increasing generations; AP alone
//! acknowledges one receipt after doing its local proof step. No overwrite,
//! no wrap, no allocate, no unbounded blocking within the primitive.
use core::sync::atomic::{fence,AtomicU64,Ordering};

#[derive(Debug,Clone,Copy,Eq,PartialEq)]
pub enum FenceError {
    InvalidCpu, InvalidGeneration, StaleGeneration, Busy,
    StaleReceipt, Overflow,
}

#[derive(Debug,Clone,Copy,Eq,PartialEq)]
pub struct Receipt { pub sequence:u64, pub generation:u64 }

pub struct LifecycleFence {
    bsp:u32,
    ap:u32,
    published:AtomicU64,
    acknowledged:AtomicU64,
    generation:AtomicU64,
}
impl LifecycleFence {
    pub const fn new(bsp:u32,ap:u32)->Self{
        Self{bsp,ap,published:AtomicU64::new(0),
             acknowledged:AtomicU64::new(0),generation:AtomicU64::new(0)}
    }
    /// BSP only; one outstanding publication at most.
    pub fn publish(&self,cpu:u32,generation:u64)->Result<Receipt,FenceError>{
        if cpu!=self.bsp || self.bsp==self.ap{return Err(FenceError::InvalidCpu);}
        if generation==0{return Err(FenceError::InvalidGeneration);}
        let published=self.published.load(Ordering::Acquire);
        if self.acknowledged.load(Ordering::Acquire)!=published{
            return Err(FenceError::Busy);
        }
        if generation<=self.generation.load(Ordering::Acquire){
            return Err(FenceError::StaleGeneration);
        }
        let next=published.checked_add(1).ok_or(FenceError::Overflow)?;
        self.generation.store(generation,Ordering::Relaxed);
        // Release publishes the generation/slot before the AP reads it.
        self.published.store(next,Ordering::Release);
        Ok(Receipt{sequence:next,generation})
    }
    /// AP only. The caller must use its actual verified hardware APIC ID.
    /// An ack is NOT an IPI ack or a TLB invalidation; callers must complete
    /// required local cleanup before using this release-store in production.
    pub fn observe_and_ack(
        &self,cpu:u32,last_seen:u64
    )->Result<Option<Receipt>,FenceError>{
        if cpu!=self.ap || self.bsp==self.ap{return Err(FenceError::InvalidCpu);}
        let seq=self.published.load(Ordering::Acquire);
        if seq==last_seen{return Ok(None);}
        if seq!=last_seen.checked_add(1).ok_or(FenceError::Overflow)?{
            return Err(FenceError::StaleReceipt);
        }
        let generation=self.generation.load(Ordering::Relaxed);
        if generation==0{return Err(FenceError::InvalidGeneration);}
        let receipt=Receipt{sequence:seq,generation};
        // Orders the local confirmation before the published ack.
        fence(Ordering::SeqCst);
        self.acknowledged.store(seq,Ordering::Release);
        Ok(Some(receipt))
    }
    /// BSP cannot treat an ack for an earlier generation as the current one.
    pub fn acknowledged(
        &self,cpu:u32,receipt:Receipt
    )->Result<bool,FenceError>{
        if cpu!=self.bsp{return Err(FenceError::InvalidCpu);}
        if self.published.load(Ordering::Acquire)!=receipt.sequence ||
           self.generation.load(Ordering::Acquire)!=receipt.generation{
            return Err(FenceError::StaleReceipt);
        }
        Ok(self.acknowledged.load(Ordering::Acquire)==receipt.sequence)
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn r131_ordered_generation_publication_and_ap_ack(){
        let fence=LifecycleFence::new(0,1);
        assert_eq!(fence.observe_and_ack(1,0),Ok(None));
        let first=fence.publish(0,4).unwrap();
        assert_eq!(first,Receipt{sequence:1,generation:4});
        assert_eq!(fence.publish(0,5),Err(FenceError::Busy));
        assert_eq!(fence.observe_and_ack(0,0),Err(FenceError::InvalidCpu));
        assert_eq!(fence.observe_and_ack(1,0),Ok(Some(first)));
        assert_eq!(fence.acknowledged(0,first),Ok(true));
        let second=fence.publish(0,9).unwrap();
        assert_eq!(fence.acknowledged(0,first),Err(FenceError::StaleReceipt));
        assert_eq!(fence.observe_and_ack(1,1),Ok(Some(second)));
        assert_eq!(fence.acknowledged(0,second),Ok(true));
        assert_eq!(fence.publish(0,9),Err(FenceError::StaleGeneration));
        assert_eq!(fence.publish(0,0),Err(FenceError::InvalidGeneration));
    }
    #[test]
    fn r131_foreign_cpu_and_stale_receipt_are_never_accepted(){
        let fence=LifecycleFence::new(3,7);
        assert_eq!(fence.publish(7,1),Err(FenceError::InvalidCpu));
        assert_eq!(fence.publish(3,2),Ok(Receipt{sequence:1,generation:2}));
        assert_eq!(fence.observe_and_ack(8,0),Err(FenceError::InvalidCpu));
        assert_eq!(fence.observe_and_ack(7,2),Err(FenceError::StaleReceipt));
        assert_eq!(fence.acknowledged(7,Receipt{sequence:1,generation:2}),
                   Err(FenceError::InvalidCpu));
        assert_eq!(fence.observe_and_ack(7,0),
                   Ok(Some(Receipt{sequence:1,generation:2})));
    }
    #[test]
    fn r131_actual_host_two_threads_exchange_64_ordered_receipts(){
        use std::sync::Arc;
        let fence=Arc::new(LifecycleFence::new(0,1));
        let ap=Arc::clone(&fence);
        let worker=std::thread::spawn(move||{
            let mut last=0;
            for expected in 1..=64u64{
                loop{
                    if let Some(receipt)=ap.observe_and_ack(1,last).unwrap(){
                        assert_eq!(receipt.sequence,expected);
                        assert_eq!(receipt.generation,expected*2);
                        last=receipt.sequence;
                        break;
                    }
                    std::thread::yield_now();
                }
            }
        });
        for seq in 1..=64u64{
            let receipt=fence.publish(0,seq*2).unwrap();
            while !fence.acknowledged(0,receipt).unwrap(){
                std::thread::yield_now();
            }
        }
        worker.join().unwrap();
    }
}
