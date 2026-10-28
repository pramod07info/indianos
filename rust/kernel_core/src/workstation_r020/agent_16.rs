//! R020/16 owner-scoped UTF-8 input, composed text and safe scalar backspace.
//! Full grapheme clusters/IME candidate windows need a separate UI service.
use super::Error;
pub struct TextInput<const N:usize>{
 owner:u32,generation:u64,text:[u8;N],len:usize,revision:u64
}
impl<const N:usize> TextInput<N>{
 pub const fn new(owner:u32,generation:u64)->Self{
  Self{owner,generation,text:[0;N],len:0,revision:0}
 }
 fn check(&self,owner:u32,generation:u64,rev:u64)->Result<(),Error>{
  if owner==0||owner!=self.owner||generation==0||
   generation!=self.generation{return Err(Error::Denied);}
  if rev!=self.revision{return Err(Error::Stale);}Ok(())
 }
 pub fn type_text(&mut self,owner:u32,generation:u64,rev:u64,
  input:&[u8])->Result<u64,Error>{
  self.check(owner,generation,rev)?;
  let s=core::str::from_utf8(input).map_err(|_|Error::Invalid)?;
  if s.chars().any(|c|c=='\0'||c.is_control()&&c!='\n'){
   return Err(Error::Invalid);
  }
  let end=self.len.checked_add(input.len()).ok_or(Error::Overflow)?;
  if end>N{return Err(Error::Full);}
  let next=rev.checked_add(1).ok_or(Error::Overflow)?;
  self.text[self.len..end].copy_from_slice(input);
  self.len=end;self.revision=next;Ok(next)
 }
 pub fn backspace(&mut self,owner:u32,generation:u64,rev:u64)
  ->Result<u64,Error>{
  self.check(owner,generation,rev)?;
  if self.len==0{return Err(Error::Missing);}
  let next=rev.checked_add(1).ok_or(Error::Overflow)?;
  let mut from=self.len-1;
  while from>0&&self.text[from]&0xc0==0x80{from-=1;}
  for b in &mut self.text[from..self.len]{*b=0;}
  self.len=from;self.revision=next;Ok(next)
 }
 pub fn replace_all(&mut self,owner:u32,generation:u64,rev:u64,
   content:&[u8])->Result<u64,Error>{
  self.check(owner,generation,rev)?;
  core::str::from_utf8(content).map_err(|_|Error::Invalid)?;
  if content.len()>N{return Err(Error::Full);}
  let next=rev.checked_add(1).ok_or(Error::Overflow)?;
  let mut staged=[0u8;N];staged[..content.len()].copy_from_slice(content);
  self.text=staged;self.len=content.len();self.revision=next;Ok(next)
 }
 pub fn copy(&self,owner:u32,generation:u64,out:&mut[u8])
  ->Result<usize,Error>{
  if owner==0||owner!=self.owner||generation!=self.generation{
   return Err(Error::Denied);
  }
  if out.len()<self.len{return Err(Error::Full);}
  out[..self.len].copy_from_slice(&self.text[..self.len]);Ok(self.len)
 }
 pub fn revision(&self)->u64{self.revision}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn unicode_source_typing_and_single_scalar_backspace(){
  let mut input=TextInput::<32>::new(7,1);
  input.type_text(7,1,0,"Aéह".as_bytes()).unwrap();
  input.backspace(7,1,1).unwrap();
  let mut out=[0u8;32];let n=input.copy(7,1,&mut out).unwrap();
  assert_eq!(&out[..n],"Aé".as_bytes());
 }
 #[test]fn malformed_utf8_and_short_destination_preserve_buffer(){
  let mut input=TextInput::<4>::new(7,1);
  input.type_text(7,1,0,b"ab").unwrap();
  assert_eq!(input.type_text(7,1,1,&[0xff]),Err(Error::Invalid));
  let mut out=[55u8;1];
  assert_eq!(input.copy(7,1,&mut out),Err(Error::Full));
  assert_eq!(out,[55]);assert_eq!(input.revision(),1);
 }
 #[test]fn recycled_pid_cannot_type_in_previous_session(){
  let mut input=TextInput::<16>::new(7,1);
  assert_eq!(input.type_text(7,2,0,b"stale"),Err(Error::Denied));
  assert_eq!(input.type_text(8,1,0,b"foreign"),Err(Error::Denied));
 }
}
