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
pub const MAX_LOAD_REGIONS:usize=8;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct LoadRegion{pub address:u64,pub file_bytes:u64,pub memory_bytes:u64,
 pub offset:u64,pub flags:u32}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum LoadError{Invalid,Capacity,Unsafe,Overlap,Entry}
/// Build a bounded W^X load plan from a verified Linux x86-64 ELF image.
/// This validates metadata but does not map pages or execute the ELF; an
/// actual loader must independently enforce virtual-memory ownership.
pub fn plan_linux_load(signed:bool,bytes:&[u8],
 out:&mut[LoadRegion])->Result<usize,LoadError>{
 crate::batch5_compat::linux_x86_64_elf(signed,bytes)
   .map_err(|_|LoadError::Invalid)?;
 let phoff=u64::from_le_bytes(bytes[32..40].try_into().map_err(|_|LoadError::Invalid)?);
 let start=usize::try_from(phoff).map_err(|_|LoadError::Invalid)?;
 let count=u16::from_le_bytes([bytes[56],bytes[57]]) as usize;
 let entry=u64::from_le_bytes(bytes[24..32].try_into().map_err(|_|LoadError::Invalid)?);
 let mut staged=[LoadRegion{address:0,file_bytes:0,memory_bytes:0,
   offset:0,flags:0};MAX_LOAD_REGIONS];
 let mut used=0usize;
 let mut entry_ok=false;
 for i in 0..count{
  let at=start.checked_add(i.checked_mul(56).ok_or(LoadError::Invalid)?)
   .ok_or(LoadError::Invalid)?;
  let hdr=bytes.get(at..at+56).ok_or(LoadError::Invalid)?;
  let kind=u32::from_le_bytes(hdr[0..4].try_into().unwrap());
  if kind!=1{continue;}
  let flags=u32::from_le_bytes(hdr[4..8].try_into().unwrap());
  let file_offset=u64::from_le_bytes(hdr[8..16].try_into().unwrap());
  let address=u64::from_le_bytes(hdr[16..24].try_into().unwrap());
  let file_bytes=u64::from_le_bytes(hdr[32..40].try_into().unwrap());
  let memory_bytes=u64::from_le_bytes(hdr[40..48].try_into().unwrap());
  let end=address.checked_add(memory_bytes).ok_or(LoadError::Unsafe)?;
  if flags&!7!=0||flags&3==3||memory_bytes==0||
     address<4096||end>0x0000_8000_0000_0000||
     file_offset%4096!=address%4096{return Err(LoadError::Unsafe);}
  for region in &staged[..used]{
   let old_end=region.address+region.memory_bytes;
   if address<old_end&&region.address<end{return Err(LoadError::Overlap);}
  }
  if used>=MAX_LOAD_REGIONS{return Err(LoadError::Capacity);}
  if flags&1!=0&&entry>=address&&entry<end{entry_ok=true;}
  staged[used]=LoadRegion{address,file_bytes,memory_bytes,
   offset:file_offset,flags};
  used+=1;
 }
 if !entry_ok{return Err(LoadError::Entry);}
 if out.len()<used{return Err(LoadError::Capacity);}
 out[..used].copy_from_slice(&staged[..used]);
 Ok(used)
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


 fn sample_linux_elf()->[u8;128]{
  let mut b=[0u8;128];
  b[..4].copy_from_slice(b"\x7fELF");b[4]=2;b[5]=1;b[6]=1;
  b[16..18].copy_from_slice(&2u16.to_le_bytes());
  b[18..20].copy_from_slice(&62u16.to_le_bytes());
  b[20..24].copy_from_slice(&1u32.to_le_bytes());
  b[24..32].copy_from_slice(&0x400010u64.to_le_bytes());
  b[32..40].copy_from_slice(&64u64.to_le_bytes());
  b[52..54].copy_from_slice(&64u16.to_le_bytes());
  b[54..56].copy_from_slice(&56u16.to_le_bytes());
  b[56..58].copy_from_slice(&1u16.to_le_bytes());
  b[64..68].copy_from_slice(&1u32.to_le_bytes());
  b[68..72].copy_from_slice(&5u32.to_le_bytes()); // read and execute
  b[80..88].copy_from_slice(&0x400000u64.to_le_bytes());
  b[96..104].copy_from_slice(&128u64.to_le_bytes());
  b[104..112].copy_from_slice(&4096u64.to_le_bytes());
  b
 }
 #[test]fn plans_one_checked_linux_segment_without_executing_image(){
  let b=sample_linux_elf();
  let mut out=[LoadRegion{address:0,file_bytes:0,memory_bytes:0,
    offset:0,flags:0};1];
  assert_eq!(plan_linux_load(true,&b,&mut out),Ok(1));
  assert_eq!(out[0].address,0x400000);
  assert_eq!(out[0].memory_bytes,4096);
 }
 #[test]fn rejects_writable_executable_and_out_of_range_entry_without_publishing(){
  let original=LoadRegion{address:9,file_bytes:9,memory_bytes:9,offset:9,flags:9};
  let mut out=[original;1];
  let mut b=sample_linux_elf();
  b[68..72].copy_from_slice(&7u32.to_le_bytes());
  assert_eq!(plan_linux_load(true,&b,&mut out),Err(LoadError::Unsafe));
  assert_eq!(out,[original]);
  let mut b=sample_linux_elf();
  b[24..32].copy_from_slice(&0x500000u64.to_le_bytes());
  assert_eq!(plan_linux_load(true,&b,&mut out),Err(LoadError::Entry));
  assert_eq!(out,[original]);
  assert_eq!(plan_linux_load(false,&sample_linux_elf(),&mut out),
    Err(LoadError::Invalid));
 }

}