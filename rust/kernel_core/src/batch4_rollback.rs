//! R038/34: transactional install rollback policy, without real disk writes.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum State{Clean,Staged,Active,Healthy}
#[derive(Debug,PartialEq,Eq)]pub enum Error{Invalid,WrongState,NoFallback}
pub struct Journal{state:State,active:u32,old:u32,pending:u32,tries:u8}
impl Journal{
 pub const fn new(active:u32)->Self{Self{state:State::Clean,active,old:0,pending:0,tries:0}}
 pub fn active(&self)->u32{self.active}
 pub fn stage(&mut self,image:u32,verified:bool)->Result<(),Error>{
  if self.state!=State::Clean||image==0||image==self.active||!verified{return Err(Error::Invalid);}
  self.old=self.active;self.pending=image;self.state=State::Staged;Ok(())
 }
 pub fn activate(&mut self,durable:bool,tries:u8)->Result<(),Error>{
  if self.state!=State::Staged||!durable||tries==0{return Err(Error::WrongState);}
  self.active=self.pending;self.tries=tries;self.state=State::Active;Ok(())
 }
 pub fn boot_attempt(&mut self)->Result<(),Error>{
  if self.state!=State::Active||self.tries==0{return Err(Error::WrongState);}
  self.tries-=1;Ok(())
 }
 pub fn confirm(&mut self,healthy:bool)->Result<(),Error>{
  if self.state!=State::Active||!healthy{return Err(Error::WrongState);}
  self.old=0;self.state=State::Healthy;Ok(())
 }
 pub fn rollback(&mut self)->Result<(),Error>{
  if self.old==0||self.state==State::Clean||self.state==State::Healthy{return Err(Error::NoFallback);}
  self.active=self.old;self.old=0;self.pending=0;self.tries=0;self.state=State::Clean;Ok(())
 }
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn failed_image_restores_last_good_image(){
  let mut j=Journal::new(10);j.stage(11,true).unwrap();j.activate(true,1).unwrap();
  j.boot_attempt().unwrap();assert!(j.boot_attempt().is_err());j.rollback().unwrap();
  assert_eq!(j.active(),10);
 }
}