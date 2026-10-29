//! R027/12: fail-closed terminal job access based on scoped project tickets.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
use crate::workstation_r025::agent_02 as rights;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Operation{Read,Write,Delete}
fn required(op:Operation)->u32{match op{
 Operation::Read=>rights::READ,Operation::Write=>rights::WRITE,
 Operation::Delete=>rights::DELETE
}}
pub fn check(ticket:rights::Ticket,actor:Identity,now:u64,
 path:&[u8],op:Operation)->Result<(),Error>{
 rights::authorize(ticket,actor,now,required(op),path)
  .map_err(|_|Error::Denied)
}
pub fn read<const F:usize>(ticket:rights::Ticket,
 actor:Identity,now:u64,fs:&RamFs<F>,path:&[u8],
 out:&mut[u8])->Result<usize,Error>{
 check(ticket,actor,now,path,Operation::Read)?;
 let (_,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if size>out.len(){return Err(Error::Full);}
 fs.read(path,0,&mut out[..size]).map_err(|_|Error::Missing)
}
pub fn authorized_write<const F:usize>(ticket:rights::Ticket,
 actor:Identity,now:u64,fs:&mut RamFs<F>,
 expected:u64,path:&[u8],bytes:&[u8])->Result<u64,Error>{
 check(ticket,actor,now,path,Operation::Write)?;
 if fs.version()!=expected{return Err(Error::Stale);}
 fs.replace(path,bytes).map_err(|_|Error::Full)
}
pub fn authorized_remove<const F:usize>(ticket:rights::Ticket,
 actor:Identity,now:u64,fs:&mut RamFs<F>,
 expected:u64,path:&[u8])->Result<u64,Error>{
 check(ticket,actor,now,path,Operation::Delete)?;
 if fs.version()!=expected{return Err(Error::Stale);}
 let (revision,_)=fs.metadata(path).map_err(|_|Error::Missing)?;
 fs.delete_if_version(path,revision).map_err(|_|Error::Stale)
}
#[cfg(test)]mod tests{
 use super::*;fn id()->Identity{Identity{pid:7,generation:1}}
 #[test]fn terminal_read_and_write_require_explicit_project_rights(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"old").unwrap();
  let ticket=rights::grant(true,id(),&fs,b"/p",rights::READ|rights::WRITE,20).unwrap();
  authorized_write(ticket,id(),1,&mut fs,1,b"/p/a",b"new").unwrap();
  let mut out=[0;3];read(ticket,id(),2,&fs,b"/p/a",&mut out).unwrap();
  assert_eq!(&out,b"new");
 }
 #[test]fn terminal_cannot_escape_slash_delimited_project_root(){
  let fs=RamFs::<1>::new();
  let t=rights::grant(true,id(),&fs,b"/p",rights::READ,20).unwrap();
  assert_eq!(check(t,id(),1,b"/px/secret",Operation::Read),Err(Error::Denied));
 }
 #[test]fn expired_capability_prevents_read_of_existing_source(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"x").unwrap();
  let t=rights::grant(true,id(),&fs,b"/p",rights::READ,2).unwrap();
  assert_eq!(read(t,id(),2,&fs,b"/p/a",&mut [0;2]),Err(Error::Denied));
 }
}
