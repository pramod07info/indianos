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
        // Bind every RX admission to the current driver/ring lifetime, not
        // just to the device number and epoch. Device replacement may start
        // at the same address with the same initial numeric epoch.
        let instance=self.instance_id()?;
        let epoch=self.epoch.load(Ordering::Acquire);
        self.admitted.fetch_update(Ordering::AcqRel,Ordering::Acquire,
            |n|n.checked_add(1)).map_err(|_|RxFenceError::Capacity)?;
        let permit=RxPermit{fence:self,epoch,instance,_not_send:PhantomData};
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
    /// Control-plane visibility into software receive handlers still in
    /// flight for this exact ring lifetime. This count does NOT include
    /// hardware DMA completions; a driver must stop those independently.
    pub fn admitted_in_flight(&self,receipt:QuiesceReceipt)->Result<u32,RxFenceError>{
        if receipt.device!=self.device || self.device==0 ||
           receipt.instance==0 ||
           receipt.instance!=self.instance.load(Ordering::Acquire) ||
           !self.quiescing.load(Ordering::Acquire) ||
           receipt.epoch!=self.epoch.load(Ordering::Acquire) {
            return Err(RxFenceError::Stale);
        }
        Ok(self.admitted.load(Ordering::Acquire))
    }
    pub fn drained(&self,receipt:QuiesceReceipt)->Result<bool,RxFenceError>{
        Ok(self.admitted_in_flight(receipt)?==0)
    }
    /// BSP-only bounded software drain probe. Polls never authorize DMA
    /// shutdown; driver must mask interrupts and stop hardware independently.
    pub fn wait_drained_bounded(&self,receipt:QuiesceReceipt,probes:u32)
        ->Result<(),RxFenceError>{
        if self.drained(receipt)?{return Ok(());}
        for _ in 0..probes{
            core::hint::spin_loop();
            if self.drained(receipt)?{return Ok(());}
        }
        Err(RxFenceError::Busy)
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
        permit.instance!=0 &&
        self.instance.load(Ordering::Acquire)==permit.instance &&
        !self.quiescing.load(Ordering::Acquire) &&
        self.epoch.load(Ordering::Acquire)==permit.epoch
    }
}

/// RAII bounded hardware software-completion admission. No Send or Sync:
/// the receiving CPU must finish its own IRQ/packet ownership processing.
#[must_use="hold through the guarded UDP ingress operation"]
pub struct RxPermit<'a>{
    fence:&'a NicRxFence,epoch:u64,instance:u64,
    _not_send:PhantomData<*mut ()>,
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
    #[test]
    fn r122_same_device_number_is_not_a_ring_lifetime_identity(){
        let a=NicRxFence::new(19);
        let b=NicRxFence::new(19);
        let pa=a.try_admit(19).unwrap();
        let pb=b.try_admit(19).unwrap();
        assert_ne!(pa.instance,pb.instance);
        assert_eq!(pa.epoch(),pb.epoch());
        assert!(a.permit_is_current(&pa));
        assert!(b.permit_is_current(&pb));
        assert!(!a.permit_is_current(&pb));
        assert!(!b.permit_is_current(&pa));
        let q=a.begin_quiesce(19).unwrap();
        assert!(!a.permit_is_current(&pa));
        assert!(b.permit_is_current(&pb));
        assert_eq!(a.drained(q),Ok(false));
        drop(pa);
        assert_eq!(a.drained(q),Ok(true));
        assert_eq!(a.finish_rebind(q),Ok(2));
        assert!(b.permit_is_current(&pb));
    }
    #[test]
    fn r122_admissions_keep_same_lifetime_during_concurrent_start(){
        use std::sync::{Arc,Barrier};
        let f=Arc::new(NicRxFence::new(11));
        let start=Arc::new(Barrier::new(6));
        let threads:std::vec::Vec<_>=(0..6).map(|_|{
            let f=Arc::clone(&f);let start=Arc::clone(&start);
            std::thread::spawn(move||{
                start.wait();
                let p=f.try_admit(11).unwrap();
                let identity=p.instance;
                assert!(identity!=0);
                assert!(f.permit_is_current(&p));
                identity
            })
        }).collect();
        let mut instance=0;
        for t in threads {
            let got=t.join().unwrap();
            if instance==0 {instance=got;}
            assert_eq!(got,instance);
        }
        let q=f.begin_quiesce(11).unwrap();
        assert_eq!(f.drained(q),Ok(true));
        assert_eq!(f.finish_rebind(q),Ok(2));
    }

}

#[cfg(test)]
mod r123_rx_drain_visibility_tests {
    use super::*;
    #[test]
    fn in_flight_count_is_exact_ring_scoped_and_stale_after_rebind() {
        let f=NicRxFence::new(23);
        let foreign=NicRxFence::new(23);
        let p1=f.try_admit(23).unwrap();
        let p2=f.try_admit(23).unwrap();
        let receipt=f.begin_quiesce(23).unwrap();
        assert_eq!(f.admitted_in_flight(receipt),Ok(2));
        assert_eq!(f.drained(receipt),Ok(false));
        assert_eq!(foreign.admitted_in_flight(receipt),Err(RxFenceError::Stale));
        drop(p1);
        assert_eq!(f.admitted_in_flight(receipt),Ok(1));
        drop(p2);
        assert_eq!(f.admitted_in_flight(receipt),Ok(0));
        assert_eq!(f.finish_rebind(receipt),Ok(2));
        assert_eq!(f.admitted_in_flight(receipt),Err(RxFenceError::Stale));
    }
}

#[cfg(test)]
mod m1_ws16_bounded_nic_drain_tests {
 use super::*;
 #[test] fn busy_ring_stays_quiesced_until_software_permit_drops() {
  let nic=NicRxFence::new(10);
  let pending=nic.try_admit(10).unwrap();
  let receipt=nic.begin_quiesce(10).unwrap();
  assert_eq!(nic.wait_drained_bounded(receipt,0),Err(RxFenceError::Busy));
  assert_eq!(nic.wait_drained_bounded(receipt,2),Err(RxFenceError::Busy));
  drop(pending);
  assert_eq!(nic.wait_drained_bounded(receipt,0),Ok(()));
  assert_eq!(nic.finish_rebind(receipt),Ok(2));
  assert_eq!(nic.wait_drained_bounded(receipt,0),Err(RxFenceError::Stale));
 }
}


// PR782 WS65-67: bind NIC reuse to software drain AND trusted hardware
// shutdown evidence, then preserve the exact permit identity across ingress.
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct RxRevision { device:u32, epoch:u64, instance:u64 }
impl NicRxFence{
    /// WS65: Driver control plane must independently establish that IRQs are
    /// masked and DMA is halted. Booleans MUST originate in trusted driver
    /// hardware state, never a guest claim or elapsed-time heuristic.
    pub fn require_hardware_drained(&self,receipt:QuiesceReceipt,
        irqs_masked:bool,dma_stopped:bool)->Result<(),RxFenceError>{
        if !irqs_masked||!dma_stopped{return Err(RxFenceError::Busy);}
        if !self.drained(receipt)?{return Err(RxFenceError::Busy);}
        Ok(())
    }
    /// WS66: Never increment queue generation and resume ingress until all
    /// three boundaries hold: software drain, stopped DMA, masked interrupts.
    pub fn finish_rebind_checked(&self,receipt:QuiesceReceipt,
        irqs_masked:bool,dma_stopped:bool)->Result<u64,RxFenceError>{
        self.require_hardware_drained(receipt,irqs_masked,dma_stopped)?;
        self.finish_rebind(receipt)
    }
    /// WS67: Capture the *live* admission identity, but only with a matching
    /// still-held permit. An epoch number by itself is not a security token.
    pub fn revision_for(&self,permit:&RxPermit<'_>)
        ->Result<RxRevision,RxFenceError>{
        if !self.permit_is_current(permit){return Err(RxFenceError::Stale);}
        Ok(RxRevision{device:self.device,epoch:permit.epoch,instance:permit.instance})
    }
    /// Validate a saved revision only while the exact original live permit
    /// continues to own its device handler slot.
    pub fn revision_is_current(&self,permit:&RxPermit<'_>,revision:RxRevision)->bool{
        self.permit_is_current(permit) &&
        revision==RxRevision{device:self.device,epoch:permit.epoch,instance:permit.instance}
    }
}
#[cfg(test)]
mod pr782_ws65_67_tests{
    use super::*;
    #[test]fn ws65_hardware_proof_and_software_drain_are_both_required(){
        let nic=NicRxFence::new(5);
        let p=nic.try_admit(5).unwrap();let q=nic.begin_quiesce(5).unwrap();
        assert_eq!(nic.require_hardware_drained(q,true,true),Err(RxFenceError::Busy));
        drop(p);
        assert_eq!(nic.require_hardware_drained(q,false,true),Err(RxFenceError::Busy));
        assert_eq!(nic.require_hardware_drained(q,true,false),Err(RxFenceError::Busy));
        assert_eq!(nic.require_hardware_drained(q,true,true),Ok(()));
    }
    #[test]fn ws66_checked_rebind_denies_untrusted_resume(){
        let nic=NicRxFence::new(5);let q=nic.begin_quiesce(5).unwrap();
        assert_eq!(nic.finish_rebind_checked(q,false,true),Err(RxFenceError::Busy));
        assert_eq!(nic.try_admit(5).err(),Some(RxFenceError::Quiescing));
        assert_eq!(nic.finish_rebind_checked(q,true,true),Ok(2));
        assert_eq!(nic.finish_rebind_checked(q,true,true),Err(RxFenceError::Stale));
    }
    #[test]fn ws67_revoked_or_foreign_permit_cannot_authorize_ingress(){
        let a=NicRxFence::new(5);let b=NicRxFence::new(5);
        let pa=a.try_admit(5).unwrap();let pb=b.try_admit(5).unwrap();
        let rev=a.revision_for(&pa).unwrap();
        assert!(a.revision_is_current(&pa,rev));
        assert!(!b.revision_is_current(&pb,rev));
        let q=a.begin_quiesce(5).unwrap();
        assert_eq!(a.revision_for(&pa),Err(RxFenceError::Stale));
        assert!(!a.revision_is_current(&pa,rev));
        drop(pa);a.finish_rebind_checked(q,true,true).unwrap();
        let fresh=a.try_admit(5).unwrap();
        assert!(!a.revision_is_current(&fresh,rev));
    }
}


/// P0 W5: kernel-owned NIC controller shutdown callback. The driver must
/// actually mask MSI/MSI-X, stop bus mastering and drain DMA descriptors
/// BEFORE returning success. This type cannot establish the physical fact;
/// a hardware driver implementor and independent qualification are required.
pub trait NicHardwareQuiesce{
 fn stop_irq_dma_and_drain(&mut self,device:u32)->Result<(),RxFenceError>;
}
impl NicRxFence{
 /// Keep the RX fence quiescing on all errors. No caller may fabricate a
 /// success boolean to reopen ingress through this driver-facing operation.
 pub fn quiesce_and_rebind_with_driver<D:NicHardwareQuiesce>(&self,
  driver:&mut D,device:u32,software_probes:u32)->Result<u64,RxFenceError>{
  let receipt=self.begin_quiesce(device)?;
  driver.stop_irq_dma_and_drain(device)?;
  self.wait_drained_bounded(receipt,software_probes)?;
  self.finish_rebind_checked(receipt,true,true)
 }
}
#[cfg(test)]
mod kernel_p0_w5_nic_teardown_tests{
 use super::*;
 struct Driver{stops:u32,fail:bool}
 impl NicHardwareQuiesce for Driver{
  fn stop_irq_dma_and_drain(&mut self,_:u32)->Result<(),RxFenceError>{
   self.stops+=1;
   if self.fail{Err(RxFenceError::Busy)}else{Ok(())}
  }
 }
 #[test]fn software_handler_must_drain_and_failed_driver_never_reopens_queue(){
  let nic=NicRxFence::new(9);
  let pending=nic.try_admit(9).unwrap();
  let mut d=Driver{stops:0,fail:false};
  assert_eq!(nic.quiesce_and_rebind_with_driver(&mut d,9,0),
     Err(RxFenceError::Busy));
  assert_eq!(nic.try_admit(9).err(),Some(RxFenceError::Quiescing));
  assert_eq!(d.stops,1);
  drop(pending);
  // The separate privileged control plane can resume ONLY after it has
  // independently checked the actual hardware stopped and the software
  // admission count drained. No auto-resume occurs on the busy path.
  assert_eq!(nic.epoch(),1);
 }
 #[test]fn successful_driver_halt_invalidates_prior_ring_epoch(){
  let nic=NicRxFence::new(10);
  let mut d=Driver{stops:0,fail:false};
  assert_eq!(nic.quiesce_and_rebind_with_driver(&mut d,10,1),Ok(2));
  assert_eq!(d.stops,1);
  assert_eq!(nic.epoch(),2);
  let p=nic.try_admit(10).unwrap();
  assert_eq!(p.epoch(),2);
 }
 #[test]fn driver_halt_failure_never_claims_new_generation(){
  let nic=NicRxFence::new(11);
  let mut d=Driver{stops:0,fail:true};
  assert_eq!(nic.quiesce_and_rebind_with_driver(&mut d,11,1),
    Err(RxFenceError::Busy));
  assert_eq!(nic.epoch(),1);
  assert_eq!(nic.try_admit(11).err(),Some(RxFenceError::Quiescing));
 }
}


/// P0 follow-up W5: no packet bytes or a parsed result may escape the NIC
/// receive admission epoch if IRQ teardown races with decoding. The closure
/// must parse into private scratch and perform NO externally visible effects;
/// a later caller still needs an authenticated socket/UDP policy decision.
impl NicRxFence{
 pub fn parse_packet_at_live_epoch<R>(&self,device:u32,
  parse:impl FnOnce()->Option<R>)->Result<R,RxFenceError>{
  let permit=self.try_admit(device)?;
  let revision=self.revision_for(&permit)?;
  let decoded=parse().ok_or(RxFenceError::Stale)?;
  if !self.revision_is_current(&permit,revision){
   return Err(RxFenceError::Stale);
  }
  Ok(decoded)
 }
}
#[cfg(test)]
mod kernel_p0_next_w5_rx_admission_tests{
 use super::*;
 #[test]fn packet_validation_result_cannot_cross_quiescing_transition(){
  let nic=NicRxFence::new(13);
  assert_eq!(nic.parse_packet_at_live_epoch(13,||Some(25u32)),Ok(25));
  assert_eq!(nic.parse_packet_at_live_epoch(12,||Some(25u32)),
   Err(RxFenceError::InvalidDevice));
  assert_eq!(nic.parse_packet_at_live_epoch(13,||{
   nic.begin_quiesce(13).unwrap();
   Some(25u32)
  }),Err(RxFenceError::Stale));
  assert_eq!(nic.parse_packet_at_live_epoch(13,||Some(25u32)),
   Err(RxFenceError::Quiescing));
 }
 #[test]fn failed_decoder_does_not_hold_a_software_completion_permit(){
  let nic=NicRxFence::new(14);
  assert_eq!(nic.parse_packet_at_live_epoch::<u32>(14,||None),
   Err(RxFenceError::Stale));
  let r=nic.begin_quiesce(14).unwrap();
  assert_eq!(nic.drained(r),Ok(true));
  assert_eq!(nic.finish_rebind_checked(r,true,true),Ok(2));
  assert_eq!(nic.parse_packet_at_live_epoch(14,||Some(4u8)),Ok(4));
 }
}


/// P0 batch three W5: a separately owned quiescence receipt allows a
/// privileged control-plane retry after a late software receive handler
/// exits. The REAL NIC driver must successfully stop IRQ/DMA on each attempt.
/// The opaque receipt must come from begin_quiesce on this exact fence.
impl NicRxFence{
 pub fn complete_quiesced_rebind_with_driver<D:NicHardwareQuiesce>(
  &self,receipt:QuiesceReceipt,driver:&mut D,probes:u32)
  ->Result<u64,RxFenceError>{
  self.admitted_in_flight(receipt)?;
  driver.stop_irq_dma_and_drain(self.device)?;
  self.wait_drained_bounded(receipt,probes)?;
  self.finish_rebind_checked(receipt,true,true)
 }
}
#[cfg(test)]
mod kernel_p0_batch3_w5_nic_retry_tests{
 use super::*;
 struct Driver{count:usize,stopped:bool}
 impl NicHardwareQuiesce for Driver{
  fn stop_irq_dma_and_drain(&mut self,_:u32)->Result<(),RxFenceError>{
   self.count+=1;
   if self.stopped{Ok(())}else{Err(RxFenceError::Busy)}
  }
 }
 #[test]fn retry_cannot_resume_while_handler_live_or_hardware_not_stopped(){
  let f=NicRxFence::new(21);
  let handler=f.try_admit(21).unwrap();
  let receipt=f.begin_quiesce(21).unwrap();
  let mut d=Driver{count:0,stopped:true};
  assert_eq!(f.complete_quiesced_rebind_with_driver(receipt,&mut d,0),
   Err(RxFenceError::Busy));
  assert_eq!(f.try_admit(21).err(),Some(RxFenceError::Quiescing));
  drop(handler);d.stopped=false;
  assert_eq!(f.complete_quiesced_rebind_with_driver(receipt,&mut d,0),
   Err(RxFenceError::Busy));
  d.stopped=true;
  assert_eq!(f.complete_quiesced_rebind_with_driver(receipt,&mut d,0),Ok(2));
  assert_eq!(f.complete_quiesced_rebind_with_driver(receipt,&mut d,0),
   Err(RxFenceError::Stale));
  assert_eq!(f.try_admit(21).unwrap().epoch(),2);
 }
 #[test]fn foreign_ring_receipt_does_not_invoke_wrong_hardware_driver(){
  let a=NicRxFence::new(24);let b=NicRxFence::new(24);
  let receipt=a.begin_quiesce(24).unwrap();
  let mut d=Driver{count:0,stopped:true};
  assert_eq!(b.complete_quiesced_rebind_with_driver(receipt,&mut d,0),
   Err(RxFenceError::Stale));
  assert_eq!(d.count,0);
 }
}

/// P0-76..77: parser runs only for a bounded nonempty frame, while an exact
/// RX revision stays live. Parser must be side-effect-free until final check.
impl NicRxFence{
 pub fn parse_bounded_frame<R>(&self,device:u32,frame:&[u8],
  maximum:usize,parse:impl FnOnce(&[u8])->Option<R>)
  ->Result<R,RxFenceError>{
  if frame.is_empty()||maximum==0||frame.len()>maximum{
   return Err(RxFenceError::Capacity);
  }
  self.parse_packet_at_live_epoch(device,||parse(frame))
 }
}
#[cfg(test)]
mod kernel_p0_batch4_w5_packet_limits_tests{
 use super::*;
 #[test]fn malformed_size_cannot_invoke_parser(){
  let f=NicRxFence::new(8);let mut calls=0;
  assert_eq!(f.parse_bounded_frame(8,&[],128,|_|{calls+=1;Some(1)}),
   Err(RxFenceError::Capacity));
  assert_eq!(f.parse_bounded_frame(8,&[1,2],1,|_|{calls+=1;Some(1)}),
   Err(RxFenceError::Capacity));
  assert_eq!(calls,0);
  assert_eq!(f.parse_bounded_frame(8,&[1,2],2,|b|Some(b.len())),Ok(2));
 }
 #[test]fn invalid_device_or_teardown_race_disallows_result(){
  let f=NicRxFence::new(8);
  assert_eq!(f.parse_bounded_frame(9,&[4],4,|_|Some(3u8)),
   Err(RxFenceError::InvalidDevice));
  assert_eq!(f.parse_bounded_frame(8,&[4],4,|_|{
   f.begin_quiesce(8).unwrap();Some(3u8)
  }),Err(RxFenceError::Stale));
  assert_eq!(f.parse_bounded_frame(8,&[4],4,|_|Some(3u8)),
   Err(RxFenceError::Quiescing));
 }
}
