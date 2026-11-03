//! R120: allocation-free, non-spinning admission for single-BSP retirement.
//! No IRQ-driven NIC, SMP scheduling, IPI delivery or hardware DMA safety is
//! implied. An interrupted owner must never spin waiting for its own lock.
use core::sync::atomic::{AtomicBool,AtomicU32,Ordering};
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
        Ok(Permit{gate:self})
    }
}
impl Default for RetirementGate {fn default()->Self{Self::new()}}
#[must_use="retain this permit during kernel table access"]
pub struct Permit<'a>{gate:&'a RetirementGate}
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
