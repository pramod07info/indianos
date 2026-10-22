//! R017 experimental CPU-only quantum circuit simulator.
//! Fixed-size state vector: at most three qubits, no allocator or QPU.
//! Not a quantum speedup, cryptographic RNG, or a kernel scheduler.
use core::f64::consts::FRAC_1_SQRT_2;
pub const MAX_QUBITS:usize=3;
pub const AMPLITUDES:usize=1<<MAX_QUBITS;
#[derive(Clone,Copy,Debug,Default,PartialEq)]
pub struct Complex {pub re:f64,pub im:f64}
impl Complex {
    const ZERO:Self=Self{re:0.0,im:0.0};
    fn probability(self)->f64{self.re*self.re+self.im*self.im}
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum QuantumError{InvalidQubit,InvalidControl,InvalidState}
pub struct Simulator {qubits:usize,amplitudes:[Complex;AMPLITUDES]}
impl Simulator {
    pub fn new(qubits:usize)->Result<Self,QuantumError>{
        if qubits==0 || qubits>MAX_QUBITS{return Err(QuantumError::InvalidQubit);}
        let mut amplitudes=[Complex::ZERO;AMPLITUDES];
        amplitudes[0].re=1.0;
        Ok(Self{qubits,amplitudes})
    }
    fn mask(&self,q:usize)->Result<usize,QuantumError>{
        if q>=self.qubits{Err(QuantumError::InvalidQubit)}else{Ok(1<<q)}
    }
    pub fn x(&mut self,q:usize)->Result<(),QuantumError>{
        let bit=self.mask(q)?;
        for i in 0..(1<<self.qubits) {
            if i&bit==0 {self.amplitudes.swap(i,i|bit);}
        }
        Ok(())
    }
    pub fn z(&mut self,q:usize)->Result<(),QuantumError>{
        let bit=self.mask(q)?;
        for i in 0..(1<<self.qubits) {
            if i&bit!=0 {self.amplitudes[i].re = -self.amplitudes[i].re;
                         self.amplitudes[i].im = -self.amplitudes[i].im;}
        }
        Ok(())
    }
    pub fn h(&mut self,q:usize)->Result<(),QuantumError>{
        let bit=self.mask(q)?;
        for i in 0..(1<<self.qubits) {
            if i&bit==0 {
                let a=self.amplitudes[i];
                let b=self.amplitudes[i|bit];
                self.amplitudes[i]=Complex{re:(a.re+b.re)*FRAC_1_SQRT_2,im:(a.im+b.im)*FRAC_1_SQRT_2};
                self.amplitudes[i|bit]=Complex{re:(a.re-b.re)*FRAC_1_SQRT_2,im:(a.im-b.im)*FRAC_1_SQRT_2};
            }
        }
        Ok(())
    }
    pub fn cnot(&mut self,control:usize,target:usize)->Result<(),QuantumError>{
        let c=self.mask(control)?;
        let t=self.mask(target)?;
        if control==target{return Err(QuantumError::InvalidControl);}
        for i in 0..(1<<self.qubits) {
            if i&c!=0 && i&t==0 {self.amplitudes.swap(i,i|t);}
        }
        Ok(())
    }
    pub fn probability(&self,basis:usize)->Result<f64,QuantumError>{
        if basis>=(1<<self.qubits){return Err(QuantumError::InvalidState);}
        Ok(self.amplitudes[basis].probability())
    }
    /// Caller supplies a uniform value in [0,1). No randomness is generated.
    pub fn sample(&self,unit:f64)->Result<usize,QuantumError>{
        if !unit.is_finite() || !(0.0..1.0).contains(&unit){return Err(QuantumError::InvalidState);}
        let mut cumulative=0.0;
        let mut last=0;
        for i in 0..(1<<self.qubits){
            let p=self.amplitudes[i].probability();
            if p>0.0{last=i;cumulative+=p;if unit<cumulative{return Ok(i);}}
        }
        Ok(last) // absorb floating-point rounding at the upper boundary
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn close(a:f64,b:f64){assert!((a-b).abs()<1e-12,"{a} != {b}");}
    #[test] fn bell_state(){
        let mut s=Simulator::new(2).unwrap();
        s.h(0).unwrap();s.cnot(0,1).unwrap();
        close(s.probability(0).unwrap(),0.5);
        close(s.probability(3).unwrap(),0.5);
        close(s.probability(1).unwrap(),0.0);
        close(s.probability(2).unwrap(),0.0);
        assert_eq!(s.sample(0.1),Ok(0));
        assert_eq!(s.sample(0.9),Ok(3));
    }
    #[test] fn gates_and_validation(){
        let mut s=Simulator::new(1).unwrap();
        s.x(0).unwrap();close(s.probability(1).unwrap(),1.0);
        s.z(0).unwrap();s.h(0).unwrap();
        close(s.probability(0).unwrap(),0.5);
        assert_eq!(s.cnot(0,0),Err(QuantumError::InvalidControl));
        assert_eq!(s.x(1),Err(QuantumError::InvalidQubit));
        assert_eq!(s.sample(f64::NAN),Err(QuantumError::InvalidState));
        assert!(Simulator::new(0).is_err());
        assert!(Simulator::new(MAX_QUBITS+1).is_err());
    }
}
