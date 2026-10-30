//! Bounded native Terminal editor and fixed shared-note command protocol.
//! This userland parser grants no filesystem authority; PID1 independently
//! validates every resulting request before its existing file syscall.
pub const CAPACITY:usize=32;
pub const MAX_NOTE:usize=20;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Action{Changed,Read,Write{bytes:[u8;MAX_NOTE],len:usize},Invalid,Leave}
pub struct Line{bytes:[u8;CAPACITY],len:usize,overflow:bool}
impl Line{
 pub const fn new()->Self{Self{bytes:[0;CAPACITY],len:0,overflow:false}}
 pub fn clear(&mut self){self.bytes.fill(0);self.len=0;self.overflow=false;}
 pub fn display(&self,out:&mut[u8;22])->usize{
  out.fill(0);out[..2].copy_from_slice(b"> ");
  let start=self.len.saturating_sub(20);
  out[2..2+self.len-start].copy_from_slice(&self.bytes[start..self.len]);
  2+self.len-start
 }
 pub fn feed(&mut self,byte:u8)->Action{
  match byte{
   b'\t'=>{self.clear();Action::Leave},
   8|127=>{if self.len>0{self.len-=1;self.bytes[self.len]=0;}Action::Changed},
   b'\r'|b'\n'=>{
    let action=if self.overflow{Action::Invalid}
     else if &self.bytes[..self.len]==b"cat"{Action::Read}
     else if self.bytes[..self.len].starts_with(b"write "){
      let data=&self.bytes[6..self.len];
      if data.is_empty()||data.len()>MAX_NOTE{Action::Invalid}else{
       let mut bytes=[0;MAX_NOTE];bytes[..data.len()].copy_from_slice(data);
       Action::Write{bytes,len:data.len()}
      }
     }else{Action::Invalid};
    self.clear();action
   },
   0x20..=0x7e=>{
    if self.len<CAPACITY{self.bytes[self.len]=byte;self.len+=1;}
    else{self.overflow=true;}
    Action::Changed
   },
   _=>Action::Invalid,
  }
 }
}
