//! R022/11: multi-architecture build-target DAG with ABI/edition constraints.
//! Plans native compile graph; a trusted toolchain worker executes actual builds.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Arch{X86_64,Aarch64,Riscv64}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Edition{Desktop,Developer,Server}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Queued,Running,Passed,Failed}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Target{pub id:u32,pub parent:u32,pub arch:Arch,pub edition:Edition,
 pub toolchain:u64,pub abi:u16,pub state:State,pub result:u16}
pub struct Matrix<const N:usize>{owner:u32,targets:[Option<Target>;N]}
impl<const N:usize> Matrix<N>{
 pub const fn new(owner:u32)->Self{Self{owner,targets:[None;N]}}
 fn index(&self,id:u32)->Option<usize>{
  self.targets.iter().position(|t|t.is_some_and(|x|x.id==id))
 }
 fn auth(&self,owner:u32)->Result<(),Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn add(&mut self,owner:u32,mut target:Target)->Result<(),Error>{
  self.auth(owner)?;
  if target.id==0||target.parent==target.id||target.toolchain==0||
   target.abi==0||self.index(target.id).is_some(){
   return Err(Error::Invalid);
  }
  if target.parent!=0&&self.index(target.parent).is_none(){
   return Err(Error::Missing);
  }
  let at=self.targets.iter().position(Option::is_none).ok_or(Error::Full)?;
  target.state=State::Queued;target.result=0;
  self.targets[at]=Some(target);Ok(())
 }
 pub fn start(&mut self,owner:u32,id:u32,verified_abi:u16,
  verified_toolchain:u64)->Result<(),Error>{
  self.auth(owner)?;
  let i=self.index(id).ok_or(Error::Missing)?;
  let mut t=self.targets[i].ok_or(Error::Missing)?;
  if t.state!=State::Queued||t.abi>verified_abi||
   t.toolchain!=verified_toolchain{return Err(Error::Denied);}
  if t.parent!=0&&!self.index(t.parent).is_some_and(|p|
    self.targets[p].is_some_and(|p|p.state==State::Passed)){
   return Err(Error::NotReady);
  }
  t.state=State::Running;self.targets[i]=Some(t);Ok(())
 }
 pub fn finish(&mut self,owner:u32,id:u32,success:bool,
  code:u16)->Result<(),Error>{
  self.auth(owner)?;
  let i=self.index(id).ok_or(Error::Missing)?;
  let mut t=self.targets[i].ok_or(Error::Missing)?;
  if t.state!=State::Running||!success&&code==0{return Err(Error::Conflict);}
  t.state=if success{State::Passed}else{State::Failed};
  t.result=code;self.targets[i]=Some(t);Ok(())
 }
 pub fn all_passed(&self,owner:u32)->Result<bool,Error>{
  self.auth(owner)?;
  Ok(self.targets.iter().flatten().count()>0&&
    self.targets.iter().flatten().all(|t|t.state==State::Passed))
 }
 pub fn get(&self,owner:u32,id:u32)->Result<Target,Error>{
  self.auth(owner)?;
  self.index(id).and_then(|i|self.targets[i]).ok_or(Error::Missing)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn target(id:u32,parent:u32,edition:Edition)->Target{
  Target{id,parent,arch:Arch::X86_64,edition,toolchain:21,abi:1,
   state:State::Queued,result:0}
 }
 #[test]fn desktop_developer_server_graph_respects_parent_build_gate(){
  let mut m=Matrix::<3>::new(7);
  m.add(7,target(1,0,Edition::Desktop)).unwrap();
  m.add(7,target(2,1,Edition::Developer)).unwrap();
  m.add(7,target(3,2,Edition::Server)).unwrap();
  assert_eq!(m.start(7,3,1,21),Err(Error::NotReady));
  for id in 1..=3{m.start(7,id,1,21).unwrap();m.finish(7,id,true,0).unwrap();}
  assert_eq!(m.all_passed(7),Ok(true));
 }
 #[test]fn wrong_toolchain_abi_and_foreign_owner_cannot_start(){
  let mut m=Matrix::<1>::new(7);m.add(7,target(1,0,Edition::Developer)).unwrap();
  assert_eq!(m.start(7,1,0,21),Err(Error::Denied));
  assert_eq!(m.start(7,1,1,17),Err(Error::Denied));
  assert_eq!(m.start(8,1,1,21),Err(Error::Denied));
 }
 #[test]fn failed_source_target_blocks_unbuilt_downstream_edition(){
  let mut m=Matrix::<2>::new(7);
  m.add(7,target(1,0,Edition::Desktop)).unwrap();
  m.add(7,target(2,1,Edition::Server)).unwrap();
  m.start(7,1,1,21).unwrap();m.finish(7,1,false,10).unwrap();
  assert_eq!(m.start(7,2,1,21),Err(Error::NotReady));
  assert_eq!(m.all_passed(7),Ok(false));
 }
}
