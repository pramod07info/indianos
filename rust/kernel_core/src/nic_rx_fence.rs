//! R134: fail-closed NIC receive admission while IRQ/DMA rings quiesce.
//!
//! Lock-free atomic admission only. This does not program physical NIC
//! interrupts, interrupt remapping, PCI bus-master disable or IOMMU domains.
//! A real driver MUST stop device DMA and mask MSI/MSI-X before accepting the
//! returned quiescence receipt; only after in-flight software handlers drain
//! may it retire port/owner authority or recycle a queue.
//! This standalone type is safe for tests and the exclusive polled QEMU NIC.

use core::{marker::PhantomData,ptr,sync::atomic::{AtomicBool,AtomicU32,AtomicU64,Ordering}};

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum RxFenceError {InvalidDevice,Quiescing,Busy,Capacity,Stale,EpochExhausted}

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct QuiesceReceipt { device:u32, epoch:u64 }

/// One receive device/ring. At most one control-plane quiescer at a time.
/// No allocations, lock spinning, or IRQ-side calls to a scheduler lock.
pub struct NicRxFence {
    device:u32,
    epoch:AtomicU64,
    admitted:AtomicU32,
    quiescing:AtomicBool,
}
impl NicRxFence {
    pub const fn new(device:u32)->Self{
        Self{device,epoch:AtomicU64::new(1),admitted:AtomicU32::new(0),
             quiescing:AtomicBool::new(false)}
    }
    pub fn device(&self)->u32{self.device}
    pub fn epoch(&self)->u64{self.epoch.load(Ordering::Acquire)}

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
        self.quiescing.compare_exchange(false,true,Ordering::AcqRel,
            Ordering::Acquire).map_err(|_|RxFenceError::Busy)?;
        Ok(QuiesceReceipt{device,epoch:self.epoch.load(Ordering::Acquire)})
    }
    /// No busy waiting in an ISR; the BSP polls this after hardware is still.
    pub fn drained(&self,receipt:QuiesceReceipt)->Result<bool,RxFenceError>{
        if receipt.device!=self.device || self.device==0 ||
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
