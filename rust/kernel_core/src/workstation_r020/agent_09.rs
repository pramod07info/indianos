//! R020/09 build output provenance binds source digest, compiler version
//! and reproducible build flags to artifact identity, without signing it.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Build{pub owner:u32,pub project:u32,pub source:u64,
 pub toolchain:u64,pub flags:u64,pub output:[u8;32],pub sequence:u64,
 pub verified_by_builder:bool}
pub struct Registry<const N:usize>{items:[Option<Build>;N],next:u64}
impl<const N:usize> Registry<N>{
 pub const fn new()->Self{Self{items:[None;N],next:0}}
 pub fn publish(&mut self,owner:u32,project:u32,source:u64,
  toolchain:u64,flags:u64,output:[u8;32],trusted_builder:bool)
  ->Result<u64,Error>{
  if owner==0||project==0||source==0||toolchain==0||
   output==[0;32]||!trusted_builder{return Err(Error::Denied);}
  if self.items.iter().flatten().any(|x|x.owner==owner&&x.project==project&&
   x.source==source&&x.toolchain==toolchain&&x.flags==flags){
   return Err(Error::Conflict);
  }
  let seq=self.next.checked_add(1).ok_or(Error::Overflow)?;
  let at=self.items.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.items[at]=Some(Build{owner,project,source,toolchain,flags,output,
   sequence:seq,verified_by_builder:true});self.next=seq;Ok(seq)
 }
 pub fn match_input(&self,owner:u32,project:u32,source:u64,
  toolchain:u64,flags:u64)->Option<Build>{
  self.items.iter().flatten().find(|b|b.owner==owner&&b.project==project&&
   b.source==source&&b.toolchain==toolchain&&b.flags==flags&&b.verified_by_builder)
   .copied()
 }
 pub fn invalidate_project(&mut self,owner:u32,project:u32)->usize{
  let mut count=0;
  for item in &mut self.items{
   if item.is_some_and(|b|b.owner==owner&&b.project==project){
    *item=None;count+=1;
   }
  }count
 }
 pub fn find_by_output(&self,owner:u32,output:[u8;32])->Option<Build>{
  self.items.iter().flatten().find(|b|b.owner==owner&&b.output==output).copied()
 }
 pub fn snapshot(&self,owner:u32,out:&mut[Build])->Result<usize,Error>{
  if owner==0{return Err(Error::Denied);}
  let n=self.items.iter().flatten().filter(|x|x.owner==owner).count();
  if n>out.len(){return Err(Error::Full);}
  let mut i=0;for a in self.items.iter().flatten(){
   if a.owner==owner{out[i]=*a;i+=1;}
  }Ok(i)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 const EMPTY:Build=Build{owner:0,project:0,source:0,
  toolchain:0,flags:0,output:[0;32],sequence:0,verified_by_builder:false};
 #[test]fn artifact_reuse_requires_identical_toolchain_flags_and_source(){
  let mut r=Registry::<2>::new();
  r.publish(7,1,10,17,1,[8;32],true).unwrap();
  assert!(r.match_input(7,1,10,17,1).is_some());
  assert!(r.match_input(7,1,10,21,1).is_none());
  assert!(r.find_by_output(8,[8;32]).is_none());
 }
 #[test]fn untrusted_and duplicate_build_cannot_be_authoritative(){
  let mut r=Registry::<1>::new();
  assert_eq!(r.publish(7,1,10,17,1,[8;32],false),Err(Error::Denied));
  r.publish(7,1,10,17,1,[8;32],true).unwrap();
  assert_eq!(r.publish(7,1,10,17,1,[9;32],true),Err(Error::Conflict));
 }
 #[test]fn invalidation_is_owner_scoped_and_overflow_output_is_atomic(){
  let mut r=Registry::<2>::new();
  r.publish(7,1,10,17,1,[8;32],true).unwrap();
  r.publish(8,1,10,17,1,[9;32],true).unwrap();
  let mut out: [Build;0]=[];
  assert_eq!(r.snapshot(7,&mut out),Err(Error::Full));
  assert_eq!(r.invalidate_project(7,1),1);
  assert!(r.find_by_output(8,[9;32]).is_some());
 }
}
