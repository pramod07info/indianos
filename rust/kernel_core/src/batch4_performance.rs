//! R038/39: bounded on-device tick metrics; never fabricate benchmark evidence.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Stage{Boot,Switch,Disk,Desktop,Network}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub struct Stats{pub total:u64,pub samples:u32,pub max:u64}
#[derive(Debug,PartialEq,Eq)]pub enum Error{Invalid,Overflow,NoSamples,OverBudget}
pub struct Monitor{values:[Stats;5]}
impl Monitor{
 pub const fn new()->Self{Self{values:[Stats{total:0,samples:0,max:0};5]}}
 pub fn record(&mut self,stage:Stage,ticks:u64)->Result<(),Error>{
  if ticks==0{return Err(Error::Invalid);}
  let s=&mut self.values[stage as usize];
  let total=s.total.checked_add(ticks).ok_or(Error::Overflow)?;
  let count=s.samples.checked_add(1).ok_or(Error::Overflow)?;
  s.total=total;s.samples=count;s.max=s.max.max(ticks);Ok(())
 }
 pub fn check(&self,stage:Stage,max:u64,mean:u64)->Result<Stats,Error>{
  let s=self.values[stage as usize];
  if s.samples==0{return Err(Error::NoSamples);}
  if s.max>max||s.total/u64::from(s.samples)>mean{return Err(Error::OverBudget);}
  Ok(s)
 }
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn actual_samples_gate_release(){
  let mut m=Monitor::new();assert_eq!(m.check(Stage::Boot,10,10),Err(Error::NoSamples));
  m.record(Stage::Boot,10).unwrap();m.record(Stage::Boot,20).unwrap();
  assert_eq!(m.check(Stage::Boot,20,15).unwrap().samples,2);
  assert_eq!(m.check(Stage::Boot,15,15),Err(Error::OverBudget));
 }
}