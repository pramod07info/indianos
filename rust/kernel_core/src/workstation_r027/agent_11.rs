//! R027/11: bounded authenticated terminal scrollback with safe log line paging.
use super::Error;
use crate::batch5_session::Identity;
pub const LINE:usize=160;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Entry{pub tick:u64,pub len:usize,pub bytes:[u8;LINE]}
pub const EMPTY:Entry=Entry{tick:0,len:0,bytes:[0;LINE]};
pub struct Scrollback<const N:usize>{owner:Identity,entries:[Option<Entry>;N],
 head:usize,count:usize,last_tick:u64}
impl<const N:usize> Scrollback<N>{
 pub const fn new(owner:Identity)->Self{
  Self{owner,entries:[None;N],head:0,count:0,last_tick:0}
 }
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn append(&mut self,who:Identity,tick:u64,
 line:&[u8])->Result<(),Error>{
  self.auth(who)?;
  if N==0||line.len()>LINE||tick<self.last_tick||
   line.iter().any(|&b|b==0||b==0x1b){return Err(Error::Invalid);}
  let mut e=EMPTY;e.tick=tick;e.len=line.len();
  e.bytes[..line.len()].copy_from_slice(line);
  self.entries[self.head]=Some(e);self.head=(self.head+1)%N;
  self.count=(self.count+1).min(N);self.last_tick=tick;Ok(())
 }
 pub fn newest(&self,who:Identity,back:usize)->Result<Entry,Error>{
  self.auth(who)?;
  if back>=self.count{return Err(Error::Missing);}
  self.entries[(self.head+N-1-back)%N].ok_or(Error::Missing)
 }
 pub fn search(&self,who:Identity,needle:&[u8])->Result<Option<Entry>,Error>{
  self.auth(who)?;
  if needle.is_empty(){return Err(Error::Invalid);}
  for i in 0..self.count{
   let entry=self.newest(who,i)?;
   if entry.bytes[..entry.len].windows(needle.len()).any(|w|w==needle){
    return Ok(Some(entry));
   }
  }Ok(None)
 }
 pub fn clear(&mut self,who:Identity)->Result<(),Error>{
  self.auth(who)?;self.entries=[None;N];self.count=0;self.head=0;Ok(())
 }
 pub fn count(&self,who:Identity)->Result<usize,Error>{
  self.auth(who)?;Ok(self.count)
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn scrollback_searches_real_reported_command_lines(){
  let mut log=Scrollback::<2>::new(id(1));
  log.append(id(1),1,b"build started").unwrap();
  log.append(id(1),2,b"build success").unwrap();
  assert_eq!(log.newest(id(1),0).unwrap().tick,2);
  assert!(log.search(id(1),b"success").unwrap().is_some());
 }
 #[test]fn ring_discards_older_lines_when_capacity_reached(){
  let mut log=Scrollback::<1>::new(id(1));
  log.append(id(1),1,b"first").unwrap();
  log.append(id(1),2,b"second").unwrap();
  assert_eq!(log.count(id(1)),Ok(1));
  assert_eq!(log.search(id(1),b"first"),Ok(None));
 }
 #[test]fn foreign_session_cannot_read_other_developer_output(){
  let mut log=Scrollback::<1>::new(id(1));
  log.append(id(1),1,b"private").unwrap();
  assert_eq!(log.newest(id(2),0),Err(Error::Denied));
 }
}
