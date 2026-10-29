//! R025/09: bounded, owner-specific real file revision change journal.
//! Records validated current RamFs revisions, not synthetic user interface events.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind{Created,Updated,Deleted}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Event{pub path:[u8;PATH_MAX],pub len:usize,pub before:u64,
 pub after:u64,pub seq:u64,pub kind:Kind}
pub const EMPTY:Event=Event{path:[0;PATH_MAX],len:0,before:0,
 after:0,seq:0,kind:Kind::Updated};
pub struct Watch<const N:usize>{owner:Identity,items:[Option<Event>;N],
 head:usize,sequence:u64}
impl<const N:usize> Watch<N>{
 pub const fn new(owner:Identity)->Self{
  Self{owner,items:[None;N],head:0,sequence:0}
 }
 fn auth(&self,owner:Identity)->Result<(),Error>{
  if owner.pid==0||owner!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn observe<const F:usize>(&mut self,owner:Identity,fs:&RamFs<F>,
  path:&[u8],before:u64)->Result<u64,Error>{
  self.auth(owner)?;
  if N==0||path.len()<2||path.len()>PATH_MAX{return Err(Error::Invalid);}
  let after=fs.metadata(path).ok().map(|x|x.0).unwrap_or(0);
  if before==after{return Err(Error::Conflict);}
  let kind=if before==0{Kind::Created}else if after==0{
   Kind::Deleted
  }else{Kind::Updated};
  let sequence=self.sequence.checked_add(1).ok_or(Error::Overflow)?;
  let mut p=[0u8;PATH_MAX];p[..path.len()].copy_from_slice(path);
  self.items[self.head]=Some(Event{path:p,len:path.len(),before,
   after,seq:sequence,kind});
  self.head=(self.head+1)%N;self.sequence=sequence;Ok(sequence)
 }
 pub fn since(&self,owner:Identity,after:u64,out:&mut[Event])
  ->Result<usize,Error>{
  self.auth(owner)?;
  let count=self.items.iter().flatten().filter(|e|e.seq>after).count();
  if count>out.len(){return Err(Error::Full);}
  let mut n=0;for e in self.items.iter().flatten().filter(|e|e.seq>after){
   out[n]=*e;n+=1;
  }
  out[..n].sort_unstable_by_key(|e|e.seq);Ok(n)
 }
 pub fn latest(&self,owner:Identity)->Result<u64,Error>{
  self.auth(owner)?;Ok(self.sequence)
 }
 pub fn clear(&mut self,owner:Identity)->Result<(),Error>{
  self.auth(owner)?;self.items=[None;N];self.head=0;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn actual_file_create_update_and_delete_emit_ordered_events(){
  let mut fs=RamFs::<1>::new();let mut w=Watch::<3>::new(id(1));
  fs.replace(b"/a",b"a").unwrap();w.observe(id(1),&fs,b"/a",0).unwrap();
  fs.replace(b"/a",b"b").unwrap();w.observe(id(1),&fs,b"/a",1).unwrap();
  fs.delete_if_version(b"/a",2).unwrap();
  w.observe(id(1),&fs,b"/a",2).unwrap();
  let mut events=[EMPTY;3];assert_eq!(w.since(id(1),0,&mut events),Ok(3));
  assert_eq!(events[2].kind,Kind::Deleted);
 }
 #[test]fn unchanged_file_is_not_reported_as_modified(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut w=Watch::<1>::new(id(1));
  assert_eq!(w.observe(id(1),&fs,b"/a",1),Err(Error::Conflict));
 }
 #[test]fn recycled_identity_cannot_inspect_other_sessions_events(){
  let w=Watch::<1>::new(id(1));let mut out=[EMPTY;1];
  assert_eq!(w.since(id(2),0,&mut out),Err(Error::Denied));
 }
}
