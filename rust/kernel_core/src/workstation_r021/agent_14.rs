//! R021/14: independent reviewer approvals, CI checks and one-commit
//! integration gate. Model-only: actual GitHub server merge is external.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Open,Reviewed,Green,Ready,Merged,Rejected}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Gate{pub author:u32,pub head:u64,pub base:u64,pub reviewer:u32,
 pub ci:u32,pub required:u32,pub state:State,pub frozen:bool}
impl Gate{
 pub fn new(author:u32,base:u64,head:u64,
  required:u32)->Result<Self,Error>{
  if author==0||head==0||base==0||head==base||required==0{
   return Err(Error::Invalid);
  }
  Ok(Self{author,base,head,reviewer:0,ci:0,required,
   state:State::Open,frozen:false})
 }
 pub fn approve(&mut self,reviewer:u32,head:u64)->Result<(),Error>{
  if reviewer==0||reviewer==self.author{return Err(Error::Denied);}
  if head!=self.head||self.frozen{return Err(Error::Stale);}
  self.reviewer=reviewer;self.state=State::Reviewed;Ok(())
 }
 pub fn report_ci(&mut self,trusted:bool,head:u64,checks:u32,
  successful:bool)->Result<(),Error>{
  if !trusted||head!=self.head||self.frozen{return Err(Error::Denied);}
  if !successful{self.ci&=!checks;return Err(Error::NotReady);}
  self.ci|=checks&self.required;
  if self.ci&self.required==self.required{
   self.state=State::Green;
  }Ok(())
 }
 pub fn ready(&mut self,base_now:u64)->Result<(),Error>{
  if self.frozen||self.reviewer==0||base_now!=self.base||
   self.ci&self.required!=self.required{
   return Err(Error::NotReady);
  }
  self.state=State::Ready;Ok(())
 }
 pub fn merge_record(&mut self,trusted:bool,
  head:u64,base_now:u64)->Result<(),Error>{
  if !trusted||head!=self.head||base_now!=self.base{
   return Err(Error::Denied);
  }
  if self.state!=State::Ready{return Err(Error::NotReady);}
  self.state=State::Merged;self.frozen=true;Ok(())
 }
 pub fn update_head(&mut self,author:u32,new_head:u64)->Result<(),Error>{
  if author!=self.author||self.frozen||new_head==0||new_head==self.head{
   return Err(Error::Denied);
  }
  self.head=new_head;self.reviewer=0;self.ci=0;self.state=State::Open;Ok(())
 }
 pub fn reject(&mut self,reviewer:u32)->Result<(),Error>{
  if reviewer==0||reviewer==self.author||self.frozen{return Err(Error::Denied);}
  self.state=State::Rejected;self.reviewer=0;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn independent_reviewer_and_all_required_checks_unlock_merge(){
  let mut g=Gate::new(7,100,101,0b111).unwrap();
  g.approve(8,101).unwrap();g.report_ci(true,101,0b011,true).unwrap();
  assert_eq!(g.ready(100),Err(Error::NotReady));
  g.report_ci(true,101,0b100,true).unwrap();
  g.ready(100).unwrap();g.merge_record(true,101,100).unwrap();
  assert_eq!(g.state,State::Merged);
 }
 #[test]fn author_cannot_approve_and_stale_head_revokes_ci(){
  let mut g=Gate::new(7,100,101,0b11).unwrap();
  assert_eq!(g.approve(7,101),Err(Error::Denied));
  g.approve(8,101).unwrap();g.report_ci(true,101,0b11,true).unwrap();
  g.update_head(7,102).unwrap();
  assert_eq!(g.ready(100),Err(Error::NotReady));
 }
 #[test]fn concurrent_main_change_prevents_old_source_merge(){
  let mut g=Gate::new(7,100,101,0b1).unwrap();
  g.approve(8,101).unwrap();g.report_ci(true,101,1,true).unwrap();
  assert_eq!(g.ready(200),Err(Error::NotReady));
  assert_eq!(g.merge_record(true,101,100),Err(Error::NotReady));
 }
}
