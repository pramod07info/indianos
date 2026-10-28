//! R021/16: searchable developer task history with per-owner retention.
//! Captures sanitized metadata, never command arguments, tokens or secrets.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind{Build,Test,Package,Launch,Debug}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Entry{pub owner:u32,pub project:u32,pub task:u32,
 pub kind:Kind,pub tick:u64,pub exit_code:u16,pub duration_ms:u32}
pub const EMPTY:Entry=Entry{owner:0,project:0,task:0,
 kind:Kind::Build,tick:0,exit_code:0,duration_ms:0};
pub struct History<const N:usize>{items:[Option<Entry>;N],cursor:usize,
 last_tick:u64}
impl<const N:usize> History<N>{
 pub const fn new()->Self{Self{items:[None;N],cursor:0,last_tick:0}}
 pub fn record(&mut self,trusted:bool,entry:Entry)->Result<(),Error>{
  if !trusted||entry.owner==0||entry.project==0||entry.task==0{
   return Err(Error::Denied);
  }
  if entry.tick<self.last_tick{return Err(Error::Stale);}
  if N==0{return Err(Error::Full);}
  self.items[self.cursor]=Some(entry);
  self.cursor=(self.cursor+1)%N;self.last_tick=entry.tick;Ok(())
 }
 pub fn search(&self,owner:u32,project:u32,kind:Option<Kind>,
  since:u64,out:&mut[Entry])->Result<usize,Error>{
  if owner==0{return Err(Error::Denied);}
  let count=self.items.iter().flatten().filter(|x|
   x.owner==owner&&(project==0||x.project==project)&&
   kind.is_none_or(|k|x.kind==k)&&x.tick>=since).count();
  if count>out.len(){return Err(Error::Full);}
  let mut n=0;
  for e in self.items.iter().flatten().filter(|x|
   x.owner==owner&&(project==0||x.project==project)&&
   kind.is_none_or(|k|x.kind==k)&&x.tick>=since){
   out[n]=*e;n+=1;
  }
  out[..n].sort_unstable_by_key(|e|e.tick);
  Ok(n)
 }
 pub fn failures(&self,owner:u32,project:u32)->usize{
  self.items.iter().flatten().filter(|e|e.owner==owner&&
   e.project==project&&e.exit_code!=0).count()
 }
 pub fn purge_owner(&mut self,owner:u32)->usize{
  if owner==0{return 0;}
  let mut n=0;
  for x in &mut self.items{
   if x.is_some_and(|e|e.owner==owner){*x=None;n+=1;}
  }n
 }
 pub fn most_recent(&self,owner:u32)->Option<Entry>{
  self.items.iter().flatten().filter(|e|e.owner==owner)
   .max_by_key(|e|e.tick).copied()
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn e(owner:u32,tick:u64,exit_code:u16)->Entry{
  Entry{owner,project:1,task:2,kind:Kind::Build,
   tick,exit_code,duration_ms:50}
 }
 #[test]fn searches_build_failures_with_source_project_scope(){
  let mut h=History::<3>::new();
  h.record(true,e(7,1,0)).unwrap();h.record(true,e(7,2,100)).unwrap();
  h.record(true,e(8,3,1)).unwrap();
  let mut out=[EMPTY;3];
  assert_eq!(h.search(7,1,Some(Kind::Build),0,&mut out),Ok(2));
  assert_eq!(h.failures(7,1),1);assert_eq!(out[1].tick,2);
 }
 #[test]fn forged_task_result_or_old_tick_cannot_change_history(){
  let mut h=History::<1>::new();h.record(true,e(7,5,0)).unwrap();
  assert_eq!(h.record(false,e(7,6,0)),Err(Error::Denied));
  assert_eq!(h.record(true,e(7,4,1)),Err(Error::Stale));
  assert_eq!(h.most_recent(7),Some(e(7,5,0)));
 }
 #[test]fn foreign_owner_and_short_output_cannot_expose_history(){
  let mut h=History::<2>::new();h.record(true,e(7,1,0)).unwrap();
  h.record(true,e(7,2,0)).unwrap();
  let mut out=[EMPTY;1];
  assert_eq!(h.search(7,1,None,0,&mut out),Err(Error::Full));
  assert_eq!(out,[EMPTY]);assert_eq!(h.purge_owner(8),0);
 }
}
