//! R026/16: trusted debugger execution control state for source-level stepping.
//! Does not implement hardware CPU traps or a native debugger transport.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Stopped,Running,Paused,Exited}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Command{Continue,Pause,StepIn,StepOver,StepOut,Stop}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Thread{pub tid:u32,pub state:State,pub pc:u64,
 pub line:u32,pub steps:u32,pub tick:u64}
pub struct Debugger<const N:usize>{owner:Identity,threads:[Option<Thread>;N]}
impl<const N:usize> Debugger<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,threads:[None;N]}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn slot(&self,id:u32)->Option<usize>{
  self.threads.iter().position(|t|t.is_some_and(|x|x.tid==id))
 }
 pub fn attach(&mut self,who:Identity,trusted:bool,id:u32,
 pc:u64,line:u32,tick:u64)->Result<(),Error>{
  self.auth(who)?;
  if !trusted||id==0||line==0||self.slot(id).is_some(){
   return Err(Error::Denied);
  }
  let at=self.threads.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.threads[at]=Some(Thread{tid:id,state:State::Paused,pc,line,steps:0,tick});Ok(())
 }
 pub fn command(&mut self,who:Identity,trusted:bool,id:u32,
 command:Command,now:u64)->Result<State,Error>{
  self.auth(who)?;if !trusted{return Err(Error::Denied);}
  let i=self.slot(id).ok_or(Error::Missing)?;
  let mut thread=self.threads[i].ok_or(Error::Missing)?;
  if now<thread.tick||thread.state==State::Exited{return Err(Error::Stale);}
  thread.state=match (thread.state,command){
   (_,Command::Stop)=>State::Exited,
   (State::Running,Command::Pause)=>State::Paused,
   (State::Paused,Command::Continue)=>State::Running,
   (State::Paused,Command::StepIn|Command::StepOver|Command::StepOut)=>{
    thread.steps=thread.steps.checked_add(1).ok_or(Error::Overflow)?;
    State::Paused
   }
   _=>return Err(Error::NotReady)
  };
  thread.tick=now;self.threads[i]=Some(thread);Ok(thread.state)
 }
 pub fn location(&mut self,trusted:bool,id:u32,
 pc:u64,line:u32,now:u64)->Result<(),Error>{
  if !trusted||line==0{return Err(Error::Denied);}
  let i=self.slot(id).ok_or(Error::Missing)?;
  let mut t=self.threads[i].ok_or(Error::Missing)?;
  if now<t.tick||t.state!=State::Paused{return Err(Error::Stale);}
  t.pc=pc;t.line=line;t.tick=now;self.threads[i]=Some(t);Ok(())
 }
 pub fn get(&self,who:Identity,id:u32)->Result<Thread,Error>{
  self.auth(who)?;self.slot(id).and_then(|i|self.threads[i]).ok_or(Error::Missing)
 }
 pub fn detach(&mut self,who:Identity,id:u32)->Result<(),Error>{
  self.auth(who)?;let i=self.slot(id).ok_or(Error::Missing)?;
  self.threads[i]=None;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn paused_thread_supports_step_and_continue_after_trusted_attach(){
  let mut d=Debugger::<1>::new(id(1));d.attach(id(1),true,1,0x1000,1,1).unwrap();
  assert_eq!(d.command(id(1),true,1,Command::StepIn,2),Ok(State::Paused));
  d.location(true,1,0x1004,2,3).unwrap();
  assert_eq!(d.get(id(1),1).unwrap().steps,1);
  assert_eq!(d.command(id(1),true,1,Command::Continue,4),Ok(State::Running));
 }
 #[test]fn untrusted_debugger_cannot_control_guest_thread_state(){
  let mut d=Debugger::<1>::new(id(1));d.attach(id(1),true,1,0,1,1).unwrap();
  assert_eq!(d.command(id(1),false,1,Command::Continue,2),Err(Error::Denied));
 }
 #[test]fn reused_process_generation_cannot_step_another_users_thread(){
  let mut d=Debugger::<1>::new(id(1));d.attach(id(1),true,1,0,1,1).unwrap();
  assert_eq!(d.command(id(2),true,1,Command::StepOver,2),Err(Error::Denied));
 }
}
