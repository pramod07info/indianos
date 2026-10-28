//! R023/09: verified source dependency graph and deterministic build order.
//! No external package resolution or dependency execution occurs here.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Unit{pub id:u32,pub abi:u16,pub toolchain:u64,
 pub deps:[u32;6],pub source_hash:u64,pub enabled:bool}
pub struct Graph<const N:usize>{owner:u32,units:[Option<Unit>;N]}
impl<const N:usize> Graph<N>{
 pub const fn new(owner:u32)->Self{Self{owner,units:[None;N]}}
 fn find(&self,id:u32)->Option<usize>{
  self.units.iter().position(|v|v.is_some_and(|u|u.id==id))
 }
 fn auth(&self,owner:u32)->Result<(),Error>{
  if owner!=self.owner||owner==0{return Err(Error::Denied);}Ok(())
 }
 pub fn add(&mut self,owner:u32,unit:Unit)->Result<(),Error>{
  self.auth(owner)?;
  if unit.id==0||unit.abi==0||unit.toolchain==0||unit.source_hash==0||
   self.find(unit.id).is_some(){return Err(Error::Invalid);}
  for (i,d) in unit.deps.iter().copied().enumerate(){
   if d==0{continue;}
   if d==unit.id||unit.deps[..i].contains(&d)||self.find(d).is_none(){
    return Err(Error::Missing);
   }
  }
  let at=self.units.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.units[at]=Some(unit);Ok(())
 }
 pub fn build_order(&self,owner:u32,out:&mut[Unit])->Result<usize,Error>{
  self.auth(owner)?;
  let count=self.units.iter().flatten().filter(|u|u.enabled).count();
  if count>out.len(){return Err(Error::Full);}
  let mut staged=[Unit{id:0,abi:0,toolchain:0,deps:[0;6],
   source_hash:0,enabled:false};N];
  let mut n=0;
  for u in self.units.iter().flatten().filter(|u|u.enabled){
   if u.deps.iter().any(|d|*d!=0&&self.find(*d).is_some_and(|i|
    !self.units[i].is_some_and(|d|d.enabled))){
    return Err(Error::NotReady);
   }
   staged[n]=*u;n+=1;
  }
  out[..n].copy_from_slice(&staged[..n]);Ok(n)
 }
 pub fn reverse_dependents(&self,owner:u32,id:u32)->Result<usize,Error>{
  self.auth(owner)?;
  if self.find(id).is_none(){return Err(Error::Missing);}
  Ok(self.units.iter().flatten().filter(|u|u.deps.contains(&id)).count())
 }
 pub fn invalidate(&mut self,owner:u32,id:u32)->Result<usize,Error>{
  self.auth(owner)?;
  let at=self.find(id).ok_or(Error::Missing)?;
  let mut count=0;
  if let Some(mut unit)=self.units[at]{unit.enabled=false;self.units[at]=Some(unit);count+=1;}
  for _ in 0..N{
   let mut changed=false;
   for i in 0..N{
    let Some(mut unit)=self.units[i]else{continue};
    if unit.enabled&&unit.deps.iter().any(|d|*d!=0&&self.find(*d)
      .is_some_and(|j|!self.units[j].is_some_and(|v|v.enabled))){
     unit.enabled=false;self.units[i]=Some(unit);count+=1;changed=true;
    }
   }
   if !changed{break;}
  }Ok(count)
 }
 pub fn count(&self,owner:u32)->Result<usize,Error>{
  self.auth(owner)?;Ok(self.units.iter().flatten().filter(|x|x.enabled).count())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn unit(id:u32,dep:u32)->Unit{Unit{id,abi:1,toolchain:21,
  deps:[dep,0,0,0,0,0],source_hash:id as u64,enabled:true}}
 #[test]fn source_change_invalidates_transitive_dependent_targets(){
  let mut g=Graph::<3>::new(7);
  g.add(7,unit(1,0)).unwrap();g.add(7,unit(2,1)).unwrap();
  g.add(7,unit(3,2)).unwrap();assert_eq!(g.invalidate(7,1),Ok(3));
  assert_eq!(g.count(7),Ok(0));
 }
 #[test]fn source_build_order_requires_enabled_dependency_versions(){
  let mut g=Graph::<2>::new(7);g.add(7,unit(1,0)).unwrap();
  g.add(7,unit(2,1)).unwrap();let mut out=[unit(0,0);2];
  assert_eq!(g.build_order(7,&mut out),Ok(2));
  assert_eq!((out[0].id,out[1].id),(1,2));
 }
 #[test]fn unknown_or_foreign_package_dependency_cannot_leak_graph(){
  let mut g=Graph::<1>::new(7);
  assert_eq!(g.add(7,unit(2,1)),Err(Error::Missing));
  assert_eq!(g.add(8,unit(1,0)),Err(Error::Denied));
 }
}
