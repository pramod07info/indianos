//! R020/17 bounded process clipboard with owner generation, expiry and wipe.
//! Caller must separately authorize cross-app clipboard transfers.
use super::Error;
use crate::batch5_session::Identity;
pub struct Clipboard<const N:usize>{owner:Identity,bytes:[u8;N],
 size:usize,expires:u64,revision:u64}
impl<const N:usize> Clipboard<N>{
 pub const fn new()->Self{Self{owner:Identity{pid:0,generation:0},
  bytes:[0;N],size:0,expires:0,revision:0}}
 pub fn copy(&mut self,owner:Identity,now:u64,ttl:u64,
   data:&[u8])->Result<u64,Error>{
  if owner.pid==0||owner.generation==0||ttl==0||
   data.len()>N{return Err(Error::Invalid);}
  let until=now.checked_add(ttl).ok_or(Error::Overflow)?;
  let next=self.revision.checked_add(1).ok_or(Error::Overflow)?;
  let mut staged=[0u8;N];staged[..data.len()].copy_from_slice(data);
  self.bytes=staged;self.size=data.len();self.owner=owner;
  self.expires=until;self.revision=next;Ok(next)
 }
 pub fn paste(&self,owner:Identity,now:u64,out:&mut[u8])
  ->Result<usize,Error>{
  if owner.pid==0||owner!=self.owner||now>=self.expires{
   return Err(Error::Denied);
  }
  if out.len()<self.size{return Err(Error::Full);}
  out[..self.size].copy_from_slice(&self.bytes[..self.size]);
  Ok(self.size)
 }
 pub fn revoke(&mut self,owner:Identity)->Result<(),Error>{
  if owner!=self.owner||owner.pid==0{return Err(Error::Denied);}
  for b in &mut self.bytes{*b=0;}
  self.size=0;self.expires=0;self.owner=Identity{pid:0,generation:0};Ok(())
 }
 pub fn expire(&mut self,now:u64)->bool{
  if self.owner.pid==0||now<self.expires{return false;}
  for b in &mut self.bytes{*b=0;}
  self.size=0;self.expires=0;self.owner=Identity{pid:0,generation:0};true
 }
 pub fn revision(&self)->u64{self.revision}
 pub fn is_empty(&self)->bool{self.size==0}
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn same_process_can_copy_and_paste_inside_expiry(){
  let mut c=Clipboard::<16>::new();c.copy(id(1),1,10,b"hello").unwrap();
  let mut out=[0u8;16];assert_eq!(c.paste(id(1),2,&mut out),Ok(5));
  assert_eq!(&out[..5],b"hello");
 }
 #[test]fn expired_clipboard_is_zeroed_and_never_shared_to_new_pid(){
  let mut c=Clipboard::<16>::new();c.copy(id(1),1,3,b"secret").unwrap();
  assert_eq!(c.paste(id(2),2,&mut [0u8;16]),Err(Error::Denied));
  assert!(c.expire(4));assert!(c.is_empty());
  assert_eq!(c.paste(id(1),4,&mut [0u8;16]),Err(Error::Denied));
 }
 #[test]fn short_paste_buffer_and_invalid_ttl_preserve_contents(){
  let mut c=Clipboard::<8>::new();c.copy(id(1),1,10,b"hello").unwrap();
  assert_eq!(c.copy(id(1),1,0,b"x"),Err(Error::Invalid));
  let mut small=[77u8;2];
  assert_eq!(c.paste(id(1),2,&mut small),Err(Error::Full));
  assert_eq!(small,[77;2]);assert_eq!(c.revision(),1);
 }
}
