//! R020/08 compiler diagnostics feed: ownership, dedup, severity,
//! source-file lookup, per-job event revision and fail-atomic result copies.
use super::Error;
use crate::workstation_r019::agent_04::{Diagnostic,Severity};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Entry{pub owner:u32,pub job:u32,pub event:u64,pub diagnostic:Diagnostic}
pub struct Feed<const N:usize>{data:[Option<Entry>;N],seq:u64}
impl<const N:usize> Feed<N>{
 pub const fn new()->Self{Self{data:[None;N],seq:0}}
 pub fn submit(&mut self,owner:u32,job:u32,line:&[u8])
  ->Result<u64,Error>{
  if owner==0||job==0{return Err(Error::Invalid);}
  let parsed=crate::workstation_r019::agent_04::parse(line)
    .map_err(|_|Error::Invalid)?;
  if self.data.iter().flatten().any(|e|e.owner==owner&&e.job==job&&
    e.diagnostic==parsed){return Err(Error::Conflict);}
  let next=self.seq.checked_add(1).ok_or(Error::Overflow)?;
  let i=self.data.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.data[i]=Some(Entry{owner,job,event:next,diagnostic:parsed});
  self.seq=next;Ok(next)
 }
 pub fn by_job(&self,owner:u32,job:u32,out:&mut[Entry])
  ->Result<usize,Error>{
  if owner==0||job==0{return Err(Error::Denied);}
  let n=self.data.iter().flatten().filter(|e|e.owner==owner&&e.job==job).count();
  if n>out.len(){return Err(Error::Full);}
  let mut i=0;for e in self.data.iter().flatten(){
   if e.owner==owner&&e.job==job{out[i]=*e;i+=1;}
  }Ok(i)
 }
 pub fn count(&self,owner:u32,severity:Severity)->usize{
  self.data.iter().flatten().filter(|e|e.owner==owner&&
    e.diagnostic.severity==severity).count()
 }
 pub fn clear_job(&mut self,owner:u32,job:u32)->usize{
  let mut n=0;if owner==0||job==0{return n;}
  for e in &mut self.data{
   if e.is_some_and(|x|x.owner==owner&&x.job==job){*e=None;n+=1;}
  }n
 }
 pub fn first_error(&self,owner:u32,job:u32)->Option<Diagnostic>{
  self.data.iter().flatten().find(|e|e.owner==owner&&e.job==job&&
    e.diagnostic.severity==Severity::Error).map(|e|e.diagnostic)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 const EMPTY:Diagnostic=Diagnostic{path:[0;80],path_len:0,line:0,column:0,
  severity:Severity::Note,message:[0;128],message_len:0};
 const SENTINEL:Entry=Entry{owner:0,job:0,event:0,diagnostic:EMPTY};
 #[test]fn aggregates_compiler_errors_and_source_locations(){
  let mut f=Feed::<3>::new();
  f.submit(7,1,b"src/a.rs:10:2:error wrong type").unwrap();
  f.submit(7,1,b"src/a.rs:11:1:warning unused").unwrap();
  assert_eq!(f.count(7,Severity::Error),1);
  assert_eq!(f.first_error(7,1).unwrap().line,10);
  let mut out=[SENTINEL;3];assert_eq!(f.by_job(7,1,&mut out),Ok(2));
 }
 #[test]fn malformed_and duplicate diagnostics cannot corrupt_feed(){
  let mut f=Feed::<1>::new();let msg=b"a.rs:1:1:error failed";
  assert_eq!(f.submit(7,1,msg),Ok(1));
  assert_eq!(f.submit(7,1,msg),Err(Error::Conflict));
  assert_eq!(f.submit(7,1,b"../a:1:1:error bad"),Err(Error::Invalid));
  assert_eq!(f.count(7,Severity::Error),1);
 }
 #[test]fn short_output_and foreign_owner_are_safe(){
  let mut f=Feed::<2>::new();
  f.submit(7,1,b"a:1:1:error a").unwrap();
  f.submit(7,1,b"b:1:1:error b").unwrap();
  let mut out=[SENTINEL;1];assert_eq!(f.by_job(7,1,&mut out),Err(Error::Full));
  assert_eq!(out,[SENTINEL]);assert_eq!(f.clear_job(8,1),0);
 }
}
