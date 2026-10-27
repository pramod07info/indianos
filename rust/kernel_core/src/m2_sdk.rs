//! M2 developer-alpha native application ABI and dependency planner.
//! This is a versioned manifest/planner for TRUSTED catalog entries, not a
//! signature checker, runnable ELF loader, full SDK or package downloader.
pub const ABI:u16=1;
pub const MAX_APPS:usize=16;
pub const MAX_DEPS:usize=4;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Package{
 pub id:u32,pub version:u64,pub abi:u16,
 pub rights:u32,pub deps:[u32;MAX_DEPS],
 pub payload_digest:[u8;32],
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Duplicate,Missing,Cycle,Capacity,UnsupportedAbi,InUse}
pub struct Catalog<const N:usize>{items:[Option<Package>;N]}
impl<const N:usize> Catalog<N>{
 pub const fn new()->Self{Self{items:[None;N]}}
 pub fn add(&mut self,item:Package)->Result<(),Error>{
  if item.id==0||item.id>99||item.version==0||item.payload_digest==[0;32]||
   item.deps.contains(&item.id)||
   item.deps.iter().any(|&dep|dep>99)||
   item.rights&!crate::batch6_security::ALL!=0||
   item.deps.iter().enumerate().any(|(i,dep)|*dep!=0&&item.deps[..i].contains(dep)){
   return Err(Error::Invalid);
  }
  if item.abi!=ABI{return Err(Error::UnsupportedAbi);}
  if self.items.iter().flatten().any(|p|p.id==item.id){return Err(Error::Duplicate);}
  let i=self.items.iter().position(Option::is_none).ok_or(Error::Capacity)?;
  self.items[i]=Some(item);Ok(())
 }
 pub fn get(&self,id:u32)->Option<Package>{
  self.items.iter().flatten().find(|p|p.id==id).copied()
 }
 /// Topological ordering: dependencies always precede dependent app.
 /// Does not trust unverified manifest strings or install any package.
 pub fn resolve(&self,id:u32,out:&mut[u32])->Result<usize,Error>{
  // Compute the entire dependency closure before publishing any entry.
  // A malformed/missing/cyclic/oversized plan cannot partially install.
  let mut visited=[0u8;N];let mut len=0usize;
  let mut staged=[0u32;N];
  self.visit(id,&mut visited,&mut staged,&mut len)?;
  if len>out.len(){return Err(Error::Capacity);}
  out[..len].copy_from_slice(&staged[..len]);
  Ok(len)
 }
 /// Remove a catalog entry only when no registered application depends
 /// on it. This is purely trusted catalog state; persisted app-store records
 /// and on-disk binaries still require transactional installer integration.
 pub fn remove_unreferenced(&mut self,id:u32)->Result<(),Error>{
  let i=self.items.iter().position(|p|p.is_some_and(|x|x.id==id))
   .ok_or(Error::Missing)?;
  if self.items.iter().flatten().any(|p|
      p.id!=id&&p.deps.contains(&id)){
   return Err(Error::InUse);
  }
  self.items[i]=None;
  Ok(())
 }
 fn visit(&self,id:u32,state:&mut[u8;N],out:&mut[u32],len:&mut usize)
  ->Result<(),Error>{
  let i=self.items.iter().position(|p|p.is_some_and(|x|x.id==id))
   .ok_or(Error::Missing)?;
  if state[i]==1{return Err(Error::Cycle);}
  if state[i]==2{return Ok(());}
  state[i]=1;
  let p=self.items[i].ok_or(Error::Missing)?;
  for dep in p.deps.into_iter().filter(|n|*n!=0){
   self.visit(dep,state,out,len)?;
  }
  if *len>=out.len(){return Err(Error::Capacity);}
  out[*len]=id;*len+=1;state[i]=2;
  Ok(())
 }
}
#[cfg(test)]
mod m2_tests{
 use super::*;
 fn pkg(id:u32,deps:[u32;MAX_DEPS])->Package{
  Package{id,version:1,abi:ABI,rights:0,deps,payload_digest:[id as u8;32]}
 }
 #[test] fn m2_dependency_plan_order_and_dedup(){
  let mut c=Catalog::<4>::new();
  c.add(pkg(1,[2,3,0,0])).unwrap();
  c.add(pkg(2,[4,0,0,0])).unwrap();
  c.add(pkg(3,[4,0,0,0])).unwrap();
  c.add(pkg(4,[0;4])).unwrap();
  let mut plan=[0u32;4];assert_eq!(c.resolve(1,&mut plan),Ok(4));
  assert_eq!(plan,[4,2,3,1]);
  assert_eq!(c.resolve(1,&mut plan[..2]),Err(Error::Capacity));
 }
 #[test] fn m2_cycle_missing_invalid_and_wrong_abi_rejected(){
  let mut c=Catalog::<3>::new();c.add(pkg(1,[2,0,0,0])).unwrap();
  c.add(pkg(2,[1,0,0,0])).unwrap();
  assert_eq!(c.resolve(1,&mut[0;3]),Err(Error::Cycle));
  assert_eq!(c.resolve(9,&mut[0;3]),Err(Error::Missing));
  assert_eq!(c.add(pkg(1,[0;4])),Err(Error::Duplicate));
  assert_eq!(c.add(pkg(3,[3,0,0,0])),Err(Error::Invalid));
  assert_eq!(c.add(pkg(3,[4,4,0,0])),Err(Error::Invalid));
  let mut bad=pkg(3,[0;4]);bad.abi=999;
  assert_eq!(c.add(bad),Err(Error::UnsupportedAbi));
 }

 #[test]fn invalid_or_undersized_dependency_plan_is_atomic(){
  let mut c=Catalog::<4>::new();
  c.add(pkg(1,[2,3,0,0])).unwrap();
  c.add(pkg(2,[0;4])).unwrap();
  c.add(pkg(3,[4,0,0,0])).unwrap();
  let mut out=[77u32;4];
  assert_eq!(c.resolve(1,&mut out),Err(Error::Missing));
  assert_eq!(out,[77;4]);
  c.add(pkg(4,[0;4])).unwrap();
  let mut short=[77u32;3];
  assert_eq!(c.resolve(1,&mut short),Err(Error::Capacity));
  assert_eq!(short,[77;3]);
  assert_eq!(c.resolve(1,&mut out),Ok(4));
  assert_eq!(out,[2,4,3,1]);
 }
 #[test]fn depended_on_package_cannot_be_uninstalled(){
  let mut c=Catalog::<3>::new();
  c.add(pkg(1,[2,0,0,0])).unwrap();
  c.add(pkg(2,[0;4])).unwrap();
  assert_eq!(c.remove_unreferenced(2),Err(Error::InUse));
  assert!(c.get(2).is_some());
  c.remove_unreferenced(1).unwrap();
  c.remove_unreferenced(2).unwrap();
  assert_eq!(c.get(2),None);
  assert_eq!(c.remove_unreferenced(2),Err(Error::Missing));
  c.add(pkg(3,[0;4])).unwrap();
  assert!(c.get(3).is_some());
 }
 #[test]fn cycle_detection_does_not_publish_partial_install_plan(){
  let mut c=Catalog::<3>::new();
  c.add(pkg(1,[2,0,0,0])).unwrap();
  c.add(pkg(2,[1,0,0,0])).unwrap();
  let mut out=[88u32;3];
  assert_eq!(c.resolve(1,&mut out),Err(Error::Cycle));
  assert_eq!(out,[88;3]);
 }

}
