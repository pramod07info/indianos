//! R019/18 reproducible toolchain package version/digest locks.
//! This is a local lock resolution plan, not a verified package installer.
use super::Error;
use crate::m2_sdk::{Catalog,Package};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Locked{pub id:u32,pub version:u64,pub digest:[u8;32]}
pub const EMPTY:Locked=Locked{id:0,version:0,digest:[0;32]};
pub struct Lock<const N:usize>{owner:u32,items:[Option<Locked>;N],len:usize}
impl<const N:usize> Lock<N>{
 pub const fn new(owner:u32)->Self{Self{owner,items:[None;N],len:0}}
 fn guard(&self,owner:u32)->Result<(),Error>{
  if owner==0||owner!=self.owner{Err(Error::Denied)}else{Ok(())}
 }
 pub fn resolve<const C:usize>(&mut self,owner:u32,
  catalog:&Catalog<C>,roots:&[u32])->Result<usize,Error>{
  self.guard(owner)?;
  let mut ordered=[0u32;C];
  let count=catalog.resolve_group(roots,&mut ordered).map_err(|_|Error::Invalid)?;
  if count>N{return Err(Error::Full);}
  let mut stage=[None;N];
  for (i,&id) in ordered[..count].iter().enumerate(){
   let Package{version,payload_digest,..}=catalog.get(id).ok_or(Error::Missing)?;
   if version==0||payload_digest==[0;32]{return Err(Error::Denied);}
   stage[i]=Some(Locked{id,version,digest:payload_digest});
  }
  self.items=stage;self.len=count;Ok(count)
 }
 pub fn validate<const C:usize>(&self,owner:u32,
  catalog:&Catalog<C>)->Result<(),Error>{
  self.guard(owner)?;
  for item in self.items[..self.len].iter().flatten(){
   let p=catalog.get(item.id).ok_or(Error::Missing)?;
   if p.version!=item.version||p.payload_digest!=item.digest{
    return Err(Error::Stale);
   }
  }Ok(())
 }
 pub fn snapshot(&self,owner:u32,out:&mut[Locked])->Result<usize,Error>{
  self.guard(owner)?;
  if out.len()<self.len{return Err(Error::Full);}
  for (i,x) in self.items[..self.len].iter().enumerate(){
   out[i]=x.ok_or(Error::Missing)?;
  }Ok(self.len)
 }
 pub fn clear(&mut self,owner:u32)->Result<(),Error>{
  self.guard(owner)?;self.items=[None;N];self.len=0;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;use crate::m2_sdk::ABI;
 fn p(id:u32,deps:[u32;4])->Package{
  Package{id,version:1,abi:ABI,rights:1,deps,payload_digest:[id as u8;32]}}
 #[test]fn locks_dependency_versions_and_digests_deterministically(){
  let mut c=Catalog::<3>::new();c.add(p(1,[0;4])).unwrap();
  c.add(p(2,[1,0,0,0])).unwrap();
  let mut l=Lock::<3>::new(7);
  assert_eq!(l.resolve(7,&c,&[2]),Ok(2));
  let mut out=[EMPTY;3];l.snapshot(7,&mut out).unwrap();
  assert_eq!(out[0].id,1);assert_eq!(out[1].id,2);
  assert_eq!(l.validate(7,&c),Ok(()));
 }
 #[test]fn changed_package_metadata_invalidates_old_lock(){
  let mut c=Catalog::<1>::new();c.add(p(1,[0;4])).unwrap();
  let mut l=Lock::<1>::new(7);l.resolve(7,&c,&[1]).unwrap();
  let mut updated=p(1,[0;4]);updated.version=2;
  c.replace_checked(updated).unwrap();
  assert_eq!(l.validate(7,&c),Err(Error::Stale));
 }
 #[test]fn foreign_user_cannot_read_or_replace_lock_and_short_view_is_atomic(){
  let mut c=Catalog::<1>::new();c.add(p(1,[0;4])).unwrap();
  let mut l=Lock::<1>::new(7);l.resolve(7,&c,&[1]).unwrap();
  assert_eq!(l.resolve(8,&c,&[1]),Err(Error::Denied));
  let mut out: [Locked;0]=[];
  assert_eq!(l.snapshot(7,&mut out),Err(Error::Full));
 }
}
