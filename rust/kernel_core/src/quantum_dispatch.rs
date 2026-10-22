//! R017.8: trusted-buffer quantum service dispatcher.
//! Integration helper only; no live syscall or user-pointer dereference.
use crate::quantum_abi::{QuantumRequest,QuantumOperation};
use crate::quantum_queue::{JobStatus,RESULT_BYTES};
use crate::quantum_service::{QuantumService,ServiceError};
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum Reply{Submitted(u64),Status(JobStatus),Result,Released}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum DispatchError{BadBuffer,Service(ServiceError)}
pub fn dispatch(service:&mut QuantumService,pid:u32,request:QuantumRequest,
    input:Option<&[u8]>,output:Option<&mut [u8;RESULT_BYTES]>)
    ->Result<Reply,DispatchError>{
    if pid==0{return Err(DispatchError::Service(ServiceError::InvalidOwner));}
    let service_error=DispatchError::Service;
    match request.operation{
        QuantumOperation::Submit=>{
            if output.is_some(){return Err(DispatchError::BadBuffer);}
            let bytes=input.ok_or(DispatchError::BadBuffer)?;
            if bytes.len()!=request.length{return Err(DispatchError::BadBuffer);}
            Ok(Reply::Submitted(service.submit(pid,bytes).map_err(service_error)?))
        }
        QuantumOperation::Status=>{
            if input.is_some()||output.is_some(){return Err(DispatchError::BadBuffer);}
            Ok(Reply::Status(service.status(pid,request.job_id).map_err(service_error)?))
        }
        QuantumOperation::Result=>{
            if input.is_some(){return Err(DispatchError::BadBuffer);}
            let out=output.ok_or(DispatchError::BadBuffer)?;
            service.result(pid,request.job_id,out).map_err(service_error)?;
            Ok(Reply::Result)
        }
        QuantumOperation::Release=>{
            if input.is_some()||output.is_some(){return Err(DispatchError::BadBuffer);}
            service.release(pid,request.job_id).map_err(service_error)?;
            Ok(Reply::Released)
        }
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn end_to_end_trusted_buffers(){
        let mut svc=QuantumService::new();
        let bytes=[1,2,2,0,1,0,0,4,0,1];
        let req=QuantumRequest::parse(0,0x400000,10,0).unwrap();
        assert_eq!(dispatch(&mut svc,1,req,Some(&bytes[..4]),None),Err(DispatchError::BadBuffer));
        let id=match dispatch(&mut svc,1,req,Some(&bytes),None).unwrap(){
            Reply::Submitted(id)=>id,_=>panic!("expected job")
        };
        let status=QuantumRequest::parse(1,0,0,id).unwrap();
        assert_eq!(dispatch(&mut svc,2,status,None,None),Err(DispatchError::Service(ServiceError::NotOwner)));
        assert_eq!(svc.run_next(),Some(id));
        let mut output=[0u8;RESULT_BYTES];
        let result=QuantumRequest::parse(2,0x400000,RESULT_BYTES as u64,id).unwrap();
        assert_eq!(dispatch(&mut svc,1,result,None,Some(&mut output)),Ok(Reply::Result));
        let p=f64::from_le_bytes(output[0..8].try_into().unwrap());
        assert!((p-0.5).abs()<1e-12);
    }
}
