//! R030/17: owner-scoped monotonic trusted test outcomes per suite.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Test{pub suite:u32,pub sequence:u64,pub passed:bool}
pub struct Ledger<const N:usize>{owner:u64,last:u64,records:[Option<Test>;N]}
impl<const N:usize> Ledger<N>{
 pub fn new(owner:u64)->Self{Self{owner,last:0,records:[None;N]}}
 pub fn record(&mut self,owner:u64,trusted:bool,test:Test)->Result<(),Error>{
  if owner==0||self.owner!=owner||!trusted{return Err(Error::Denied);}
  if test.sequence<=self.last||test.suite==0{return Err(Error::Stale);}
  let slot=self.records.iter().position(|r|r.is_some_and(|v|v.suite==test.suite))
   .or_else(||self.records.iter().position(Option::is_none)).ok_or(Error::Full)?;
  self.records[slot]=Some(test);self.last=test.sequence;Ok(())
 }
 pub fn green(&self,owner:u64)->Result<bool,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  Ok(self.records.iter().flatten().next().is_some()&&
    self.records.iter().flatten().all(|test|test.passed))
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn newer_verified_success_replaces_previous_failure(){
  let mut ledger=Ledger::<1>::new(9);
  ledger.record(9,true,Test{suite:1,sequence:1,passed:false}).unwrap();
  assert_eq!(ledger.green(9),Ok(false));
  ledger.record(9,true,Test{suite:1,sequence:2,passed:true}).unwrap();
  assert_eq!(ledger.green(9),Ok(true));
 }
 #[test]fn foreign_and_stale_test_results_are_rejected(){
  let mut ledger=Ledger::<1>::new(9);let t=Test{suite:1,sequence:1,passed:true};
  assert_eq!(ledger.record(3,true,t),Err(Error::Denied));
  ledger.record(9,true,t).unwrap();
  assert_eq!(ledger.record(9,true,t),Err(Error::Stale));
 }
}