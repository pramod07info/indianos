//! R029/15: trusted failure evidence for actual test IDs and output excerpts.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Failure{pub id:u32,pub exit:i32,pub attempt:u8,pub tick:u64,
 pub message:[u8;128],pub len:usize}
pub const EMPTY:Failure=Failure{id:0,exit:0,attempt:0,tick:0,message:[0;128],len:0};
pub struct Reports<const N:usize>{owner:u32,items:[Option<Failure>;N]}
impl<const N:usize> Reports<N>{
 pub const fn new(owner:u32)->Self{Self{owner,items:[None;N]}}
 fn auth(&self,who:u32)->Result<(),Error>{
  if who==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn record(&mut self,who:u32,trusted:bool,id:u32,exit:i32,
 attempt:u8,tick:u64,message:&[u8])->Result<(),Error>{
  self.auth(who)?;
  if !trusted||id==0||exit==0||attempt==0||message.len()>128||
   message.iter().any(|b|*b==0){return Err(Error::Invalid);}
  let i=self.items.iter().position(|x|x.is_some_and(|v|v.id==id))
   .or_else(||self.items.iter().position(Option::is_none)).ok_or(Error::Full)?;
  let mut item=EMPTY;item.id=id;item.exit=exit;item.attempt=attempt;item.tick=tick;
  item.message[..message.len()].copy_from_slice(message);item.len=message.len();
  self.items[i]=Some(item);Ok(())
 }
 pub fn get(&self,who:u32,id:u32)->Result<Failure,Error>{
  self.auth(who)?;
  self.items.iter().flatten().find(|x|x.id==id).copied().ok_or(Error::Missing)
 }
 pub fn release_ready(&self,who:u32)->Result<bool,Error>{
  self.auth(who)?;Ok(self.items.iter().flatten().count()==0)
 }
 pub fn clear_verified(&mut self,who:u32,trusted:bool,id:u32)->Result<(),Error>{
  self.auth(who)?;if !trusted{return Err(Error::Denied);}
  let i=self.items.iter().position(|x|x.is_some_and(|v|v.id==id))
   .ok_or(Error::Missing)?;
  self.items[i]=None;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn failing_test_stores_diagnostic_until_trusted_clear(){
  let mut r=Reports::<1>::new(7);
  r.record(7,true,1,101,2,10,b"assertion failed").unwrap();
  assert_eq!(r.release_ready(7),Ok(false));
  assert_eq!(r.get(7,1).unwrap().exit,101);
  r.clear_verified(7,true,1).unwrap();assert_eq!(r.release_ready(7),Ok(true));
 }
 #[test]fn arbitrary_user_cannot_forge_or_clear_failure(){
  let mut r=Reports::<1>::new(7);
  assert_eq!(r.record(8,true,1,3,1,1,b"bad"),Err(Error::Denied));
  assert_eq!(r.record(7,false,1,3,1,1,b"bad"),Err(Error::Invalid));
 }
}