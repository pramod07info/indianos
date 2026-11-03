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
        let a=g.try_enter(7,true).unwrap();
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
