//! R131: bounded single-BSP to one-secondary-CPU lifecycle rendezvous.
//!
//! Genuine inter-CPU atomic release/acquire ordering on two QEMU vCPUs is
//! exercised by the opt-in R118 AP fixture. This is a *protocol primitive*,
//! not APIC interrupt routing, TLB shootdown, SMP scheduler or IOMMU support.
//! A CPU is identified by its trusted boot local-APIC ID, not user input.
//! BSP alone publishes monotonically increasing generations; AP alone
//! acknowledges one receipt after doing its local proof step. No overwrite,
//! no wrap, no allocate, no unbounded blocking within the primitive.
use core::sync::atomic::{fence,AtomicBool,AtomicU64,Ordering};

#[derive(Debug,Clone,Copy,Eq,PartialEq)]
pub enum FenceError {
    InvalidCpu, InvalidGeneration, StaleGeneration, Busy,
    StaleReceipt, Overflow, NoProgress,
}

#[derive(Debug,Clone,Copy,Eq,PartialEq)]
pub struct Receipt { pub sequence:u64, pub generation:u64 }

pub struct LifecycleFence {
    bsp:u32,
    ap:u32,
    published:AtomicU64,
    acknowledged:AtomicU64,
    generation:AtomicU64,
    publishing:AtomicBool,
}
impl LifecycleFence {
    pub const fn new(bsp:u32,ap:u32)->Self{
        Self{bsp,ap,published:AtomicU64::new(0),
             acknowledged:AtomicU64::new(0),generation:AtomicU64::new(0),
             publishing:AtomicBool::new(false)}
    }
    /// BSP only; one outstanding publication at most.
    pub fn publish(&self,cpu:u32,generation:u64)->Result<Receipt,FenceError>{
        if cpu!=self.bsp || self.bsp==self.ap{return Err(FenceError::InvalidCpu);}
        if generation==0{return Err(FenceError::InvalidGeneration);}
        // The real caller must be the trusted BSP, but make the primitive
        // resistant to unexpected reentrancy too: two publishers must not
        // both read the same acknowledged slot and issue duplicate receipts.
        // No blocking or spinning on an interrupt-sensitive path.
        self.publishing.compare_exchange(false,true,Ordering::AcqRel,
            Ordering::Acquire).map_err(|_|FenceError::Busy)?;
        let result=(||{
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
        })();
        self.publishing.store(false,Ordering::Release);
        result
    }
    /// AP only. The caller must use its actual verified hardware APIC ID.
    /// An ack is NOT an IPI ack or a TLB invalidation; callers must complete
    /// required local cleanup before using this release-store in production.
    pub fn observe_and_ack(
        &self,cpu:u32,last_seen:u64
    )->Result<Option<Receipt>,FenceError>{
        if cpu!=self.ap || self.bsp==self.ap{return Err(FenceError::InvalidCpu);}
        let seq=self.published.load(Ordering::Acquire);
        if seq==last_seen{
            // None means no NEW publication after a genuine prior AP ack.
            // A caller must not claim a publication was already seen when
            // its acknowledgement is still outstanding.
            return if self.acknowledged.load(Ordering::Acquire)==seq{
                Ok(None)
            }else{
                Err(FenceError::StaleReceipt)
            };
        }
        if seq!=last_seen.checked_add(1).ok_or(FenceError::Overflow)?{
            return Err(FenceError::StaleReceipt);
        }
        let generation=self.generation.load(Ordering::Relaxed);
        if generation==0{return Err(FenceError::InvalidGeneration);}
        let receipt=Receipt{sequence:seq,generation};
        // One-time acknowledgement: even two AP threads claiming the same
        // trusted ID must never both emit a receipt for the same sequence.
        // Release ordering publishes the caller's local proof before BSP
        // observes the completion. No lock or spin in this primitive.
        fence(Ordering::SeqCst);
        self.acknowledged.compare_exchange(
            last_seen,seq,Ordering::AcqRel,Ordering::Acquire
        ).map_err(|_|FenceError::StaleReceipt)?;
        Ok(Some(receipt))
    }
    /// BSP-side bounded software probe for a previously published AP receipt.
    /// The budget counts polling probes, NOT wall-clock time. This MUST NOT
    /// run in an interrupt handler or while holding an SMP scheduler lock.
    /// An unresponsive AP leaves the publication outstanding (fail closed):
    /// the caller must not recycle memory or issue another lifecycle receipt.
    /// Actual IPI delivery, timeout clocks and TLB shootdown are NOT provided.
    pub fn poll_ack_bounded(
        &self,cpu:u32,receipt:Receipt,budget:u32
    )->Result<(),FenceError>{
        if self.acknowledged(cpu,receipt)?{return Ok(());}
        for _ in 0..budget{
            core::hint::spin_loop();
            if self.acknowledged(cpu,receipt)?{return Ok(());}
        }
        Err(FenceError::NoProgress)
    }
    /// BSP cannot treat an ack for an earlier generation as the current one.
    pub fn acknowledged(
        &self,cpu:u32,receipt:Receipt
    )->Result<bool,FenceError>{
        if cpu!=self.bsp{return Err(FenceError::InvalidCpu);}
        // Sequence zero is the initial empty slot, never a published receipt.
        // Without this guard (0,0) matched the initial atomic values and
        // incorrectly reported that an AP had acknowledged a lifecycle IPI.
        if receipt.sequence==0 || receipt.generation==0 {
            return Err(FenceError::StaleReceipt);
        }
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
    fn r121_initial_and_partially_zero_receipts_cannot_fake_an_ack(){
        let fence=LifecycleFence::new(0,1);
        assert_eq!(fence.acknowledged(0,Receipt{sequence:0,generation:0}),
                   Err(FenceError::StaleReceipt));
        assert_eq!(fence.acknowledged(0,Receipt{sequence:0,generation:1}),
                   Err(FenceError::StaleReceipt));
        assert_eq!(fence.acknowledged(0,Receipt{sequence:1,generation:0}),
                   Err(FenceError::StaleReceipt));
        let issued=fence.publish(0,5).unwrap();
        assert_eq!(fence.acknowledged(0,issued),Ok(false));
        assert_eq!(fence.observe_and_ack(1,0),Ok(Some(issued)));
        assert_eq!(fence.acknowledged(0,issued),Ok(true));
        assert_eq!(fence.acknowledged(0,Receipt{sequence:0,generation:0}),
                   Err(FenceError::StaleReceipt));
    }
    #[test]
    fn r121_concurrent_bsp_claims_cannot_issue_duplicate_receipts(){
        use std::sync::{Arc,Barrier};
        let fence=Arc::new(LifecycleFence::new(0,1));
        let start=Arc::new(Barrier::new(8));
        let threads:std::vec::Vec<_>=(0..8).map(|n|{
            let f=Arc::clone(&fence);
            let barrier=Arc::clone(&start);
            std::thread::spawn(move||{
                barrier.wait();
                f.publish(0,n+1)
            })
        }).collect();
        let mut accepted=None;
        for t in threads {
            match t.join().unwrap() {
                Ok(receipt)=>{
                    assert!(accepted.replace(receipt).is_none(),
                        "duplicate outstanding publication");
                },
                Err(FenceError::Busy)=>{},
                Err(e)=>panic!("unexpected publication result: {e:?}"),
            }
        }
        let issued=accepted.expect("exactly one BSP publication");
        assert_eq!(issued.sequence,1);
        assert_eq!(fence.acknowledged(0,issued),Ok(false));
        assert_eq!(fence.observe_and_ack(1,0),Ok(Some(issued)));
        assert_eq!(fence.acknowledged(0,issued),Ok(true));
        assert_eq!(fence.publish(0,issued.generation),
                   Err(FenceError::StaleGeneration));
        assert_eq!(fence.publish(1,issued.generation+1),
                   Err(FenceError::InvalidCpu));
        let second=fence.publish(0,issued.generation+1).unwrap();
        assert_eq!(second.sequence,2);
    }
    #[test]
    fn r121_duplicate_ap_ack_and_premature_last_seen_are_denied(){
        let fence=LifecycleFence::new(0,1);
        assert_eq!(fence.observe_and_ack(1,0),Ok(None));
        let first=fence.publish(0,9).unwrap();
        assert_eq!(fence.observe_and_ack(1,1),
                   Err(FenceError::StaleReceipt));
        assert_eq!(fence.acknowledged(0,first),Ok(false));
        assert_eq!(fence.observe_and_ack(1,0),Ok(Some(first)));
        assert_eq!(fence.observe_and_ack(1,0),
                   Err(FenceError::StaleReceipt));
        assert_eq!(fence.observe_and_ack(1,1),Ok(None));
        let next=fence.publish(0,10).unwrap();
        assert_eq!(fence.observe_and_ack(1,0),
                   Err(FenceError::StaleReceipt));
        assert_eq!(fence.observe_and_ack(1,1),Ok(Some(next)));
        assert_eq!(fence.observe_and_ack(1,1),
                   Err(FenceError::StaleReceipt));
    }
    #[test]
    fn r121_concurrent_claimed_ap_acks_are_single_use(){
        use std::sync::{Arc,Barrier};
        let fence=Arc::new(LifecycleFence::new(0,1));
        let issued=fence.publish(0,5).unwrap();
        let barrier=Arc::new(Barrier::new(2));
        let mut handles=std::vec::Vec::new();
        for _ in 0..2{
            let f=Arc::clone(&fence);
            let b=Arc::clone(&barrier);
            handles.push(std::thread::spawn(move||{
                b.wait();
                f.observe_and_ack(1,0)
            }));
        }
        let replies=handles.into_iter().map(|h|h.join().unwrap())
            .collect::<std::vec::Vec<_>>();
        assert_eq!(replies.iter().filter(|result|**result==Ok(Some(issued))).count(),1);
        assert_eq!(replies.iter().filter(|result|
            **result==Err(FenceError::StaleReceipt)).count(),1);
        assert_eq!(fence.acknowledged(0,issued),Ok(true));
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
    #[test]
    fn r122_no_progress_does_not_recycle_outstanding_ap_receipt(){
        let f=LifecycleFence::new(0,1);
        let issued=f.publish(0,11).unwrap();
        assert_eq!(f.poll_ack_bounded(0,issued,4),Err(FenceError::NoProgress));
        assert_eq!(f.publish(0,12),Err(FenceError::Busy));
        assert_eq!(f.poll_ack_bounded(0,Receipt{
            sequence:issued.sequence,generation:12},1),Err(FenceError::StaleReceipt));
        assert_eq!(f.poll_ack_bounded(1,issued,1),Err(FenceError::InvalidCpu));
        assert_eq!(f.acknowledged(0,issued),Ok(false));
        assert_eq!(f.observe_and_ack(1,0),Ok(Some(issued)));
        assert_eq!(f.poll_ack_bounded(0,issued,1),Ok(()));
        let next=f.publish(0,12).unwrap();
        assert_eq!(next.sequence,issued.sequence+1);
        assert_eq!(f.poll_ack_bounded(0,issued,1),Err(FenceError::StaleReceipt));
    }

}
