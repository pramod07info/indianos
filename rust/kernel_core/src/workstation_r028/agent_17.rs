//! R028/17: owner-bound trusted debugger execution trace ring and seek.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Frame{pub seq:u64,pub thread:u32,pub pc:u64,pub line:u32,pub tick:u64}
pub const EMPTY:Frame=Frame{seq:0,thread:0,pc:0,line:0,tick:0};
pub struct Trace<const N:usize>{owner:Identity,frames:[Option<Frame>;N],
 head:usize,sequence:u64,last_tick:u64}
impl<const N:usize> Trace<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,frames:[None;N],
 head:0,sequence:0,last_tick:0}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn record(&mut self,who:Identity,trusted:bool,thread:u32,
 pc:u64,line:u32,tick:u64)->Result<u64,Error>{
  self.auth(who)?;
  if !trusted||thread==0||pc==0||line==0{return Err(Error::Denied);}
  if N==0||tick<self.last_tick{return Err(Error::Stale);}
  let seq=self.sequence.checked_add(1).ok_or(Error::Overflow)?;
  self.frames[self.head]=Some(Frame{seq,thread,pc,line,tick});
  self.head=(self.head+1)%N;self.sequence=seq;self.last_tick=tick;Ok(seq)
 }
 pub fn since(&self,who:Identity,after:u64,
 out:&mut[Frame])->Result<usize,Error>{
  self.auth(who)?;
  let count=self.frames.iter().flatten().filter(|f|f.seq>after).count();
  if count>out.len(){return Err(Error::Full);}
  let mut n=0;for f in self.frames.iter().flatten().filter(|f|f.seq>after){
   out[n]=*f;n+=1;
  }
  out[..n].sort_unstable_by_key(|f|f.seq);Ok(n)
 }
 pub fn latest(&self,who:Identity)->Result<u64,Error>{
  self.auth(who)?;Ok(self.sequence)
 }
 pub fn clear(&mut self,who:Identity)->Result<(),Error>{
  self.auth(who)?;self.frames=[None;N];self.head=0;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn trace_paginates_actual_trusted_thread_step_reports(){
  let mut t=Trace::<2>::new(id(1));
  t.record(id(1),true,1,0x1000,1,1).unwrap();
  t.record(id(1),true,1,0x1004,2,2).unwrap();
  let mut out=[EMPTY;2];
  assert_eq!(t.since(id(1),1,&mut out),Ok(1));
  assert_eq!(out[0].line,2);
 }
 #[test]fn fake_debugger_cannot_report_instruction_events(){
  let mut t=Trace::<1>::new(id(1));
  assert_eq!(t.record(id(1),false,1,0x1000,1,1),Err(Error::Denied));
  assert_eq!(t.latest(id(2)),Err(Error::Denied));
 }
}