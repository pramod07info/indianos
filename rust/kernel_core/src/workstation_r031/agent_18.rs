//! R031/18: bounded, authenticated monotonic debugger execution trace.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Frame{pub sequence:u64,pub pc:u64,pub line:u32,pub value:i64}
pub const EMPTY:Frame=Frame{sequence:0,pc:0,line:0,value:0};
pub struct Trace<const N:usize>{owner:u64,frames:[Frame;N],used:usize}
impl<const N:usize> Trace<N>{
 pub fn new(owner:u64)->Self{Self{owner,frames:[EMPTY;N],used:0}}
 pub fn observe(&mut self,owner:u64,trusted:bool,frame:Frame)->Result<(),Error>{
  if owner==0||owner!=self.owner||!trusted{return Err(Error::Denied);}
  if frame.sequence==0||frame.pc==0||frame.line==0{return Err(Error::Invalid);}
  if self.used>0&&frame.sequence<=self.frames[self.used-1].sequence{
   return Err(Error::Stale);
  }
  if N==0{return Err(Error::Full);}
  if self.used==N{
   self.frames.copy_within(1..N,0);self.used-=1;
  }
  self.frames[self.used]=frame;self.used+=1;Ok(())
 }
 pub fn after(&self,owner:u64,sequence:u64,out:&mut[Frame])->Result<usize,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  let mut n=0;
  for frame in &self.frames[..self.used]{
   if frame.sequence<=sequence{continue;}
   if n==out.len(){return Err(Error::Full);}
   n+=1;
  }
  let mut at=0;
  for frame in &self.frames[..self.used]{
   if frame.sequence>sequence{out[at]=*frame;at+=1;}
  }
  Ok(n)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn debugger_trace_drops_oldest_frame_and_preserves_sequence(){
  let mut t=Trace::<2>::new(7);
  for seq in 1..=3{t.observe(7,true,Frame{sequence:seq,pc:100+seq,line:1,value:0}).unwrap();}
  let mut out=[EMPTY;2];assert_eq!(t.after(7,1,&mut out),Ok(2));
  assert_eq!(out[0].sequence,2);assert_eq!(out[1].sequence,3);
 }
 #[test]fn stale_or_foreign_debugger_frames_do_not_update_trace(){
  let mut t=Trace::<1>::new(7);let frame=Frame{sequence:1,pc:1,line:1,value:0};
  assert_eq!(t.observe(8,true,frame),Err(Error::Denied));
  t.observe(7,true,frame).unwrap();
  assert_eq!(t.observe(7,true,frame),Err(Error::Stale));
 }
}