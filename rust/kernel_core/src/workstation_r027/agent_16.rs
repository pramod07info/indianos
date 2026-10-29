//! R027/16: trusted bounded debugger stack-frame view with session isolation.
//! Frames are externally reported, not collected from arbitrary guest memory.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Frame{pub ip:u64,pub sp:u64,pub line:u32,pub file_id:u32}
pub const EMPTY:Frame=Frame{ip:0,sp:0,line:0,file_id:0};
pub struct Callstack<const N:usize>{owner:Identity,frames:[Frame;N],len:usize,
 tick:u64}
impl<const N:usize> Callstack<N>{
 pub const fn new(owner:Identity)->Self{
  Self{owner,frames:[EMPTY;N],len:0,tick:0}
 }
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn update(&mut self,who:Identity,trusted:bool,tick:u64,
  frames:&[Frame])->Result<(),Error>{
  self.auth(who)?;
  if !trusted||frames.is_empty(){return Err(Error::Denied);}
  if frames.len()>N{return Err(Error::Full);}
  if tick<self.tick||frames.iter().any(|f|f.ip==0||f.line==0){
   return Err(Error::Invalid);
  }
  let mut next=[EMPTY;N];next[..frames.len()].copy_from_slice(frames);
  self.frames=next;self.len=frames.len();self.tick=tick;Ok(())
 }
 pub fn frame(&self,who:Identity,depth:usize)->Result<Frame,Error>{
  self.auth(who)?;
  self.frames.get(depth).copied().filter(|_|depth<self.len)
   .ok_or(Error::Missing)
 }
 pub fn find_file(&self,who:Identity,file_id:u32)
  ->Result<Option<usize>,Error>{
  self.auth(who)?;
  Ok(self.frames[..self.len].iter().position(|f|f.file_id==file_id))
 }
 pub fn depth(&self,who:Identity)->Result<usize,Error>{
  self.auth(who)?;Ok(self.len)
 }
 pub fn clear(&mut self,who:Identity)->Result<(),Error>{
  self.auth(who)?;self.frames=[EMPTY;N];self.len=0;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn debugger_lists_two_trusted_callstack_frames(){
  let mut stack=Callstack::<2>::new(id(1));
  let a=Frame{ip:0x1000,sp:0x2000,line:3,file_id:4};
  let b=Frame{ip:0x1100,sp:0x2100,line:5,file_id:7};
  stack.update(id(1),true,1,&[a,b]).unwrap();
  assert_eq!(stack.frame(id(1),1),Ok(b));
  assert_eq!(stack.find_file(id(1),7),Ok(Some(1)));
 }
 #[test]fn untrusted_debugger_cannot_forge_stack_trace(){
  let mut stack=Callstack::<1>::new(id(1));
  assert_eq!(stack.update(id(1),false,1,&[Frame{ip:1,sp:1,line:1,file_id:1}]),
   Err(Error::Denied));
 }
 #[test]fn stale_pid_generation_cannot_view_private_callstack(){
  let stack=Callstack::<1>::new(id(1));
  assert_eq!(stack.depth(id(2)),Err(Error::Denied));
 }
}
