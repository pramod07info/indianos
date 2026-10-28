//! R021/07 owner-scoped compiler/test log stream with sequencing and truncation.
//! No stdout injection into UI and no arbitrary process execution.
use super::Error;
pub const CHUNK:usize=120;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Level{Info,Warning,Error}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Record{pub seq:u64,pub job:u32,pub owner:u32,
 pub level:Level,pub len:usize,pub data:[u8;CHUNK]}
pub const EMPTY:Record=Record{seq:0,job:0,owner:0,level:Level::Info,
 len:0,data:[0;CHUNK]};
pub struct Stream<const N:usize>{entries:[Option<Record>;N],cursor:usize,seq:u64}
impl<const N:usize> Stream<N>{
 pub const fn new()->Self{Self{entries:[None;N],cursor:0,seq:0}}
 pub fn append(&mut self,trusted:bool,owner:u32,job:u32,level:Level,
  text:&[u8])->Result<u64,Error>{
  if !trusted||owner==0||job==0{return Err(Error::Denied);}
  if N==0||text.len()>CHUNK||text.is_empty(){return Err(Error::Full);}
  if text.iter().any(|b|*b==0||*b==0x1b||!b.is_ascii()&&*b<0xa0){
   return Err(Error::Invalid);
  }
  let seq=self.seq.checked_add(1).ok_or(Error::Overflow)?;
  let mut data=[0u8;CHUNK];data[..text.len()].copy_from_slice(text);
  self.entries[self.cursor]=Some(Record{
   seq,job,owner,level,len:text.len(),data
  });self.cursor=(self.cursor+1)%N;self.seq=seq;Ok(seq)
 }
 pub fn since(&self,owner:u32,job:u32,after:u64,out:&mut[Record])
  ->Result<usize,Error>{
  if owner==0||job==0{return Err(Error::Denied);}
  let count=self.entries.iter().flatten().filter(|r|
   r.owner==owner&&r.job==job&&r.seq>after).count();
  if count>out.len(){return Err(Error::Full);}
  let mut i=0;
  for r in self.entries.iter().flatten().filter(|r|
    r.owner==owner&&r.job==job&&r.seq>after){
   out[i]=*r;i+=1;
  }
  out[..i].sort_unstable_by_key(|r|r.seq);Ok(i)
 }
 pub fn last_error(&self,owner:u32,job:u32)->Option<Record>{
  self.entries.iter().flatten().filter(|r|r.owner==owner&&r.job==job&&
    r.level==Level::Error).max_by_key(|r|r.seq).copied()
 }
 pub fn purge(&mut self,owner:u32,job:u32)->usize{
  if owner==0||job==0{return 0;}
  let mut n=0;
  for x in &mut self.entries{
   if x.is_some_and(|r|r.owner==owner&&r.job==job){*x=None;n+=1;}
  }n
 }
 pub fn latest_seq(&self)->u64{self.seq}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn compiler_stderr_and_stdout_have_separate_sequenced_levels(){
  let mut log=Stream::<3>::new();
  log.append(true,7,1,Level::Info,b"rustc started").unwrap();
  log.append(true,7,1,Level::Error,b"E0308 mismatch").unwrap();
  assert_eq!(log.last_error(7,1).unwrap().seq,2);
  let mut results=[EMPTY;3];
  assert_eq!(log.since(7,1,1,&mut results),Ok(1));
  assert_eq!(results[0].level,Level::Error);
 }
 #[test]fn process_escape_sequences_and_fake_log_publisher_denied(){
  let mut log=Stream::<1>::new();
  assert_eq!(log.append(false,7,1,Level::Info,b"fake"),Err(Error::Denied));
  assert_eq!(log.append(true,7,1,Level::Info,b"\x1b[31m"),
   Err(Error::Invalid));
  assert_eq!(log.latest_seq(),0);
 }
 #[test]fn foreign_log_owner_and_insufficient_view_are_atomic(){
  let mut log=Stream::<2>::new();
  log.append(true,7,1,Level::Info,b"a").unwrap();
  log.append(true,7,1,Level::Info,b"b").unwrap();
  let mut out=[EMPTY;1];
  assert_eq!(log.since(7,1,0,&mut out),Err(Error::Full));
  assert_eq!(out,[EMPTY]);assert_eq!(log.last_error(8,1),None);
 }
}
