//! R020/11 owner-scoped debugger tracing: monotonic stops/continues and
//! bounded supervisor-authenticated event history, without target memory reads.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum EventKind{Break,Step,Resume,Fault,Detach}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Event{pub owner:u32,pub session:u32,pub sequence:u64,
 pub tick:u64,pub instruction:u64,pub kind:EventKind}
pub struct Trace<const N:usize>{data:[Option<Event>;N],next:usize,
 count:usize,seq:u64,last_tick:u64}
impl<const N:usize> Trace<N>{
 pub const fn new()->Self{Self{data:[None;N],next:0,count:0,seq:0,last_tick:0}}
 pub fn record(&mut self,supervisor:bool,owner:u32,session:u32,
  tick:u64,instruction:u64,kind:EventKind)->Result<u64,Error>{
  if !supervisor||owner==0||session==0||instruction==0{return Err(Error::Denied);}
  if tick<self.last_tick{return Err(Error::Stale);}
  if N==0{return Err(Error::Full);}
  let seq=self.seq.checked_add(1).ok_or(Error::Overflow)?;
  self.data[self.next]=Some(Event{owner,session,sequence:seq,tick,
   instruction,kind});
  self.next=(self.next+1)%N;self.count=self.count.saturating_add(1).min(N);
  self.seq=seq;self.last_tick=tick;Ok(seq)
 }
 pub fn events_for(&self,owner:u32,session:u32,
  out:&mut[Event])->Result<usize,Error>{
  if owner==0||session==0{return Err(Error::Denied);}
  let matches=self.data.iter().flatten().filter(|e|
   e.owner==owner&&e.session==session).count();
  if matches>out.len(){return Err(Error::Full);}
  let start=if self.count==N{self.next}else{0};
  let mut n=0;for offset in 0..self.count{
   if let Some(e)=self.data[(start+offset)%N]{
    if e.owner==owner&&e.session==session{out[n]=e;n+=1;}
   }
  }Ok(n)
 }
 pub fn last_fault(&self,owner:u32,session:u32)->Option<Event>{
  self.data.iter().flatten().filter(|e|e.owner==owner&&e.session==session&&
   e.kind==EventKind::Fault).max_by_key(|e|e.sequence).copied()
 }
 pub fn clear_session(&mut self,owner:u32,session:u32)->usize{
  if owner==0||session==0{return 0;}
  let mut n=0;for e in &mut self.data{
   if e.is_some_and(|x|x.owner==owner&&x.session==session){
    *e=None;n+=1;
   }
  }n
 }
 pub fn count_for(&self,owner:u32,session:u32)->usize{
  self.data.iter().flatten().filter(|e|
    e.owner==owner&&e.session==session).count()
 }
}
#[cfg(test)]mod tests{
 use super::*;
 const ZERO:Event=Event{owner:0,session:0,sequence:0,tick:0,
  instruction:0,kind:EventKind::Step};
 #[test]fn debugger_timeline_is_monotonic_and_sorted(){
  let mut t=Trace::<3>::new();
  t.record(true,7,1,1,0x1000,EventKind::Break).unwrap();
  t.record(true,7,1,2,0x1010,EventKind::Step).unwrap();
  let mut output=[ZERO;3];
  assert_eq!(t.events_for(7,1,&mut output),Ok(2));
  assert_eq!(output[1].sequence,2);
 }
 #[test]fn old_timestamp_or_forged_tracer_is_denied(){
  let mut t=Trace::<2>::new();t.record(true,7,1,4,0x1000,EventKind::Fault).unwrap();
  assert_eq!(t.record(true,7,1,3,0x1004,EventKind::Step),Err(Error::Stale));
  assert_eq!(t.record(false,7,1,5,0x1004,EventKind::Step),Err(Error::Denied));
  assert_eq!(t.last_fault(7,1).unwrap().instruction,0x1000);
 }
 #[test]fn ring_reaps_oldest_and_short_output_is_atomic(){
  let mut t=Trace::<2>::new();for i in 1..=3{
   t.record(true,7,1,i,0x1000,EventKind::Step).unwrap();
  }
  let mut out=[ZERO;1];assert_eq!(t.events_for(7,1,&mut out),Err(Error::Full));
  assert_eq!(out,[ZERO]);assert_eq!(t.count_for(7,1),2);
  assert_eq!(t.clear_session(7,1),2);
 }
}
