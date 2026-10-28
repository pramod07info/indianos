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
  let hdr_end=at.checked_add(56).ok_or(LoadError::Invalid)?;
  let hdr=bytes.get(at..hdr_end).ok_or(LoadError::Invalid)?;
  let kind=u32::from_le_bytes(hdr[0..4].try_into().unwrap());
  // PT_INTERP requires an unsupported userspace dynamic linker.
  if kind==3{return Err(LoadError::Invalid);}
  if kind!=1{continue;}
  let flags=u32::from_le_bytes(hdr[4..8].try_into().unwrap());
  let file_offset=u64::from_le_bytes(hdr[8..16].try_into().unwrap());
  let address=u64::from_le_bytes(hdr[16..24].try_into().unwrap());
  let file_bytes=u64::from_le_bytes(hdr[32..40].try_into().unwrap());
  let memory_bytes=u64::from_le_bytes(hdr[40..48].try_into().unwrap());
  let end=address.checked_add(memory_bytes).ok_or(LoadError::Unsafe)?;
  let file_end=file_offset.checked_add(file_bytes).ok_or(LoadError::Unsafe)?;
  // Page-based W^X enforcement: disjoint byte ranges in the same page
  // cannot safely have different read/write/execute protection.
  let page_start=address&!4095u64;
  let page_end=end.checked_add(4095).ok_or(LoadError::Unsafe)?&!4095u64;
  if flags&!7!=0||flags&3==3||memory_bytes==0||file_bytes>memory_bytes||
     address<4096||end>0x0000_8000_0000_0000||
     file_end>bytes.len() as u64||
     file_offset%4096!=address%4096{return Err(LoadError::Unsafe);}
  for region in &staged[..used]{
   let old_start=region.address&!4095u64;
   let old_end=region.address.checked_add(region.memory_bytes)
      .and_then(|v|v.checked_add(4095)).ok_or(LoadError::Unsafe)?&!4095u64;
   if page_start<old_end&&old_start<page_end{return Err(LoadError::Overlap);}
  }
  if used>=MAX_LOAD_REGIONS{return Err(LoadError::Capacity);}
  // Reject entry points into zero-filled BSS or unmapped segment tails.
  let executable_end=address.checked_add(file_bytes).ok_or(LoadError::Unsafe)?;
  if flags&1!=0&&entry>=address&&entry<executable_end{entry_ok=true;}
  staged[used]=LoadRegion{address,file_bytes,memory_bytes,
   offset:file_offset,flags};
  used+=1;
 }
 if !entry_ok{return Err(LoadError::Entry);}
 if out.len()<used{return Err(LoadError::Capacity);}
 out[..used].copy_from_slice(&staged[..used]);
 Ok(used)
}
/// Materialize a verified PT_LOAD region into a *trusted* staging buffer.
/// Page-table mapping, W^X PTEs, ASLR, dynamic linking and syscall runtime
/// remain outside this pure loader. This never executes any foreign code.
pub fn stage_linux_segment(image:&[u8],region:LoadRegion,
 output:&mut[u8])->Result<usize,LoadError>{
 let memory=usize::try_from(region.memory_bytes).map_err(|_|LoadError::Capacity)?;
 let file=usize::try_from(region.file_bytes).map_err(|_|LoadError::Capacity)?;
 let offset=usize::try_from(region.offset).map_err(|_|LoadError::Invalid)?;
 if region.address<4096||region.flags&!7!=0||region.flags&3==3||
    memory==0||file>memory||memory>output.len(){
  return Err(LoadError::Unsafe);
 }
 let end=offset.checked_add(file).ok_or(LoadError::Invalid)?;
 let payload=image.get(offset..end).ok_or(LoadError::Invalid)?;
 output[..file].copy_from_slice(payload);
 output[file..memory].fill(0);
 Ok(memory)
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


 #[test]fn linux_segment_materialization_copies_file_and_zero_fills_bss(){
  let image=sample_linux_elf();
  let mut plan=[LoadRegion{address:0,file_bytes:0,memory_bytes:0,
    offset:0,flags:0};1];
  plan_linux_load(true,&image,&mut plan).unwrap();
  let mut dst=[0xaau8;4096];
  assert_eq!(stage_linux_segment(&image,plan[0],&mut dst),Ok(4096));
  assert_eq!(&dst[..128],&image);
  assert!(dst[128..].iter().all(|x|*x==0));
 }
 #[test]fn invalid_elf_region_does_not_modify_target_memory(){
  let image=sample_linux_elf();
  let mut dst=[9u8;32];
  let mut region=LoadRegion{address:0x400000,file_bytes:8,
    memory_bytes:16,offset:0,flags:5};
  region.flags=7;
  assert_eq!(stage_linux_segment(&image,region,&mut dst),Err(LoadError::Unsafe));
  assert_eq!(dst,[9;32]);
  region.flags=5;region.offset=1000;
  assert_eq!(stage_linux_segment(&image,region,&mut dst),Err(LoadError::Invalid));
  assert_eq!(dst,[9;32]);
 }

}

 
/// R009 syscall argument translation for a minimal, statically linked Linux
/// process. This is a kernel-facing ABI adapter, NOT an ELF runtime: the
/// native file service still needs to dispatch these validated operations.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum LinuxCall{
 StdinRead{address:u64,length:u32},
 StdoutWrite{fd:u32,address:u64,length:u32},
 Close{fd:u32},
 GetPid{pid:u32},
 Exit{status:i32},
}
pub fn translate_stdio<const N:usize>(
 auth:&crate::batch6_security::Access<N>,
 actor:crate::batch5_session::Identity,now:u64,
 number:u64,args:[u64;6],user_top:u64
)->Result<LinuxCall,Error>{
 let op=authorize(auth,actor,now,number,args[1],args[2],user_top)?;
 match op{
  Op::Read=>{
   if args[0]!=0||args[2]>u32::MAX as u64{return Err(Error::Denied);}
   Ok(LinuxCall::StdinRead{address:args[1],length:args[2] as u32})
  },
  Op::Write=>{
   if !(args[0]==1||args[0]==2)||args[2]>u32::MAX as u64{
    return Err(Error::Denied);
   }
   Ok(LinuxCall::StdoutWrite{fd:args[0] as u32,
     address:args[1],length:args[2] as u32})
  },
  Op::Close=>{
   if args[0]>2{return Err(Error::Denied);}
   Ok(LinuxCall::Close{fd:args[0] as u32})
  },
  Op::GetPid=>Ok(LinuxCall::GetPid{pid:actor.pid}),
  Op::Exit=>Ok(LinuxCall::Exit{status:args[0] as u32 as i32}),
  Op::MemoryMap=>Err(Error::Unsupported),
 }
}
#[cfg(test)]
mod r009_linux_std_io_tests{
 use super::*;
 use crate::batch5_session::Identity;
 use crate::batch6_security::{Access,VIEW,FILE_READ,FILE_WRITE};
 fn actor()->Identity{Identity{pid:14,generation:7}}
 fn auth()->Access<1>{
  let mut a=Access::new();
  a.grant(actor(),VIEW|FILE_READ|FILE_WRITE,100).unwrap();a
 }
 #[test]fn stdio_and_process_identity_translate_without_trusting_user_pid(){
  let a=auth();
  assert_eq!(translate_stdio(&a,actor(),2,39,[0;6],0x800000),
    Ok(LinuxCall::GetPid{pid:14}));
  assert_eq!(translate_stdio(&a,actor(),2,1,[1,0x1000,32,0,0,0],0x800000),
    Ok(LinuxCall::StdoutWrite{fd:1,address:0x1000,length:32}));
  assert_eq!(translate_stdio(&a,actor(),2,0,[0,0x2000,4,0,0,0],0x800000),
    Ok(LinuxCall::StdinRead{address:0x2000,length:4}));
 }
 #[test]fn unrelated_descriptors_invalid_user_buffers_and_foreign_pids_denied(){
  let a=auth();
  assert_eq!(translate_stdio(&a,actor(),2,1,[7,0x1000,4,0,0,0],0x800000),
    Err(Error::Denied));
  assert_eq!(translate_stdio(&a,actor(),2,1,[1,0x10,4,0,0,0],0x800000),
    Err(Error::Pointer));
  assert_eq!(translate_stdio(&a,Identity{generation:8,..actor()},
    2,1,[1,0x1000,4,0,0,0],0x800000),Err(Error::Denied));
  assert_eq!(translate_stdio(&a,actor(),2,999,[0;6],0x800000),
    Err(Error::Unsupported));
 }
}

 
/// R010 Linux ABI adapter for process-scoped virtual file descriptors.
pub const FD_READ:u8=1;
pub const FD_WRITE:u8=2;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Descriptor{owner:crate::batch5_session::Identity,
 fd:u32,handle:u64,rights:u8}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum FdError{Invalid,Full,Duplicate,Denied,NotFound}
pub struct FileDescriptors<const N:usize>{entries:[Option<Descriptor>;N]}
impl<const N:usize> FileDescriptors<N>{
 pub const fn new()->Self{Self{entries:[None;N]}}
 pub fn attach(&mut self,owner:crate::batch5_session::Identity,
   fd:u32,handle:u64,rights:u8)->Result<(),FdError>{
  if owner.pid==0||owner.generation==0||fd<3||handle==0||
     rights==0||rights&!(FD_READ|FD_WRITE)!=0{return Err(FdError::Invalid);}
  // File descriptors cannot ambiguously refer to two generations of the
  // same numeric PID. A trusted scheduler must retire the old generation.
  if self.entries.iter().flatten().any(|e|e.owner.pid==owner.pid&&e.fd==fd){
   return Err(FdError::Duplicate);
  }
  let slot=self.entries.iter_mut().find(|s|s.is_none()).ok_or(FdError::Full)?;
  *slot=Some(Descriptor{owner,fd,handle,rights});Ok(())
 }
 pub fn resolve(&self,owner:crate::batch5_session::Identity,
  fd:u32,needed:u8)->Result<u64,FdError>{
  if owner.pid==0||owner.generation==0||needed==0||
    needed&!(FD_READ|FD_WRITE)!=0{return Err(FdError::Invalid);}
  let entry=self.entries.iter().flatten().find(|e|e.owner.pid==owner.pid&&e.fd==fd)
    .ok_or(FdError::NotFound)?;
  if entry.owner!=owner||entry.rights&needed!=needed{return Err(FdError::Denied);}
  Ok(entry.handle)
 }
 pub fn close(&mut self,owner:crate::batch5_session::Identity,
   fd:u32)->Result<u64,FdError>{
  let slot=self.entries.iter_mut().find(|s|
    s.is_some_and(|e|e.owner.pid==owner.pid&&e.fd==fd))
    .ok_or(FdError::NotFound)?;
  let entry=slot.ok_or(FdError::NotFound)?;
  if entry.owner!=owner{return Err(FdError::Denied);}
  *slot=None;Ok(entry.handle)
 }
 pub fn retire(&mut self,owner:crate::batch5_session::Identity)->usize{
  let mut n=0;
  for slot in &mut self.entries{
   if slot.is_some_and(|e|e.owner==owner){*slot=None;n+=1;}
  }
  n
 }
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum FileIo{Read{handle:u64,addr:u64,len:u32},
 Write{handle:u64,addr:u64,len:u32}}
pub fn translate_file_io<const CAP:usize,const GRANTS:usize>(
 table:&FileDescriptors<CAP>,auth:&crate::batch6_security::Access<GRANTS>,
 owner:crate::batch5_session::Identity,now:u64,
 number:u64,fd:u32,addr:u64,length:u64,user_top:u64
)->Result<FileIo,Error>{
 let operation=authorize(auth,owner,now,number,addr,length,user_top)?;
 let needed=match operation{
  Op::Read=>FD_READ,Op::Write=>FD_WRITE,_=>return Err(Error::Unsupported),
 };
 let handle=table.resolve(owner,fd,needed).map_err(|_|Error::Denied)?;
 let len=u32::try_from(length).map_err(|_|Error::Pointer)?;
 match operation{
  Op::Read=>Ok(FileIo::Read{handle,addr,len}),
  Op::Write=>Ok(FileIo::Write{handle,addr,len}),
  _=>Err(Error::Unsupported),
 }
}
#[cfg(test)]
mod r010_linux_fd_tests{
 use super::*;
 use crate::batch5_session::Identity;
 use crate::batch6_security::{Access,FILE_READ,FILE_WRITE};
 fn id(g:u64)->Identity{Identity{pid:41,generation:g}}
 fn auth()->Access<1>{
  let mut access=Access::new();
  access.grant(id(2),FILE_READ|FILE_WRITE,100).unwrap();access
 }
 #[test]fn directory_handles_are_not_shared_after_pid_reuse(){
  let mut table=FileDescriptors::<2>::new();
  table.attach(id(2),3,99,FD_READ).unwrap();
  assert_eq!(table.resolve(id(2),3,FD_READ),Ok(99));
  assert_eq!(table.resolve(id(3),3,FD_READ),Err(FdError::Denied));
  assert_eq!(table.resolve(id(2),3,FD_WRITE),Err(FdError::Denied));
  assert_eq!(table.retire(id(2)),1);
  assert_eq!(table.resolve(id(2),3,FD_READ),Err(FdError::NotFound));
 }
 #[test]fn io_translation_enforces_fd_rights_and_user_ranges(){
  let mut table=FileDescriptors::<1>::new();table.attach(id(2),5,0x1000,FD_READ).unwrap();
  let a=auth();
  assert_eq!(translate_file_io(&table,&a,id(2),10,0,5,0x2000,12,0x8000),
    Ok(FileIo::Read{handle:0x1000,addr:0x2000,len:12}));
  assert_eq!(translate_file_io(&table,&a,id(2),10,1,5,0x2000,12,0x8000),
    Err(Error::Denied));
  assert_eq!(translate_file_io(&table,&a,id(2),10,0,5,1,12,0x8000),
    Err(Error::Pointer));
 }
 #[test]fn descriptor_capacity_duplicate_and_close_are_fail_closed(){
  let mut table=FileDescriptors::<1>::new();
  table.attach(id(2),3,15,FD_READ|FD_WRITE).unwrap();
  assert_eq!(table.attach(id(2),3,16,FD_READ),Err(FdError::Duplicate));
  assert_eq!(table.attach(id(2),4,16,FD_READ),Err(FdError::Full));
  assert_eq!(table.close(id(3),3),Err(FdError::Denied));
  assert_eq!(table.close(id(2),3),Ok(15));
  assert_eq!(table.close(id(2),3),Err(FdError::NotFound));
 }
}

 
/// R011 Linux-style descriptor duplication, but no actual foreign binary is
/// executed without authenticated process loader, stack and ring3 syscall ABI.
impl<const N:usize> FileDescriptors<N>{
 pub fn duplicate(&mut self,owner:crate::batch5_session::Identity,
  source:u32,target:u32,rights:u8)->Result<(),FdError>{
  if source==target||target<3||rights==0||
    rights&!(FD_READ|FD_WRITE)!=0{return Err(FdError::Invalid);}
  let handle=self.resolve(owner,source,rights)?;
  if self.entries.iter().flatten().any(|e|e.owner==owner&&e.fd==target){
   return Err(FdError::Duplicate);
  }
  self.attach(owner,target,handle,rights)
 }
 /// Transfer only granted rights to a child created by the trusted scheduler.
 /// All child descriptors are installed or none are.
 pub fn inherit_atomic(&mut self,parent:crate::batch5_session::Identity,
  child:crate::batch5_session::Identity,
  allowed:u8)->Result<usize,FdError>{
  if child.pid==0||child.generation==0||parent.pid==0||
    parent==child||allowed==0||allowed&!(FD_READ|FD_WRITE)!=0{
   return Err(FdError::Invalid);
  }
  let mut staged=Self{entries:self.entries};
  let mut count=0;
  for entry in self.entries.iter().flatten(){
   if entry.owner!=parent{continue;}
   let rights=entry.rights&allowed;
   if rights==0{continue;}
   staged.attach(child,entry.fd,entry.handle,rights)?;
   count+=1;
  }
  self.entries=staged.entries;Ok(count)
 }
 /// New executable image starts with an empty nonstandard descriptor table.
 /// Stdio 0/1/2 are mediated separately by translate_stdio.
 pub fn exec_close_all(&mut self,owner:crate::batch5_session::Identity)->usize{
  self.retire(owner)
 }
 pub fn active_count(&self,owner:crate::batch5_session::Identity)->usize{
  self.entries.iter().flatten().filter(|e|e.owner==owner).count()
 }
}
#[cfg(test)]
mod r011_linux_dup_tests{
 use super::*;
 use crate::batch5_session::Identity;
 fn id(pid:u32)->Identity{Identity{pid,generation:2}}
 #[test]fn duplicate_creates_distinct_fd_with_attenuated_rights(){
  let mut f=FileDescriptors::<3>::new();
  f.attach(id(1),3,99,FD_READ|FD_WRITE).unwrap();
  assert_eq!(f.duplicate(id(1),3,4,FD_READ),Ok(()));
  assert_eq!(f.resolve(id(1),4,FD_READ),Ok(99));
  assert_eq!(f.resolve(id(1),4,FD_WRITE),Err(FdError::Denied));
  assert_eq!(f.duplicate(id(1),3,4,FD_READ),Err(FdError::Duplicate));
 }
 #[test]fn failed_child_inheritance_does_not_publish_partial_fds(){
  let mut f=FileDescriptors::<2>::new();
  f.attach(id(1),3,11,FD_READ).unwrap();
  f.attach(id(1),4,12,FD_READ).unwrap();
  assert_eq!(f.inherit_atomic(id(1),id(2),FD_READ),Err(FdError::Full));
  assert_eq!(f.active_count(id(2)),0);
  assert_eq!(f.active_count(id(1)),2);
 }
 #[test]fn exec_closes_owned_handles_only_and_fork_fences_owner(){
  let mut f=FileDescriptors::<3>::new();
  f.attach(id(1),3,55,FD_READ).unwrap();
  f.attach(id(2),4,66,FD_WRITE).unwrap();
  assert_eq!(f.inherit_atomic(id(1),id(3),FD_READ),Ok(1));
  assert_eq!(f.resolve(id(3),3,FD_READ),Ok(55));
  assert_eq!(f.exec_close_all(id(1)),1);
  assert_eq!(f.active_count(id(3)),1);
  assert_eq!(f.resolve(id(2),4,FD_WRITE),Ok(66));
 }
}


/// R012 authenticated Linux descriptor close-range plan. A userspace-provided
/// range never bypasses owner/epoch checks and cannot affect other processes.
impl<const N:usize> FileDescriptors<N>{
 pub fn close_range(&mut self,owner:crate::batch5_session::Identity,
  first:u32,last:u32)->Result<usize,FdError>{
  if owner.pid==0||owner.generation==0||first<3||last<first{
   return Err(FdError::Invalid);
  }
  let mut removed=0;
  for slot in &mut self.entries{
   if slot.is_some_and(|e|e.owner==owner&&
       e.fd>=first&&e.fd<=last){
    *slot=None;removed+=1;
   }
  }
  Ok(removed)
 }
 pub fn close_range_checked(&mut self,owner:crate::batch5_session::Identity,
  first:u32,last:u32,max_count:usize)->Result<usize,FdError>{
  if owner.pid==0||owner.generation==0||first<3||last<first{
   return Err(FdError::Invalid);
  }
  let matches=self.entries.iter().flatten().filter(|e|
    e.owner==owner&&e.fd>=first&&e.fd<=last).count();
  if matches>max_count{return Err(FdError::Full);}
  self.close_range(owner,first,last)
 }
 pub fn owned_fds(&self,owner:crate::batch5_session::Identity,
  out:&mut[u32])->Result<usize,FdError>{
  if owner.pid==0||owner.generation==0{return Err(FdError::Invalid);}
  let count=self.entries.iter().flatten().filter(|e|e.owner==owner).count();
  if out.len()<count{return Err(FdError::Full);}
  let mut used=0;
  for e in self.entries.iter().flatten(){
   if e.owner==owner{out[used]=e.fd;used+=1;}
  }
  Ok(used)
 }
}
#[cfg(test)]
mod r012_linux_close_range_tests{
 use super::*;use crate::batch5_session::Identity;
 fn id(pid:u32,gen:u64)->Identity{Identity{pid,generation:gen}}
 #[test]fn close_range_is_epoch_scoped_and_protects_other_processes(){
  let mut f=FileDescriptors::<3>::new();
  f.attach(id(2,1),3,11,FD_READ).unwrap();
  f.attach(id(2,2),4,22,FD_READ).unwrap();
  f.attach(id(3,1),5,33,FD_WRITE).unwrap();
  assert_eq!(f.close_range(id(2,1),3,5),Ok(1));
  assert_eq!(f.resolve(id(2,2),4,FD_READ),Ok(22));
  assert_eq!(f.resolve(id(3,1),5,FD_WRITE),Ok(33));
 }
 #[test]fn bounded_close_does_not_publish_partial_mutation(){
  let mut f=FileDescriptors::<2>::new();
  f.attach(id(2,1),3,11,FD_READ).unwrap();
  f.attach(id(2,1),4,22,FD_READ).unwrap();
  assert_eq!(f.close_range_checked(id(2,1),3,4,1),Err(FdError::Full));
  assert_eq!(f.active_count(id(2,1)),2);
  assert_eq!(f.close_range_checked(id(2,1),3,4,2),Ok(2));
 }
 #[test]fn fd_listing_never_overwrites_short_output(){
  let mut f=FileDescriptors::<2>::new();
  f.attach(id(2,1),3,11,FD_READ).unwrap();
  f.attach(id(2,1),4,22,FD_READ).unwrap();
  let mut out=[99u32;1];
  assert_eq!(f.owned_fds(id(2,1),&mut out),Err(FdError::Full));
  assert_eq!(out,[99]);
 }
}

/// R013 verified ELF region set intended for a trusted ring3 loader.
/// This validates the virtual memory plan; actual page table mappings,
/// relocation and running foreign Linux binaries remain separate work.
pub fn validate_exec_regions(regions:&[LoadRegion],entry:u64,
 user_top:u64)->Result<usize,LoadError>{
 if regions.is_empty()||regions.len()>MAX_LOAD_REGIONS||
    entry<4096||entry>=user_top{return Err(LoadError::Entry);}
 let mut executable=false;
 for (i,region) in regions.iter().enumerate(){
  let end=region.address.checked_add(region.memory_bytes)
    .ok_or(LoadError::Unsafe)?;
  if region.address<4096||region.memory_bytes==0||
     region.file_bytes>region.memory_bytes||end>user_top||
     region.flags&!7!=0||region.flags&3==3{
   return Err(LoadError::Unsafe);
  }
  let exec_end=region.address.checked_add(region.file_bytes)
     .ok_or(LoadError::Unsafe)?;
  if region.flags&1!=0&&entry>=region.address&&entry<exec_end{
   executable=true;
  }
  let page_start=region.address&!4095u64;
  let page_end=end.checked_add(4095).ok_or(LoadError::Unsafe)?&!4095u64;
  for other in &regions[..i]{
   let old_start=other.address&!4095u64;
   let old_end=other.address.checked_add(other.memory_bytes)
     .and_then(|e|e.checked_add(4095)).ok_or(LoadError::Unsafe)?&!4095u64;
   if page_start<old_end&&old_start<page_end{
    return Err(LoadError::Overlap);
   }
  }
 }
 if !executable{return Err(LoadError::Entry);}
 Ok(regions.len())
}
/// POSIX-style dup2 with atomic replacement. Only the source process
/// generation can overwrite its own existing file descriptor.
impl<const N:usize> FileDescriptors<N>{
 pub fn dup2_scoped(&mut self,owner:crate::batch5_session::Identity,
   source:u32,target:u32,rights:u8)->Result<(),FdError>{
  if owner.pid==0||owner.generation==0||target<3||
     rights==0||rights&!(FD_READ|FD_WRITE)!=0{return Err(FdError::Invalid);}
  let handle=self.resolve(owner,source,rights)?;
  if source==target{return Ok(());}
  let mut staged=Self{entries:self.entries};
  if staged.entries.iter().flatten().any(|e|e.owner.pid==owner.pid&&
    e.fd==target&&e.owner!=owner){
   return Err(FdError::Denied);
  }
  if staged.entries.iter().flatten().any(|e|e.owner==owner&&e.fd==target){
   staged.close(owner,target)?;
  }
  staged.attach(owner,target,handle,rights)?;
  self.entries=staged.entries;Ok(())
 }
}
#[cfg(test)]
mod r013_linux_loader_tests{
 use super::*;use crate::batch5_session::Identity;
 fn id(g:u64)->Identity{Identity{pid:41,generation:g}}
 fn region(start:u64,flags:u32)->LoadRegion{LoadRegion{
  address:start,file_bytes:5,memory_bytes:4096,offset:0,flags}}
 #[test]fn region_set_accepts_rx_and_rw_without_overlap(){
  assert_eq!(validate_exec_regions(&[
   region(0x1000,5),region(0x3000,6)],0x1000,0x8000),Ok(2));
  assert_eq!(validate_exec_regions(&[
   region(0x1000,5),region(0x2000,6)],0x1000,0x8000),Ok(2));
 }
 #[test]fn overlapping_or_wx_regions_fail_before_mapping(){
  assert_eq!(validate_exec_regions(&[
    region(0x1000,5),region(0x1800,6)],0x1000,0x8000),Err(LoadError::Overlap));
  assert_eq!(validate_exec_regions(&[
    region(0x1000,7)],0x1000,0x8000),Err(LoadError::Unsafe));
  assert_eq!(validate_exec_regions(&[
    region(0x1000,6)],0x1000,0x8000),Err(LoadError::Entry));
 }
 #[test]fn dup2_replaces_only_owner_fd_and_retains_other_epoch(){
  let mut f=FileDescriptors::<3>::new();
  f.attach(id(1),3,11,FD_READ).unwrap();
  f.attach(id(1),4,22,FD_WRITE).unwrap();
  assert_eq!(f.attach(id(2),4,33,FD_READ),Err(FdError::Duplicate));
  f.attach(id(2),5,33,FD_READ).unwrap();
  f.dup2_scoped(id(1),3,4,FD_READ).unwrap();
  assert_eq!(f.resolve(id(1),4,FD_READ),Ok(11));
  assert_eq!(f.resolve(id(2),5,FD_READ),Ok(33));
  assert_eq!(f.dup2_scoped(id(2),5,3,FD_WRITE),Err(FdError::Denied));
 }
}

/// R014 atomic POSIX descriptor remapping. All source handles and rights
/// are validated in a private table before any live process FD is replaced.
impl<const N:usize> FileDescriptors<N>{
 pub fn dup2_batch_atomic(&mut self,
  owner:crate::batch5_session::Identity,
  mappings:&[(u32,u32,u8)])->Result<usize,FdError>{
  if owner.pid==0||owner.generation==0||mappings.is_empty()||
    mappings.len()>N{return Err(FdError::Invalid);}
  for (i,&(_,target,_)) in mappings.iter().enumerate(){
   if mappings[..i].iter().any(|(_,prior,_)|*prior==target){
    return Err(FdError::Duplicate);
   }
  }
  let mut staged=Self{entries:self.entries};
  for &(source,target,rights) in mappings{
   staged.dup2_scoped(owner,source,target,rights)?;
  }
  self.entries=staged.entries;Ok(mappings.len())
 }
 /// Execve-style FD hygiene: close all descriptors except explicitly
 /// selected owner-scoped inheritable handles. The caller must verify the
 /// executable image and process generation before entering ring3.
 pub fn exec_retain_atomic(&mut self,
  owner:crate::batch5_session::Identity,keep:&[u32])
  ->Result<usize,FdError>{
  if owner.pid==0||owner.generation==0||keep.len()>N{
   return Err(FdError::Invalid);
  }
  for (i,&fd) in keep.iter().enumerate(){
   if fd<3||keep[..i].contains(&fd){return Err(FdError::Duplicate);}
   self.resolve(owner,fd,FD_READ)
     .or_else(|_|self.resolve(owner,fd,FD_WRITE))?;
  }
  let mut removed=0;
  for slot in &mut self.entries{
   if slot.is_some_and(|s|s.owner==owner&&!keep.contains(&s.fd)){
    *slot=None;removed+=1;
   }
  }
  Ok(removed)
 }
 pub fn descriptors_for_epoch(&self,owner:crate::batch5_session::Identity)
  ->usize{
  self.entries.iter().flatten().filter(|entry|entry.owner==owner).count()
 }
}
#[cfg(test)]
mod r014_posix_exec_descriptor_tests{
 use super::*;use crate::batch5_session::Identity;
 fn owner(g:u64)->Identity{Identity{pid:19,generation:g}}
 #[test]fn atomic_multiple_dup2_replaces_sources_without_rights_escalation(){
  let mut t=FileDescriptors::<4>::new();
  t.attach(owner(1),3,11,FD_READ|FD_WRITE).unwrap();
  t.attach(owner(1),4,22,FD_READ).unwrap();
  assert_eq!(t.dup2_batch_atomic(owner(1),&[
   (3,5,FD_READ),(3,6,FD_WRITE)]),Ok(2));
  assert_eq!(t.resolve(owner(1),5,FD_READ),Ok(11));
  assert_eq!(t.resolve(owner(1),6,FD_WRITE),Ok(11));
  assert_eq!(t.resolve(owner(1),5,FD_WRITE),Err(FdError::Denied));
 }
 #[test]fn invalid_second_dup_does_not_commit_first(){
  let mut t=FileDescriptors::<3>::new();
  t.attach(owner(1),3,11,FD_READ).unwrap();
  assert_eq!(t.dup2_batch_atomic(owner(1),&[
    (3,5,FD_READ),(3,6,FD_WRITE)]),Err(FdError::Denied));
  assert_eq!(t.descriptors_for_epoch(owner(1)),1);
  assert_eq!(t.resolve(owner(1),5,FD_READ),Err(FdError::NotFound));
 }
 #[test]fn exec_retains_only_authenticated_process_descriptors(){
  let mut t=FileDescriptors::<4>::new();
  t.attach(owner(1),3,11,FD_READ).unwrap();
  t.attach(owner(1),4,22,FD_WRITE).unwrap();
  t.attach(owner(2),5,33,FD_READ).unwrap();
  assert_eq!(t.exec_retain_atomic(owner(1),&[3]),Ok(1));
  assert_eq!(t.descriptors_for_epoch(owner(1)),1);
  assert_eq!(t.resolve(owner(2),5,FD_READ),Ok(33));
  assert_eq!(t.exec_retain_atomic(owner(1),&[4]),Err(FdError::NotFound));
 }
}

/// R015: source-validated Linux ELF fixture adversarial tests.
/// The load plan remains a pure preflight; no foreign Linux process runs here.
#[cfg(test)]
mod r015_linux_mapping_safety_tests {
 use super::*;
 fn fixture()->[u8;128]{
  let mut b=[0u8;128];
  b[..4].copy_from_slice(b"\x7fELF");b[4]=2;b[5]=1;b[6]=1;
  b[16..18].copy_from_slice(&2u16.to_le_bytes());
  b[18..20].copy_from_slice(&62u16.to_le_bytes());
  b[20..24].copy_from_slice(&1u32.to_le_bytes());
  b[24..32].copy_from_slice(&0x401000u64.to_le_bytes());
  b[32..40].copy_from_slice(&64u64.to_le_bytes());
  b[52..54].copy_from_slice(&64u16.to_le_bytes());
  b[54..56].copy_from_slice(&56u16.to_le_bytes());
  b[56..58].copy_from_slice(&1u16.to_le_bytes());
  b[64..68].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
  b[68..72].copy_from_slice(&5u32.to_le_bytes()); // RX
  b[80..88].copy_from_slice(&0x401000u64.to_le_bytes());
  b[96..104].copy_from_slice(&128u64.to_le_bytes());
  b[104..112].copy_from_slice(&4096u64.to_le_bytes());
  b
 }
 const EMPTY:LoadRegion=LoadRegion{address:0,file_bytes:0,
  memory_bytes:0,offset:0,flags:0};
 #[test]
 fn rejects_truncated_segment_and_file_larger_than_virtual_mem(){
  let mut b=fixture();let mut output=[EMPTY;1];
  b[96..104].copy_from_slice(&129u64.to_le_bytes());
  // The earlier general ELF parser may reject a bad span as Invalid
  // before the load planner reaches its Unsafe guard. Both fail closed.
  assert!(matches!(plan_linux_load(true,&b,&mut output),
    Err(LoadError::Invalid)|Err(LoadError::Unsafe)));
  assert_eq!(output,[EMPTY]);
  let mut b=fixture();
  b[104..112].copy_from_slice(&64u64.to_le_bytes());
  // The earlier general ELF parser may reject a bad span as Invalid
  // before the load planner reaches its Unsafe guard. Both fail closed.
  assert!(matches!(plan_linux_load(true,&b,&mut output),
    Err(LoadError::Invalid)|Err(LoadError::Unsafe)));
  assert_eq!(output,[EMPTY]);
 }
 #[test]
 fn rejects_entry_inside_non_file_backed_bss(){
  let mut b=fixture();let mut output=[EMPTY;1];
  b[24..32].copy_from_slice(&0x401100u64.to_le_bytes());
  assert_eq!(plan_linux_load(true,&b,&mut output),Err(LoadError::Entry));
  assert_eq!(output,[EMPTY]);
 }
 #[test]
 fn rejects_interpreter_instead_of_falsely_accepting_dynamic_binary(){
  let mut b=fixture();let mut output=[EMPTY;1];
  b[64..68].copy_from_slice(&3u32.to_le_bytes());
  assert_eq!(plan_linux_load(true,&b,&mut output),Err(LoadError::Invalid));
  assert_eq!(output,[EMPTY]);
 }
 #[test]
 fn rejects_same_page_mixed_permissions_without_pte_alias(){
  let rx=LoadRegion{address:0x4000,file_bytes:4,memory_bytes:4,offset:0,flags:5};
  let rw=LoadRegion{address:0x4008,file_bytes:4,memory_bytes:4,offset:8,flags:6};
  assert_eq!(validate_exec_regions(&[rx,rw],0x4000,0x10000),
    Err(LoadError::Overlap));
  let safe=LoadRegion{address:0x5000,..rw};
  assert_eq!(validate_exec_regions(&[rx,safe],0x4000,0x10000),Ok(2));
 }
}


/// R016 parallel POSIX dup2 remap. Unlike sequential dup operations,
/// each source descriptor is captured from the ORIGINAL process table,
/// allowing safe swaps with no accidental handle overwrite.
impl<const N:usize> FileDescriptors<N>{
 pub fn dup2_parallel_atomic(&mut self,
  owner:crate::batch5_session::Identity,
  moves:&[(u32,u32,u8)])->Result<usize,FdError>{
  if owner.pid==0||owner.generation==0||moves.is_empty()||
    moves.len()>N{return Err(FdError::Invalid);}
  let mut handles=[0u64;N];
  for (i,&(source,target,rights)) in moves.iter().enumerate(){
   if target<3||rights==0||rights&!(FD_READ|FD_WRITE)!=0{
    return Err(FdError::Invalid);
   }
   if moves[..i].iter().any(|(_,dest,_)|*dest==target){
    return Err(FdError::Duplicate);
   }
   handles[i]=self.resolve(owner,source,rights)?;
  }
  let mut scratch=Self{entries:self.entries};
  for (i,&(_,target,rights)) in moves.iter().enumerate(){
   // A numeric PID cannot overwrite descriptors from a stale generation.
   if scratch.entries.iter().flatten().any(|e|
      e.owner.pid==owner.pid&&e.owner!=owner&&e.fd==target){
    return Err(FdError::Denied);
   }
   if scratch.entries.iter().flatten().any(|e|
      e.owner==owner&&e.fd==target){
    scratch.close(owner,target)?;
   }
   scratch.attach(owner,target,handles[i],rights)?;
  }
  self.entries=scratch.entries;Ok(moves.len())
 }
 pub fn close_all_on_exit(&mut self,
  owner:crate::batch5_session::Identity)->usize{
  if owner.pid==0||owner.generation==0{return 0;}
  self.retire(owner)
 }
}
#[cfg(test)]mod r016_posix_parallel_tests{
 use super::*;use crate::batch5_session::Identity;
 fn id(g:u64)->Identity{Identity{pid:30,generation:g}}
 #[test]fn swap_reads_both_sources_before_mutating_table(){
  let mut f=FileDescriptors::<3>::new();
  f.attach(id(1),3,100,FD_READ).unwrap();
  f.attach(id(1),4,200,FD_READ).unwrap();
  assert_eq!(f.dup2_parallel_atomic(id(1),&[
    (3,4,FD_READ),(4,3,FD_READ)]),Ok(2));
  assert_eq!(f.resolve(id(1),3,FD_READ),Ok(200));
  assert_eq!(f.resolve(id(1),4,FD_READ),Ok(100));
 }
 #[test]fn bad_second_source_does_not_change_first_fd(){
  let mut f=FileDescriptors::<3>::new();
  f.attach(id(1),3,100,FD_READ).unwrap();
  assert_eq!(f.dup2_parallel_atomic(id(1),&[
    (3,4,FD_READ),(99,5,FD_READ)]),Err(FdError::NotFound));
  assert_eq!(f.resolve(id(1),4,FD_READ),Err(FdError::NotFound));
  assert_eq!(f.resolve(id(1),3,FD_READ),Ok(100));
 }
 #[test]fn reused_pid_does_not_inherit_or_close_old_descriptor(){
  let mut f=FileDescriptors::<3>::new();
  f.attach(id(2),3,300,FD_READ).unwrap();
  assert_eq!(f.dup2_parallel_atomic(id(1),&[(3,5,FD_READ)]),
    Err(FdError::Denied));
  assert_eq!(f.close_all_on_exit(id(1)),0);
  assert_eq!(f.resolve(id(2),3,FD_READ),Ok(300));
  assert_eq!(f.close_all_on_exit(id(2)),1);
 }
}


/// R017 developer POSIX FD inspector and transactional close selection.
/// This is process-scoped descriptor bookkeeping, not foreign ELF execution.
impl<const N:usize> FileDescriptors<N>{
 pub fn snapshot_owner_handles(&self,
  owner:crate::batch5_session::Identity,
  out:&mut[(u32,u64,u8)])->Result<usize,FdError>{
  if owner.pid==0||owner.generation==0{return Err(FdError::Invalid);}
  let total=self.entries.iter().flatten().filter(|e|e.owner==owner).count();
  if total>out.len(){return Err(FdError::Full);}
  let mut count=0;
  for entry in self.entries.iter().flatten(){
   if entry.owner==owner{
    out[count]=(entry.fd,entry.handle,entry.rights);count+=1;
   }
  }
  Ok(count)
 }
 pub fn close_selected_atomic(&mut self,
  owner:crate::batch5_session::Identity,ids:&[u32])
  ->Result<usize,FdError>{
  if owner.pid==0||owner.generation==0||ids.is_empty()||
   ids.len()>N{return Err(FdError::Invalid);}
  let mut staged=Self{entries:self.entries};
  for (i,&fd) in ids.iter().enumerate(){
   if fd<3||ids[..i].contains(&fd){return Err(FdError::Duplicate);}
   staged.close(owner,fd)?;
  }
  self.entries=staged.entries;Ok(ids.len())
 }
}
#[cfg(test)]mod r017_dev_posix_fd_tests{
 use super::*;use crate::batch5_session::Identity;
 fn id(g:u64)->Identity{Identity{pid:55,generation:g}}
 #[test]fn owner_can_preview_and_close_two_handles_atomically(){
  let mut t=FileDescriptors::<3>::new();
  t.attach(id(1),3,10,FD_READ).unwrap();t.attach(id(1),4,20,FD_WRITE).unwrap();
  let mut out=[(0,0,0);2];
  assert_eq!(t.snapshot_owner_handles(id(1),&mut out),Ok(2));
  assert_eq!(out[0].1,10);
  assert_eq!(t.close_selected_atomic(id(1),&[3,4]),Ok(2));
  assert_eq!(t.descriptors_for_epoch(id(1)),0);
 }
 #[test]fn invalid_second_fd_never_partially_closes_first(){
  let mut t=FileDescriptors::<2>::new();t.attach(id(1),3,10,FD_READ).unwrap();
  assert_eq!(t.close_selected_atomic(id(1),&[3,9]),Err(FdError::NotFound));
  assert_eq!(t.resolve(id(1),3,FD_READ),Ok(10));
 }
 #[test]fn recycled_pid_cannot_read_old_open_descriptors(){
  let mut t=FileDescriptors::<1>::new();t.attach(id(2),3,10,FD_READ).unwrap();
  let mut out=[(99,99,99);1];
  assert_eq!(t.snapshot_owner_handles(id(1),&mut out),Ok(0));
  assert_eq!(out,[(99,99,99)]);
  assert_eq!(t.close_selected_atomic(id(1),&[3]),Err(FdError::Denied));
 }
}
