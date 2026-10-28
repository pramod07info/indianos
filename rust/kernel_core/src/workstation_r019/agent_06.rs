//! R019/06 content-addressed build artifact index with bounded eviction.
//! Indexed digests are cache hints, not signed executable authority.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Artifact{pub owner:u32,pub task:u32,pub inputs:u64,
 pub output_digest:[u8;32],pub tick:u64,pub valid:bool}
pub struct Cache<const N:usize>{entries:[Option<Artifact>;N]}
impl<const N:usize> Cache<N>{
 pub const fn new()->Self{Self{entries:[None;N]}}
 pub fn publish(&mut self,item:Artifact)->Result<(),Error>{
  if item.owner==0||item.task==0||item.inputs==0||
   item.output_digest==[0;32]||!item.valid{return Err(Error::Invalid);}
  if let Some(i)=self.entries.iter().position(|a|a.is_some_and(
   |v|v.owner==item.owner&&v.task==item.task)){
   if self.entries[i].is_some_and(|a|a.tick>item.tick){return Err(Error::Stale);}
   self.entries[i]=Some(item);return Ok(());
  }
  let index=self.entries.iter().position(Option::is_none)
   .or_else(||self.entries.iter().enumerate().min_by_key(
     |(_,e)|e.map_or(u64::MAX,|v|v.tick)).map(|(i,_)|i))
   .ok_or(Error::Full)?;
  self.entries[index]=Some(item);Ok(())
 }
 pub fn lookup(&self,owner:u32,task:u32,inputs:u64)->Option<Artifact>{
  if owner==0{return None;}
  self.entries.iter().flatten().find(|e|
   e.owner==owner&&e.task==task&&e.inputs==inputs&&e.valid).copied()
 }
 pub fn invalidate(&mut self,owner:u32,task:u32)->Result<(),Error>{
  let e=self.entries.iter_mut().flatten().find(|a|a.owner==owner&&a.task==task)
   .ok_or(Error::Missing)?;
  e.valid=false;Ok(())
 }
 pub fn evict_owner(&mut self,owner:u32)->usize{
  if owner==0{return 0;}
  let mut n=0;for entry in &mut self.entries{
   if entry.is_some_and(|v|v.owner==owner){*entry=None;n+=1;}
  }n
 }
 pub fn live(&self,owner:u32)->usize{
  self.entries.iter().flatten().filter(|a|a.owner==owner&&a.valid).count()
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn a(o:u32,t:u32,i:u64,k:u64)->Artifact{
  Artifact{owner:o,task:t,inputs:i,output_digest:[1;32],tick:k,valid:true}}
 #[test]fn exact_source_hash_reuses_only_matching_build(){
  let mut c=Cache::<2>::new();c.publish(a(1,2,10,1)).unwrap();
  assert!(c.lookup(1,2,10).is_some());assert_eq!(c.lookup(1,2,11),None);
  assert_eq!(c.lookup(2,2,10),None);
  c.invalidate(1,2).unwrap();assert_eq!(c.lookup(1,2,10),None);
 }
 #[test]fn evicts_oldest_entry_but_preserves_new_owner_data(){
  let mut c=Cache::<2>::new();
  c.publish(a(1,1,10,1)).unwrap();c.publish(a(2,1,20,2)).unwrap();
  c.publish(a(1,3,30,3)).unwrap();
  assert!(c.lookup(1,1,10).is_none());assert!(c.lookup(2,1,20).is_some());
  assert_eq!(c.evict_owner(1),1);assert_eq!(c.live(2),1);
 }
 #[test]fn replayed_artifact_revision_does_not_replace_newer_cache(){
  let mut c=Cache::<1>::new();c.publish(a(1,1,10,9)).unwrap();
  assert_eq!(c.publish(a(1,1,15,8)),Err(Error::Stale));
  assert!(c.lookup(1,1,10).is_some());
 }
}
