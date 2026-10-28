//! R021/12: trusted bounded stack-frame timeline and frame ownership.
//! Never unwinds arbitrary kernel memory or synthesizes CPU register state.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Frame{pub addr:u64,pub function:u32,pub file:u32,
 pub line:u32,pub depth:u16,pub snapshot:u64}
pub const EMPTY:Frame=Frame{addr:0,function:0,file:0,line:0,depth:0,snapshot:0};
pub struct Stack<const N:usize>{owner:Identity,session:u32,frames:[Option<Frame>;N],
 len:usize,epoch:u64,stopped:bool}
impl<const N:usize> Stack<N>{
 pub const fn new(owner:Identity,session:u32)->Self{
  Self{owner,session,frames:[None;N],len:0,epoch:0,stopped:false}
 }
 fn auth(&self,caller:Identity)->Result<(),Error>{
  if caller.pid==0||caller!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn begin_stop(&mut self,supervisor:bool,epoch:u64)->Result<(),Error>{
  if !supervisor||epoch<=self.epoch||self.session==0{return Err(Error::Denied);}
  self.frames=[None;N];self.len=0;self.epoch=epoch;self.stopped=true;
  Ok(())
 }
 pub fn push_frame(&mut self,supervisor:bool,epoch:u64,addr:u64,
  function:u32,file:u32,line:u32)->Result<(),Error>{
  if !supervisor||!self.stopped||epoch!=self.epoch{
   return Err(Error::Denied);
  }
  if addr==0||function==0||file==0||line==0{return Err(Error::Invalid);}
  if self.len>=N||self.len>u16::MAX as usize{return Err(Error::Full);}
  self.frames[self.len]=Some(Frame{addr,function,file,line,
   depth:self.len as u16,snapshot:epoch});self.len+=1;Ok(())
 }
 pub fn select(&self,caller:Identity,depth:usize)->Result<Frame,Error>{
  self.auth(caller)?;
  if !self.stopped{return Err(Error::Conflict);}
  self.frames.get(depth).copied().flatten().ok_or(Error::Missing)
 }
 pub fn list(&self,caller:Identity,out:&mut[Frame])->Result<usize,Error>{
  self.auth(caller)?;
  if out.len()<self.len{return Err(Error::Full);}
  for i in 0..self.len{out[i]=self.frames[i].ok_or(Error::Missing)?;}
  Ok(self.len)
 }
 pub fn resume(&mut self,caller:Identity)->Result<(),Error>{
  self.auth(caller)?;
  self.frames=[None;N];self.len=0;self.stopped=false;Ok(())
 }
 pub fn depth(&self,caller:Identity)->Result<usize,Error>{
  self.auth(caller)?;Ok(self.len)
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn debugger_stack_preserves_frame_order_and_source_lines(){
  let mut s=Stack::<3>::new(id(1),1);s.begin_stop(true,1).unwrap();
  s.push_frame(true,1,0x1000,1,1,10).unwrap();
  s.push_frame(true,1,0x1010,2,1,20).unwrap();
  assert_eq!(s.select(id(1),1).unwrap().line,20);
  assert_eq!(s.depth(id(1)),Ok(2));s.resume(id(1)).unwrap();
 }
 #[test]fn forged_frames_and_old_stop_generation_are_denied(){
  let mut s=Stack::<1>::new(id(1),1);
  assert_eq!(s.begin_stop(false,1),Err(Error::Denied));
  s.begin_stop(true,1).unwrap();
  assert_eq!(s.push_frame(false,1,0x1000,1,1,1),Err(Error::Denied));
  assert_eq!(s.push_frame(true,0,0x1000,1,1,1),Err(Error::Denied));
 }
 #[test]fn short_view_and_foreign_session_do_not_leak_stack_frames(){
  let mut s=Stack::<1>::new(id(1),1);s.begin_stop(true,1).unwrap();
  s.push_frame(true,1,0x1000,1,1,1).unwrap();
  let mut out:[Frame;0]=[];
  assert_eq!(s.list(id(1),&mut out),Err(Error::Full));
  assert_eq!(s.select(id(2),0),Err(Error::Denied));
 }
}
