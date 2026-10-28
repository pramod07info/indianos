//! R021/09 bounded test execution registry with per-test attempts and flaky count.
//! Results require trusted test worker attestation at the call boundary.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Outcome{Pending,Passed,Failed,Skipped}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Case{pub owner:u32,pub run:u32,pub id:u32,
 pub attempts:u8,pub max_attempts:u8,pub outcome:Outcome,
 pub last_error:u16,pub duration_ms:u32,pub flaky:bool}
pub struct Suite<const N:usize>{cases:[Option<Case>;N]}
impl<const N:usize> Suite<N>{
 pub const fn new()->Self{Self{cases:[None;N]}}
 fn locate(&self,owner:u32,run:u32,id:u32)->Option<usize>{
  self.cases.iter().position(|c|c.is_some_and(|c|
    c.owner==owner&&c.run==run&&c.id==id))
 }
 pub fn register(&mut self,owner:u32,run:u32,id:u32,
  max_attempts:u8)->Result<(),Error>{
  if owner==0||run==0||id==0||max_attempts==0{return Err(Error::Invalid);}
  if self.locate(owner,run,id).is_some(){return Err(Error::Conflict);}
  let at=self.cases.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.cases[at]=Some(Case{owner,run,id,max_attempts,attempts:0,
   outcome:Outcome::Pending,last_error:0,duration_ms:0,flaky:false});Ok(())
 }
 pub fn record(&mut self,trusted:bool,owner:u32,run:u32,id:u32,
  passed:bool,code:u16,duration_ms:u32)->Result<Outcome,Error>{
  if !trusted{return Err(Error::Denied);}
  let i=self.locate(owner,run,id).ok_or(Error::Missing)?;
  let mut c=self.cases[i].ok_or(Error::Missing)?;
  if c.outcome!=Outcome::Pending||c.attempts>=c.max_attempts||
   !passed&&code==0{return Err(Error::Conflict);}
  c.attempts+=1;c.duration_ms=duration_ms;c.last_error=if passed{0}else{code};
  c.outcome=if passed{Outcome::Passed}else{Outcome::Failed};
  if passed&&c.attempts>1{c.flaky=true;}
  self.cases[i]=Some(c);Ok(c.outcome)
 }
 pub fn retry(&mut self,owner:u32,run:u32,id:u32)->Result<(),Error>{
  let i=self.locate(owner,run,id).ok_or(Error::Missing)?;
  let mut c=self.cases[i].ok_or(Error::Missing)?;
  if c.outcome!=Outcome::Failed||c.attempts>=c.max_attempts{
   return Err(Error::Conflict);
  }
  c.outcome=Outcome::Pending;self.cases[i]=Some(c);Ok(())
 }
 pub fn skip(&mut self,owner:u32,run:u32,id:u32)->Result<(),Error>{
  let i=self.locate(owner,run,id).ok_or(Error::Missing)?;
  let mut c=self.cases[i].ok_or(Error::Missing)?;
  if c.outcome!=Outcome::Pending{return Err(Error::Conflict);}
  c.outcome=Outcome::Skipped;self.cases[i]=Some(c);Ok(())
 }
 pub fn totals(&self,owner:u32,run:u32)->(usize,usize,usize,usize){
  let mut out=(0,0,0,0);
  for c in self.cases.iter().flatten().filter(|c|c.owner==owner&&c.run==run){
   match c.outcome{
    Outcome::Pending=>out.0+=1,Outcome::Passed=>out.1+=1,
    Outcome::Failed=>out.2+=1,Outcome::Skipped=>out.3+=1
   }
  }out
 }
 pub fn get(&self,owner:u32,run:u32,id:u32)->Result<Case,Error>{
  if owner==0{return Err(Error::Denied);}
  self.locate(owner,run,id).and_then(|i|self.cases[i]).ok_or(Error::Missing)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn repeated_test_passes_after_retry_and_records_flakiness(){
  let mut s=Suite::<1>::new();s.register(7,1,2,2).unwrap();
  s.record(true,7,1,2,false,12,4).unwrap();
  s.retry(7,1,2).unwrap();s.record(true,7,1,2,true,0,2).unwrap();
  assert!(s.get(7,1,2).unwrap().flaky);
  assert_eq!(s.totals(7,1),(0,1,0,0));
 }
 #[test]fn untrusted_worker_cannot_forge_succeeding_tests(){
  let mut s=Suite::<1>::new();s.register(7,1,2,1).unwrap();
  assert_eq!(s.record(false,7,1,2,true,0,1),Err(Error::Denied));
  assert_eq!(s.totals(7,1),(1,0,0,0));
 }
 #[test]fn exhausted_test_retry_does_not_reset_failure(){
  let mut s=Suite::<1>::new();s.register(7,1,2,1).unwrap();
  s.record(true,7,1,2,false,22,3).unwrap();
  assert_eq!(s.retry(7,1,2),Err(Error::Conflict));
  assert_eq!(s.totals(7,1),(0,0,1,0));
 }
}
