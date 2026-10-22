//! R017.6: validated, allocation-free quantum syscall request metadata.
//!
//! This is a security boundary helper, not a live ring-3 trap handler.
//! A privileged caller must copy request bytes from userspace into kernel
//! memory, authenticate the process identity and copy responses back safely.
use crate::quantum_protocol::{MAX_REQUEST_BYTES,HEADER_BYTES};
use crate::quantum_queue::RESULT_BYTES;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum QuantumOperation {Submit,Status,Result,Release}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum QuantumAbiError {UnknownOperation,InvalidLength,InvalidAddress,InvalidJob}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct QuantumRequest {
    pub operation:QuantumOperation,
    pub user_address:u64,
    pub length:usize,
    pub job_id:u64,
}
impl QuantumRequest {
    /// Parse the four syscall register values. This validates shape only:
    /// mapped and accessible userspace memory must be checked by the kernel.
    pub fn parse(op:u64,address:u64,length:u64,job_id:u64)
        ->Result<Self,QuantumAbiError>{
        let operation=match op{
            0=>QuantumOperation::Submit,
            1=>QuantumOperation::Status,
            2=>QuantumOperation::Result,
            3=>QuantumOperation::Release,
            _=>return Err(QuantumAbiError::UnknownOperation),
        };
        let expected=match operation{
            QuantumOperation::Submit=>{
                if length<(HEADER_BYTES as u64) || length>(MAX_REQUEST_BYTES as u64){
                    return Err(QuantumAbiError::InvalidLength);
                }
                if job_id!=0{return Err(QuantumAbiError::InvalidJob);}
                length
            }
            QuantumOperation::Result=>{
                if length!=RESULT_BYTES as u64{return Err(QuantumAbiError::InvalidLength);}
                if job_id==0{return Err(QuantumAbiError::InvalidJob);}
                length
            }
            QuantumOperation::Status|QuantumOperation::Release=>{
                if length!=0 || address!=0{return Err(QuantumAbiError::InvalidLength);}
                if job_id==0{return Err(QuantumAbiError::InvalidJob);}
                0
            }
        };
        if expected!=0 && (address==0 || address.checked_add(expected).is_none()){
            return Err(QuantumAbiError::InvalidAddress);
        }
        Ok(Self{operation,user_address:address,length:length as usize,job_id})
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn valid_operations(){
        assert_eq!(QuantumRequest::parse(0,0x400000,4,0).unwrap().operation,QuantumOperation::Submit);
        assert_eq!(QuantumRequest::parse(1,0,0,42).unwrap().operation,QuantumOperation::Status);
        assert_eq!(QuantumRequest::parse(2,0x400000,RESULT_BYTES as u64,42).unwrap().operation,QuantumOperation::Result);
        assert_eq!(QuantumRequest::parse(3,0,0,42).unwrap().operation,QuantumOperation::Release);
    }
    #[test] fn rejects_bad_metadata(){
        assert_eq!(QuantumRequest::parse(4,0,0,0),Err(QuantumAbiError::UnknownOperation));
        assert_eq!(QuantumRequest::parse(0,0,4,0),Err(QuantumAbiError::InvalidAddress));
        assert_eq!(QuantumRequest::parse(0,u64::MAX,4,0),Err(QuantumAbiError::InvalidAddress));
        assert_eq!(QuantumRequest::parse(0,0x400000,3,0),Err(QuantumAbiError::InvalidLength));
        assert_eq!(QuantumRequest::parse(0,0x400000,4,5),Err(QuantumAbiError::InvalidJob));
        assert_eq!(QuantumRequest::parse(1,0,0,0),Err(QuantumAbiError::InvalidJob));
        assert_eq!(QuantumRequest::parse(1,1,0,5),Err(QuantumAbiError::InvalidLength));
        assert_eq!(QuantumRequest::parse(2,0x400000,7,5),Err(QuantumAbiError::InvalidLength));
        assert_eq!(QuantumRequest::parse(3,0,1,5),Err(QuantumAbiError::InvalidLength));
    }
}
