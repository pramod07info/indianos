//! R021/03: bounded filesystem change journal with owner-bound watch prefixes.
//! Events require a trusted filesystem publisher, not client-supplied claims.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::PATH_MAX;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind{Create,Edit,Rename,Delete}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Event{pub seq:u64,pub version:u64,pub kind:Kind,pub len:usize,
 pub path:[u8;PATH_MAX]}
pub const EMPTY:Event=Event{seq:0,version:0,kind:Kind::Edit,len:0,path:[0;PATH_MAX]};
pub struct Watch<const N:usize>{owner:Identity,prefix:[u8;PATH_MAX],
 length:usize,events:[Option<Event>;N],seq:u64,next:usize}
impl<const N:usize> Watch<N>{
 pub fn new(owner:Identity,prefix:&[u8])->Result<Self,Error>{
  if owner.pid==0||owner.generation==0||prefix.is_empty()||
   prefix.len()>PATH_MAX||prefix[0]!=b'/'||prefix.windows(2).any(|w|w==b".."){
   return Err(Error::Invalid);
  }
  let mut p=[0u8;PATH_MAX];p[..prefix.len()].copy_from_slice(prefix);
  Ok(Self{owner,prefix:p,length:prefix.len(),events:[None;N],seq:0,next:0})
 }
 fn guard(&self,actor:Identity)->Result<(),Error>{
  if actor!=self.owner||actor.pid==0{return Err(Error::Denied);}Ok(())
 }
 pub fn publish(&mut self,trusted:bool,path:&[u8],version:u64,
   kind:Kind)->Result<Option<u64>,Error>{
  if !trusted||version==0{return Err(Error::Denied);}
  if path.len()>PATH_MAX||path.is_empty(){return Err(Error::Invalid);}
  let prefix=&self.prefix[..self.length];
  if !path.starts_with(prefix)||self.length>1&&path.len()>self.length&&
   path[self.length]!=b'/'{return Ok(None);}
  if N==0{return Err(Error::Full);}
  let seq=self.seq.checked_add(1).ok_or(Error::Overflow)?;
  let mut name=[0;PATH_MAX];name[..path.len()].copy_from_slice(path);
  self.events[self.next]=Some(Event{seq,version,kind,len:path.len(),path:name});
  self.next=(self.next+1)%N;self.seq=seq;Ok(Some(seq))
 }
 pub fn since(&self,actor:Identity,after:u64,out:&mut[Event])
  ->Result<usize,Error>{
  self.guard(actor)?;
  let count=self.events.iter().flatten().filter(|e|e.seq>after).count();
  if count>out.len(){return Err(Error::Full);}
  let mut written=0;
  for e in self.events.iter().flatten().filter(|e|e.seq>after){
   out[written]=*e;written+=1;
  }out[..written].sort_unstable_by_key(|e|e.seq);Ok(written)
 }
 pub fn latest(&self,actor:Identity)->Result<u64,Error>{
  self.guard(actor)?;Ok(self.seq)
 }
 pub fn clear(&mut self,actor:Identity)->Result<(),Error>{
  self.guard(actor)?;self.events=[None;N];Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn subscribed_editor_receives_only_its_project_file_changes(){
  let mut w=Watch::<3>::new(id(1),b"/src").unwrap();
  assert_eq!(w.publish(true,b"/other/a",1,Kind::Edit),Ok(None));
  w.publish(true,b"/src/main.rs",2,Kind::Edit).unwrap();
  let mut events=[EMPTY;3];assert_eq!(w.since(id(1),0,&mut events),Ok(1));
  assert_eq!(&events[0].path[..events[0].len],b"/src/main.rs");
 }
 #[test]fn untrusted_and_recycled_process_cannot_inject_or_read(){
  let mut w=Watch::<2>::new(id(1),b"/").unwrap();
  assert_eq!(w.publish(false,b"/a",1,Kind::Delete),Err(Error::Denied));
  assert_eq!(w.since(id(2),0,&mut [EMPTY;2]),Err(Error::Denied));
 }
 #[test]fn short_destination_does_not_publish_partial_change_feed(){
  let mut w=Watch::<2>::new(id(1),b"/").unwrap();
  w.publish(true,b"/a",1,Kind::Create).unwrap();
  w.publish(true,b"/b",2,Kind::Edit).unwrap();
  let mut out=[EMPTY;1];assert_eq!(w.since(id(1),0,&mut out),Err(Error::Full));
  assert_eq!(out,[EMPTY]);
 }
}
