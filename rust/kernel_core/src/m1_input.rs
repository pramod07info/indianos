//! M1 interactive desktop Terminal input, designed for events copied from a
//! trusted kernel keyboard/IPC broker. No direct port I/O, pointer access or
//! privileged framebuffer writes. Each completed line is a separate command.
pub const MAX_LINE:usize=192;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Full,TooSmall}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Event{Typed,Erased,Submitted(usize),Empty}

pub struct InputLine {
 bytes:[u8;MAX_LINE],
 length:usize,
}
impl InputLine{
 pub const fn new()->Self{Self{bytes:[0;MAX_LINE],length:0}}
 pub fn length(&self)->usize{self.length}
 pub fn reset(&mut self){
  self.bytes.fill(0);
  self.length=0;
 }
 pub fn feed(&mut self,key:u8,out:&mut[u8])->Result<Event,Error>{
  match key {
   b'\n'|b'\r' =>{
    if self.length==0{return Ok(Event::Empty);}
    if out.len()<self.length{return Err(Error::TooSmall);}
    let len=self.length;
    out[..len].copy_from_slice(&self.bytes[..len]);
    self.reset();
    Ok(Event::Submitted(len))
   }
   8|127=>{
    if self.length==0{return Ok(Event::Empty);}
    self.length-=1;self.bytes[self.length]=0;
    Ok(Event::Erased)
   }
   b' '..=b'~'=>{
    if self.length>=MAX_LINE{return Err(Error::Full);}
    self.bytes[self.length]=key;
    self.length+=1;Ok(Event::Typed)
   }
   _=>Err(Error::Invalid)
  }
 }
}
#[cfg(test)] mod tests{
 use super::*;
 #[test] fn real_typing_backspace_and_return(){
  let mut input=InputLine::new();let mut buf=[0u8;MAX_LINE];
  for &b in b"write /note helloo"{
   assert_eq!(input.feed(b,&mut buf),Ok(Event::Typed));
  }
  assert_eq!(input.feed(8,&mut buf),Ok(Event::Erased));
  assert_eq!(input.feed(b'\r',&mut buf),Ok(Event::Submitted(17)));
  assert_eq!(&buf[..17],b"write /note hello");
  assert_eq!(input.length(),0);
 }
 #[test] fn input_overflow_and_failed_submission_never_drop_data(){
  let mut input=InputLine::new();
  for _ in 0..MAX_LINE{input.feed(b'x',&mut[]).unwrap();}
  assert_eq!(input.feed(b'a',&mut[]),Err(Error::Full));
  assert_eq!(input.feed(0,&mut[]),Err(Error::Invalid));
  assert_eq!(input.feed(b'\n',&mut[]),Err(Error::TooSmall));
  assert_eq!(input.length(),MAX_LINE);
  let mut out=[0u8;MAX_LINE];
  assert_eq!(input.feed(b'\n',&mut out),Ok(Event::Submitted(MAX_LINE)));
  assert_eq!(out,[b'x';MAX_LINE]);
 }
 #[test] fn empty_command_backspace_does_not_underflow(){
  let mut input=InputLine::new();
  assert_eq!(input.feed(8,&mut[]),Ok(Event::Empty));
  assert_eq!(input.feed(b'\n',&mut[]),Ok(Event::Empty));
 }
}