//! R025/18: workbench typed UI commands into capability-gated actual files.
//! Does not render a compositor: dispatches open/read, search and focus actions.
use super::{Error,agent_02,agent_05,agent_10};
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Action{OpenFile,Search,FileStats}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Request{pub id:u32,pub owner:Identity,pub action:Action,
 pub required:u32,pub head:u64}
pub struct Router<const N:usize>{owner:Identity,queue:[Option<Request>;N]}
impl<const N:usize> Router<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,queue:[None;N]}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn slot(&self,id:u32)->Option<usize>{
  self.queue.iter().position(|r|r.is_some_and(|r|r.id==id))
 }
 pub fn submit(&mut self,who:Identity,id:u32,action:Action,
  expected:u64)->Result<(),Error>{
  self.auth(who)?;
  if id==0||self.slot(id).is_some(){return Err(Error::Invalid);}
  let at=self.queue.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.queue[at]=Some(Request{id,owner:who,action,
   required:agent_02::READ,head:expected});Ok(())
 }
 pub fn execute<const F:usize>(&mut self,who:Identity,id:u32,
  ticket:agent_02::Ticket,fs:&RamFs<F>,path:&[u8],
  query:&[u8],out:&mut[u8])->Result<usize,Error>{
  self.auth(who)?;
  let at=self.slot(id).ok_or(Error::Missing)?;
  let req=self.queue[at].ok_or(Error::Missing)?;
  if req.head!=fs.version(){return Err(Error::Stale);}
  agent_02::authorize(ticket,who,1,req.required,path)?;
  let count=match req.action{
   Action::OpenFile=>agent_02::read(ticket,who,1,fs,path,out)?,
   Action::Search=>{
    let mut hits=[agent_05::EMPTY;16];
    let mut search=agent_05::Search::<16>::new(who);
    let count=search.refresh(who,fs,path,query)?;
    let mut number=[0u8;16];let mut n=count;let mut digits=0;
    loop{number[digits]=b'0'+(n%10) as u8;digits+=1;n/=10;if n==0{break;}}
    if digits>out.len(){return Err(Error::Full);}
    for i in 0..digits{out[i]=number[digits-i-1];}
    let _=&mut hits;digits
   }
   Action::FileStats=>agent_10::run(fs,path,agent_10::Command::Stat,b"",out)?
  };
  self.queue[at]=None;Ok(count)
 }
 pub fn pending(&self,who:Identity)->Result<usize,Error>{
  self.auth(who)?;Ok(self.queue.iter().flatten().count())
 }
 pub fn cancel(&mut self,who:Identity,id:u32)->Result<(),Error>{
  self.auth(who)?;let at=self.slot(id).ok_or(Error::Missing)?;
  self.queue[at]=None;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn authorized_editor_window_reads_actual_source_file(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/app/main",b"hello").unwrap();
  let ticket=agent_02::grant(true,id(1),&fs,b"/app",agent_02::READ,100).unwrap();
  let mut ui=Router::<1>::new(id(1));
  ui.submit(id(1),1,Action::OpenFile,1).unwrap();
  let mut out=[0u8;10];let n=ui.execute(id(1),1,ticket,&fs,b"/app/main",b"",&mut out).unwrap();
  assert_eq!(&out[..n],b"hello");assert_eq!(ui.pending(id(1)),Ok(0));
 }
 #[test]fn foreign_generation_cannot_execute_another_users_ui_actions(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/app/a",b"a").unwrap();
  let ticket=agent_02::grant(true,id(1),&fs,b"/app",agent_02::READ,100).unwrap();
  let mut ui=Router::<1>::new(id(1));ui.submit(id(1),1,Action::OpenFile,1).unwrap();
  assert_eq!(ui.execute(id(2),1,ticket,&fs,b"/app/a",b"",&mut [0;8]),Err(Error::Denied));
 }
 #[test]fn stale_file_version_blocks_file_window_open(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/app/a",b"a").unwrap();
  let ticket=agent_02::grant(true,id(1),&fs,b"/app",agent_02::READ,100).unwrap();
  let mut ui=Router::<1>::new(id(1));ui.submit(id(1),1,Action::OpenFile,1).unwrap();
  fs.replace(b"/app/a",b"new").unwrap();
  assert_eq!(ui.execute(id(1),1,ticket,&fs,b"/app/a",b"",&mut [0;8]),Err(Error::Stale));
 }
}
