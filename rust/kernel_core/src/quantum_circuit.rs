//! R017.2: bounded, allocation-free quantum circuit executor.
//! Validate the entire circuit before execution to prevent partial runs.
use crate::quantum::{QuantumError, Simulator, MAX_QUBITS};
pub const MAX_GATES:usize=64;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum Gate { H(usize), X(usize), Z(usize), Cnot{control:usize,target:usize} }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum CircuitError { InvalidQubit, InvalidGate, CapacityExceeded }
pub struct Circuit {
    qubits:usize,
    gates:[Option<Gate>;MAX_GATES],
    len:usize,
}
impl Circuit {
    pub fn new(qubits:usize)->Result<Self,CircuitError>{
        if qubits==0 || qubits>MAX_QUBITS{return Err(CircuitError::InvalidQubit);}
        Ok(Self{qubits,gates:[None;MAX_GATES],len:0})
    }
    fn validate(&self,gate:Gate)->Result<(),CircuitError>{
        match gate {
            Gate::H(q)|Gate::X(q)|Gate::Z(q) if q<self.qubits=>Ok(()),
            Gate::Cnot{control,target} if control<self.qubits && target<self.qubits && control!=target=>Ok(()),
            _=>Err(CircuitError::InvalidGate),
        }
    }
    pub fn push(&mut self,gate:Gate)->Result<(),CircuitError>{
        self.validate(gate)?;
        if self.len==MAX_GATES{return Err(CircuitError::CapacityExceeded);}
        self.gates[self.len]=Some(gate);
        self.len+=1;
        Ok(())
    }
    pub fn len(&self)->usize{self.len}
    pub fn is_empty(&self)->bool{self.len==0}
    pub fn execute(&self)->Result<Simulator,CircuitError>{
        let mut sim=Simulator::new(self.qubits).map_err(|_|CircuitError::InvalidQubit)?;
        for gate in self.gates[..self.len].iter().flatten(){
            let result:Result<(),QuantumError>=match *gate {
                Gate::H(q)=>sim.h(q),
                Gate::X(q)=>sim.x(q),
                Gate::Z(q)=>sim.z(q),
                Gate::Cnot{control,target}=>sim.cnot(control,target),
            };
            result.map_err(|_|CircuitError::InvalidGate)?;
        }
        Ok(sim)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn near(a:f64,b:f64){assert!((a-b).abs()<1e-12,"{a} != {b}");}
    #[test] fn bell_circuit(){
        let mut circuit=Circuit::new(2).unwrap();
        circuit.push(Gate::H(0)).unwrap();
        circuit.push(Gate::Cnot{control:0,target:1}).unwrap();
        assert_eq!(circuit.len(),2);
        let sim=circuit.execute().unwrap();
        near(sim.probability(0).unwrap(),0.5);
        near(sim.probability(3).unwrap(),0.5);
        near(sim.probability(1).unwrap(),0.0);
        near(sim.probability(2).unwrap(),0.0);
    }
    #[test] fn ghz_circuit(){
        let mut circuit=Circuit::new(3).unwrap();
        circuit.push(Gate::H(0)).unwrap();
        circuit.push(Gate::Cnot{control:0,target:1}).unwrap();
        circuit.push(Gate::Cnot{control:1,target:2}).unwrap();
        let sim=circuit.execute().unwrap();
        for basis in 0..8 {near(sim.probability(basis).unwrap(),if basis==0 || basis==7{0.5}else{0.0});}
    }
    #[test] fn rejects_invalid_and_overflow_without_mutation(){
        assert!(matches!(Circuit::new(0),Err(CircuitError::InvalidQubit)));
        let mut circuit=Circuit::new(2).unwrap();
        assert_eq!(circuit.push(Gate::H(2)),Err(CircuitError::InvalidGate));
        assert_eq!(circuit.push(Gate::Cnot{control:0,target:0}),Err(CircuitError::InvalidGate));
        assert!(circuit.is_empty());
        for _ in 0..MAX_GATES {circuit.push(Gate::X(0)).unwrap();}
        assert_eq!(circuit.push(Gate::Z(0)),Err(CircuitError::CapacityExceeded));
        assert_eq!(circuit.len(),MAX_GATES);
        near(circuit.execute().unwrap().probability(0).unwrap(),1.0);
    }
}
