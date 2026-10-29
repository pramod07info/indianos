//! R026/15: bounded automated test retries with flaky/quarantine classification.
//! Only trusted test runner reports execution results; no tests run in this registry.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Queued,Running,Passed,Failed,Quarantined}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Case{pub id:u32,pub attempts:u8,pub max_attempts:u8,
 pub failures:u8,pub state:State}
pub struct Suite<const N:usize>{owner:u32,cases:[Option<Case>;N]}
impl<const N:usize> Suite<N>{
 pub const fn new(owner:u32)->Self{Self{owner,cases:[None;N]}}
 fn auth(&self,who:u32)->Result<(),Error>{
  if who==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn find(&self,id:u32)->Option<usize>{
  self.cases.iter().position(|c|c.is_some_and(|c|c.id==id))
 }
 pub fn add(&mut self,who:u32,id:u32,max_attempts:u8)->Result<(),Error>{
  self.auth(who)?;
  if id==0||max_attempts==0||max_attempts>10||self.find(id).is_some(){
   return Err(Error::Invalid);
  }
  let at=self.cases.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.cases[at]=Some(Case{id,attempts:0,max_attempts,
   failures:0,state:State::Queued});Ok(())
 }
 pub fn start(&mut self,who:u32,id:u32)->Result<(),Error>{
  self.auth(who)?;
  let index=self.find(id).ok_or(Error::Missing)?;
  let mut c=self.cases[index].ok_or(Error::Missing)?;
  if c.state!=State::Queued||c.attempts>=c.max_attempts{
   return Err(Error::NotReady);
  }
  c.state=State::Running;c.attempts+=1;self.cases[index]=Some(c);Ok(())
 }
 pub fn finish(&mut self,trusted:bool,id:u32,exit:i32)->Result<State,Error>{
  if !trusted{return Err(Error::Denied);}
  let index=self.find(id).ok_or(Error::Missing)?;
  let mut c=self.cases[index].ok_or(Error::Missing)?;
  if c.state!=State::Running{return Err(Error::Conflict);}
  if exit==0{c.state=State::Passed;}else{
   c.failures+=1;c.state=if c.attempts>=c.max_attempts{
    State::Failed
   }else{State::Queued};
  }
  let state=c.state;self.cases[index]=Some(c);Ok(state)
 }
 pub fn quarantine(&mut self,who:u32,id:u32)->Result<(),Error>{
  self.auth(who)?;
  let i=self.find(id).ok_or(Error::Missing)?;
  let mut c=self.cases[i].ok_or(Error::Missing)?;
  if c.state==State::Running{return Err(Error::Conflict);}
  c.state=State::Quarantined;self.cases[i]=Some(c);Ok(())
 }
 pub fn release_ready(&self,who:u32)->Result<bool,Error>{
  self.auth(who)?;
  Ok(self.cases.iter().flatten().count()>0&&
   self.cases.iter().flatten().all(|x|x.state==State::Passed))
 }
 pub fn get(&self,who:u32,id:u32)->Result<Case,Error>{
  self.auth(who)?;self.find(id).and_then(|i|self.cases[i]).ok_or(Error::Missing)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn test_runner_retries_then_publishes_real_success_state(){
  let mut s=Suite::<1>::new(7);s.add(7,1,2).unwrap();
  s.start(7,1).unwrap();assert_eq!(s.finish(true,1,1),Ok(State::Queued));
  s.start(7,1).unwrap();assert_eq!(s.finish(true,1,0),Ok(State::Passed));
  assert_eq!(s.get(7,1).unwrap().attempts,2);
 }
 #[test]fn flaky_quarantine_prevents_release_gate_pass(){
  let mut s=Suite::<1>::new(7);s.add(7,1,1).unwrap();
  s.start(7,1).unwrap();s.finish(true,1,9).unwrap();
  s.quarantine(7,1).unwrap();assert_eq!(s.release_ready(7),Ok(false));
 }
 #[test]fn untrusted_test_results_cannot_forge_a_passing_case(){
  let mut s=Suite::<1>::new(7);s.add(7,1,1).unwrap();s.start(7,1).unwrap();
  assert_eq!(s.finish(false,1,0),Err(Error::Denied));
  assert_eq!(s.release_ready(7),Ok(false));
 }
}
