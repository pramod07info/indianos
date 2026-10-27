//! R038/35: Linux x86-64 ABI classification only; no Linux execution.
#[derive(Debug,PartialEq,Eq)]pub enum Op{Read,Write,Close,GetPid,Exit,MemoryMap}
#[derive(Debug,PartialEq,Eq)]pub enum Error{Unsupported,Pointer,Denied}
pub fn classify(n:u64)->Result<Op,Error>{
 match n{0=>Ok(Op::Read),1=>Ok(Op::Write),3=>Ok(Op::Close),
  9=>Ok(Op::MemoryMap),39=>Ok(Op::GetPid),60=>Ok(Op::Exit),_=>Err(Error::Unsupported)}
}
pub fn validate_user_span(start:u64,len:u64,top:u64)->Result<(),Error>{
 if start<4096||len==0||len>1024*1024||
  start.checked_add(len).is_none_or(|end|end>top){return Err(Error::Pointer);}
 Ok(())
}
/// Security admission for a first Linux compatibility syscall subset.
/// Caller identity and validated user range must come from trusted kernel
/// process/context machinery. This does not execute or emulate Linux ELF.
pub fn authorize<const N:usize>(
 auth:&crate::batch6_security::Access<N>,
 owner:crate::batch5_session::Identity,now:u64,
 syscall:u64,addr:u64,len:u64,top:u64,
)->Result<Op,Error>{
 let op=classify(syscall)?;
 let needed=match op{
  Op::Read=>crate::batch6_security::FILE_READ,
  Op::Write=>crate::batch6_security::FILE_WRITE,
  Op::Close|Op::GetPid|Op::Exit=>crate::batch6_security::VIEW,
  // Full mmap is not implemented. Never grant it on label alone.
  Op::MemoryMap=>return Err(Error::Unsupported),
 };
 auth.require(owner,needed,now).map_err(|_|Error::Denied)?;
 if matches!(op,Op::Read|Op::Write){
  validate_user_span(addr,len,top)?;
 }
 Ok(op)
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn unknown_call_denied(){assert_eq!(classify(500),Err(Error::Unsupported));}
 #[test]fn userspace_pointer_bounded(){assert_eq!(validate_user_span(4096,4,8192),Ok(()));
 assert_eq!(validate_user_span(u64::MAX,4,8192),Err(Error::Pointer));}

 #[test]fn linux_read_write_require_exact_rights_and_bounded_buffers(){
  use crate::batch5_session::Identity;
  use crate::batch6_security::{Access,VIEW,FILE_READ};
  let person=Identity{pid:11,generation:5};
  let mut acl=Access::<1>::new();
  acl.grant(person,VIEW|FILE_READ,100).unwrap();
  assert_eq!(authorize(&acl,person,1,0,4096,12,8192),Ok(Op::Read));
  assert_eq!(authorize(&acl,person,1,1,4096,12,8192),Err(Error::Denied));
  assert_eq!(authorize(&acl,person,1,0,4090,20,8192),Err(Error::Pointer));
  assert_eq!(authorize(&acl,person,1,0,u64::MAX,20,u64::MAX),Err(Error::Pointer));
  assert_eq!(authorize(&acl,person,1,9,4096,12,8192),Err(Error::Unsupported));
 }
 #[test]fn linux_abi_rejects_stale_generations_and_unknown_syscalls(){
  use crate::batch5_session::Identity;
  use crate::batch6_security::{Access,VIEW};
  let original=Identity{pid:11,generation:5};
  let mut acl=Access::<1>::new();acl.grant(original,VIEW,5).unwrap();
  assert_eq!(authorize(&acl,original,1,39,0,0,8192),Ok(Op::GetPid));
  assert_eq!(authorize(&acl,original,5,60,0,0,8192),Err(Error::Denied));
  assert_eq!(authorize(&acl,Identity{generation:6,..original},
                       1,39,0,0,8192),Err(Error::Denied));
  assert_eq!(authorize(&acl,original,1,999,0,0,8192),Err(Error::Unsupported));
 }

}