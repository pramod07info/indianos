//! R027/10: session-owned typed terminal aliases, no arbitrary shell expansion.
//! Aliases are command name mappings, not executable programs.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Alias{pub source:[u8;32],pub slen:usize,
 pub target:[u8;32],pub tlen:usize}
pub struct Aliases<const N:usize>{owner:Identity,items:[Option<Alias>;N]}
fn identifier(s:&[u8])->bool{
 !s.is_empty()&&s.len()<=32&&s.iter().all(|b|
 b.is_ascii_alphanumeric()||*b==b'_'||*b==b'-')
}
impl<const N:usize> Aliases<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,items:[None;N]}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn find(&self,name:&[u8])->Option<usize>{
  self.items.iter().position(|a|a.is_some_and(|a|&a.source[..a.slen]==name))
 }
 pub fn set(&mut self,who:Identity,from:&[u8],to:&[u8])->Result<(),Error>{
  self.auth(who)?;
  if !identifier(from)||!identifier(to)||from==to{return Err(Error::Invalid);}
  let at=self.find(from).or_else(||self.items.iter().position(Option::is_none))
   .ok_or(Error::Full)?;
  let mut alias=Alias{source:[0;32],slen:from.len(),target:[0;32],tlen:to.len()};
  alias.source[..from.len()].copy_from_slice(from);
  alias.target[..to.len()].copy_from_slice(to);
  self.items[at]=Some(alias);Ok(())
 }
 pub fn resolve(&self,who:Identity,name:&[u8],
 out:&mut[u8])->Result<usize,Error>{
  self.auth(who)?;
  let mut current=[0u8;32];let mut len=name.len();
  if len>32{return Err(Error::Invalid);}
  current[..len].copy_from_slice(name);
  for _ in 0..N{
   let Some(i)=self.find(&current[..len])else{break};
   let alias=self.items[i].ok_or(Error::Missing)?;
   len=alias.tlen;current[..len].copy_from_slice(&alias.target[..len]);
  }
  if self.find(&current[..len]).is_some(){return Err(Error::Conflict);}
  if len>out.len(){return Err(Error::Full);}
  out[..len].copy_from_slice(&current[..len]);Ok(len)
 }
 pub fn remove(&mut self,who:Identity,name:&[u8])->Result<(),Error>{
  self.auth(who)?;let i=self.find(name).ok_or(Error::Missing)?;
  self.items[i]=None;Ok(())
 }
 pub fn count(&self,who:Identity)->Result<usize,Error>{
  self.auth(who)?;Ok(self.items.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn resolves_two_safe_terminal_aliases(){
  let mut a=Aliases::<2>::new(id(1));
  a.set(id(1),b"ll",b"list").unwrap();
  a.set(id(1),b"list",b"cat").unwrap();
  let mut out=[0u8;16];let n=a.resolve(id(1),b"ll",&mut out).unwrap();
  assert_eq!(&out[..n],b"cat");
 }
 #[test]fn alias_control_characters_are_rejected(){
  let mut a=Aliases::<1>::new(id(1));
  assert_eq!(a.set(id(1),b"bad;rm",b"cat"),Err(Error::Invalid));
 }
 #[test]fn reused_process_identity_cannot_read_private_command_aliases(){
  let mut a=Aliases::<1>::new(id(1));a.set(id(1),b"l",b"cat").unwrap();
  assert_eq!(a.resolve(id(2),b"l",&mut[0;16]),Err(Error::Denied));
 }
}
