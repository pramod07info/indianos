//! R032/12: authenticated cross-file terminal grep, reporting real matching lines.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
use crate::workstation_r025::agent_02::{self,Ticket,READ};
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Hit{pub path:[u8;PATH_MAX],pub len:usize,pub line:usize,
 pub offset:usize,pub revision:u64}
pub const EMPTY:Hit=Hit{path:[0;PATH_MAX],len:0,line:0,offset:0,revision:0};
pub fn grep<const F:usize,const N:usize>(fs:&RamFs<F>,who:Identity,ticket:Ticket,
 now:u64,root:&[u8],pattern:&[u8],out:&mut[Hit;N])->Result<usize,Error>{
 if root.len()<2||root[0]!=b'/'||pattern.is_empty()||pattern.len()>64{
  return Err(Error::Invalid);
 }
 let mut stage=[EMPTY;N];let mut n=0;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];let(len,size,revision)=fs.entry(i,&mut path)
   .map_err(|_|Error::Invalid)?.ok_or(Error::Missing)?;
  if !path[..len].starts_with(root)||path.get(root.len())!=Some(&b'/'){continue;}
  agent_02::authorize(ticket,who,now,READ,&path[..len]).map_err(|_|Error::Denied)?;
  let mut bytes=[0u8;CONTENT_MAX];fs.read(&path[..len],0,&mut bytes[..size])
   .map_err(|_|Error::Missing)?;
  let mut line=1;let mut begin=0;
  for(pos,&byte) in bytes[..size].iter().enumerate(){
   if byte!=b'\n'{continue;}
   let end=pos+1;
   if bytes[begin..end].windows(pattern.len()).any(|x|x==pattern){
    if n==N{return Err(Error::Full);}
    stage[n]=Hit{path,len,line,offset:begin,revision};n+=1;
   }
   begin=end;line+=1;
  }
  if begin<size&&bytes[begin..size].windows(pattern.len()).any(|x|x==pattern){
   if n==N{return Err(Error::Full);}
   stage[n]=Hit{path,len,line,offset:begin,revision};n+=1;
  }
 }
 *out=stage;Ok(n)
}
#[cfg(test)]mod tests{
 use super::*;
 use crate::workstation_r025::agent_02 as rights;
 #[test]fn searches_two_real_files_under_authenticated_directory(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/p/a",b"ok\nerror\n").unwrap();fs.replace(b"/p/b",b"error").unwrap();
  let who=Identity{pid:3,generation:1};let t=rights::grant(true,who,&fs,b"/p",READ,10).unwrap();
  let mut out=[EMPTY;3];assert_eq!(grep(&fs,who,t,1,b"/p",b"error",&mut out),Ok(2));
  assert_eq!(out[0].line,2);
 }
 #[test]fn invalid_pattern_and expired_grant_are_denied(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"secret").unwrap();
  let who=Identity{pid:3,generation:1};let t=rights::grant(true,who,&fs,b"/p",READ,10).unwrap();
  assert_eq!(grep(&fs,who,t,10,b"/p",b"secret",&mut[EMPTY;1]),Err(Error::Denied));
  assert_eq!(grep(&fs,who,t,1,b"/p",b"",&mut[EMPTY;1]),Err(Error::Invalid));
 }
}