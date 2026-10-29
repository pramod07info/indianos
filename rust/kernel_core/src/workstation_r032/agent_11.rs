//! R032/11: permission-checked real project file listing for a terminal command.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
use crate::workstation_r025::agent_02::{self,Ticket,READ};
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Record{pub path:[u8;PATH_MAX],pub len:usize,pub size:usize}
pub const EMPTY:Record=Record{path:[0;PATH_MAX],len:0,size:0};
pub fn list<const F:usize,const N:usize>(fs:&RamFs<F>,who:Identity,
 ticket:Ticket,now:u64,directory:&[u8],out:&mut[Record;N])->Result<usize,Error>{
 if directory.len()<2||directory[0]!=b'/'{return Err(Error::Invalid);}
 let mut staged=[EMPTY;N];let mut n=0;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];let (len,size,_)=fs.entry(i,&mut path)
   .map_err(|_|Error::Invalid)?.ok_or(Error::Missing)?;
  if !path[..len].starts_with(directory)||path.get(directory.len())!=Some(&b'/'){
   continue;
  }
  agent_02::authorize(ticket,who,now,READ,&path[..len])
    .map_err(|_|Error::Denied)?;
  if n==N{return Err(Error::Full);}
  staged[n]=Record{path,len,size};n+=1;
 }
 staged[..n].sort_unstable_by(|a,b|a.path[..a.len].cmp(&b.path[..b.len]));
 *out=staged;Ok(n)
}
#[cfg(test)]mod tests{
 use super::*;
 use crate::workstation_r025::agent_02 as rights;
 #[test]fn terminal_lists_real_approved_project_files(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/p/z",b"Z").unwrap();fs.replace(b"/p/a",b"A").unwrap();
  fs.replace(b"/private/item",b"secret").unwrap();
  let who=Identity{pid:7,generation:1};
  let ticket=rights::grant(true,who,&fs,b"/p",READ,10).unwrap();
  let mut records=[EMPTY;3];assert_eq!(list(&fs,who,ticket,1,b"/p",&mut records),Ok(2));
  assert_eq!(&records[0].path[..records[0].len],b"/p/a");
 }
 #[test]fn unauthorized_workspace_cannot_enumerate_outside_files(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/private/item",b"secret").unwrap();
  let who=Identity{pid:7,generation:1};
  let ticket=rights::grant(true,who,&fs,b"/p",READ,10).unwrap();
  assert_eq!(list(&fs,who,ticket,1,b"/private",&mut[EMPTY;1]),Err(Error::Denied));
 }
}