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

/// Validate the requested user buffer against the process's actual mapped
/// segments and stack permissions. This is metadata-only validation: a
/// privileged copy_from_user/copy_to_user is still required before execution.
pub(crate) fn validate_user_request(
    plan:&crate::syscall::ProgramPlan,
    request:QuantumRequest,
)->Result<(),QuantumAbiError>{
    use crate::syscall::user_range;
    match request.operation{
        QuantumOperation::Submit=>{
            if !user_range(plan,request.user_address,request.length as u64,false){
                return Err(QuantumAbiError::InvalidAddress);
            }
        }
        QuantumOperation::Result=>{
            if !user_range(plan,request.user_address,request.length as u64,true){
                return Err(QuantumAbiError::InvalidAddress);
            }
        }
        QuantumOperation::Status|QuantumOperation::Release=>{}
    }
    Ok(())
}
#[cfg(test)]
mod user_mapping_tests{
    use super::*;
    use crate::syscall::{ProgramPlan,ProgramSegment,MAX_SEGMENTS,USER_MIN,USER_TOP,STACK_BYTES,PAGE_BYTES};
    fn plan()->ProgramPlan{
        let mut p=ProgramPlan{
            entry:USER_MIN,stack_begin:USER_TOP-STACK_BYTES,
            stack_end:USER_TOP,guard_begin:USER_TOP-STACK_BYTES-PAGE_BYTES,
            segment_count:1,segments:[ProgramSegment::empty();MAX_SEGMENTS],
        };
        p.segments[0]=ProgramSegment{
            page_begin:USER_MIN,page_end:USER_MIN+PAGE_BYTES,
            writable:false,executable:true,..ProgramSegment::empty()
        };
        p
    }
    #[test] fn submit_reads_code_but_result_needs_writable_memory(){
        let p=plan();
        let submit=QuantumRequest::parse(0,USER_MIN,4,0).unwrap();
        assert_eq!(validate_user_request(&p,submit),Ok(()));
        let result=QuantumRequest::parse(2,USER_MIN,RESULT_BYTES as u64,1).unwrap();
        assert_eq!(validate_user_request(&p,result),Err(QuantumAbiError::InvalidAddress));
        let writable=QuantumRequest::parse(2,p.stack_end-RESULT_BYTES as u64,RESULT_BYTES as u64,1).unwrap();
        assert_eq!(validate_user_request(&p,writable),Ok(()));
    }
    #[test] fn reject_guard_crossing_and_unmapped_requests(){
        let p=plan();
        let guard=QuantumRequest::parse(0,p.guard_begin,4,0).unwrap();
        assert_eq!(validate_user_request(&p,guard),Err(QuantumAbiError::InvalidAddress));
        let crossing=QuantumRequest::parse(0,USER_MIN+PAGE_BYTES-2,4,0).unwrap();
        assert_eq!(validate_user_request(&p,crossing),Err(QuantumAbiError::InvalidAddress));
        let unmapped=QuantumRequest::parse(0,USER_MIN+2*PAGE_BYTES,4,0).unwrap();
        assert_eq!(validate_user_request(&p,unmapped),Err(QuantumAbiError::InvalidAddress));
    }
}
