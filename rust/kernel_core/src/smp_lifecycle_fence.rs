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
    StaleReceipt, Overflow, NoProgress, IncompleteLocalWork,
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
    // A local AP cleanup must not run twice when two host threads claim
    // the same published sequence. This software gate is NOT an IPI lock.
    verifying:AtomicBool,
}
impl LifecycleFence {
    pub const fn new(bsp:u32,ap:u32)->Self{
        Self{bsp,ap,published:AtomicU64::new(0),
             acknowledged:AtomicU64::new(0),generation:AtomicU64::new(0),
             publishing:AtomicBool::new(false),
             verifying:AtomicBool::new(false)}
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
    /// The compatibility protocol has no local side-effect. For real AP
    /// lifecycle work prefer observe_and_ack_after, which validates completion
    /// BEFORE issuing the release acknowledgement.
    pub fn observe_and_ack(
        &self,cpu:u32,last_seen:u64
    )->Result<Option<Receipt>,FenceError>{
        self.observe_and_ack_after(cpu,last_seen,|_|true)
    }
    /// AP only. Call the supplied (single-use) local proof before acking.
    /// False means NO acknowledgement; BSP is still forbidden to recycle
    /// the publication. This does not provide an actual IPI or TLB flush.
    pub fn observe_and_ack_after(
        &self,cpu:u32,last_seen:u64,local_complete:impl FnOnce(Receipt)->bool
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
        // R122 CI regression: a replay must NOT run cleanup again. Merely
        // checking the final acknowledgement after the callback is too late.
        if self.acknowledged.load(Ordering::Acquire)!=last_seen{
            return Err(FenceError::StaleReceipt);
        }
        // Two would-be AP claimants must not both execute kernel cleanup.
        // The losing claimant may retry with fresh state; no spinning.
        self.verifying.compare_exchange(false,true,
            Ordering::AcqRel,Ordering::Acquire)
            .map_err(|_|FenceError::StaleReceipt)?;
        let result=(||{
            // Re-check under the single-callback gate: publication may
            // advance while the first claimant is already returning.
            if self.published.load(Ordering::Acquire)!=seq ||
               self.acknowledged.load(Ordering::Acquire)!=last_seen{
                return Err(FenceError::StaleReceipt);
            }
            let generation=self.generation.load(Ordering::Relaxed);
            if generation==0{return Err(FenceError::InvalidGeneration);}
            let receipt=Receipt{sequence:seq,generation};
            if !local_complete(receipt){
                return Err(FenceError::IncompleteLocalWork);
            }
            // Only completed local proof becomes visible to the BSP.
            fence(Ordering::SeqCst);
            self.acknowledged.compare_exchange(
                last_seen,seq,Ordering::AcqRel,Ordering::Acquire
            ).map_err(|_|FenceError::StaleReceipt)?;
            Ok(Some(receipt))
        })();
        self.verifying.store(false,Ordering::Release);
        result
    }
    /// Read-only BSP diagnostic for the one outstanding AP publication.
    /// This is not an IPI delivery or completion guarantee: callers must
    /// revalidate a returned receipt before recycling any kernel resource.
    /// An in-progress publisher yields Busy instead of a mixed snapshot.
    pub fn pending_receipt(&self,cpu:u32)->Result<Option<Receipt>,FenceError>{
        if cpu!=self.bsp || self.bsp==self.ap{
            return Err(FenceError::InvalidCpu);
        }
        if self.publishing.load(Ordering::Acquire){
            return Err(FenceError::Busy);
        }
        let sequence=self.published.load(Ordering::Acquire);
        let generation=self.generation.load(Ordering::Acquire);
        let acknowledged=self.acknowledged.load(Ordering::Acquire);
        if self.publishing.load(Ordering::Acquire) ||
           self.published.load(Ordering::Acquire)!=sequence{
            return Err(FenceError::Busy);
        }
        if sequence==0 || acknowledged==sequence{return Ok(None);}
        if generation==0{return Err(FenceError::InvalidGeneration);}
        Ok(Some(Receipt{sequence,generation}))
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
    /// Fail closed unless the exact AP generation has acknowledged cleanup.
    /// Caller must still arrange real TLB shootdown/IPI where required.
    pub fn require_ack_for_reclaim(&self,cpu:u32,receipt:Receipt)
        ->Result<(),FenceError>{
        if self.acknowledged(cpu,receipt)?{Ok(())}
        else{Err(FenceError::NoProgress)}
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

    #[test]
    fn r122_two_claimants_cannot_run_duplicate_local_cleanup(){
        use std::sync::{Arc,Barrier};
        use std::sync::atomic::{AtomicU32,Ordering};
        let fence=Arc::new(LifecycleFence::new(0,1));
        let issued=fence.publish(0,44).unwrap();
        let hit=Arc::new(AtomicU32::new(0));
        let start=Arc::new(Barrier::new(2));
        let mut threads=std::vec::Vec::new();
        for _ in 0..2{
            let f=Arc::clone(&fence);
            let count=Arc::clone(&hit);
            let gate=Arc::clone(&start);
            threads.push(std::thread::spawn(move||{
                gate.wait();
                f.observe_and_ack_after(1,0,|r|{
                    assert_eq!(r,issued);
                    count.fetch_add(1,Ordering::AcqRel);
                    true
                })
            }));
        }
        let results=threads.into_iter().map(|x|x.join().unwrap())
            .collect::<std::vec::Vec<_>>();
        assert_eq!(results.iter().filter(|x|**x==Ok(Some(issued))).count(),1);
        assert_eq!(results.iter().filter(|x|
            **x==Err(FenceError::StaleReceipt)).count(),1);
        assert_eq!(hit.load(Ordering::Acquire),1);
        assert_eq!(fence.acknowledged(0,issued),Ok(true));
        // A successfully acknowledged packet never reruns the callback.
        assert_eq!(fence.observe_and_ack_after(1,0,|_|{
            panic!("duplicate AP cleanup after ACK")
        }),Err(FenceError::StaleReceipt));
    }

    #[test]
    fn r122_ap_rejects_local_cleanup_failure_without_acknowledging(){
        let f=LifecycleFence::new(0,1);
        let receipt=f.publish(0,22).unwrap();
        let mut called=0u32;
        assert_eq!(f.observe_and_ack_after(1,0,|r|{
            called+=1;
            assert_eq!(r,receipt);
            false
        }),Err(FenceError::IncompleteLocalWork));
        assert_eq!(called,1);
        assert_eq!(f.acknowledged(0,receipt),Ok(false));
        assert_eq!(f.publish(0,24),Err(FenceError::Busy));
        assert_eq!(f.observe_and_ack_after(1,0,|r|{
            r.generation==23
        }),Err(FenceError::IncompleteLocalWork));
        assert_eq!(f.poll_ack_bounded(0,receipt,1),Err(FenceError::NoProgress));
        assert_eq!(f.observe_and_ack_after(0,0,|_|{
            panic!("wrong-CPU cleanup must never run")
        }),Err(FenceError::InvalidCpu));
        assert_eq!(f.observe_and_ack_after(1,0,|r|{
            r.sequence==1 && r.generation==22
        }),Ok(Some(receipt)));
        assert_eq!(f.acknowledged(0,receipt),Ok(true));
        assert_eq!(f.observe_and_ack_after(1,0,|_|{
            panic!("replayed local cleanup must never run")
        }),Err(FenceError::StaleReceipt));
    }

}

#[cfg(test)]
mod r123_bsp_pending_receipt_tests {
    use super::*;
    #[test]
    fn outstanding_receipt_is_observable_but_not_recyclable() {
        let f=LifecycleFence::new(0,1);
        assert_eq!(f.pending_receipt(0),Ok(None));
        assert_eq!(f.pending_receipt(1),Err(FenceError::InvalidCpu));
        let first=f.publish(0,51).unwrap();
        assert_eq!(f.pending_receipt(0),Ok(Some(first)));
        assert_eq!(f.poll_ack_bounded(0,first,0),Err(FenceError::NoProgress));
        assert_eq!(f.publish(0,52),Err(FenceError::Busy));
        assert_eq!(f.observe_and_ack_after(1,0,|receipt|receipt==first),
                   Ok(Some(first)));
        assert_eq!(f.pending_receipt(0),Ok(None));
        let second=f.publish(0,52).unwrap();
        assert_eq!(f.pending_receipt(0),Ok(Some(second)));
        assert_eq!(f.acknowledged(0,first),Err(FenceError::StaleReceipt));
    }
}

#[cfg(test)]
mod m1_ws17_reclaim_ack_tests {
 use super::*;
 #[test] fn memory_reclaim_never_accepts_pending_or_replayed_ap_ack() {
  let fence=LifecycleFence::new(0,1);
  assert_eq!(fence.require_ack_for_reclaim(0,Receipt{sequence:0,generation:0}),
      Err(FenceError::StaleReceipt));
  let r=fence.publish(0,5).unwrap();
  assert_eq!(fence.require_ack_for_reclaim(0,r),Err(FenceError::NoProgress));
  assert_eq!(fence.require_ack_for_reclaim(1,r),Err(FenceError::InvalidCpu));
  fence.observe_and_ack(1,0).unwrap();
  assert_eq!(fence.require_ack_for_reclaim(0,r),Ok(()));
  let next=fence.publish(0,6).unwrap();
  assert_eq!(fence.require_ack_for_reclaim(0,r),Err(FenceError::StaleReceipt));
  assert_eq!(fence.require_ack_for_reclaim(0,next),Err(FenceError::NoProgress));
 }
}


// PR782 WS62-64: exact-generation kernel reclaim coordination.
// These are software barriers only, not physical IPIs or TLB shootdown.
impl LifecycleFence{
    /// WS62: Inspect the BSP's *currently pending* publication and require
    /// its complete identity. An already acknowledged receipt is not pending.
    pub fn require_pending_exact(&self,cpu:u32,expected:Receipt)
        ->Result<(),FenceError>{
        if expected.sequence==0||expected.generation==0{
            return Err(FenceError::InvalidGeneration);
        }
        match self.pending_receipt(cpu)? {
            Some(live) if live==expected=>Ok(()),
            _=>Err(FenceError::StaleReceipt)
        }
    }
    /// WS64: A next publication must depend on the explicitly verified
    /// previous generation, not just a numeric monotonic generation increase.
    pub fn publish_after_exact_ack(&self,cpu:u32,previous:Receipt,
        next_generation:u64)->Result<Receipt,FenceError>{
        self.require_ack_for_reclaim(cpu,previous)?;
        self.publish(cpu,next_generation)
    }
}
/// WS63: Software admission gate for two independent AP lifecycle receipts.
/// Caller must still execute real CPU-local invalidations and verify hardware.
pub fn require_dual_ack_for_reclaim(
    first:&LifecycleFence,first_bsp:u32,first_receipt:Receipt,
    second:&LifecycleFence,second_bsp:u32,second_receipt:Receipt
)->Result<(),FenceError>{
    first.require_ack_for_reclaim(first_bsp,first_receipt)?;
    second.require_ack_for_reclaim(second_bsp,second_receipt)?;
    Ok(())
}
#[cfg(test)]
mod pr782_ws62_64_tests{
    use super::*;
    #[test]fn ws62_only_live_exact_pending_receipt_is_admitted(){
        let f=LifecycleFence::new(0,1);
        let receipt=f.publish(0,4).unwrap();
        assert_eq!(f.require_pending_exact(0,receipt),Ok(()));
        assert_eq!(f.require_pending_exact(0,Receipt{generation:5,..receipt}),
            Err(FenceError::StaleReceipt));
        assert_eq!(f.require_pending_exact(2,receipt),Err(FenceError::InvalidCpu));
        f.observe_and_ack_after(1,0,|_|true).unwrap();
        assert_eq!(f.require_pending_exact(0,receipt),Err(FenceError::StaleReceipt));
    }
    #[test]fn ws63_dual_ack_denies_either_unacknowledged_generation(){
        let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(2,3);
        let ra=a.publish(0,7).unwrap();let rb=b.publish(2,8).unwrap();
        a.observe_and_ack_after(1,0,|_|true).unwrap();
        assert_eq!(require_dual_ack_for_reclaim(&a,0,ra,&b,2,rb),
            Err(FenceError::NoProgress));
        b.observe_and_ack_after(3,0,|_|true).unwrap();
        assert_eq!(require_dual_ack_for_reclaim(&a,0,ra,&b,2,rb),Ok(()));
        assert_eq!(require_dual_ack_for_reclaim(&a,0,ra,&b,2,
            Receipt{generation:9,..rb}),Err(FenceError::StaleReceipt));
    }
    #[test]fn ws64_previous_ack_must_match_before_advancing(){
        let f=LifecycleFence::new(0,1);
        let one=f.publish(0,7).unwrap();
        assert_eq!(f.publish_after_exact_ack(0,one,8),Err(FenceError::NoProgress));
        f.observe_and_ack_after(1,0,|_|true).unwrap();
        assert_eq!(f.publish_after_exact_ack(0,Receipt{generation:6,..one},8),
            Err(FenceError::StaleReceipt));
        let two=f.publish_after_exact_ack(0,one,8).unwrap();
        assert_eq!(two.generation,8);
        assert_eq!(f.require_pending_exact(0,two),Ok(()));
    }
}

/// BSP-only multi-AP reclaim barrier: every independently owned receipt
/// must be acknowledged at its exact generation. This does not issue IPIs.
pub fn require_all_acks_for_reclaim(receipts:&[(&LifecycleFence,u32,Receipt)])
 ->Result<(),FenceError>{
 if receipts.is_empty(){return Err(FenceError::NoProgress);}
 for &(fence,cpu,receipt) in receipts{
  fence.require_ack_for_reclaim(cpu,receipt)?;
 }
 Ok(())
}
#[cfg(test)]
mod m1_gate20_w5_next_smp_tests{
 use super::*;
 #[test]fn reclaim_requires_both_ap_acknowledgements(){
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(2,3);
  let one=a.publish(0,10).unwrap();let two=b.publish(2,12).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  assert_eq!(require_all_acks_for_reclaim(&[(&a,0,one),(&b,2,two)]),
      Err(FenceError::NoProgress));
  b.observe_and_ack_after(3,0,|_|true).unwrap();
  assert_eq!(require_all_acks_for_reclaim(&[(&a,0,one),(&b,2,two)]),Ok(()));
  assert_eq!(require_all_acks_for_reclaim(&[]),Err(FenceError::NoProgress));
 }
}


/// P0 batch three W1. A privileged CPU driver publishes the exact lifecycle
/// receipt BEFORE delivering an IPI. A failed send leaves the receipt pending,
/// forbidding the BSP from recycling frames. Only the real xAPIC/x2APIC
/// implementation may satisfy this trait in production.
pub trait LifecycleIpiSender {
 fn send_exact(&mut self,from_apic_id:u32,to_apic_id:u32,
  receipt:Receipt)->Result<(),FenceError>;
}
impl LifecycleFence {
 pub fn publish_and_signal<I:LifecycleIpiSender>(
  &self,cpu:u32,ap:u32,generation:u64,sender:&mut I)
  ->Result<Receipt,FenceError>{
  if cpu!=self.bsp||ap!=self.ap||self.bsp==self.ap {
   return Err(FenceError::InvalidCpu);
  }
  let receipt=self.publish(cpu,generation)?;
  sender.send_exact(cpu,ap,receipt)?;
  // No return-path acknowledgement is accepted: the AP must call the local
  // completion path and the BSP must inspect the exact subsequent receipt.
  Ok(receipt)
 }
 /// AP-side completion ordering for a privileged local CPU operation. This
 /// callback MUST execute the actual TLB invalidation or teardown and report
 /// success only after serializing it. A callback error never produces an ack.
 pub fn acknowledge_after_local_invalidate(
  &self,ap:u32,previous:u64,
  invalidate:impl FnOnce(Receipt)->Result<(),FenceError>)
   ->Result<Option<Receipt>,FenceError>{
  self.observe_and_ack_after(ap,previous,|receipt|
    invalidate(receipt).is_ok())
 }
}
#[cfg(test)]
mod kernel_p0_batch3_w1_ipi_delivery_tests{
 use super::*;
 struct Sender{fails:bool,sent:usize,last:Option<Receipt>}
 impl LifecycleIpiSender for Sender{
  fn send_exact(&mut self,_:u32,_:u32,receipt:Receipt)
   ->Result<(),FenceError>{
   self.sent+=1;self.last=Some(receipt);
   if self.fails{Err(FenceError::NoProgress)}else{Ok(())}
  }
 }
 #[test]fn delivery_failure_cannot_fabricate_ack_or_publish_another_generation(){
  let fence=LifecycleFence::new(0,7);
  let mut sender=Sender{fails:true,sent:0,last:None};
  assert_eq!(fence.publish_and_signal(0,7,4,&mut sender),Err(FenceError::NoProgress));
  let pending=sender.last.unwrap();
  assert_eq!(fence.require_pending_exact(0,pending),Ok(()));
  assert_eq!(fence.require_ack_for_reclaim(0,pending),Err(FenceError::NoProgress));
  assert_eq!(fence.publish_and_signal(0,7,5,&mut sender),Err(FenceError::Busy));
  assert_eq!(sender.sent,1);
 }
 #[test]fn cpu_id_exact_delivery_and_local_completion_are_required(){
  let fence=LifecycleFence::new(0,7);
  let mut sender=Sender{fails:false,sent:0,last:None};
  assert_eq!(fence.publish_and_signal(0,8,4,&mut sender),Err(FenceError::InvalidCpu));
  assert_eq!(sender.sent,0);
  let receipt=fence.publish_and_signal(0,7,4,&mut sender).unwrap();
  assert_eq!(fence.acknowledge_after_local_invalidate(7,0,|_|Err(FenceError::NoProgress)),
    Err(FenceError::IncompleteLocalWork));
  assert_eq!(fence.require_ack_for_reclaim(0,receipt),Err(FenceError::NoProgress));
  assert_eq!(fence.acknowledge_after_local_invalidate(7,0,|r|{
   assert_eq!(r,receipt);Ok(())
  }),Ok(Some(receipt)));
  assert_eq!(fence.require_ack_for_reclaim(0,receipt),Ok(()));
  assert_eq!(fence.acknowledge_after_local_invalidate(7,0,|_|Ok(())),
    Err(FenceError::StaleReceipt));
 }
}

/// P0-61..64: each AP must independently acknowledge one exact, bounded
/// nonzero generation before reclaim. Software proof, NOT actual TLB IPIs.
pub fn require_distinct_acks_for_reclaim(receipts:&[(&LifecycleFence,u32,Receipt)])
 ->Result<(),FenceError>{
 if receipts.is_empty()||receipts.len()>64{return Err(FenceError::NoProgress);}
 for (i,&(fence,cpu,r)) in receipts.iter().enumerate(){
  if r.sequence==0||r.generation==0{return Err(FenceError::InvalidGeneration);}
  // A reclaim transaction is scoped to one BSP ownership domain.
  if fence.bsp != receipts[0].0.bsp {return Err(FenceError::InvalidCpu);}
  if receipts[..i].iter().any(|&(prior,_,_)|
     core::ptr::eq(prior,fence)||prior.ap==fence.ap){
   return Err(FenceError::StaleReceipt);
  }
  fence.require_ack_for_reclaim(cpu,r)?;
 }
 Ok(())
}
#[cfg(test)]
mod kernel_p0_batch4_w1_distinct_aps_tests{
 use super::*;
 #[test]fn cannot_count_one_cpu_twice(){
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(0,1);
  let x=a.publish(0,4).unwrap();let y=b.publish(0,5).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  b.observe_and_ack_after(1,0,|_|true).unwrap();
  assert_eq!(require_distinct_acks_for_reclaim(&[(&a,0,x),(&a,0,x)]),
   Err(FenceError::StaleReceipt));
  assert_eq!(require_distinct_acks_for_reclaim(&[(&a,0,x),(&b,0,y)]),
   Err(FenceError::StaleReceipt));
 }
 #[test]fn missing_stale_and_zero_receipts_cannot_unlock_reclaim(){
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(0,2);
  let x=a.publish(0,4).unwrap();let y=b.publish(0,5).unwrap();
  assert_eq!(require_distinct_acks_for_reclaim(&[]),Err(FenceError::NoProgress));
  assert_eq!(require_distinct_acks_for_reclaim(&[(&a,0,Receipt{sequence:0,..x})]),
   Err(FenceError::InvalidGeneration));
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  assert_eq!(require_distinct_acks_for_reclaim(&[(&a,0,x),(&b,0,y)]),
   Err(FenceError::NoProgress));
  b.observe_and_ack_after(2,0,|_|true).unwrap();
  assert_eq!(require_distinct_acks_for_reclaim(&[(&a,0,x),(&b,0,y)]),Ok(()));
  assert_eq!(require_distinct_acks_for_reclaim(&[(&a,0,x),
   (&b,0,Receipt{generation:9,..y})]),Err(FenceError::StaleReceipt));
 }
}

#[cfg(test)]
mod kernel_p0_batch5_w1_coherent_bsp_tests {
 use super::*;
 #[test] fn two_distinct_aps_of_one_bsp_authorize_one_barrier(){
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(0,2);
  let x=a.publish(0,5).unwrap();let y=b.publish(0,6).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  b.observe_and_ack_after(2,0,|_|true).unwrap();
  assert_eq!(require_distinct_acks_for_reclaim(&[(&a,0,x),(&b,0,y)]),Ok(()));
 }
 #[test] fn separate_bsp_domains_cannot_be_merged_into_one_reclaim(){
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(3,4);
  let x=a.publish(0,5).unwrap();let y=b.publish(3,6).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  b.observe_and_ack_after(4,0,|_|true).unwrap();
  assert_eq!(require_distinct_acks_for_reclaim(&[(&a,0,x),(&b,3,y)]),
   Err(FenceError::InvalidCpu));
 }
}
/// P0-101..104: an authenticated kernel topology snapshot names the EXACT
/// set of APs that must finish local teardown. A partial or extra collection
/// cannot authorize reclaim, even when each supplied receipt is authentic.
/// Caller holds the SMP topology lock across this check and actual reclaim.
pub fn require_topology_acks_for_reclaim(
 receipts:&[(&LifecycleFence,u32,Receipt)], expected_aps:&[u32]
)->Result<(),FenceError>{
 if expected_aps.is_empty()||expected_aps.len()>64||
    expected_aps.len()!=receipts.len(){return Err(FenceError::NoProgress);}
 for (i,&ap) in expected_aps.iter().enumerate(){
  if expected_aps[..i].contains(&ap){
   return Err(FenceError::InvalidCpu);
  }
  if !receipts.iter().any(|&(fence,_,_)|fence.ap==ap){
   return Err(FenceError::StaleReceipt);
  }
 }
 for &(fence,cpu,_) in receipts {
  if !expected_aps.contains(&fence.ap)||fence.bsp!=receipts[0].0.bsp||
     cpu!=fence.bsp || fence.ap==fence.bsp {
   return Err(FenceError::InvalidCpu);
  }
 }
 require_distinct_acks_for_reclaim(receipts)
}
#[cfg(test)]
mod kernel_p0_batch6_w1_topology_tests{
 use super::*;
 #[test] fn entire_authenticated_topology_required_not_merely_some_acks(){
  let a=LifecycleFence::new(9,1);let b=LifecycleFence::new(9,2);
  let ra=a.publish(9,4).unwrap();let rb=b.publish(9,5).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  b.observe_and_ack_after(2,0,|_|true).unwrap();
  let proof=[(&a,9,ra),(&b,9,rb)];
  assert_eq!(require_topology_acks_for_reclaim(&proof,&[2,1]),Ok(()));
  assert_eq!(require_topology_acks_for_reclaim(&proof,&[1]),
   Err(FenceError::NoProgress));
  assert_eq!(require_topology_acks_for_reclaim(&proof,&[1,3]),
   Err(FenceError::StaleReceipt));
  assert_eq!(require_topology_acks_for_reclaim(&proof,&[1,1]),
   Err(FenceError::InvalidCpu));
 }
 #[test] fn stale_or_pending_or_foreign_bsp_receipts_never_pass(){
  let a=LifecycleFence::new(9,1);let b=LifecycleFence::new(9,2);
  let ra=a.publish(9,4).unwrap();let rb=b.publish(9,5).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  assert_eq!(require_topology_acks_for_reclaim(&[(&a,9,ra),(&b,9,rb)],
   &[1,2]),Err(FenceError::NoProgress));
  b.observe_and_ack_after(2,0,|_|true).unwrap();
  assert_eq!(require_topology_acks_for_reclaim(&[(&a,9,ra),
   (&b,9,Receipt{generation:6,..rb})],&[1,2]),Err(FenceError::StaleReceipt));
  let foreign=LifecycleFence::new(3,4);
  let rf=foreign.publish(3,7).unwrap();
  foreign.observe_and_ack_after(4,0,|_|true).unwrap();
  assert_eq!(require_topology_acks_for_reclaim(&[(&a,9,ra),(&foreign,3,rf)],
   &[1,4]),Err(FenceError::InvalidCpu));
 }
}


#[cfg(test)]
mod kernel_p0_batch6_w1_zero_apic_id_tests{
 use super::*;
 #[test] fn physical_apic_id_zero_is_valid_for_an_ap_when_bsp_is_nonzero(){
  let f=LifecycleFence::new(9,0);
  let r=f.publish(9,3).unwrap();
  f.observe_and_ack_after(0,0,|_|true).unwrap();
  assert_eq!(require_topology_acks_for_reclaim(&[(&f,9,r)],&[0]),Ok(()));
  assert_eq!(require_topology_acks_for_reclaim(&[(&f,9,r)],&[9]),
   Err(FenceError::StaleReceipt));
 }
}


/// P0-121..124: generation-coherent complete AP topology proof. A collection
/// of individually valid acknowledgements from DIFFERENT release epochs is
/// not one teardown barrier. Caller pins the authenticated topology and holds
/// the kernel's SMP lifecycle lock until the protected reclaim completes.
pub fn require_epoch_topology_acks_for_reclaim(
 receipts:&[(&LifecycleFence,u32,Receipt)],expected_aps:&[u32],
 expected_generation:u64
)->Result<(),FenceError>{
 if expected_generation==0{return Err(FenceError::InvalidGeneration);}
 if receipts.iter().any(|&(_,_,receipt)|receipt.generation!=expected_generation){
  return Err(FenceError::StaleReceipt);
 }
 require_topology_acks_for_reclaim(receipts,expected_aps)
}
#[cfg(test)]
mod kernel_p0_batch7_w1_epoch_coherence_tests{
 use super::*;
 #[test]fn exact_epoch_and_complete_topology_authorize_reclaim(){
  let a=LifecycleFence::new(9,1);let b=LifecycleFence::new(9,2);
  let x=a.publish(9,40).unwrap();let y=b.publish(9,40).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  b.observe_and_ack_after(2,0,|_|true).unwrap();
  let refs=[(&a,9,x),(&b,9,y)];
  assert_eq!(require_epoch_topology_acks_for_reclaim(&refs,&[2,1],40),Ok(()));
  assert_eq!(require_epoch_topology_acks_for_reclaim(&refs,&[1,2],0),
   Err(FenceError::InvalidGeneration));
  assert_eq!(require_epoch_topology_acks_for_reclaim(&refs,&[1],40),
   Err(FenceError::NoProgress));
 }
 #[test]fn mixed_epochs_pending_and_recycled_acks_are_rejected(){
  let a=LifecycleFence::new(9,1);let b=LifecycleFence::new(9,2);
  let x=a.publish(9,40).unwrap();let y=b.publish(9,41).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  b.observe_and_ack_after(2,0,|_|true).unwrap();
  let refs=[(&a,9,x),(&b,9,y)];
  assert_eq!(require_epoch_topology_acks_for_reclaim(&refs,&[1,2],40),
   Err(FenceError::StaleReceipt));
  assert_eq!(require_epoch_topology_acks_for_reclaim(
   &[(&a,9,x),(&a,9,x)],&[1,2],40),Err(FenceError::StaleReceipt));
  let c=LifecycleFence::new(9,3);let z=c.publish(9,40).unwrap();
  assert_eq!(require_epoch_topology_acks_for_reclaim(&[(&a,9,x),(&c,9,z)],
   &[1,3],40),Err(FenceError::NoProgress));
 }
}


/// P0-141..144: retry the SAME outstanding IPI publication after transient
/// hardware send failure, never issue a second release generation or forge
/// an AP acknowledgement. Only the trusted APIC driver may satisfy sender.
impl LifecycleFence{
 pub fn retry_pending_ipi_exact<I:LifecycleIpiSender>(
  &self,bsp:u32,ap:u32,receipt:Receipt,sender:&mut I
 )->Result<(),FenceError>{
  if bsp!=self.bsp||ap!=self.ap||bsp==ap{
   return Err(FenceError::InvalidCpu);
  }
  self.require_pending_exact(bsp,receipt)?;
  sender.send_exact(bsp,ap,receipt)
 }
}
#[cfg(test)]
mod kernel_p0_batch8_w1_ipi_retry_tests{
 use super::*;
 struct Sender{fail:bool,attempts:usize,last:Option<Receipt>}
 impl LifecycleIpiSender for Sender{
  fn send_exact(&mut self,_:u32,_:u32,receipt:Receipt)->Result<(),FenceError>{
   self.attempts+=1;self.last=Some(receipt);
   if self.fail{Err(FenceError::NoProgress)}else{Ok(())}
  }
 }
 #[test]fn failed_first_send_can_retransmit_exact_pending_receipt_once(){
  let fence=LifecycleFence::new(3,7);
  let mut ipi=Sender{fail:true,attempts:0,last:None};
  assert_eq!(fence.publish_and_signal(3,7,12,&mut ipi),
   Err(FenceError::NoProgress));
  let receipt=ipi.last.unwrap();
  assert_eq!(ipi.attempts,1);
  ipi.fail=false;
  assert_eq!(fence.retry_pending_ipi_exact(3,7,receipt,&mut ipi),Ok(()));
  assert_eq!(ipi.attempts,2);assert_eq!(ipi.last,Some(receipt));
  assert_eq!(fence.require_ack_for_reclaim(3,receipt),
   Err(FenceError::NoProgress));
  assert_eq!(fence.acknowledge_after_local_invalidate(7,0,|_|Ok(())),
   Ok(Some(receipt)));
  assert_eq!(fence.require_ack_for_reclaim(3,receipt),Ok(()));
  assert_eq!(fence.retry_pending_ipi_exact(3,7,receipt,&mut ipi),
   Err(FenceError::StaleReceipt));
  assert_eq!(ipi.attempts,2);
 }
 #[test]fn forged_or_foreign_retry_does_not_touch_apic_sender(){
  let fence=LifecycleFence::new(3,7);let receipt=fence.publish(3,14).unwrap();
  let mut sender=Sender{fail:false,attempts:0,last:None};
  assert_eq!(fence.retry_pending_ipi_exact(3,8,receipt,&mut sender),
   Err(FenceError::InvalidCpu));
  assert_eq!(fence.retry_pending_ipi_exact(2,7,receipt,&mut sender),
   Err(FenceError::InvalidCpu));
  assert_eq!(fence.retry_pending_ipi_exact(3,7,Receipt{
   generation:15,..receipt},&mut sender),Err(FenceError::StaleReceipt));
  assert_eq!(sender.attempts,0);
  sender.fail=true;
  assert_eq!(fence.retry_pending_ipi_exact(3,7,receipt,&mut sender),
   Err(FenceError::NoProgress));
  assert_eq!(fence.require_ack_for_reclaim(3,receipt),
   Err(FenceError::NoProgress));
 }
}


/// P0-161..164: publish complete trusted AP topology for one teardown epoch.
/// Expose each published receipt before attempting its hardware IPI send.
/// After a partial failure, every issued AP fence remains pending; never
/// reclaim, fabricate acknowledgements or silently reuse the receipt.
pub fn publish_and_signal_epoch_topology<I:LifecycleIpiSender>(
 topology:&[(&LifecycleFence,u32)],bsp:u32,epoch:u64,
 sender:&mut I,issued:&mut[Option<Receipt>]
)->Result<usize,FenceError>{
 if topology.is_empty()||topology.len()>64||issued.len()<topology.len(){
  return Err(FenceError::NoProgress);
 }
 if epoch==0{return Err(FenceError::InvalidGeneration);}
 for (i,&(fence,ap)) in topology.iter().enumerate(){
  if fence.bsp!=bsp||fence.ap!=ap||ap==bsp{return Err(FenceError::InvalidCpu);}
  if topology[..i].iter().any(|&(prior,id)|
   core::ptr::eq(prior,fence)||id==ap){return Err(FenceError::InvalidCpu);}
 }
 if issued.iter().any(Option::is_some){return Err(FenceError::Busy);}
 for (i,&(fence,ap)) in topology.iter().enumerate(){
  let receipt=fence.publish(bsp,epoch)?;
  issued[i]=Some(receipt);
  sender.send_exact(bsp,ap,receipt)?;
 }
 Ok(topology.len())
}
#[cfg(test)]
mod kernel_p0_batch9_w1_topology_publish_tests{
 use super::*;
 struct Sender{fail_at:usize,attempts:usize}
 impl LifecycleIpiSender for Sender{
  fn send_exact(&mut self,_:u32,_:u32,_:Receipt)->Result<(),FenceError>{
   self.attempts+=1;
   if self.attempts==self.fail_at{Err(FenceError::NoProgress)}else{Ok(())}
  }
 }
 #[test]fn partial_send_cannot_hide_any_issued_ap_fence(){
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(0,2);
  let c=LifecycleFence::new(0,3);let mut issued=[None;3];
  let mut sender=Sender{fail_at:2,attempts:0};
  assert_eq!(publish_and_signal_epoch_topology(
   &[(&a,1),(&b,2),(&c,3)],0,27,&mut sender,&mut issued),
   Err(FenceError::NoProgress));
  assert_eq!(sender.attempts,2);
  assert!(issued[0].is_some()&&issued[1].is_some()&&issued[2].is_none());
  let x=issued[0].unwrap();let y=issued[1].unwrap();
  assert_eq!(a.require_ack_for_reclaim(0,x),Err(FenceError::NoProgress));
  assert_eq!(b.require_ack_for_reclaim(0,y),Err(FenceError::NoProgress));
  assert_eq!(a.publish(0,28),Err(FenceError::Busy));
  sender.fail_at=usize::MAX;
  assert_eq!(b.retry_pending_ipi_exact(0,2,y,&mut sender),Ok(()));
  b.acknowledge_after_local_invalidate(2,0,|_|Ok(())).unwrap();
  a.acknowledge_after_local_invalidate(1,0,|_|Ok(())).unwrap();
  assert_eq!(a.require_ack_for_reclaim(0,x),Ok(()));
  assert_eq!(b.require_ack_for_reclaim(0,y),Ok(()));
 }
 #[test]fn duplicate_and_foreign_topology_is_rejected_before_publication(){
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(0,1);
  let foreign=LifecycleFence::new(4,5);
  let mut issued=[None;2];let mut sender=Sender{fail_at:usize::MAX,attempts:0};
  assert_eq!(publish_and_signal_epoch_topology(
   &[(&a,1),(&b,1)],0,8,&mut sender,&mut issued),Err(FenceError::InvalidCpu));
  assert_eq!(publish_and_signal_epoch_topology(
   &[(&a,1),(&foreign,5)],0,8,&mut sender,&mut issued),
   Err(FenceError::InvalidCpu));
  assert_eq!(sender.attempts,0);assert_eq!(issued,[None,None]);
  assert_eq!(publish_and_signal_epoch_topology(
   &[(&a,1)],0,8,&mut sender,&mut issued),Ok(1));
  assert_eq!(sender.attempts,1);
  assert_eq!(publish_and_signal_epoch_topology(
   &[(&a,1)],0,9,&mut sender,&mut issued),Err(FenceError::Busy));
 }
}


/// P0-181..184: resume an interrupted multi-AP IPI fanout without creating
/// new lifecycle receipts. Preflight the ENTIRE authenticated AP inventory
/// before attempting any send, and expose attempted sends to the coordinator.
/// Re-sends are safe only for an idempotent exact-receipt APIC protocol.
/// The caller holds the topology lock; actual APIC/TLB work remains external.
pub fn retry_incomplete_epoch_topology<I:LifecycleIpiSender>(
 receipts:&[(&LifecycleFence,u32,Receipt)],bsp:u32,
 expected_aps:&[u32],epoch:u64,sender:&mut I,attempted:&mut[bool]
)->Result<usize,FenceError>{
 if epoch==0{return Err(FenceError::InvalidGeneration);}
 if receipts.is_empty()||receipts.len()>64||
    receipts.len()!=expected_aps.len()||attempted.len()!=receipts.len(){
  return Err(FenceError::NoProgress);
 }
 if attempted.iter().any(|value|*value){return Err(FenceError::Busy);}
 for (i,&(fence,ap,receipt)) in receipts.iter().enumerate(){
  if bsp!=fence.bsp||ap!=fence.ap||ap==bsp||
     !expected_aps.contains(&ap)||
     receipts[..i].iter().any(|&(prior,old,_)|
      old==ap||core::ptr::eq(prior,fence)){
   return Err(FenceError::InvalidCpu);
  }
  if receipt.generation!=epoch{return Err(FenceError::StaleReceipt);}
  if !fence.acknowledged(bsp,receipt)?{
   fence.require_pending_exact(bsp,receipt)?;
  }
 }
 for (i,&ap) in expected_aps.iter().enumerate(){
  if expected_aps[..i].contains(&ap){
   return Err(FenceError::InvalidCpu);
  }
 }
 let mut sends=0;
 for (i,&(fence,ap,receipt)) in receipts.iter().enumerate(){
  if fence.acknowledged(bsp,receipt)?{continue;}
  fence.require_pending_exact(bsp,receipt)?;
  attempted[i]=true;
  sender.send_exact(bsp,ap,receipt)?;
  sends+=1;
 }
 Ok(sends)
}
#[cfg(test)]
mod kernel_p0_batch10_w1_exact_ipi_resume_tests{
 use super::*;
 struct Driver{calls:usize,fail_at:usize}
 impl LifecycleIpiSender for Driver{
  fn send_exact(&mut self,_:u32,_:u32,_:Receipt)->Result<(),FenceError>{
   self.calls+=1;
   if self.calls==self.fail_at{Err(FenceError::NoProgress)}else{Ok(())}
  }
 }
 #[test]fn preflight_rejects_foreign_duplicate_and_stale_without_ipi(){
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(0,2);
  let x=a.publish(0,55).unwrap();let y=b.publish(0,55).unwrap();
  let mut d=Driver{calls:0,fail_at:0};let mut attempted=[false;2];
  assert_eq!(retry_incomplete_epoch_topology(
   &[(&a,1,x),(&b,2,y)],0,&[1,3],55,&mut d,&mut attempted),
   Err(FenceError::InvalidCpu));
  assert_eq!(retry_incomplete_epoch_topology(
   &[(&a,1,x),(&b,2,y)],0,&[1,1],55,&mut d,&mut attempted),
   Err(FenceError::InvalidCpu));
  assert_eq!(retry_incomplete_epoch_topology(
   &[(&a,1,x),(&b,2,y)],0,&[1,2],56,&mut d,&mut attempted),
   Err(FenceError::StaleReceipt));
  assert_eq!(d.calls,0);assert_eq!(attempted,[false;2]);
 }
 #[test]fn partial_hardware_send_preserves_exact_pending_receipts(){
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(0,2);
  let x=a.publish(0,55).unwrap();let y=b.publish(0,55).unwrap();
  let mut d=Driver{calls:0,fail_at:2};let mut attempted=[false;2];
  assert_eq!(retry_incomplete_epoch_topology(
   &[(&a,1,x),(&b,2,y)],0,&[1,2],55,&mut d,&mut attempted),
   Err(FenceError::NoProgress));
  assert_eq!(attempted,[true,true]);
  assert_eq!(a.require_ack_for_reclaim(0,x),Err(FenceError::NoProgress));
  assert_eq!(b.require_ack_for_reclaim(0,y),Err(FenceError::NoProgress));
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  d.fail_at=usize::MAX;attempted=[false;2];
  assert_eq!(retry_incomplete_epoch_topology(
   &[(&a,1,x),(&b,2,y)],0,&[1,2],55,&mut d,&mut attempted),Ok(1));
  assert_eq!(attempted,[false,true]);
  b.observe_and_ack_after(2,0,|_|true).unwrap();
  attempted=[false;2];
  assert_eq!(retry_incomplete_epoch_topology(
   &[(&a,1,x),(&b,2,y)],0,&[1,2],55,&mut d,&mut attempted),Ok(0));
  assert_eq!(require_epoch_topology_acks_for_reclaim(
   &[(&a,0,x),(&b,0,y)],&[1,2],55),Ok(()));
 }
}



/// P0-201..204: refuse malformed user-space TLB shootdown ranges BEFORE
/// publishing any AP lifecycle receipt. Every attempted hardware IPI remains
/// visible in issued[] after a partial failure. The trusted AP handler must
/// invalidate the specified pages before acknowledging its exact receipt.
/// This function dispatches receipts; it does NOT itself invalidate a TLB.
pub fn dispatch_user_tlb_invalidation<I:LifecycleIpiSender>(
 topology:&[(&LifecycleFence,u32)],bsp:u32,epoch:u64,
 first_page:u64,page_count:u32,sender:&mut I,
 issued:&mut[Option<Receipt>]
)->Result<usize,FenceError>{
 const PAGE:u64=4096;
 const USER_END:u64=0x0000_8000_0000_0000;
 if first_page<PAGE||first_page%PAGE!=0||page_count==0||
    page_count>4096||first_page.checked_add((page_count as u64)*PAGE)
      .is_none_or(|end|end>USER_END){
  return Err(FenceError::InvalidGeneration);
 }
 publish_and_signal_epoch_topology(topology,bsp,epoch,sender,issued)
}
#[cfg(test)]
mod kernel_p0_batch11_w1_tlb_dispatch_tests{
 use super::*;
 struct Ipi{calls:usize,fail_on:usize}
 impl LifecycleIpiSender for Ipi{
  fn send_exact(&mut self,_:u32,_:u32,_:Receipt)->Result<(),FenceError>{
   self.calls+=1;
   if self.calls==self.fail_on{Err(FenceError::NoProgress)}else{Ok(())}
  }
 }
 #[test]fn invalid_pages_cannot_publish_or_send(){
  let a=LifecycleFence::new(0,1);let mut issued=[None];
  let mut ipi=Ipi{calls:0,fail_on:usize::MAX};
  for (at,n) in [(0,1),(1,1),(4096,0),(4096,4097),
                 (0x0000_8000_0000_0000,1),(u64::MAX-4095,2)]{
   assert_eq!(dispatch_user_tlb_invalidation(
    &[(&a,1)],0,9,at,n,&mut ipi,&mut issued),
    Err(FenceError::InvalidGeneration));
  }
  assert_eq!(ipi.calls,0);
  assert_eq!(issued,[None]);
 }
 #[test]fn attempted_ipis_never_count_as_local_invalidation(){
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(0,2);
  let mut issued=[None;2];let mut ipi=Ipi{calls:0,fail_on:2};
  assert_eq!(dispatch_user_tlb_invalidation(
    &[(&a,1),(&b,2)],0,19,0x4000,3,&mut ipi,&mut issued),
    Err(FenceError::NoProgress));
  assert_eq!(ipi.calls,2);
  let x=issued[0].unwrap();let y=issued[1].unwrap();
  assert_eq!(a.require_ack_for_reclaim(0,x),Err(FenceError::NoProgress));
  assert_eq!(b.require_ack_for_reclaim(0,y),Err(FenceError::NoProgress));
  assert_eq!(a.acknowledge_after_local_invalidate(1,0,|_|Err(FenceError::IncompleteLocalWork)),
    Err(FenceError::IncompleteLocalWork));
  assert_eq!(a.require_ack_for_reclaim(0,x),Err(FenceError::NoProgress));
  a.acknowledge_after_local_invalidate(1,0,|_|Ok(())).unwrap();
  b.acknowledge_after_local_invalidate(2,0,|_|Ok(())).unwrap();
  assert_eq!(a.require_ack_for_reclaim(0,x),Ok(()));
  assert_eq!(b.require_ack_for_reclaim(0,y),Ok(()));
 }
}



/// P0-221..225: bind a canonical page range to every exact AP IPI receipt.
/// Only a trusted APIC driver may implement this protocol. Publication and
/// acknowledgements alone do not perform the hardware TLB invalidation.
#[derive(Debug,Clone,Copy,Eq,PartialEq)]
pub struct TlbShootdownRange{
 pub first_page:u64,
 pub pages:u32,
 pub epoch:u64,
}
impl TlbShootdownRange{
 pub fn validate(self)->Result<(),FenceError>{
  const USER_TOP:u64=0x0000_8000_0000_0000;
  if self.epoch==0||self.first_page<4096||
    self.first_page%4096!=0||self.pages==0||self.pages>4096||
    self.first_page.checked_add((self.pages as u64)*4096)
     .is_none_or(|end|end>USER_TOP){
   return Err(FenceError::InvalidGeneration);
  }
  Ok(())
 }
}
pub trait TlbRangeIpiSender{
 fn send_range_exact(&mut self,bsp:u32,ap:u32,
  receipt:Receipt,range:TlbShootdownRange)->Result<(),FenceError>;
}
pub fn dispatch_exact_tlb_range<I:TlbRangeIpiSender>(
 topology:&[(&LifecycleFence,u32)],bsp:u32,
 range:TlbShootdownRange,driver:&mut I,issued:&mut[Option<Receipt>]
)->Result<usize,FenceError>{
 range.validate()?;
 struct Bridge<'a,I>{driver:&'a mut I,range:TlbShootdownRange}
 impl<I:TlbRangeIpiSender> LifecycleIpiSender for Bridge<'_,I>{
  fn send_exact(&mut self,bsp:u32,ap:u32,receipt:Receipt)
   ->Result<(),FenceError>{
   if receipt.generation!=self.range.epoch{
    return Err(FenceError::StaleReceipt);
   }
   self.driver.send_range_exact(bsp,ap,receipt,self.range)
  }
 }
 publish_and_signal_epoch_topology(
  topology,bsp,range.epoch,&mut Bridge{driver,range},issued)
}
pub fn acknowledge_exact_tlb_range(
 fence:&LifecycleFence,ap:u32,previous_sequence:u64,
 expected:Receipt,range:TlbShootdownRange,
 invalidate:impl FnOnce(TlbShootdownRange)->Result<(),FenceError>
)->Result<(),FenceError>{
 range.validate()?;
 if range.epoch!=expected.generation{return Err(FenceError::StaleReceipt);}
 fence.require_pending_exact(fence.bsp,expected)?;
 let observed=fence.acknowledge_after_local_invalidate(
  ap,previous_sequence,|receipt|{
   if receipt!=expected{return Err(FenceError::StaleReceipt);}
   invalidate(range)
  })?;
 if observed==Some(expected){Ok(())}else{Err(FenceError::StaleReceipt)}
}
#[cfg(test)]
mod kernel_p0_batch12_w1_bound_tlb_tests{
 use super::*;
 struct Driver{calls:usize,fail_at:usize,observed:Option<TlbShootdownRange>}
 impl TlbRangeIpiSender for Driver{
  fn send_range_exact(&mut self,_:u32,_:u32,_:Receipt,range:TlbShootdownRange)
   ->Result<(),FenceError>{
   self.calls+=1;self.observed=Some(range);
   if self.calls==self.fail_at{Err(FenceError::NoProgress)}else{Ok(())}
  }
 }
 fn range()->TlbShootdownRange{
  TlbShootdownRange{first_page:0x4000,pages:2,epoch:17}
 }
 #[test]fn malformed_range_cannot_publish_or_call_driver(){
  let f=LifecycleFence::new(0,1);
  let mut driver=Driver{calls:0,fail_at:0,observed:None};
  let mut issued=[None];
  for invalid in [
   TlbShootdownRange{epoch:0,..range()},
   TlbShootdownRange{first_page:0x4001,..range()},
   TlbShootdownRange{pages:0,..range()},
   TlbShootdownRange{pages:4097,..range()},
   TlbShootdownRange{first_page:0x0000_8000_0000_0000,..range()},
  ]{
   assert_eq!(dispatch_exact_tlb_range(
    &[(&f,1)],0,invalid,&mut driver,&mut issued),
    Err(FenceError::InvalidGeneration));
  }
  assert_eq!(driver.calls,0);
  assert_eq!(issued,[None]);
 }
 #[test]fn partial_ipi_requires_exact_range_and_actual_local_callback(){
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(0,2);
  let mut d=Driver{calls:0,fail_at:2,observed:None};
  let mut issued=[None;2];let range=range();
  assert_eq!(dispatch_exact_tlb_range(
   &[(&a,1),(&b,2)],0,range,&mut d,&mut issued),
   Err(FenceError::NoProgress));
  assert_eq!(d.observed,Some(range));
  let x=issued[0].unwrap();let y=issued[1].unwrap();
  assert_eq!(a.require_ack_for_reclaim(0,x),Err(FenceError::NoProgress));
  assert_eq!(acknowledge_exact_tlb_range(&a,1,0,x,
   TlbShootdownRange{epoch:18,..range},|_|Ok(())),
   Err(FenceError::StaleReceipt));
  assert_eq!(acknowledge_exact_tlb_range(&a,1,0,x,range,
   |_|Err(FenceError::IncompleteLocalWork)),
   Err(FenceError::IncompleteLocalWork));
  assert_eq!(a.require_ack_for_reclaim(0,x),Err(FenceError::NoProgress));
  let mut invalidated=0;
  assert_eq!(acknowledge_exact_tlb_range(&a,1,0,x,range,
   |actual|{assert_eq!(actual,range);invalidated+=1;Ok(())}),Ok(()));
  assert_eq!(invalidated,1);
  assert_eq!(a.require_ack_for_reclaim(0,x),Ok(()));
  assert_eq!(b.require_ack_for_reclaim(0,y),Err(FenceError::NoProgress));
  assert_eq!(acknowledge_exact_tlb_range(&a,1,0,x,range,
   |_|{invalidated+=1;Ok(())}),Err(FenceError::StaleReceipt));
  assert_eq!(invalidated,1);
 }
}



/// P0-251..255: caller-provided local per-page TLB invalidator. AP cleanup
/// must cover EVERY page in the exact range before the lifecycle receipt is
/// acknowledged. The concrete x86 INVLPG implementation remains a hardware
/// integration requirement; mock callbacks cannot substitute for it.
pub trait LocalUserTlbInvalidator{
 fn invalidate_page(&mut self,page:u64)->Result<(),FenceError>;
}
pub fn acknowledge_all_tlb_pages<I:LocalUserTlbInvalidator>(
 fence:&LifecycleFence,ap:u32,previous_sequence:u64,
 receipt:Receipt,range:TlbShootdownRange,driver:&mut I
)->Result<(),FenceError>{
 acknowledge_exact_tlb_range(fence,ap,previous_sequence,receipt,range,|request|{
  for i in 0..request.pages{
   let address=request.first_page+(i as u64)*4096;
   driver.invalidate_page(address)?;
  }
  Ok(())
 })
}
#[cfg(test)]
mod kernel_p0_batch13_w1_page_ack_tests{
 use super::*;
 struct Flush{addresses:[u64;8],calls:usize,fail_at:usize}
 impl LocalUserTlbInvalidator for Flush{
  fn invalidate_page(&mut self,at:u64)->Result<(),FenceError>{
   if self.calls>=self.addresses.len(){return Err(FenceError::NoProgress);}
   self.addresses[self.calls]=at;self.calls+=1;
   if self.calls==self.fail_at{Err(FenceError::IncompleteLocalWork)}
   else{Ok(())}
  }
 }
 fn request()->TlbShootdownRange{
  TlbShootdownRange{first_page:0x3000,pages:3,epoch:31}
 }
 #[test]fn all_pages_flush_before_single_exact_ap_ack(){
  let f=LifecycleFence::new(0,2);
  let receipt=f.publish(0,31).unwrap();
  let mut flush=Flush{addresses:[0;8],calls:0,fail_at:0};
  assert_eq!(acknowledge_all_tlb_pages(&f,2,0,receipt,request(),&mut flush),Ok(()));
  assert_eq!(flush.calls,3);
  assert_eq!(&flush.addresses[..3],&[0x3000,0x4000,0x5000]);
  assert_eq!(f.require_ack_for_reclaim(0,receipt),Ok(()));
  assert_eq!(acknowledge_all_tlb_pages(&f,2,0,receipt,request(),&mut flush),
   Err(FenceError::StaleReceipt));
  assert_eq!(flush.calls,3);
 }
 #[test]fn failed_partial_flush_retains_pending_receipt(){
  let f=LifecycleFence::new(0,2);
  let receipt=f.publish(0,31).unwrap();
  let mut flush=Flush{addresses:[0;8],calls:0,fail_at:2};
  assert_eq!(acknowledge_all_tlb_pages(&f,2,0,receipt,request(),&mut flush),
   Err(FenceError::IncompleteLocalWork));
  assert_eq!(flush.calls,2);
  assert_eq!(f.require_ack_for_reclaim(0,receipt),Err(FenceError::NoProgress));
  flush.fail_at=0;flush.calls=0;
  assert_eq!(acknowledge_all_tlb_pages(&f,2,0,receipt,request(),&mut flush),Ok(()));
  assert_eq!(flush.calls,3);
 }
 #[test]fn wrong_cpu_epoch_and_zero_page_never_touch_driver(){
  let f=LifecycleFence::new(0,2);
  let receipt=f.publish(0,31).unwrap();
  let mut flush=Flush{addresses:[0;8],calls:0,fail_at:0};
  assert_eq!(acknowledge_all_tlb_pages(&f,3,0,receipt,request(),&mut flush),
   Err(FenceError::InvalidCpu));
  assert_eq!(acknowledge_all_tlb_pages(&f,2,0,receipt,
   TlbShootdownRange{epoch:32,..request()},&mut flush),
   Err(FenceError::StaleReceipt));
  assert_eq!(acknowledge_all_tlb_pages(&f,2,0,receipt,
   TlbShootdownRange{pages:0,..request()},&mut flush),
   Err(FenceError::InvalidGeneration));
  assert_eq!(flush.calls,0);
 }
}


/// P0-281..284: turn an unaligned user byte span into one canonical,
/// overflow-checked page shootdown contract. No receipt may be acknowledged
/// for a different byte span, even if its epoch is identical.
pub fn tlb_range_for_bytes(start:u64,length:u64,epoch:u64)
 ->Result<TlbShootdownRange,FenceError>{
 if length==0||start<4096{return Err(FenceError::InvalidGeneration);}
 let end=start.checked_add(length).ok_or(FenceError::InvalidGeneration)?;
 let first=start & !4095u64;
 let rounded=end.checked_add(4095).ok_or(FenceError::InvalidGeneration)? & !4095u64;
 let bytes=rounded.checked_sub(first).ok_or(FenceError::InvalidGeneration)?;
 let pages=u32::try_from(bytes/4096).map_err(|_|FenceError::InvalidGeneration)?;
 let span=TlbShootdownRange{first_page:first,pages,epoch};
 span.validate()?;
 Ok(span)
}
pub fn acknowledge_tlb_byte_span<I:LocalUserTlbInvalidator>(
 fence:&LifecycleFence,ap:u32,previous_sequence:u64,receipt:Receipt,
 first_byte:u64,length:u64,pinned:TlbShootdownRange,invalidator:&mut I
)->Result<(),FenceError>{
 let actual=tlb_range_for_bytes(first_byte,length,receipt.generation)?;
 if actual!=pinned{return Err(FenceError::StaleReceipt);}
 acknowledge_all_tlb_pages(fence,ap,previous_sequence,receipt,pinned,invalidator)
}
#[cfg(test)]
mod kernel_p0_batch14_w1_byte_span_tests{
 use super::*;
 struct Sink{pages:[u64;4],count:usize}
 impl LocalUserTlbInvalidator for Sink{
  fn invalidate_page(&mut self,page:u64)->Result<(),FenceError>{
   self.pages[self.count]=page;self.count+=1;Ok(())
  }
 }
 #[test]fn unaligned_byte_span_covers_each_exact_page_once(){
  let span=tlb_range_for_bytes(0x3ffe,4,71).unwrap();
  assert_eq!((span.first_page,span.pages),(0x3000,2));
  let fence=LifecycleFence::new(0,3);
  let receipt=fence.publish(0,71).unwrap();
  let mut sink=Sink{pages:[0;4],count:0};
  assert_eq!(acknowledge_tlb_byte_span(&fence,3,0,receipt,0x3ffe,4,
    span,&mut sink),Ok(()));
  assert_eq!(&sink.pages[..sink.count],&[0x3000,0x4000]);
  assert_eq!(fence.require_ack_for_reclaim(0,receipt),Ok(()));
 }
 #[test]fn changed_extent_and_invalid_ranges_never_invalidate(){
  for (start,len) in [(0,1),(4096,0),(u64::MAX-2,8),
   (0x0000_7fff_ffff_f000,8192),(4096,4096*4097)]{
   assert_eq!(tlb_range_for_bytes(start,len,71),Err(FenceError::InvalidGeneration));
  }
  let span=tlb_range_for_bytes(0x5001,1,71).unwrap();
  let fence=LifecycleFence::new(0,3);
  let receipt=fence.publish(0,71).unwrap();
  let mut sink=Sink{pages:[0;4],count:0};
  assert_eq!(acknowledge_tlb_byte_span(&fence,3,0,receipt,
    0x5001,4097,span,&mut sink),Err(FenceError::StaleReceipt));
  assert_eq!(sink.count,0);
  assert_eq!(fence.require_ack_for_reclaim(0,receipt),Err(FenceError::NoProgress));
 }
}



/// P0-301..305: build an exact, bounded, disjoint TLB invalidation plan
/// before publishing any AP IPI. These are preflight contracts, NOT
/// proof of hardware INVLPG or cross-core completion.
pub fn plan_disjoint_user_tlb_spans<const N:usize>(
 spans:&[(u64,u64)],epoch:u64
)->Result<([TlbShootdownRange;N],usize),FenceError>{
 if epoch==0||spans.is_empty()||spans.len()>N||spans.len()>64{
  return Err(FenceError::InvalidGeneration);
 }
 let empty=TlbShootdownRange{first_page:4096,pages:1,epoch};
 let mut plan=[empty;N];
 for (i,&(begin,length)) in spans.iter().enumerate(){
  let range=tlb_range_for_bytes(begin,length,epoch)?;
  let end=range.first_page+(range.pages as u64)*4096;
  for previous in plan[..i].iter(){
   let previous_end=previous.first_page+(previous.pages as u64)*4096;
   if range.first_page<previous_end&&previous.first_page<end{
    return Err(FenceError::InvalidGeneration);
   }
  }
  plan[i]=range;
 }
 Ok((plan,spans.len()))
}
#[cfg(test)]
mod kernel_p0_batch15_w1_tlb_plan_tests{
 use super::*;
 #[test]fn canonical_unaligned_spans_are_disjoint_and_preserve_request_order(){
  let (plan,count)=plan_disjoint_user_tlb_spans::<3>(
   &[(0x1001,4096),(0x5002,1)],31).unwrap();
  assert_eq!(count,2);
  assert_eq!((plan[0].first_page,plan[0].pages,plan[0].epoch),(0x1000,2,31));
  assert_eq!((plan[1].first_page,plan[1].pages),(0x5000,1));
 }
 #[test]fn aliasing_subpages_and_zero_or_overlong_spans_fail_preflight(){
  assert!(plan_disjoint_user_tlb_spans::<2>(
   &[(0x1001,1),(0x1002,1)],31).is_err());
  assert!(plan_disjoint_user_tlb_spans::<2>(
   &[(0x1000,0)],31).is_err());
  assert!(plan_disjoint_user_tlb_spans::<2>(
   &[(0x1000,4096*4097)],31).is_err());
 }
 #[test]fn plan_capacity_epoch_and_user_boundary_are_strict(){
  assert!(plan_disjoint_user_tlb_spans::<1>(
   &[(0x1000,1),(0x3000,1)],31).is_err());
  assert!(plan_disjoint_user_tlb_spans::<0>(&[],31).is_err());
  assert!(plan_disjoint_user_tlb_spans::<1>(&[(0x1000,1)],0).is_err());
  assert!(plan_disjoint_user_tlb_spans::<1>(
   &[(0x0000_7fff_ffff_f000,8192)],31).is_err());
  assert!(plan_disjoint_user_tlb_spans::<1>(
   &[(u64::MAX-2,8)],31).is_err());
 }
 #[test]fn adjacent_full_pages_are_legal_without_silent_coalescing(){
  let (plan,n)=plan_disjoint_user_tlb_spans::<2>(
   &[(0x3000,4096),(0x4000,4096)],8).unwrap();
  assert_eq!(n,2);
  assert_eq!(plan[0].first_page,0x3000);
  assert_eq!(plan[1].first_page,0x4000);
 }
}



/// P0-331..335: complete a disjoint ordered TLB plan as ONE AP receipt.
/// Preflight every range and aggregate page bound before any callback.
/// This callback is not the real x86 INVLPG implementation.
pub fn acknowledge_disjoint_tlb_plan<I:LocalUserTlbInvalidator>(
 fence:&LifecycleFence,ap:u32,last_seen:u64,receipt:Receipt,
 plan:&[TlbShootdownRange],driver:&mut I
)->Result<(),FenceError>{
 if plan.is_empty()||plan.len()>64||receipt.generation==0{
  return Err(FenceError::InvalidGeneration);
 }
 let mut previous_end=0u64;
 let mut total=0u32;
 for range in plan{
  range.validate()?;
  if range.epoch!=receipt.generation{return Err(FenceError::StaleReceipt);}
  if range.first_page<previous_end{return Err(FenceError::InvalidGeneration);}
  previous_end=range.first_page+(range.pages as u64)*4096;
  total=total.checked_add(range.pages).ok_or(FenceError::InvalidGeneration)?;
  if total>4096{return Err(FenceError::InvalidGeneration);}
 }
 fence.require_pending_exact(fence.bsp,receipt)?;
 let observed=fence.acknowledge_after_local_invalidate(
  ap,last_seen,|actual|{
   if actual!=receipt{return Err(FenceError::StaleReceipt);}
   for range in plan{
    for offset in 0..range.pages{
     driver.invalidate_page(range.first_page+(offset as u64)*4096)?;
    }
   }
   Ok(())
  })?;
 if observed==Some(receipt){Ok(())}else{Err(FenceError::StaleReceipt)}
}
#[cfg(test)]
mod kernel_p0_batch16_w1_disjoint_tlb_ack_tests{
 use super::*;
 struct Flush{pages:[u64;8],count:usize,fail_at:usize}
 impl LocalUserTlbInvalidator for Flush{
  fn invalidate_page(&mut self,page:u64)->Result<(),FenceError>{
   if self.count==self.fail_at{return Err(FenceError::IncompleteLocalWork);}
   self.pages[self.count]=page;self.count+=1;Ok(())
  }
 }
 fn plan()->[TlbShootdownRange;2]{[
  TlbShootdownRange{first_page:0x1000,pages:2,epoch:42},
  TlbShootdownRange{first_page:0x9000,pages:1,epoch:42}
 ]}
 #[test]fn every_disjoint_page_flushes_before_one_ap_receipt(){
  let f=LifecycleFence::new(0,3);let receipt=f.publish(0,42).unwrap();
  let mut driver=Flush{pages:[0;8],count:0,fail_at:8};
  assert_eq!(acknowledge_disjoint_tlb_plan(&f,3,0,receipt,&plan(),&mut driver),Ok(()));
  assert_eq!(&driver.pages[..3],&[0x1000,0x2000,0x9000]);
  assert_eq!(f.require_ack_for_reclaim(0,receipt),Ok(()));
  assert_eq!(acknowledge_disjoint_tlb_plan(&f,3,0,receipt,&plan(),&mut driver),
   Err(FenceError::StaleReceipt));
  assert_eq!(driver.count,3);
 }
 #[test]fn failed_local_page_flush_cannot_ack_or_release_memory(){
  let f=LifecycleFence::new(0,3);let receipt=f.publish(0,42).unwrap();
  let mut driver=Flush{pages:[0;8],count:0,fail_at:1};
  assert_eq!(acknowledge_disjoint_tlb_plan(&f,3,0,receipt,&plan(),&mut driver),
   Err(FenceError::IncompleteLocalWork));
  assert_eq!(f.require_ack_for_reclaim(0,receipt),Err(FenceError::NoProgress));
  let mut retry=Flush{pages:[0;8],count:0,fail_at:8};
  assert_eq!(acknowledge_disjoint_tlb_plan(&f,3,0,receipt,&plan(),&mut retry),Ok(()));
  assert_eq!(retry.count,3);
 }
 #[test]fn bad_order_overlap_epoch_and_total_limit_have_no_callback(){
  let f=LifecycleFence::new(0,3);let receipt=f.publish(0,42).unwrap();
  let mut driver=Flush{pages:[0;8],count:0,fail_at:8};
  let mut backward=plan();backward.swap(0,1);
  assert!(acknowledge_disjoint_tlb_plan(&f,3,0,receipt,&backward,&mut driver).is_err());
  let mut overlap=plan();overlap[1].first_page=0x2000;
  assert!(acknowledge_disjoint_tlb_plan(&f,3,0,receipt,&overlap,&mut driver).is_err());
  let mut stale=plan();stale[1].epoch=43;
  assert_eq!(acknowledge_disjoint_tlb_plan(&f,3,0,receipt,&stale,&mut driver),
   Err(FenceError::StaleReceipt));
  let huge=[TlbShootdownRange{first_page:0x1000,pages:4096,epoch:42},
   TlbShootdownRange{first_page:0x1001000,pages:1,epoch:42}];
  assert!(acknowledge_disjoint_tlb_plan(&f,3,0,receipt,&huge,&mut driver).is_err());
  assert_eq!(driver.count,0);
  assert_eq!(f.require_ack_for_reclaim(0,receipt),Err(FenceError::NoProgress));
 }
}



/// P0-361..367: dispatch ONE immutable disjoint page plan to an authenticated
/// AP topology. An attempted IPI is not an AP ack; partial dispatch leaves
/// all issued receipts exposed to the BSP for recovery, never fabricated.
pub trait TlbPlanIpiSender {
 fn send_exact_plan(&mut self,bsp:u32,ap:u32,receipt:Receipt,
  plan:&[TlbShootdownRange])->Result<(),FenceError>;
}
pub fn validate_user_tlb_plan(plan:&[TlbShootdownRange],epoch:u64)
 ->Result<u32,FenceError>{
 if plan.is_empty()||plan.len()>64||epoch==0{
  return Err(FenceError::InvalidGeneration);
 }
 let mut end=0u64;let mut pages=0u32;
 for range in plan{
  range.validate()?;
  if range.epoch!=epoch{return Err(FenceError::StaleReceipt);}
  if range.first_page<end{return Err(FenceError::InvalidGeneration);}
  end=range.first_page+(range.pages as u64)*4096;
  pages=pages.checked_add(range.pages).ok_or(FenceError::InvalidGeneration)?;
  if pages>4096{return Err(FenceError::InvalidGeneration);}
 }
 Ok(pages)
}
pub fn dispatch_user_tlb_plan<I:TlbPlanIpiSender>(
 topology:&[(&LifecycleFence,u32)],bsp:u32,
 plan:&[TlbShootdownRange],driver:&mut I,
 issued:&mut[Option<Receipt>]
)->Result<usize,FenceError>{
 let epoch=plan.first().map(|r|r.epoch)
  .ok_or(FenceError::InvalidGeneration)?;
 validate_user_tlb_plan(plan,epoch)?;
 struct Driver<'a,I>{inner:&'a mut I,plan:&'a [TlbShootdownRange]}
 impl<I:TlbPlanIpiSender> LifecycleIpiSender for Driver<'_,I>{
  fn send_exact(&mut self,bsp:u32,ap:u32,receipt:Receipt)
   ->Result<(),FenceError>{
   if receipt.generation!=self.plan[0].epoch{
    return Err(FenceError::StaleReceipt);
   }
   self.inner.send_exact_plan(bsp,ap,receipt,self.plan)
  }
 }
 publish_and_signal_epoch_topology(topology,bsp,epoch,
  &mut Driver{inner:driver,plan},issued)
}
#[cfg(test)]
mod kernel_p0_batch17_w1_tlb_multicore_plan_tests{
 use super::*;
 struct Sender{attempts:usize,fail_on:usize,seen:[u32;3]}
 impl TlbPlanIpiSender for Sender{
  fn send_exact_plan(&mut self,bsp:u32,ap:u32,receipt:Receipt,
   plan:&[TlbShootdownRange])->Result<(),FenceError>{
   assert_eq!(bsp,0);assert_eq!(receipt.generation,77);
   assert_eq!((plan.len(),plan[0].first_page),(2,0x2000));
   self.seen[self.attempts]=ap;self.attempts+=1;
   if self.attempts==self.fail_on{Err(FenceError::NoProgress)}else{Ok(())}
  }
 }
 fn plan()->[TlbShootdownRange;2]{[
  TlbShootdownRange{first_page:0x2000,pages:2,epoch:77},
  TlbShootdownRange{first_page:0x8000,pages:1,epoch:77},
 ]}
 struct Flush{count:usize}
 impl LocalUserTlbInvalidator for Flush{
  fn invalidate_page(&mut self,_:u64)->Result<(),FenceError>{
   self.count+=1;Ok(())
  }
 }
 #[test]fn two_distinct_aps_must_flush_full_plan_before_reclaim(){
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(0,2);
  let mut send=Sender{attempts:0,fail_on:0,seen:[0;3]};
  let mut issued=[None;2];
  assert_eq!(dispatch_user_tlb_plan(&[(&a,1),(&b,2)],0,&plan(),
   &mut send,&mut issued),Ok(2));
  assert_eq!(send.seen[..2],[1,2]);
  let x=issued[0].unwrap();let y=issued[1].unwrap();
  assert_eq!(a.require_ack_for_reclaim(0,x),Err(FenceError::NoProgress));
  let mut first=Flush{count:0};let mut second=Flush{count:0};
  assert_eq!(acknowledge_disjoint_tlb_plan(&a,1,0,x,&plan(),&mut first),Ok(()));
  assert_eq!(acknowledge_disjoint_tlb_plan(&b,2,0,y,&plan(),&mut second),Ok(()));
  assert_eq!((first.count,second.count),(3,3));
  assert_eq!(require_epoch_topology_acks_for_reclaim(
   &[(&a,0,x),(&b,0,y)],&[1,2],77),Ok(()));
 }
 #[test]fn partial_ipi_send_exposes_both_unacked_receipts(){
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(0,2);
  let mut driver=Sender{attempts:0,fail_on:2,seen:[0;3]};
  let mut issued=[None;2];
  assert_eq!(dispatch_user_tlb_plan(&[(&a,1),(&b,2)],0,&plan(),
   &mut driver,&mut issued),Err(FenceError::NoProgress));
  assert!(issued.iter().all(Option::is_some));
  assert_eq!(a.require_ack_for_reclaim(0,issued[0].unwrap()),
   Err(FenceError::NoProgress));
  assert_eq!(b.require_ack_for_reclaim(0,issued[1].unwrap()),
   Err(FenceError::NoProgress));
 }
 #[test]fn invalid_plan_or_duplicate_topology_never_signals(){
  let a=LifecycleFence::new(0,1);let mut driver=Sender{attempts:0,
   fail_on:0,seen:[0;3]};let mut issued=[None;2];
  let mut overlap=plan();overlap[1].first_page=0x3000;
  assert!(dispatch_user_tlb_plan(&[(&a,1)],0,&overlap,
   &mut driver,&mut issued).is_err());
  assert_eq!(dispatch_user_tlb_plan(&[(&a,1),(&a,1)],0,&plan(),
   &mut driver,&mut issued),Err(FenceError::InvalidCpu));
  let mut stale=plan();stale[1].epoch=78;
  assert_eq!(validate_user_tlb_plan(&stale,77),Err(FenceError::StaleReceipt));
  assert_eq!(driver.attempts,0);assert_eq!(issued,[None,None]);
 }
 #[test]fn page_budget_and_unsorted_spans_fail_before_any_ipi(){
  let a=LifecycleFence::new(0,1);
  let mut driver=Sender{attempts:0,fail_on:0,seen:[0;3]};
  let mut issued=[None;1];
  let huge=[
   TlbShootdownRange{first_page:0x1000,pages:4096,epoch:77},
   TlbShootdownRange{first_page:0x1001000,pages:1,epoch:77}
  ];
  assert!(dispatch_user_tlb_plan(&[(&a,1)],0,&huge,&mut driver,&mut issued).is_err());
  let mut backward=plan();backward.swap(0,1);
  assert!(dispatch_user_tlb_plan(&[(&a,1)],0,&backward,&mut driver,&mut issued).is_err());
  assert_eq!(driver.attempts,0);assert_eq!(issued,[None]);
 }
}
