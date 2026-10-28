//! R020/03 multi-level source history with redo invalidation after a new edit.
//! Every edit snapshots the previous buffer before publishing a new revision.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Revision<const B:usize>{pub bytes:[u8;B],pub len:usize}
pub struct Document<const B:usize,const H:usize>{
 owner:u32,now:Revision<B>,revision:u64,
 undo:[Option<Revision<B>>;H],undo_len:usize,
 redo:[Option<Revision<B>>;H],redo_len:usize,
}
impl<const B:usize,const H:usize> Document<B,H>{
 pub const fn new(owner:u32)->Self{Self{owner,now:Revision{bytes:[0;B],len:0},
  revision:0,undo:[None;H],undo_len:0,redo:[None;H],redo_len:0}}
 fn verify(&self,owner:u32,rev:u64)->Result<(),Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  if rev!=self.revision{return Err(Error::Stale);}Ok(())
 }
 fn push(stack:&mut[Option<Revision<B>>;H],count:&mut usize,item:Revision<B>){
  if H==0{return;}
  if *count==H{
   for i in 1..H{stack[i-1]=stack[i];}
   *count-=1;
  }
  stack[*count]=Some(item);*count+=1;
 }
 fn next(&self)->Result<u64,Error>{
  self.revision.checked_add(1).ok_or(Error::Overflow)
 }
 pub fn replace(&mut self,owner:u32,revision:u64,start:usize,end:usize,
   bytes:&[u8])->Result<u64,Error>{
  self.verify(owner,revision)?;
  if start>end||end>self.now.len{return Err(Error::Invalid);}
  let len=self.now.len-(end-start);
  let newlen=len.checked_add(bytes.len()).ok_or(Error::Overflow)?;
  if newlen>B{return Err(Error::Full);}
  let next_rev=self.next()?;
  let mut next=Revision{bytes:[0;B],len:newlen};
  next.bytes[..start].copy_from_slice(&self.now.bytes[..start]);
  next.bytes[start..start+bytes.len()].copy_from_slice(bytes);
  next.bytes[start+bytes.len()..newlen]
   .copy_from_slice(&self.now.bytes[end..self.now.len]);
  Self::push(&mut self.undo,&mut self.undo_len,self.now);
  self.now=next;self.redo=[None;H];self.redo_len=0;
  self.revision=next_rev;Ok(next_rev)
 }
 pub fn undo(&mut self,owner:u32,revision:u64)->Result<u64,Error>{
  self.verify(owner,revision)?;
  if self.undo_len==0{return Err(Error::Missing);}
  let r=self.next()?;
  let previous=self.undo[self.undo_len-1].ok_or(Error::Missing)?;
  Self::push(&mut self.redo,&mut self.redo_len,self.now);
  self.undo_len-=1;self.undo[self.undo_len]=None;self.now=previous;self.revision=r;Ok(r)
 }
 pub fn redo(&mut self,owner:u32,revision:u64)->Result<u64,Error>{
  self.verify(owner,revision)?;
  if self.redo_len==0{return Err(Error::Missing);}
  let r=self.next()?;
  let next=self.redo[self.redo_len-1].ok_or(Error::Missing)?;
  Self::push(&mut self.undo,&mut self.undo_len,self.now);
  self.redo_len-=1;self.redo[self.redo_len]=None;self.now=next;self.revision=r;Ok(r)
 }
 pub fn copy(&self,owner:u32,out:&mut[u8])->Result<usize,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  if out.len()<self.now.len{return Err(Error::Full);}
  out[..self.now.len].copy_from_slice(&self.now.bytes[..self.now.len]);Ok(self.now.len)
 }
 pub fn revision(&self)->u64{self.revision}
 pub fn history_depth(&self)->(usize,usize){(self.undo_len,self.redo_len)}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn multi_level_undo_and_redo_restore_code(){
  let mut d=Document::<32,3>::new(7);
  d.replace(7,0,0,0,b"one").unwrap();d.replace(7,1,3,3,b" two").unwrap();
  d.undo(7,2).unwrap();let mut out=[0;32];
  assert_eq!(d.copy(7,&mut out),Ok(3));assert_eq!(&out[..3],b"one");
  d.redo(7,3).unwrap();assert_eq!(d.copy(7,&mut out),Ok(7));
 }
 #[test]fn new_edit_clears_redo_and_rejects_stale_generation(){
  let mut d=Document::<16,2>::new(7);
  d.replace(7,0,0,0,b"x").unwrap();d.undo(7,1).unwrap();
  d.replace(7,2,0,0,b"y").unwrap();
  assert_eq!(d.redo(7,3),Err(Error::Missing));
  assert_eq!(d.replace(7,1,0,0,b"z"),Err(Error::Stale));
 }
 #[test]fn rejected_overflow_keeps_undo_and_bytes_intact(){
  let mut d=Document::<2,1>::new(7);d.replace(7,0,0,0,b"ok").unwrap();
  assert_eq!(d.replace(7,1,2,2,b"!"),Err(Error::Full));
  assert_eq!(d.history_depth(),(1,0));assert_eq!(d.revision(),1);
 }
}
