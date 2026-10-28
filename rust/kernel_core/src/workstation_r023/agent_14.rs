//! R023/14: terminal output ring, line indexes and safe text rendering.
//! Input must be trusted worker output; no ANSI escape sequence execution.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Stream{Stdout,Stderr,System}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Record{pub owner:Identity,pub job:u32,pub seq:u64,pub tick:u64,
 pub stream:Stream,pub len:usize,pub data:[u8;128]}
pub const EMPTY:Record=Record{owner:Identity{pid:0,generation:0},
 job:0,seq:0,tick:0,stream:Stream::System,len:0,data:[0;128]};
pub struct Console<const N:usize>{ring:[Option<Record>;N],
 next:usize,sequence:u64}
impl<const N:usize> Console<N>{
 pub const fn new()->Self{Self{ring:[None;N],next:0,sequence:0}}
 pub fn append(&mut self,trusted:bool,owner:Identity,job:u32,
  tick:u64,stream:Stream,text:&[u8])->Result<u64,Error>{
  if !trusted||owner.pid==0||owner.generation==0||job==0{
   return Err(Error::Denied);
  }
  if N==0||text.is_empty()||text.len()>128{return Err(Error::Full);}
  if text.iter().any(|b|*b==0||*b==0x1b||b.is_ascii_control()&&
   *b!=b'\t'&&*b!=b'\n'){return Err(Error::Invalid);}
  let seq=self.sequence.checked_add(1).ok_or(Error::Overflow)?;
  let mut data=[0u8;128];data[..text.len()].copy_from_slice(text);
  self.ring[self.next]=Some(Record{owner,job,seq,tick,stream,
   len:text.len(),data});
  self.next=(self.next+1)%N;self.sequence=seq;Ok(seq)
 }
 pub fn since(&self,owner:Identity,job:u32,after:u64,
  out:&mut[Record])->Result<usize,Error>{
  if owner.pid==0||job==0{return Err(Error::Denied);}
  let count=self.ring.iter().flatten().filter(|r|
   r.owner==owner&&r.job==job&&r.seq>after).count();
  if count>out.len(){return Err(Error::Full);}
  let mut n=0;
  for r in self.ring.iter().flatten().filter(|r|
   r.owner==owner&&r.job==job&&r.seq>after){
   out[n]=*r;n+=1;
  }
  out[..n].sort_unstable_by_key(|r|r.seq);Ok(n)
 }
 pub fn stream_text<const B:usize>(&self,owner:Identity,job:u32,
  after:u64,out:&mut[u8;B])->Result<usize,Error>{
  let mut staging=[0u8;B];let mut cursor=0usize;
  // Sort by actual sequence, not physical ring order.
  let mut previous=after;
  loop{
   let Some(record)=self.ring.iter().flatten().filter(|r|
    r.owner==owner&&r.job==job&&r.seq>previous)
    .min_by_key(|r|r.seq)else{break};
   if cursor+record.len>B{return Err(Error::Full);}
   staging[cursor..cursor+record.len].copy_from_slice(&record.data[..record.len]);
   cursor+=record.len;previous=record.seq;
  }
  *out=staging;Ok(cursor)
 }
 pub fn error_count(&self,owner:Identity,job:u32)->usize{
  self.ring.iter().flatten().filter(|r|r.owner==owner&&r.job==job&&
   r.stream==Stream::Stderr).count()
 }
 pub fn clear_job(&mut self,owner:Identity,job:u32)->usize{
  let mut count=0;for entry in &mut self.ring{
   if entry.is_some_and(|v|v.owner==owner&&v.job==job){
    *entry=None;count+=1;
   }
  }count
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn interleaved_compiler_stdout_stderr_reassembles_ordered_console(){
  let mut c=Console::<3>::new();
  c.append(true,id(1),1,1,Stream::Stdout,b"compile\n").unwrap();
  c.append(true,id(1),1,2,Stream::Stderr,b"error\n").unwrap();
  let mut out=[0u8;32];
  let n=c.stream_text(id(1),1,0,&mut out).unwrap();
  assert_eq!(&out[..n],b"compile\nerror\n");
  assert_eq!(c.error_count(id(1),1),1);
 }
 #[test]fn terminal_escape_injection_and_forged_worker_rejected(){
  let mut c=Console::<1>::new();
  assert_eq!(c.append(false,id(1),1,1,Stream::Stdout,b"fake"),
   Err(Error::Denied));
  assert_eq!(c.append(true,id(1),1,1,Stream::Stdout,b"\x1b[31m"),
   Err(Error::Invalid));
 }
 #[test]fn recycled_pid_and_small_output_cannot_read_partial_logs(){
  let mut c=Console::<2>::new();
  c.append(true,id(1),1,1,Stream::Stdout,b"hello").unwrap();
  c.append(true,id(1),1,2,Stream::Stdout,b"world").unwrap();
  let mut out=[55u8;6];
  assert_eq!(c.stream_text(id(1),1,0,&mut out),Err(Error::Full));
  assert_eq!(out,[55;6]);
  let mut old=[EMPTY;2];assert_eq!(c.since(id(2),1,0,&mut old),Ok(0));
 }
}
