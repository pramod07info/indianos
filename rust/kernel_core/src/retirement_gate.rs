//! R120: allocation-free, non-spinning admission for single-BSP retirement.
//! No IRQ-driven NIC, SMP scheduling, IPI delivery or hardware DMA safety is
//! implied. An interrupted owner must never spin waiting for its own lock.
use core::{marker::PhantomData,sync::atomic::{AtomicBool,AtomicU32,Ordering}};
const UNBOUND:u32=u32::MAX;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum GateError { Unbound, WrongCpu, InterruptsEnabled, Reentrant, InvalidCpu }

pub struct RetirementGate { bsp:AtomicU32, held:AtomicBool }
impl RetirementGate {
    pub const fn new()->Self {
        Self { bsp:AtomicU32::new(UNBOUND), held:AtomicBool::new(false) }
    }
    /// Called by the trusted scheduler on the bootstrap CPU only.
    pub fn register_bsp(&self,cpu:u32,masked:bool)->Result<(),GateError>{
        if cpu==UNBOUND{return Err(GateError::InvalidCpu);}
        if !masked{return Err(GateError::InterruptsEnabled);}
        match self.bsp.compare_exchange(UNBOUND,cpu,Ordering::AcqRel,Ordering::Acquire){
            Ok(_)=>Ok(()),
            Err(same) if same==cpu=>Ok(()),
            Err(_)=>Err(GateError::WrongCpu),
        }
    }
    /// No spinning or nested ownership. Lifetime of returned permit scopes
    /// exclusive coordinator and UDP-queue access.
    /// Validate an exact live BSP permit before touching protected tables.
    /// A same-CPU permit issued by a distinct gate is never authority.
    pub fn permit_is_current(&self,permit:&Permit<'_>)->bool{
        core::ptr::eq(self,permit.gate)&&self.held.load(Ordering::Acquire)
    }
    pub fn try_enter(&self,cpu:u32,masked:bool)->Result<Permit<'_>,GateError>{
        if !masked{return Err(GateError::InterruptsEnabled);}
        let bsp=self.bsp.load(Ordering::Acquire);
        if bsp==UNBOUND{return Err(GateError::Unbound);}
        if bsp!=cpu{return Err(GateError::WrongCpu);}
        self.held.compare_exchange(false,true,Ordering::Acquire,Ordering::Relaxed)
            .map_err(|_|GateError::Reentrant)?;
        Ok(Permit{gate:self,_not_send:PhantomData})
    }
}
impl Default for RetirementGate {fn default()->Self{Self::new()}}
#[must_use="retain this permit during kernel table access"]
pub struct Permit<'a>{gate:&'a RetirementGate,_not_send:PhantomData<*mut ()>}
impl Permit<'_>{
    /// A caller must prove this permit was issued by the EXACT gate which
    /// protects the mutable production tables. Holding a permit from a
    /// separate, locally created gate is not sufficient admission.
    pub fn issued_by(&self,expected:&RetirementGate)->bool{
        core::ptr::eq(self.gate,expected)
    }
}
impl Drop for Permit<'_>{
    fn drop(&mut self){self.gate.held.store(false,Ordering::Release);}
}

#[cfg(test)]
mod r120_gate_tests {
    use super::*;
    #[test] fn denies_unbound_wrong_core_and_interrupts(){
        let g=RetirementGate::new();
        assert!(matches!(g.try_enter(0,true),Err(GateError::Unbound)));
        assert_eq!(g.register_bsp(1,false),Err(GateError::InterruptsEnabled));
        assert_eq!(g.register_bsp(u32::MAX,true),Err(GateError::InvalidCpu));
        assert_eq!(g.register_bsp(1,true),Ok(()));
        assert_eq!(g.register_bsp(2,true),Err(GateError::WrongCpu));
        assert!(matches!(g.try_enter(2,true),Err(GateError::WrongCpu)));
        assert!(matches!(g.try_enter(1,false),Err(GateError::InterruptsEnabled)));
    }
    #[test] fn nested_cannot_spin_and_drop_releases(){
        let g=RetirementGate::new();g.register_bsp(7,true).unwrap();
        let other=RetirementGate::new();
        other.register_bsp(7,true).unwrap();
        let a=g.try_enter(7,true).unwrap();
        assert!(a.issued_by(&g));
        assert!(!a.issued_by(&other));
        assert!(matches!(g.try_enter(7,true),Err(GateError::Reentrant)));
        drop(a);
        assert!(g.try_enter(7,true).is_ok());
    }
}

#[cfg(test)]
mod r118_host_concurrency_regressions {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize,Ordering as HostOrdering},
        Arc,Barrier,
    };
    use std::thread;

    #[test]
    fn untrusted_cross_cpu_threads_cannot_enter_bsp_retirement() {
        let gate=Arc::new(RetirementGate::new());
        gate.register_bsp(0,true).unwrap();
        let mut workers=std::vec::Vec::new();
        for cpu in 1..=8 {
            let gate=Arc::clone(&gate);
            workers.push(thread::spawn(move || {
                for _ in 0..1024 {
                    assert!(matches!(gate.try_enter(cpu,true),
                        Err(GateError::WrongCpu)));
                }
            }));
        }
        for thread in workers {thread.join().unwrap();}
        let permit=gate.try_enter(0,true).unwrap();
        assert!(matches!(gate.try_enter(0,true),Err(GateError::Reentrant)));
        drop(permit);
    }

    #[test]
    fn host_contention_never_admits_two_simultaneous_permits() {
        // All host threads deliberately claim BSP ID 0; this tests the
        // atomic admission primitive only, not true multiprocessor IDs,
        // IRQ ordering or live scheduler IPI delivery.
        let gate=Arc::new(RetirementGate::new());
        gate.register_bsp(0,true).unwrap();
        let start=Arc::new(Barrier::new(8));
        let active=Arc::new(AtomicUsize::new(0));
        let admitted=Arc::new(AtomicUsize::new(0));
        let mut workers=std::vec::Vec::new();
        for _ in 0..8 {
            let (gate,start,active,admitted)=(
                Arc::clone(&gate),Arc::clone(&start),
                Arc::clone(&active),Arc::clone(&admitted));
            workers.push(thread::spawn(move || {
                start.wait();
                for _ in 0..2048 {
                    match gate.try_enter(0,true) {
                        Ok(permit)=>{
                            assert_eq!(active.fetch_add(1,HostOrdering::SeqCst),0);
                            thread::yield_now();
                            assert_eq!(active.fetch_sub(1,HostOrdering::SeqCst),1);
                            admitted.fetch_add(1,HostOrdering::Relaxed);
                            drop(permit);
                        },
                        Err(GateError::Reentrant)=>thread::yield_now(),
                        Err(other)=>panic!("unexpected gate failure: {other:?}"),
                    }
                }
            }));
        }
        for thread in workers {thread.join().unwrap();}
        assert_eq!(active.load(HostOrdering::SeqCst),0);
        assert!(admitted.load(HostOrdering::SeqCst)>0);
    }
}

#[cfg(test)]
mod m1_ws15_gate_identity_tests {
 use super::*;
 #[test] fn permit_never_transfers_to_foreign_gate_on_same_cpu() {
  let a=RetirementGate::new();let b=RetirementGate::new();
  a.register_bsp(3,true).unwrap();b.register_bsp(3,true).unwrap();
  let permit=a.try_enter(3,true).unwrap();
  assert!(a.permit_is_current(&permit));
  assert!(!b.permit_is_current(&permit));
  assert!(matches!(a.try_enter(3,true),Err(GateError::Reentrant)));
 }
}


// PR782 WS88-90: small BSP-only critical-section policy adapters.
// No locks, real interrupt masking, IPI or NIC DMA control are performed.
impl RetirementGate{
 /// WS88: Expose an immutable registered BSP identity for trusted kernel
 /// lifecycle assertions. An unbound gate never reports CPU 0 as the owner.
 pub fn registered_bsp(&self)->Result<u32,GateError>{
  let cpu=self.bsp.load(Ordering::Acquire);
  if cpu==UNBOUND{return Err(GateError::Unbound);}
  Ok(cpu)
 }
 /// WS89: Scope exclusive access to a single caller operation. The
 /// permit is automatically released on return or early error.
 pub fn with_bsp_exclusive<R,F:FnOnce(&Permit<'_>)->R>(&self,
  cpu:u32,masked:bool,operation:F)->Result<R,GateError>{
  let permit=self.try_enter(cpu,masked)?;
  let value=operation(&permit);
  Ok(value)
 }
 /// WS90: Enforce both the currently active permit and exact BSP/interrupt
 /// preconditions at the protected-table access boundary.
 pub fn require_active_permit(&self,permit:&Permit<'_>,cpu:u32,
  interrupts_masked:bool)->Result<(),GateError>{
  if !interrupts_masked{return Err(GateError::InterruptsEnabled);}
  if self.registered_bsp()?!=cpu{return Err(GateError::WrongCpu);}
  if !self.permit_is_current(permit){return Err(GateError::Reentrant);}
  Ok(())
 }
}
#[cfg(test)]
mod pr782_ws88_90_tests{
 use super::*;
 #[test]fn ws88_bsp_identity_is_never_implicitly_zero(){
  let g=RetirementGate::new();
  assert_eq!(g.registered_bsp(),Err(GateError::Unbound));
  g.register_bsp(9,true).unwrap();
  assert_eq!(g.registered_bsp(),Ok(9));
 }
 #[test]fn ws89_scoped_permit_prevents_reentrant_access_and_releases(){
  let gate=RetirementGate::new();gate.register_bsp(3,true).unwrap();
  let result=gate.with_bsp_exclusive(3,true,|permit|{
   assert!(gate.permit_is_current(permit));
   assert!(matches!(gate.try_enter(3,true),Err(GateError::Reentrant)));
   42
  });
  assert_eq!(result,Ok(42));
  assert!(gate.try_enter(3,true).is_ok());
  assert_eq!(gate.with_bsp_exclusive(3,false,|_|1),
   Err(GateError::InterruptsEnabled));
 }
 #[test]fn ws90_foreign_gate_cpu_or_unmasked_access_is_denied(){
  let a=RetirementGate::new();let b=RetirementGate::new();
  a.register_bsp(1,true).unwrap();b.register_bsp(1,true).unwrap();
  let permit=a.try_enter(1,true).unwrap();
  assert_eq!(a.require_active_permit(&permit,1,true),Ok(()));
  assert_eq!(a.require_active_permit(&permit,1,false),
   Err(GateError::InterruptsEnabled));
  assert_eq!(a.require_active_permit(&permit,2,true),Err(GateError::WrongCpu));
  assert_eq!(b.require_active_permit(&permit,1,true),Err(GateError::Reentrant));
 }
}
