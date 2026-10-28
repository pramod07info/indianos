//! R025/02: fail-closed workspace root file access ticket for trusted callers.
//! Scopes action rights, session generation and expiry to real RamFs paths.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
pub const READ:u32=1;pub const WRITE:u32=2;pub const DELETE:u32=4;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Ticket{pub owner:Identity,pub root:[u8;PATH_MAX],pub len:usize,
 pub rights:u32,pub expires:u64,pub issued_rev:u64,pub revoked:bool}
pub fn grant<const F:usize>(supervisor:bool,owner:Identity,fs:&RamFs<F>,
 root:&[u8],rights:u32,expires:u64)->Result<Ticket,Error>{
 if !supervisor||owner.pid==0||owner.generation==0||root.len()<2||
  root.len()>PATH_MAX||root[0]!=b'/'||root.ends_with(b"/")||
  root.windows(2).any(|x|x==b"..")||rights==0||rights&!(READ|WRITE|DELETE)!=0||
  expires==0{return Err(Error::Denied);}
 let mut name=[0u8;PATH_MAX];name[..root.len()].copy_from_slice(root);
 Ok(Ticket{owner,root:name,len:root.len(),rights,expires,issued_rev:fs.version(),
  revoked:false})
}
pub fn authorize(ticket:Ticket,actor:Identity,now:u64,op:u32,
 path:&[u8])->Result<(),Error>{
 if actor.pid==0||actor!=ticket.owner||ticket.revoked||now>=ticket.expires||
  op==0||op&ticket.rights!=op{return Err(Error::Denied);}
 if !path.starts_with(&ticket.root[..ticket.len])||
  path.get(ticket.len)!=Some(&b'/')||
  path.windows(2).any(|w|w==b".."){return Err(Error::Denied);}
 Ok(())
}
pub fn read<const F:usize>(ticket:Ticket,actor:Identity,
 now:u64,fs:&RamFs<F>,path:&[u8],out:&mut[u8])->Result<usize,Error>{
 authorize(ticket,actor,now,READ,path)?;
 let (_,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if len>out.len(){return Err(Error::Full);}
 fs.read(path,0,&mut out[..len]).map_err(|_|Error::Missing)
}
pub fn write<const F:usize>(ticket:Ticket,actor:Identity,now:u64,
 fs:&mut RamFs<F>,expected:u64,path:&[u8],data:&[u8])->Result<u64,Error>{
 authorize(ticket,actor,now,WRITE,path)?;
 if expected!=fs.version(){return Err(Error::Stale);}
 fs.replace(path,data).map_err(|_|Error::Full)
}
pub fn revoke(ticket:&mut Ticket,supervisor:bool)->Result<(),Error>{
 if !supervisor{return Err(Error::Denied);}ticket.revoked=true;Ok(())
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn scoped_file_reader_and_writer_operate_on_real_project_bytes(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/app/a",b"old").unwrap();
  let t=grant(true,id(1),&fs,b"/app",READ|WRITE,100).unwrap();
  write(t,id(1),1,&mut fs,1,b"/app/a",b"new").unwrap();
  let mut out=[0;3];read(t,id(1),2,&fs,b"/app/a",&mut out).unwrap();
  assert_eq!(&out,b"new");
 }
 #[test]fn sibling_prefix_and_foreign_session_do_not_escape_project(){
  let fs=RamFs::<1>::new();let t=grant(true,id(1),&fs,b"/app",READ,100).unwrap();
  assert_eq!(authorize(t,id(1),1,READ,b"/appX/a"),Err(Error::Denied));
  assert_eq!(authorize(t,id(2),1,READ,b"/app/a"),Err(Error::Denied));
 }
 #[test]fn expired_or_revoked_ticket_stops_later_file_reads(){
  let fs=RamFs::<1>::new();let mut t=grant(true,id(1),&fs,b"/app",READ,10).unwrap();
  assert_eq!(authorize(t,id(1),10,READ,b"/app/a"),Err(Error::Denied));
  revoke(&mut t,true).unwrap();
  assert_eq!(authorize(t,id(1),1,READ,b"/app/a"),Err(Error::Denied));
 }
}
