//! R032/17: authenticated source-bound debugger breakpoints and guest PC matching.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
use crate::workstation_r031::agent_18::Frame;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Breakpoint{pub path:[u8;PATH_MAX],pub len:usize,pub rev:u64,
 pub line:u32,pub pc:u64,pub enabled:bool}
pub struct Breakpoints<const N:usize>{owner:u64,items:[Option<Breakpoint>;N]}
impl<const N:usize> Breakpoints<N>{
 pub fn new(owner:u64)->Self{Self{owner,items:[None;N]}}
 pub fn install<const F:usize>(&mut self,owner:u64,fs:&RamFs<F>,path:&[u8],
  line:u32,pc:u64)->Result<usize,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  if line==0||pc==0||path.len()>PATH_MAX{return Err(Error::Invalid);}
  let(rev,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut source=[0u8;CONTENT_MAX];fs.read(path,0,&mut source[..size])
   .map_err(|_|Error::Missing)?;
  if source[..size].split(|b|*b==b'\n').nth(line as usize-1).is_none(){
   return Err(Error::Invalid);
  }
  let i=self.items.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut name=[0u8;PATH_MAX];name[..path.len()].copy_from_slice(path);
  self.items[i]=Some(Breakpoint{path:name,len:path.len(),rev,line,pc,enabled:true});Ok(i)
 }
 pub fn check<const F:usize>(&self,owner:u64,trusted:bool,fs:&RamFs<F>,
  path:&[u8],frame:Frame)->Result<bool,Error>{
  if owner==0||owner!=self.owner||!trusted{return Err(Error::Denied);}
  if frame.pc==0||frame.sequence==0{return Err(Error::Invalid);}
  for b in self.items.iter().flatten(){
   if !b.enabled||&b.path[..b.len]!=path||b.pc!=frame.pc||b.line!=frame.line{continue;}
   if fs.metadata(path).is_ok_and(|(rev,_)|rev==b.rev){return Ok(true);}
  }Ok(false)
 }
 pub fn disable(&mut self,owner:u64,slot:usize)->Result<(),Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  self.items.get_mut(slot).and_then(Option::as_mut).ok_or(Error::Missing)?.enabled=false;
  Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn breakpoint_hits_matching_trusted_guest_pc_and_line(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.rs",b"fn run() {}\n").unwrap();
  let mut bp=Breakpoints::<1>::new(7);
  bp.install(7,&fs,b"/p/a.rs",1,4096).unwrap();
  let event=Frame{pc:4096,line:1,sequence:1,value:0};
  assert_eq!(bp.check(7,true,&fs,b"/p/a.rs",event),Ok(true));
  bp.disable(7,0).unwrap();assert_eq!(bp.check(7,true,&fs,b"/p/a.rs",event),Ok(false));
 }
 #[test]fn newer_source_generation_and_untrusted_pc_cannot_stop_editor(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"foo").unwrap();
  let mut bp=Breakpoints::<1>::new(7);bp.install(7,&fs,b"/p/a",1,42).unwrap();
  let frame=Frame{pc:42,line:1,sequence:1,value:0};
  assert_eq!(bp.check(7,false,&fs,b"/p/a",frame),Err(Error::Denied));
  fs.replace(b"/p/a",b"bar").unwrap();
  assert_eq!(bp.check(7,true,&fs,b"/p/a",frame),Ok(false));
 }
}