//! R029/16: trusted source revision-bound breakpoint registry with safe navigation.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Breakpoint{pub path:[u8;PATH_MAX],pub len:usize,pub revision:u64,
 pub line:u32,pub enabled:bool}
pub struct Registry<const N:usize>{owner:Identity,items:[Option<Breakpoint>;N]}
impl<const N:usize> Registry<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,items:[None;N]}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn add<const F:usize>(&mut self,who:Identity,fs:&RamFs<F>,
 path:&[u8],line:u32)->Result<(),Error>{
  self.auth(who)?;
  if line==0||path.len()<2||path.len()>PATH_MAX{return Err(Error::Invalid);}
  let (revision,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut data=[0u8;CONTENT_MAX];fs.read(path,0,&mut data[..size])
   .map_err(|_|Error::Missing)?;
  let lines=data[..size].iter().filter(|&&b|b==b'\n').count()
   +usize::from(size>0&&data[size-1]!=b'\n');
  if line as usize>lines{return Err(Error::Invalid);}
  if self.items.iter().flatten().any(|b|
   &b.path[..b.len]==path&&b.line==line){return Err(Error::Conflict);}
  let i=self.items.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut name=[0u8;PATH_MAX];name[..path.len()].copy_from_slice(path);
  self.items[i]=Some(Breakpoint{path:name,len:path.len(),revision,line,enabled:true});
  Ok(())
 }
 pub fn hit<const F:usize>(&self,who:Identity,fs:&RamFs<F>,
 trusted:bool,path:&[u8],line:u32)->Result<bool,Error>{
  self.auth(who)?;if !trusted{return Err(Error::Denied);}
  let item=self.items.iter().flatten().find(|b|&b.path[..b.len]==path&&b.line==line);
  let Some(bp)=item else{return Ok(false)};
  if fs.metadata(path).map_err(|_|Error::Missing)?.0!=bp.revision{return Err(Error::Stale);}
  Ok(bp.enabled)
 }
 pub fn count(&self,who:Identity)->Result<usize,Error>{
  self.auth(who)?;Ok(self.items.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn trusted_source_breakpoint_hit_matches_real_file_revision(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.rs",b"fn a(){}\n").unwrap();
  let mut b=Registry::<1>::new(id(1));b.add(id(1),&fs,b"/p/a.rs",1).unwrap();
  assert_eq!(b.hit(id(1),&fs,true,b"/p/a.rs",1),Ok(true));
  fs.replace(b"/p/a.rs",b"new").unwrap();
  assert_eq!(b.hit(id(1),&fs,true,b"/p/a.rs",1),Err(Error::Stale));
 }
 #[test]fn untrusted_debugger_cannot_claim_breakpoint_hit(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a.rs",b"fn x(){}\n").unwrap();
  let mut b=Registry::<1>::new(id(1));b.add(id(1),&fs,b"/a.rs",1).unwrap();
  assert_eq!(b.hit(id(1),&fs,false,b"/a.rs",1),Err(Error::Denied));
 }
}