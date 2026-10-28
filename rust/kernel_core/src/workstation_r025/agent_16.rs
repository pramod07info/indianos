//! R025/16: owner-generation and source-revision-bound debugger breakpoints.
//! Trusted debugger reports hits; no claim of actual CPU trap installation.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Breakpoint{pub id:u32,pub path:[u8;PATH_MAX],pub len:usize,
 pub line:u32,pub revision:u64,pub enabled:bool,pub hits:u32}
pub struct Breakpoints<const N:usize>{owner:Identity,entries:[Option<Breakpoint>;N]}
impl<const N:usize> Breakpoints<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,entries:[None;N]}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn slot(&self,id:u32)->Option<usize>{
  self.entries.iter().position(|b|b.is_some_and(|b|b.id==id))
 }
 pub fn add<const F:usize>(&mut self,who:Identity,fs:&RamFs<F>,
  id:u32,path:&[u8],line:u32)->Result<(),Error>{
  self.auth(who)?;
  if id==0||line==0||path.len()>PATH_MAX||self.slot(id).is_some(){
   return Err(Error::Invalid);
  }
  let (revision,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut bytes=[0u8;CONTENT_MAX];fs.read(path,0,&mut bytes[..size])
   .map_err(|_|Error::Missing)?;
  let count=bytes[..size].iter().filter(|&&b|b==b'\n').count()+
   usize::from(size>0&&bytes[size-1]!=b'\n');
  if line as usize>count{return Err(Error::Invalid);}
  let at=self.entries.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut name=[0u8;PATH_MAX];name[..path.len()].copy_from_slice(path);
  self.entries[at]=Some(Breakpoint{id,path:name,len:path.len(),line,
   revision,enabled:true,hits:0});Ok(())
 }
 pub fn hit<const F:usize>(&mut self,who:Identity,trusted_debugger:bool,
  fs:&RamFs<F>,id:u32)->Result<u32,Error>{
  self.auth(who)?;if !trusted_debugger{return Err(Error::Denied);}
  let i=self.slot(id).ok_or(Error::Missing)?;
  let mut bp=self.entries[i].ok_or(Error::Missing)?;
  if !bp.enabled{return Err(Error::Denied);}
  if fs.metadata(&bp.path[..bp.len]).map_err(|_|Error::Missing)?.0!=bp.revision{
   return Err(Error::Stale);
  }
  bp.hits=bp.hits.checked_add(1).ok_or(Error::Overflow)?;
  self.entries[i]=Some(bp);Ok(bp.hits)
 }
 pub fn toggle(&mut self,who:Identity,id:u32,enabled:bool)->Result<(),Error>{
  self.auth(who)?;
  let i=self.slot(id).ok_or(Error::Missing)?;
  let mut bp=self.entries[i].ok_or(Error::Missing)?;
  bp.enabled=enabled;self.entries[i]=Some(bp);Ok(())
 }
 pub fn remove(&mut self,who:Identity,id:u32)->Result<(),Error>{
  self.auth(who)?;
  let i=self.slot(id).ok_or(Error::Missing)?;
  self.entries[i]=None;Ok(())
 }
 pub fn get(&self,who:Identity,id:u32)->Result<Breakpoint,Error>{
  self.auth(who)?;self.slot(id).and_then(|i|self.entries[i]).ok_or(Error::Missing)
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn debugger_breakpoint_tracks_actual_source_line_and_hits(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main.rs",b"fn x(){}\n").unwrap();
  let mut b=Breakpoints::<1>::new(id(1));
  b.add(id(1),&fs,1,b"/main.rs",1).unwrap();
  assert_eq!(b.hit(id(1),true,&fs,1),Ok(1));
  assert_eq!(b.get(id(1),1).unwrap().hits,1);
 }
 #[test]fn mutated_source_invalidates_breakpoint_until_rebound(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"x").unwrap();
  let mut b=Breakpoints::<1>::new(id(1));b.add(id(1),&fs,1,b"/main",1).unwrap();
  fs.replace(b"/main",b"new").unwrap();
  assert_eq!(b.hit(id(1),true,&fs,1),Err(Error::Stale));
 }
 #[test]fn forged_debugger_and_recycled_session_cannot_report_breakpoint_hit(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"x").unwrap();
  let mut b=Breakpoints::<1>::new(id(1));b.add(id(1),&fs,1,b"/main",1).unwrap();
  assert_eq!(b.hit(id(1),false,&fs,1),Err(Error::Denied));
  assert_eq!(b.hit(id(2),true,&fs,1),Err(Error::Denied));
 }
}
