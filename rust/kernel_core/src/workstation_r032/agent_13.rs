//! R032/13: write-capability checked append to real guest log/source file.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
use crate::batch5_session::Identity;
use crate::workstation_r025::agent_02::{self,Ticket,WRITE};
pub fn append<const F:usize>(fs:&mut RamFs<F>,who:Identity,
 ticket:Ticket,now:u64,path:&[u8],expected:u64,
 data:&[u8])->Result<u64,Error>{
 agent_02::authorize(ticket,who,now,WRITE,path).map_err(|_|Error::Denied)?;
 if data.is_empty()||data.contains(&0)||core::str::from_utf8(data).is_err(){
  return Err(Error::Invalid);
 }
 let (rev,n)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if rev!=expected{return Err(Error::Stale);}
 let required=n.checked_add(data.len()).ok_or(Error::Overflow)?;
 if required>CONTENT_MAX{return Err(Error::Full);}
 let mut staged=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut staged[..n]).map_err(|_|Error::Missing)?;
 staged[n..required].copy_from_slice(data);
 fs.replace(path,&staged[..required]).map_err(|_|Error::Full)
}
#[cfg(test)]mod tests{
 use super::*;
 use crate::workstation_r025::agent_02 as rights;
 #[test]fn authorized_console_appends_to_actual_project_log(){
  let who=Identity{pid:7,generation:1};let mut fs=RamFs::<1>::new();
  fs.replace(b"/p/log",b"first\n").unwrap();
  let t=rights::grant(true,who,&fs,b"/p",WRITE,10).unwrap();
  assert_eq!(append(&mut fs,who,t,1,b"/p/log",1,b"second\n"),Ok(2));
  let mut buf=[0;13];fs.read(b"/p/log",0,&mut buf).unwrap();
  assert_eq!(&buf,b"first\nsecond\n");
 }
 #[test]fn read_only_ticket_and_stale_revision_rejected(){
  let who=Identity{pid:7,generation:1};let mut fs=RamFs::<1>::new();
  fs.replace(b"/p/a",b"old").unwrap();let t=rights::grant(true,who,&fs,b"/p",rights::READ,10).unwrap();
  assert_eq!(append(&mut fs,who,t,1,b"/p/a",1,b"new"),Err(Error::Denied));
  let w=rights::grant(true,who,&fs,b"/p",WRITE,10).unwrap();
  assert_eq!(append(&mut fs,who,w,1,b"/p/a",0,b"new"),Err(Error::Stale));
  assert_eq!(fs.version(),1);
 }
}