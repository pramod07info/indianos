//! R023/18: typed workbench command dispatch for Files/Terminal/editor actions.
//! Enforces authenticated caller, explicit rights and current workspace revision.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
use super::agent_12;
use crate::workstation_r022::agent_17;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Action{ShowFiles,ReadFile,Search,OpenSettings,Build,RunTests}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Request{pub id:u32,pub owner:Identity,pub action:Action,
 pub rights:u32,pub revision:u64,pub approved:bool}
pub struct Dispatcher<const N:usize>{owner:Identity,
 queue:[Option<Request>;N],next:usize,last_id:u32}
impl<const N:usize> Dispatcher<N>{
 pub const fn new(owner:Identity)->Self{
  Self{owner,queue:[None;N],next:0,last_id:0}
 }
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn submit(&mut self,actor:Identity,id:u32,action:Action,
  rights:u32,revision:u64)->Result<(),Error>{
  self.auth(actor)?;
  if id==0||id<=self.last_id||rights==0{return Err(Error::Invalid);}
  let at=self.queue.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.queue[at]=Some(Request{id,owner:actor,action,rights,revision,
   approved:false});
  self.last_id=id;self.next=at;Ok(())
 }
 pub fn approve(&mut self,actor:Identity,id:u32,
  lease:&mut agent_17::Authority<N>,lease_id:u32,
  now:u64)->Result<(),Error>{
  self.auth(actor)?;
  let at=self.queue.iter().position(|x|x.is_some_and(|r|r.id==id))
   .ok_or(Error::Missing)?;
  let mut request=self.queue[at].ok_or(Error::Missing)?;
  lease.use_rights(actor,lease_id,request.rights,now)
   .map_err(|_|Error::Denied)?;
  request.approved=true;self.queue[at]=Some(request);Ok(())
 }
 pub fn execute_readonly<const F:usize,const B:usize>(
  &mut self,actor:Identity,id:u32,fs:&RamFs<F>,path:&[u8],
  expected_rev:u64,out:&mut[u8;B])->Result<usize,Error>{
  self.auth(actor)?;
  let at=self.queue.iter().position(|r|r.is_some_and(|r|r.id==id))
   .ok_or(Error::Missing)?;
  let r=self.queue[at].ok_or(Error::Missing)?;
  if !r.approved||r.revision!=expected_rev{return Err(Error::Denied);}
  let mut command=[0u8;128];let mut len=0;
  let prefix=match r.action{
   Action::ShowFiles=>b"ls ".as_slice(),
   Action::ReadFile=>b"cat ".as_slice(),
   _=>return Err(Error::Denied)
  };
  if prefix.len()+path.len()>command.len(){return Err(Error::Full);}
  command[..prefix.len()].copy_from_slice(prefix);len+=prefix.len();
  command[len..len+path.len()].copy_from_slice(path);len+=path.len();
  let count=agent_12::run(fs,&command[..len],out)?;
  self.queue[at]=None;Ok(count)
 }
 pub fn pending(&self,actor:Identity)->Result<usize,Error>{
  self.auth(actor)?;Ok(self.queue.iter().flatten().count())
 }
 pub fn cancel(&mut self,actor:Identity,id:u32)->Result<(),Error>{
  self.auth(actor)?;
  let at=self.queue.iter().position(|r|r.is_some_and(|r|r.id==id))
   .ok_or(Error::Missing)?;
  self.queue[at]=None;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 fn lease()->agent_17::Lease{agent_17::Lease{id:1,owner:id(1),
  rights:3,expires:100,uses_remaining:3,revoked:false}}
 #[test]fn authorized_gui_file_open_reads_real_kernel_file_contents(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"hello").unwrap();
  let mut authority=agent_17::Authority::<2>::new();
  authority.issue(true,lease()).unwrap();
  let mut ui=Dispatcher::<2>::new(id(1));
  ui.submit(id(1),1,Action::ReadFile,1,fs.version()).unwrap();
  ui.approve(id(1),1,&mut authority,1,1).unwrap();
  let mut out=[0u8;16];
  let n=ui.execute_readonly(id(1),1,&fs,b"/main",1,&mut out).unwrap();
  assert_eq!(&out[..n],b"hello");
 }
 #[test]fn recycled_pid_cannot_approve_previous_session_requests(){
  let mut authority=agent_17::Authority::<1>::new();
  authority.issue(true,lease()).unwrap();
  let mut ui=Dispatcher::<1>::new(id(1));
  ui.submit(id(1),1,Action::ReadFile,1,1).unwrap();
  assert_eq!(ui.approve(id(2),1,&mut authority,1,1),Err(Error::Denied));
 }
 #[test]fn unapproved_or_wrong_revision_ui_command_not_executed(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut ui=Dispatcher::<1>::new(id(1));
  ui.submit(id(1),1,Action::ReadFile,1,1).unwrap();
  assert_eq!(ui.execute_readonly(id(1),1,&fs,b"/a",1,&mut [0;5]),Err(Error::Denied));
 }
}
