//! R024/13: per-session bounded terminal command history, search and deduplication.
//! Strict session generation isolation; sensitive command forms can be refused.
use super::Error;
use crate::batch5_session::Identity;
pub const LINE:usize=120;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Entry{pub tick:u64,pub len:usize,pub text:[u8;LINE]}
pub const EMPTY:Entry=Entry{tick:0,len:0,text:[0;LINE]};
pub struct History<const N:usize>{owner:Identity,entries:[Option<Entry>;N],
 count:usize,head:usize,last_tick:u64}
impl<const N:usize> History<N>{
 pub const fn new(owner:Identity)->Self{
  Self{owner,entries:[None;N],count:0,head:0,last_tick:0}
 }
 fn auth(&self,owner:Identity)->Result<(),Error>{
  if owner.pid==0||owner!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn record(&mut self,owner:Identity,tick:u64,
  command:&[u8])->Result<(),Error>{
  self.auth(owner)?;
  if N==0||command.is_empty()||command.len()>LINE{return Err(Error::Full);}
  if tick<self.last_tick||command.iter().any(|x|*x==0||*x==b'\n'||*x==0x1b){
   return Err(Error::Invalid);
  }
  if command.windows(6).any(|w|w==b"--pass")||
   command.windows(6).any(|w|w==b"SECRET"){return Err(Error::Denied);}
  if self.count>0{
   let prev=(self.head+N-1)%N;
   if self.entries[prev].is_some_and(|e|&e.text[..e.len]==command){
    return Ok(());
   }
  }
  let mut item=EMPTY;item.tick=tick;item.len=command.len();
  item.text[..command.len()].copy_from_slice(command);
  self.entries[self.head]=Some(item);
  self.head=(self.head+1)%N;self.count=(self.count+1).min(N);
  self.last_tick=tick;Ok(())
 }
 pub fn newest(&self,owner:Identity,back:usize)->Result<Entry,Error>{
  self.auth(owner)?;
  if back>=self.count{return Err(Error::Missing);}
  let index=(self.head+N-1-back)%N;
  self.entries[index].ok_or(Error::Missing)
 }
 pub fn search(&self,owner:Identity,needle:&[u8])->Result<Option<Entry>,Error>{
  self.auth(owner)?;
  if needle.is_empty(){return Err(Error::Invalid);}
  for back in 0..self.count{
   let e=self.newest(owner,back)?;
   if e.text[..e.len].windows(needle.len()).any(|w|w==needle){
    return Ok(Some(e));
   }
  }Ok(None)
 }
 pub fn clear(&mut self,owner:Identity)->Result<(),Error>{
  self.auth(owner)?;
  self.entries=[None;N];self.head=0;self.count=0;Ok(())
 }
 pub fn len(&self,owner:Identity)->Result<usize,Error>{
  self.auth(owner)?;Ok(self.count)
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn terminal_command_history_returns_newest_and_matching_source_search(){
  let mut h=History::<2>::new(id(1));
  h.record(id(1),1,b"ls /src").unwrap();
  h.record(id(1),2,b"cat /src/a").unwrap();
  assert_eq!(&h.newest(id(1),0).unwrap().text[..10],b"cat /src/a");
  assert!(h.search(id(1),b"ls").unwrap().is_some());
 }
 #[test]fn duplicate_commands_and_sensitive_flags_do_not_fill_history(){
  let mut h=History::<1>::new(id(1));
  h.record(id(1),1,b"pwd").unwrap();h.record(id(1),2,b"pwd").unwrap();
  assert_eq!(h.len(id(1)),Ok(1));
  assert_eq!(h.record(id(1),3,b"curl --pass secret"),Err(Error::Denied));
 }
 #[test]fn recycled_pid_generation_cannot_view_other_session_history(){
  let mut h=History::<1>::new(id(1));h.record(id(1),1,b"pwd").unwrap();
  assert_eq!(h.newest(id(2),0),Err(Error::Denied));
 }
}
