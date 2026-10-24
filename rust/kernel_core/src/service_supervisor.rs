//! R031.2 strictly bounded x86-64 QEMU service-supervision policy.
//! Independent of syscall-supplied identities: only the kernel scheduler
//! can call these methods after validating a real faulted process PCB.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Decision{Restart{new_pid:u32},Deny}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct TerminalSupervisor{
    original:u32, replacement:u32, faults:u8, restarted:bool
}
impl TerminalSupervisor{
    pub const fn new()->Self{
        Self{original:3,replacement:4,faults:0,restarted:false}
    }
    /// Only a #UD fault in the original allowlisted Terminal is recoverable.
    pub fn crashed(&mut self,pid:u32,vector:u64)->Decision{
        if pid!=self.original||vector!=6||self.faults!=0{return Decision::Deny;}
        self.faults=1;
        Decision::Restart{new_pid:self.replacement}
    }
    /// The GUI grant is NEVER installed until the new process mappings,
    /// IPC mailbox and kernel-owned ownership table are prepared.
    pub fn prepared(&mut self,pid:u32)->bool{
        if pid!=self.replacement||self.faults!=1||self.restarted{return false;}
        self.restarted=true;true
    }
    pub fn ready(&self)->bool{self.restarted&&self.faults==1}
    pub fn active_pid(&self)->Option<u32>{
        if self.ready(){Some(self.replacement)}else{None}
    }
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]fn only_one_expected_terminal_fault_gets_a_fresh_pid(){
  let mut s=TerminalSupervisor::new();
  assert_eq!(s.crashed(1,6),Decision::Deny);
  assert_eq!(s.crashed(2,6),Decision::Deny);
  assert_eq!(s.crashed(3,14),Decision::Deny);
  assert_eq!(s.active_pid(),None);
  assert_eq!(s.crashed(3,6),Decision::Restart{new_pid:4});
  assert_eq!(s.crashed(3,6),Decision::Deny);
  assert!(!s.prepared(3));assert!(!s.prepared(5));
  assert!(s.prepared(4));assert!(!s.prepared(4));
  assert_eq!(s.active_pid(),Some(4));
  assert_eq!(s.crashed(4,6),Decision::Deny);
 }
 #[test]fn fresh_session_has_isolated_restart_budget(){
  let s=TerminalSupervisor::new();let mut other=s;
  assert_eq!(other.crashed(3,6),Decision::Restart{new_pid:4});
  assert_eq!(s.active_pid(),None);
 }
}
