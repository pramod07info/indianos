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
/// Orbit keyboard task E1-E5: cursor-aware editing, independent of PS/2 scan
/// codes or privileged IPC. Only validated ASCII is currently supported.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum EditKey{Left,Right,Home,End,Insert(u8),Backspace,Delete,Submit,Clear}
/// Bounded line editor; not wired to GUI Terminal until the live input broker
/// can deliver stable keyboard event codes. No allocation or raw user pointers.
pub struct LineEditor{
    line:[u8;MAX_LINE],
    len:usize,
    caret:usize
}
impl LineEditor{
    pub const fn new()->Self{Self{line:[0;MAX_LINE],len:0,caret:0}}
    pub fn caret(&self)->usize{self.caret}
    pub fn bytes(&self)->&[u8]{&self.line[..self.len]}
    pub fn clear(&mut self){self.line.fill(0);self.len=0;self.caret=0;}
    pub fn edit(&mut self,key:EditKey,out:&mut[u8])->Result<Event,Error>{
        match key{
            // E1: move cursor without causing side effects or underflow.
            EditKey::Left=>{self.caret=self.caret.saturating_sub(1);Ok(Event::Empty)}
            EditKey::Right=>{
                self.caret=(self.caret+1).min(self.len);Ok(Event::Empty)
            }
            // E2: jump to either end, bounded by actual line length.
            EditKey::Home=>{self.caret=0;Ok(Event::Empty)}
            EditKey::End=>{self.caret=self.len;Ok(Event::Empty)}
            // E3: insert within the buffer, shifting only occupied bytes.
            EditKey::Insert(ch)=>{
                if !(b' '..=b'~').contains(&ch){return Err(Error::Invalid);}
                if self.len==MAX_LINE{return Err(Error::Full);}
                for i in (self.caret..self.len).rev(){
                    self.line[i+1]=self.line[i];
                }
                self.line[self.caret]=ch;self.len+=1;self.caret+=1;
                Ok(Event::Typed)
            }
            // E4: delete/backspace with non-leaking tail zeroing.
            EditKey::Delete|EditKey::Backspace=>{
                let index=if key==EditKey::Backspace{
                    if self.caret==0{return Ok(Event::Empty);}
                    self.caret-1
                }else{
                    if self.caret==self.len{return Ok(Event::Empty);}
                    self.caret
                };
                for i in index..self.len-1{self.line[i]=self.line[i+1];}
                self.len-=1;self.line[self.len]=0;
                if key==EditKey::Backspace{self.caret-=1;}
                Ok(Event::Erased)
            }
            EditKey::Clear=>{
                self.clear();Ok(Event::Erased)
            }
            // E5: failed submit must preserve entire state (no partial line).
            EditKey::Submit=>{
                if self.len==0{return Ok(Event::Empty);}
                if out.len()<self.len{return Err(Error::TooSmall);}
                let size=self.len;out[..size].copy_from_slice(self.bytes());
                self.clear();Ok(Event::Submitted(size))
            }
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

 #[test]fn orbit_editor_caret_navigation_and_home_end(){
  let mut e=LineEditor::new();
  for &b in b"abc"{e.edit(EditKey::Insert(b),&mut[]).unwrap();}
  e.edit(EditKey::Left,&mut[]).unwrap();
  e.edit(EditKey::Left,&mut[]).unwrap();
  assert_eq!(e.caret(),1);
  e.edit(EditKey::Home,&mut[]).unwrap();
  for _ in 0..4{e.edit(EditKey::Left,&mut[]).unwrap();}
  assert_eq!(e.caret(),0);
  e.edit(EditKey::End,&mut[]).unwrap();
  for _ in 0..4{e.edit(EditKey::Right,&mut[]).unwrap();}
  assert_eq!(e.caret(),3);
 }
 #[test]fn orbit_editor_middle_insertion_delete_and_backspace(){
  let mut e=LineEditor::new();
  for &b in b"ac"{e.edit(EditKey::Insert(b),&mut[]).unwrap();}
  e.edit(EditKey::Left,&mut[]).unwrap();
  e.edit(EditKey::Insert(b'b'),&mut[]).unwrap();
  assert_eq!(e.bytes(),b"abc");
  e.edit(EditKey::Backspace,&mut[]).unwrap();
  assert_eq!(e.bytes(),b"ac");
  e.edit(EditKey::Delete,&mut[]).unwrap();
  assert_eq!(e.bytes(),b"a");
  assert_eq!(e.edit(EditKey::Delete,&mut[]),Ok(Event::Empty));
 }
 #[test]fn orbit_editor_submit_atomic_and_clear(){
  let mut e=LineEditor::new();let mut out=[0xaa;8];
  for &b in b"hello"{e.edit(EditKey::Insert(b),&mut[]).unwrap();}
  assert_eq!(e.edit(EditKey::Submit,&mut out[..3]),Err(Error::TooSmall));
  assert_eq!(e.bytes(),b"hello");assert_eq!(e.caret(),5);
  assert_eq!(out,[0xaa;8]);
  assert_eq!(e.edit(EditKey::Submit,&mut out),Ok(Event::Submitted(5)));
  assert_eq!(&out[..5],b"hello");
  assert_eq!(e.bytes(),b"");assert_eq!(e.caret(),0);
  e.edit(EditKey::Insert(b'a'),&mut[]).unwrap();
  e.edit(EditKey::Clear,&mut[]).unwrap();
  assert!(e.bytes().is_empty());
 }
 #[test]fn orbit_editor_capacity_and_invalid_input_never_mutate(){
  let mut e=LineEditor::new();
  for _ in 0..MAX_LINE{e.edit(EditKey::Insert(b'x'),&mut[]).unwrap();}
  assert_eq!(e.edit(EditKey::Insert(b'y'),&mut[]),Err(Error::Full));
  assert_eq!(e.edit(EditKey::Insert(0),&mut[]),Err(Error::Invalid));
  assert_eq!(e.bytes().len(),MAX_LINE);assert_eq!(e.caret(),MAX_LINE);
 }

}