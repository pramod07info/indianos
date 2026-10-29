//! R030/18: debugger value-change watch requires authenticated fresh snapshots.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Sample{pub address:u64,pub sequence:u64,pub value:i64}
pub struct Watch{owner:u64,latest:Option<Sample>}
impl Watch{
 pub fn new(owner:u64)->Self{Self{owner,latest:None}}
 pub fn observe(&mut self,owner:u64,trusted:bool,sample:Sample)
  ->Result<bool,Error>{
  if owner==0||owner!=self.owner||!trusted{return Err(Error::Denied);}
  if sample.address==0{return Err(Error::Invalid);}
  let changed=if let Some(old)=self.latest{
   if old.address!=sample.address{return Err(Error::Invalid);}
   if sample.sequence<=old.sequence{return Err(Error::Stale);}
   old.value!=sample.value
  }else{false};
  self.latest=Some(sample);Ok(changed)
 }
 pub fn value(&self,owner:u64)->Result<Option<i64>,Error>{
  if owner!=self.owner||owner==0{return Err(Error::Denied);}
  Ok(self.latest.map(|s|s.value))
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn debug_watch_detects_actual_value_transitions(){
  let mut w=Watch::new(5);
  assert_eq!(w.observe(5,true,Sample{address:4096,sequence:1,value:2}),Ok(false));
  assert_eq!(w.observe(5,true,Sample{address:4096,sequence:2,value:3}),Ok(true));
  assert_eq!(w.value(5),Ok(Some(3)));
 }
 #[test]fn stale_debugger_snapshot_and_untrusted_data_are_denied(){
  let mut w=Watch::new(5);let v=Sample{address:1,sequence:1,value:4};
  assert_eq!(w.observe(5,false,v),Err(Error::Denied));
  w.observe(5,true,v).unwrap();
  assert_eq!(w.observe(5,true,v),Err(Error::Stale));
 }
}