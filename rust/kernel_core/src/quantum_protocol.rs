//! R017.3 quantum application protocol foundation.
//! This module decodes a bounded untrusted byte message into a validated circuit.
//! It does not yet install a ring-3 syscall or copy data from userspace.
//!
//! Wire format (little-endian, version 1):
//! byte 0 version, byte 1 qubits, byte 2 gate count, byte 3 reserved=0.
//! Each gate is 3 bytes: opcode, first qubit, second qubit.
//! Opcodes: 1=H, 2=X, 3=Z (second byte must be 0), 4=CNOT.
use crate::quantum::{MAX_QUBITS,AMPLITUDES};
use crate::quantum_circuit::{Circuit,CircuitError,Gate,MAX_GATES};
pub const VERSION:u8=1;
pub const HEADER_BYTES:usize=4;
pub const GATE_BYTES:usize=3;
pub const MAX_REQUEST_BYTES:usize=HEADER_BYTES+MAX_GATES*GATE_BYTES;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum ProtocolError {Malformed,UnsupportedVersion,InvalidQubit,InvalidGate,TooManyGates,OutputTooSmall}
impl From<CircuitError> for ProtocolError {
    fn from(error:CircuitError)->Self {
        match error {
            CircuitError::InvalidQubit=>Self::InvalidQubit,
            CircuitError::InvalidGate=>Self::InvalidGate,
            CircuitError::CapacityExceeded=>Self::TooManyGates,
        }
    }
}
pub fn decode(request:&[u8])->Result<Circuit,ProtocolError>{
    if request.len()<HEADER_BYTES{return Err(ProtocolError::Malformed);}
    if request[0]!=VERSION{return Err(ProtocolError::UnsupportedVersion);}
    let qubits=request[1] as usize;
    let count=request[2] as usize;
    if count>MAX_GATES{return Err(ProtocolError::TooManyGates);}
    if request[3]!=0 || request.len()!=HEADER_BYTES+count*GATE_BYTES{
        return Err(ProtocolError::Malformed);
    }
    let mut circuit=Circuit::new(qubits)?;
    for chunk in request[HEADER_BYTES..].chunks_exact(GATE_BYTES){
        let a=chunk[1] as usize;
        let b=chunk[2] as usize;
        let gate=match chunk[0]{
            1 if b==0=>Gate::H(a),
            2 if b==0=>Gate::X(a),
            3 if b==0=>Gate::Z(a),
            4=>Gate::Cnot{control:a,target:b},
            _=>return Err(ProtocolError::InvalidGate),
        };
        circuit.push(gate)?;
    }
    Ok(circuit)
}
/// Output is a fixed-size, little-endian array of f64 basis probabilities.
/// The caller supplies a trusted output buffer; no user pointers are touched.
/// On failure the output is not modified.
pub fn execute_probabilities(request:&[u8],output:&mut [u8])->Result<usize,ProtocolError>{
    let circuit=decode(request)?;
    // The wire request was fully validated before any simulation work.
    // This fixed response supports MAX_QUBITS, with unused entries zeroed.
    const RESPONSE_BYTES:usize=AMPLITUDES*8;
    if output.len()<RESPONSE_BYTES{return Err(ProtocolError::OutputTooSmall);}
    let sim=circuit.execute()?;
    let mut response=[0u8;RESPONSE_BYTES];
    for basis in 0..AMPLITUDES {
        // Indices outside the circuit's qubit count return InvalidState.
        let probability=sim.probability(basis).unwrap_or(0.0);
        response[basis*8..(basis+1)*8].copy_from_slice(&probability.to_le_bytes());
    }
    output[..RESPONSE_BYTES].copy_from_slice(&response);
    Ok(RESPONSE_BYTES)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn probability(bytes:&[u8],index:usize)->f64{
        f64::from_le_bytes(bytes[index*8..index*8+8].try_into().unwrap())
    }
    #[test] fn bell_wire_request(){
        let request=[VERSION,2,2,0,1,0,0,4,0,1];
        let mut out=[0u8;AMPLITUDES*8];
        assert_eq!(execute_probabilities(&request,&mut out),Ok(out.len()));
        assert!((probability(&out,0)-0.5).abs()<1e-12);
        assert!((probability(&out,3)-0.5).abs()<1e-12);
        assert_eq!(probability(&out,1),0.0);
        assert_eq!(probability(&out,7),0.0);
    }
    #[test] fn malformed_requests_do_not_mutate_output(){
        let mut out=[0x55u8;AMPLITUDES*8];
        let invalid:[&[u8];6]=[
            &[VERSION,2,1,0,1,0],
            &[VERSION+1,2,0,0],
            &[VERSION,2,1,1,1,0,0],
            &[VERSION,2,1,0,1,0,1],
            &[VERSION,2,1,0,4,1,1],
            &[VERSION,2,1,0,99,0,0],
        ];
        for request in invalid {
            assert!(execute_probabilities(request,&mut out).is_err());
            assert!(out.iter().all(|&v|v==0x55));
        }
        assert_eq!(execute_probabilities(&[VERSION,2,0,0],&mut [0;7]),Err(ProtocolError::OutputTooSmall));
        assert_eq!(decode(&[VERSION,2,65,0]),Err(ProtocolError::TooManyGates));
        assert_eq!(decode(&[VERSION,MAX_QUBITS as u8+1,0,0]).err(),Some(ProtocolError::InvalidQubit));
    }
}
