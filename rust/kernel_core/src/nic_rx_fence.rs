//! R134: fail-closed NIC receive admission while IRQ/DMA rings quiesce.
//!
//! Lock-free atomic admission only. This does not program physical NIC
//! interrupts, interrupt remapping, PCI bus-master disable or IOMMU domains.
//! A real driver MUST stop device DMA and mask MSI/MSI-X before accepting the
//! returned quiescence receipt; only after in-flight software handlers drain
//! may it retire port/owner authority or recycle a queue.
//! This standalone type is safe for tests and the exclusive polled QEMU NIC.

use core::{marker::PhantomData,ptr,sync::atomic::{AtomicBool,AtomicU32,AtomicU64,Ordering}};

/// Every ring lifetime receives a distinct nonzero ID; fail closed at wrap.
/// Pointer identity alone is unsafe when a driver is replaced at the same
/// memory address with the same device number and initial epoch.
static NEXT_RX_INSTANCE:AtomicU64=AtomicU64::new(1);

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum RxFenceError {InvalidDevice,Quiescing,Busy,Capacity,Stale,EpochExhausted}

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct QuiesceReceipt { device:u32, epoch:u64, instance:u64 }

/// One receive device/ring. At most one control-plane quiescer at a time.
/// No allocations, lock spinning, or IRQ-side calls to a scheduler lock.
pub struct NicRxFence {
    device:u32,
    epoch:AtomicU64,
    admitted:AtomicU32,
    quiescing:AtomicBool,
    instance:AtomicU64,
}
impl NicRxFence {
    pub const fn new(device:u32)->Self{
        Self{device,epoch:AtomicU64::new(1),admitted:AtomicU32::new(0),
             quiescing:AtomicBool::new(false),instance:AtomicU64::new(0)}
    }
    pub fn device(&self)->u32{self.device}
    pub fn epoch(&self)->u64{self.epoch.load(Ordering::Acquire)}

    /// Assign once, lazily, so static const-initialized fences also work.
    /// Unused IDs caused by contention are harmless; IDs are never reused.
    fn instance_id(&self)->Result<u64,RxFenceError>{
        let existing=self.instance.load(Ordering::Acquire);
        if existing!=0{return Ok(existing);}
        let candidate=NEXT_RX_INSTANCE.fetch_update(
            Ordering::AcqRel,Ordering::Acquire,|next|next.checked_add(1)
        ).map_err(|_|RxFenceError::EpochExhausted)?;
        match self.instance.compare_exchange(0,candidate,
            Ordering::AcqRel,Ordering::Acquire) {
            Ok(_)=>Ok(candidate),
            Err(winner)=>Ok(winner),
        }
    }

    /// IRQ may use this without waiting. If teardown races with admission,
    /// rechecking quiescing and the captured epoch forces fail-closed exit.
    pub fn try_admit(&self,device:u32)->Result<RxPermit<'_>,RxFenceError>{
        if device==0 || device!=self.device {return Err(RxFenceError::InvalidDevice);}
        if self.quiescing.load(Ordering::Acquire){return Err(RxFenceError::Quiescing);}
        let epoch=self.epoch.load(Ordering::Acquire);
        self.admitted.fetch_update(Ordering::AcqRel,Ordering::Acquire,
            |n|n.checked_add(1)).map_err(|_|RxFenceError::Capacity)?;
        let permit=RxPermit{fence:self,epoch,_not_send:PhantomData};
        if self.quiescing.load(Ordering::Acquire) ||
           self.epoch.load(Ordering::Acquire)!=epoch {
            drop(permit);
            return Err(RxFenceError::Quiescing);
        }
        Ok(permit)
    }
    /// BSP teardown only. Caller must *also* mask NIC interrupts, stop DMA,
    /// drain hardware completion descriptors and hold the kernel lifecycle
    /// permit before publishing a changed UDP-process owner generation.
    pub fn begin_quiesce(&self,device:u32)->Result<QuiesceReceipt,RxFenceError>{
        if device==0 || device!=self.device{return Err(RxFenceError::InvalidDevice);}
        let instance=self.instance_id()?;
        self.quiescing.compare_exchange(false,true,Ordering::AcqRel,
            Ordering::Acquire).map_err(|_|RxFenceError::Busy)?;
        // The opaque nonce survives neither driver replacement nor rollback
        // of a NIC queue descriptor. Matching device+epoch is insufficient.
        Ok(QuiesceReceipt{device,epoch:self.epoch.load(Ordering::Acquire),
                          instance})
    }
    /// No busy waiting in an ISR; the BSP polls this after hardware is still.
    pub fn drained(&self,receipt:QuiesceReceipt)->Result<bool,RxFenceError>{
        if receipt.device!=self.device || self.device==0 ||
           receipt.instance==0 ||
           receipt.instance!=self.instance.load(Ordering::Acquire) ||
           !self.quiescing.load(Ordering::Acquire) ||
           receipt.epoch!=self.epoch.load(Ordering::Acquire) {
            return Err(RxFenceError::Stale);
        }
        Ok(self.admitted.load(Ordering::Acquire)==0)
    }
    /// Resumption rejects an invalid/stale receipt and any in-flight
    /// handler; epoch never wraps and invalidates every former RxPermit.
    pub fn finish_rebind(&self,receipt:QuiesceReceipt)
        ->Result<u64,RxFenceError>{
        if !self.drained(receipt)?{return Err(RxFenceError::Busy);}
        let next=receipt.epoch.checked_add(1).ok_or(RxFenceError::EpochExhausted)?;
        self.epoch.compare_exchange(receipt.epoch,next,
            Ordering::AcqRel,Ordering::Acquire)
            .map_err(|_|RxFenceError::Stale)?;
        self.quiescing.store(false,Ordering::Release);
        Ok(next)
    }
    pub fn permit_is_current(&self,permit:&RxPermit<'_>)->bool{
        ptr::eq(self,permit.fence) && self.device!=0 &&
        !self.quiescing.load(Ordering::Acquire) &&
        self.epoch.load(Ordering::Acquire)==permit.epoch
    }
}

/// RAII bounded hardware software-completion admission. No Send or Sync:
/// the receiving CPU must finish its own IRQ/packet ownership processing.
#[must_use="hold through the guarded UDP ingress operation"]
pub struct RxPermit<'a>{
    fence:&'a NicRxFence,epoch:u64,_not_send:PhantomData<*mut ()>,
}
impl RxPermit<'_>{
    pub fn epoch(&self)->u64{self.epoch}
}
impl Drop for RxPermit<'_>{
    fn drop(&mut self){self.fence.admitted.fetch_sub(1,Ordering::AcqRel);}
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn wrong_device_foreign_fence_and_reused_epoch_deny(){
        let a=NicRxFence::new(1);let b=NicRxFence::new(2);
        assert!(matches!(a.try_admit(2),Err(RxFenceError::InvalidDevice)));
        let old=a.try_admit(1).unwrap();
        assert!(a.permit_is_current(&old));
        assert!(!b.permit_is_current(&old));
        let q=a.begin_quiesce(1).unwrap();
        assert!(!a.permit_is_current(&old));
        assert_eq!(a.drained(q),Ok(false));
        assert_eq!(a.finish_rebind(q),Err(RxFenceError::Busy));
        assert!(matches!(a.try_admit(1),Err(RxFenceError::Quiescing)));
        drop(old);
        assert_eq!(a.drained(q),Ok(true));
        assert_eq!(a.finish_rebind(q),Ok(2));
        assert_eq!(a.drained(q),Err(RxFenceError::Stale));
        let fresh=a.try_admit(1).unwrap();
        assert_eq!(fresh.epoch(),2);
        assert!(a.permit_is_current(&fresh));
    }
    #[test]
    fn cannot_present_other_device_receipt_or_reuse_previous_one(){
        let a=NicRxFence::new(1);let b=NicRxFence::new(2);
        let q=a.begin_quiesce(1).unwrap();
        assert_eq!(b.finish_rebind(q),Err(RxFenceError::Stale));
        assert!(matches!(a.begin_quiesce(1),Err(RxFenceError::Busy)));
        assert_eq!(a.finish_rebind(q),Ok(2));
        assert_eq!(a.finish_rebind(q),Err(RxFenceError::Stale));
        assert_eq!(a.try_admit(0).err(),Some(RxFenceError::InvalidDevice));
    }
    #[test]
    fn r121_same_device_different_fence_receipt_is_not_transferable(){
        let original=NicRxFence::new(9);
        let replacement=NicRxFence::new(9);
        let original_receipt=original.begin_quiesce(9).unwrap();
        let replacement_receipt=replacement.begin_quiesce(9).unwrap();
        // Both start at epoch one; ID+epoch alone is not authority.
        assert_eq!(original.epoch(),replacement.epoch());
        assert_eq!(replacement.drained(original_receipt),
                   Err(RxFenceError::Stale));
        assert_eq!(replacement.finish_rebind(original_receipt),
                   Err(RxFenceError::Stale));
        assert_eq!(original.drained(replacement_receipt),
                   Err(RxFenceError::Stale));
        assert_eq!(original.finish_rebind(replacement_receipt),
                   Err(RxFenceError::Stale));
        assert_eq!(original.finish_rebind(original_receipt),Ok(2));
        assert_eq!(replacement.finish_rebind(replacement_receipt),Ok(2));
    }
    #[test]
    fn r121_driver_replacement_at_same_address_rejects_old_receipt(){
        let mut slot=std::boxed::Box::new(NicRxFence::new(9));
        let stale=slot.begin_quiesce(9).unwrap();
        let old_epoch=slot.epoch();
        // Drop/recreate in the identical Box location after the old driver
        // has stopped: pointer-based authority would incorrectly accept
        // this stale receipt for the replacement at epoch one.
        *slot=NicRxFence::new(9);
        let fresh=slot.begin_quiesce(9).unwrap();
        assert_eq!(slot.epoch(),old_epoch);
        assert_ne!(fresh.instance,stale.instance);
        assert_eq!(slot.drained(stale),Err(RxFenceError::Stale));
        assert_eq!(slot.finish_rebind(stale),Err(RxFenceError::Stale));
        assert_eq!(slot.drained(fresh),Ok(true));
        assert_eq!(slot.finish_rebind(fresh),Ok(2));
    }
    #[test]
    fn cross_thread_inflight_admission_blocks_rebind_until_drop(){
        use std::sync::{Arc,Barrier};
        let f=Arc::new(NicRxFence::new(5));
        let started=Arc::new(Barrier::new(2));
        let release=Arc::new(Barrier::new(2));
        let (thread_f,s,r)=(Arc::clone(&f),Arc::clone(&started),Arc::clone(&release));
        let t=std::thread::spawn(move||{
            let active=thread_f.try_admit(5).unwrap();
            s.wait();r.wait();
            assert!(!thread_f.permit_is_current(&active));
            drop(active);
        });
        started.wait();
        let q=f.begin_quiesce(5).unwrap();
        assert_eq!(f.finish_rebind(q),Err(RxFenceError::Busy));
        release.wait();
        t.join().unwrap();
        assert_eq!(f.finish_rebind(q),Ok(2));
    }
}
