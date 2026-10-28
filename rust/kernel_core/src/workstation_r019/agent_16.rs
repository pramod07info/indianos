//! R019/16 authenticated localhost-style preview and revisioned hot reload.
//! This is a preview state controller, not a network listener or browser.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Stopped,Starting,Ready,Reloading,Failed}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Preview{pub owner:u32,pub project:u32,pub build:u64,
 pub port:u16,pub revision:u64,pub state:State,pub last_error:u16}
impl Preview{
 pub fn new(owner:u32,project:u32,port:u16)->Result<Self,Error>{
  if owner==0||project==0||port<1024{return Err(Error::Invalid);}
  Ok(Self{owner,project,port,build:0,revision:0,state:State::Stopped,last_error:0})
 }
 fn check(&self,caller:u32)->Result<(),Error>{
  if caller==0||caller!=self.owner{Err(Error::Denied)}else{Ok(())}
 }
 pub fn start(&mut self,caller:u32,build:u64)->Result<(),Error>{
  self.check(caller)?;
  if build==0||self.state!=State::Stopped{return Err(Error::Conflict);}
  self.build=build;self.state=State::Starting;Ok(())
 }
 pub fn ready(&mut self,supervisor:bool,verified:bool)->Result<(),Error>{
  if !supervisor||!verified{return Err(Error::Denied);}
  if self.state!=State::Starting&&self.state!=State::Reloading{
   return Err(Error::Conflict);
  }
  self.revision=self.revision.checked_add(1).ok_or(Error::Overflow)?;
  self.state=State::Ready;self.last_error=0;Ok(())
 }
 pub fn reload(&mut self,caller:u32,new_build:u64)->Result<(),Error>{
  self.check(caller)?;
  if self.state!=State::Ready||new_build==0||new_build==self.build{
   return Err(Error::Conflict);
  }
  self.build=new_build;self.state=State::Reloading;Ok(())
 }
 pub fn fail(&mut self,supervisor:bool,code:u16)->Result<(),Error>{
  if !supervisor||code==0{return Err(Error::Denied);}
  if self.state!=State::Starting&&self.state!=State::Reloading{
   return Err(Error::Conflict);
  }
  self.state=State::Failed;self.last_error=code;Ok(())
 }
 pub fn stop(&mut self,caller:u32)->Result<(),Error>{
  self.check(caller)?;
  self.state=State::Stopped;self.last_error=0;Ok(())
 }
 pub fn status(&self,caller:u32)->Result<Self,Error>{
  self.check(caller)?;Ok(*self)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn verified_build_is_previewed_and_reloaded(){
  let mut p=Preview::new(1,10,8080).unwrap();
  p.start(1,100).unwrap();p.ready(true,true).unwrap();
  p.reload(1,101).unwrap();p.ready(true,true).unwrap();
  assert_eq!(p.revision,2);assert_eq!(p.build,101);
 }
 #[test]fn unauthorized_guest_cannot_publish_unverified_preview(){
  let mut p=Preview::new(1,10,8080).unwrap();
  assert_eq!(p.start(2,10),Err(Error::Denied));
  p.start(1,10).unwrap();assert_eq!(p.ready(false,true),Err(Error::Denied));
  assert_eq!(p.state,State::Starting);
 }
 #[test]fn failed_reload_keeps_previous_revision_but_reports_error(){
  let mut p=Preview::new(1,10,8080).unwrap();
  p.start(1,10).unwrap();p.ready(true,true).unwrap();
  p.reload(1,20).unwrap();p.fail(true,50).unwrap();
  assert_eq!(p.revision,1);assert_eq!(p.last_error,50);
  p.stop(1).unwrap();assert_eq!(p.state,State::Stopped);
 }
}
