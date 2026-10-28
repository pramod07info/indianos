//! R021/01: dependency-ordered project manifest/lock view.
//! Declares verified metadata; no arbitrary package execution or network fetching.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Package{pub id:u32,pub version:u64,pub digest:[u8;32],
 pub depends:[u32;4],pub owner:u32}
pub struct Manifest<const N:usize>{owner:u32,packages:[Option<Package>;N],revision:u64}
impl<const N:usize> Manifest<N>{
 pub const fn new(owner:u32)->Self{Self{owner,packages:[None;N],revision:0}}
 fn auth(&self,owner:u32,rev:u64)->Result<(),Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  if rev!=self.revision{return Err(Error::Stale);}Ok(())
 }
 fn index(&self,id:u32)->Option<usize>{
  self.packages.iter().position(|p|p.is_some_and(|v|v.id==id))
 }
 pub fn add(&mut self,owner:u32,rev:u64,p:Package)->Result<u64,Error>{
  self.auth(owner,rev)?;
  if p.owner!=owner||p.id==0||p.version==0||p.digest==[0;32]||
   self.index(p.id).is_some(){return Err(Error::Invalid);}
  for (n,dep) in p.depends.iter().copied().filter(|d|*d!=0).enumerate(){
   if dep==p.id||p.depends[..n].contains(&dep)||self.index(dep).is_none(){
    return Err(Error::Missing);
   }
  }
  let at=self.packages.iter().position(Option::is_none).ok_or(Error::Full)?;
  let next=rev.checked_add(1).ok_or(Error::Overflow)?;
  self.packages[at]=Some(p);self.revision=next;Ok(next)
 }
 pub fn remove_leaf(&mut self,owner:u32,rev:u64,id:u32)->Result<u64,Error>{
  self.auth(owner,rev)?;
  let at=self.index(id).ok_or(Error::Missing)?;
  if self.packages.iter().flatten().any(|p|p.depends.contains(&id)){
   return Err(Error::Conflict);
  }
  let next=rev.checked_add(1).ok_or(Error::Overflow)?;
  self.packages[at]=None;self.revision=next;Ok(next)
 }
 pub fn topological(&self,owner:u32,out:&mut[Package])->Result<usize,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  let count=self.packages.iter().flatten().count();
  if out.len()<count{return Err(Error::Full);}
  let mut written=0;
  for entry in self.packages.iter().flatten(){out[written]=*entry;written+=1;}
  Ok(written)
 }
 pub fn lock_digest(&self,owner:u32)->Result<u64,Error>{
  if owner!=self.owner||owner==0{return Err(Error::Denied);}
  let mut h=0xcbf29ce484222325u64;
  for p in self.packages.iter().flatten(){
   for x in p.id.to_le_bytes().iter().chain(p.version.to_le_bytes().iter())
    .chain(p.digest.iter()){
    h=(h^(*x as u64)).wrapping_mul(0x100000001b3);
   }
  }Ok(h)
 }
 pub fn revision(&self)->u64{self.revision}
}
#[cfg(test)]mod tests{
 use super::*;const ZERO:Package=Package{id:0,version:0,digest:[0;32],depends:[0;4],owner:0};
 fn pack(id:u32,dep:u32)->Package{Package{id,version:1,digest:[id as u8;32],
  depends:[dep,0,0,0],owner:7}}
 #[test]fn packages_follow_dependency_order_and_lock_digest(){
  let mut m=Manifest::<3>::new(7);
  m.add(7,0,pack(1,0)).unwrap();m.add(7,1,pack(2,1)).unwrap();
  let mut items=[ZERO;3];assert_eq!(m.topological(7,&mut items),Ok(2));
  assert_eq!((items[0].id,items[1].id),(1,2));assert_ne!(m.lock_digest(7).unwrap(),0);
 }
 #[test]fn dependency_cannot_be_removed_before_dependents(){
  let mut m=Manifest::<2>::new(7);
  m.add(7,0,pack(1,0)).unwrap();m.add(7,1,pack(2,1)).unwrap();
  assert_eq!(m.remove_leaf(7,2,1),Err(Error::Conflict));
  assert_eq!(m.remove_leaf(7,2,2),Ok(3));
 }
 #[test]fn owner_and_full_output_checks_preserve_existing_data(){
  let mut m=Manifest::<1>::new(7);m.add(7,0,pack(1,0)).unwrap();
  let mut out: [Package;0]=[];
  assert_eq!(m.topological(7,&mut out),Err(Error::Full));
  assert_eq!(m.add(8,1,pack(2,1)),Err(Error::Denied));
  assert_eq!(m.revision(),1);
 }
}
