//! R031/12: bounded owner-scoped terminal scrollback with line-aware eviction.
use super::Error;
pub struct Scrollback<const N:usize>{owner:u64,buffer:[u8;N],length:usize}
impl<const N:usize> Scrollback<N>{
 pub fn new(owner:u64)->Self{Self{owner,buffer:[0;N],length:0}}
 pub fn append(&mut self,owner:u64,bytes:&[u8])->Result<usize,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  if bytes.len()>N||core::str::from_utf8(bytes).is_err(){return Err(Error::Full);}
  let mut drop=0;
  while self.length-drop+bytes.len()>N{
   let end=self.buffer[drop..self.length].iter().position(|b|*b==b'\n')
    .map(|p|drop+p+1).unwrap_or(self.length);
   if end==drop{return Err(Error::Full);}
   drop=end;
  }
  if drop>0{self.buffer.copy_within(drop..self.length,0);self.length-=drop;}
  self.buffer[self.length..self.length+bytes.len()].copy_from_slice(bytes);
  self.length+=bytes.len();Ok(drop)
 }
 pub fn read(&self,owner:u64,out:&mut[u8])->Result<usize,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  if self.length>out.len(){return Err(Error::Full);}
  out[..self.length].copy_from_slice(&self.buffer[..self.length]);Ok(self.length)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn scrollback_evicts_oldest_complete_line_not_partial_words(){
  let mut log=Scrollback::<12>::new(7);
  log.append(7,b"one\ntwo\n").unwrap();
  log.append(7,b"three\n").unwrap();
  let mut out=[0;12];let n=log.read(7,&mut out).unwrap();
  assert_eq!(&out[..n],b"two\nthree\n");
 }
 #[test]fn output_cannot_be_read_by_foreign_session(){
  let mut log=Scrollback::<6>::new(7);
  assert_eq!(log.append(8,b"a"),Err(Error::Denied));
  log.append(7,b"a").unwrap();
  assert_eq!(log.read(8,&mut[0;6]),Err(Error::Denied));
 }
}