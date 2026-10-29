//! R029/11: per-user terminal command history, safe bounded text, duplicate suppression.
use super::Error;
use crate::batch5_session::Identity;
pub const MAX:usize=160;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Entry{pub text:[u8;MAX],pub len:usize,pub tick:u64}
pub const EMPTY:Entry=Entry{text:[0;MAX],len:0,tick:0};
pub struct History<const N:usize>{owner:Identity,items:[Entry;N],count:usize,next:usize}
impl<const N:usize> History<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,items:[EMPTY;N],count:0,next:0}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn push(&mut self,who:Identity,command:&[u8],tick:u64)->Result<bool,Error>{
  self.auth(who)?;
  if N==0||command.is_empty()||command.len()>MAX||
   command.iter().any(|b|*b==0||*b==b'\n'||*b==0x1b){return Err(Error::Invalid);}
  if self.count>0{
   let last=&self.items[(self.next+N-1)%N];
   if &last.text[..last.len]==command{return Ok(false);}
   if tick<last.tick{return Err(Error::Stale);}
  }
  let mut entry=EMPTY;entry.text[..command.len()].copy_from_slice(command);
  entry.len=command.len();entry.tick=tick;
  self.items[self.next]=entry;self.next=(self.next+1)%N;
  self.count=(self.count+1).min(N);Ok(true)
 }
 pub fn back(&self,who:Identity,offset:usize)->Result<Entry,Error>{
  self.auth(who)?;
  if offset>=self.count{return Err(Error::Missing);}
  Ok(self.items[(self.next+N-1-offset)%N])
 }
 pub fn find(&self,who:Identity,needle:&[u8])->Result<Option<Entry>,Error>{
  self.auth(who)?;
  if needle.is_empty(){return Err(Error::Invalid);}
  for i in 0..self.count{
   let item=self.back(who,i)?;
   if item.text[..item.len].windows(needle.len()).any(|w|w==needle){
    return Ok(Some(item));
   }
  }Ok(None)
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn terminal_history_avoids_duplicate_and_supports_reverse_search(){
  let mut h=History::<2>::new(id(1));
  assert_eq!(h.push(id(1),b"cat /p/a",1),Ok(true));
  assert_eq!(h.push(id(1),b"cat /p/a",2),Ok(false));
  h.push(id(1),b"grep /p/a x",3).unwrap();
  assert!(h.find(id(1),b"grep").unwrap().is_some());
 }
 #[test]fn private_terminal_history_is_denied_to_foreign_generation(){
  let mut h=History::<1>::new(id(1));h.push(id(1),b"cat /p/a",1).unwrap();
  assert_eq!(h.back(id(2),0),Err(Error::Denied));
 }
}