//! R027/15: owner-scoped trusted test result registry with failure summaries.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Status{Queued,Running,Passed,Failed,Skipped}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Case{pub id:u32,pub duration_ms:u32,pub attempts:u8,pub status:Status}
pub struct Results<const N:usize>{owner:u32,cases:[Option<Case>;N]}
impl<const N:usize> Results<N>{
 pub const fn new(owner:u32)->Self{Self{owner,cases:[None;N]}}
 fn auth(&self,who:u32)->Result<(),Error>{
  if who==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn find(&self,id:u32)->Option<usize>{
  self.cases.iter().position(|c|c.is_some_and(|x|x.id==id))
 }
 pub fn register(&mut self,who:u32,id:u32)->Result<(),Error>{
  self.auth(who)?;
  if id==0||self.find(id).is_some(){return Err(Error::Invalid);}
  let at=self.cases.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.cases[at]=Some(Case{id,duration_ms:0,attempts:0,status:Status::Queued});Ok(())
 }
 pub fn run(&mut self,who:u32,id:u32)->Result<(),Error>{
  self.auth(who)?;
  let at=self.find(id).ok_or(Error::Missing)?;
  let mut case=self.cases[at].ok_or(Error::Missing)?;
  if !matches!(case.status,Status::Queued|Status::Failed){return Err(Error::NotReady);}
  case.status=Status::Running;
  case.attempts=case.attempts.checked_add(1).ok_or(Error::Overflow)?;
  self.cases[at]=Some(case);Ok(())
 }
 pub fn report(&mut self,trusted:bool,id:u32,exit:i32,
  duration_ms:u32)->Result<(),Error>{
  if !trusted{return Err(Error::Denied);}
  let at=self.find(id).ok_or(Error::Missing)?;
  let mut case=self.cases[at].ok_or(Error::Missing)?;
  if case.status!=Status::Running{return Err(Error::Conflict);}
  case.duration_ms=duration_ms;
  case.status=if exit==0{Status::Passed}else{Status::Failed};
  self.cases[at]=Some(case);Ok(())
 }
 pub fn summary(&self,who:u32)->Result<(usize,usize,usize),Error>{
  self.auth(who)?;
  let mut pass=0;let mut fail=0;let mut other=0;
  for c in self.cases.iter().flatten(){
   match c.status{
    Status::Passed=>pass+=1,
    Status::Failed=>fail+=1,
    _=>other+=1
   }
  }Ok((pass,fail,other))
 }
 pub fn release_ready(&self,who:u32)->Result<bool,Error>{
  let (passed,failed,other)=self.summary(who)?;
  Ok(passed>0&&failed==0&&other==0)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn accepted_runner_reports_real_test_results_and_duration(){
  let mut r=Results::<2>::new(7);r.register(7,1).unwrap();
  r.run(7,1).unwrap();r.report(true,1,0,120).unwrap();
  assert_eq!(r.summary(7),Ok((1,0,0)));
  assert_eq!(r.release_ready(7),Ok(true));
 }
 #[test]fn failed_test_blocks_release_even_if_another_passes(){
  let mut r=Results::<2>::new(7);
  r.register(7,1).unwrap();r.register(7,2).unwrap();
  r.run(7,1).unwrap();r.report(true,1,0,1).unwrap();
  r.run(7,2).unwrap();r.report(true,2,9,1).unwrap();
  assert_eq!(r.release_ready(7),Ok(false));
 }
 #[test]fn untrusted_worker_cannot_claim_green_test_run(){
  let mut r=Results::<1>::new(7);r.register(7,1).unwrap();r.run(7,1).unwrap();
  assert_eq!(r.report(false,1,0,1),Err(Error::Denied));
 }
}
