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


/// A bounded, kernel-configured supervisor for multiple allowlisted services.
/// It never loads executables; trusted process code performs actual startup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceAction {
    Start { pid: u32, generation: u32 },
    Restart { pid: u32, generation: u32 },
    Deny,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceSlot {
    pub service_id: u32,
    pub pid: u32,
    pub generation: u32,
    pub restarts: u8,
    pub max_restarts: u8,
    pub active: bool,
}
pub struct ServiceTable<const N: usize> {
    slots: [Option<ServiceSlot>; N],
}
impl<const N: usize> ServiceTable<N> {
    pub const fn new() -> Self { Self { slots: [None; N] } }

    /// Trusted init configures a service before handing it to the scheduler.
    pub fn configure(&mut self, service_id: u32, pid: u32, max_restarts: u8) -> bool {
        if service_id == 0 || pid == 0 || self.slots.iter().flatten()
            .any(|s| s.service_id == service_id || s.pid == pid) {
            return false;
        }
        let Some(free) = self.slots.iter_mut().find(|s| s.is_none()) else { return false; };
        *free = Some(ServiceSlot {
            service_id, pid, generation: 0, restarts: 0, max_restarts, active: false,
        });
        true
    }

    pub fn start(&mut self, service_id: u32) -> ServiceAction {
        let Some(slot) = self.slots.iter_mut().flatten()
            .find(|s| s.service_id == service_id) else { return ServiceAction::Deny; };
        if slot.active { return ServiceAction::Deny; }
        let Some(generation) = slot.generation.checked_add(1) else { return ServiceAction::Deny; };
        slot.generation = generation;
        slot.active = true;
        ServiceAction::Start { pid: slot.pid, generation }
    }

    /// Only the scheduler's authenticated PID may cause a service restart.
    /// Prevent generation and retry wraparound before modifying state.
    pub fn crash(&mut self, pid: u32, replacement_pid: u32) -> ServiceAction {
        if pid == 0 || replacement_pid == 0 || pid == replacement_pid ||
            self.slots.iter().flatten().any(|s| s.pid == replacement_pid) {
            return ServiceAction::Deny;
        }
        let Some(slot) = self.slots.iter_mut().flatten()
            .find(|s| s.pid == pid && s.active) else { return ServiceAction::Deny; };
        if slot.restarts >= slot.max_restarts { slot.active = false; return ServiceAction::Deny; }
        let (Some(restarts), Some(generation)) =
            (slot.restarts.checked_add(1), slot.generation.checked_add(1)) else {
            slot.active = false;
            return ServiceAction::Deny;
        };
        slot.pid = replacement_pid;
        slot.restarts = restarts;
        slot.generation = generation;
        slot.active = false; // no grants until the replacement is prepared
        ServiceAction::Restart { pid: replacement_pid, generation }
    }

    /// Only the replacement for the *same* service/generation may become active.
    pub fn prepared(&mut self, service_id: u32, pid: u32, generation: u32) -> bool {
        if let Some(slot) = self.slots.iter_mut().flatten()
            .find(|s| s.service_id == service_id && s.pid == pid && s.generation == generation) {
            if slot.active || slot.restarts == 0 { return false; }
            slot.active = true;
            return true;
        }
        false
    }

    pub fn stop(&mut self, service_id: u32, pid: u32) -> bool {
        if let Some(slot) = self.slots.iter_mut().flatten()
            .find(|s| s.service_id == service_id && s.pid == pid && s.active) {
            slot.active = false;
            return true;
        }
        false
    }

    pub fn snapshot(&self, service_id: u32) -> Option<ServiceSlot> {
        self.slots.iter().flatten().find(|s| s.service_id == service_id).copied()
    }
}

#[cfg(test)]
mod r033_service_tests {
    use super::*;
    #[test]
    fn two_services_start_without_cross_pid_control() {
        let mut t = ServiceTable::<2>::new();
        assert!(t.configure(1, 10, 1));
        assert!(t.configure(2, 20, 2));
        assert!(!t.configure(3, 10, 1));
        assert!(!t.configure(3, 30, 1));
        assert_eq!(t.start(1), ServiceAction::Start { pid: 10, generation: 1 });
        assert_eq!(t.start(2), ServiceAction::Start { pid: 20, generation: 1 });
        assert_eq!(t.start(1), ServiceAction::Deny);
        assert!(!t.stop(1, 20));
        assert!(t.stop(1, 10));
    }
    #[test]
    fn authenticated_fault_replaces_one_service_and_requires_preparation() {
        let mut t = ServiceTable::<2>::new();
        assert!(t.configure(1, 10, 1));
        assert!(t.configure(2, 20, 1));
        t.start(1); t.start(2);
        assert_eq!(t.crash(10, 20), ServiceAction::Deny);
        assert_eq!(t.crash(30, 31), ServiceAction::Deny);
        assert_eq!(t.crash(10, 11), ServiceAction::Restart { pid: 11, generation: 2 });
        assert!(!t.snapshot(1).unwrap().active);
        assert!(!t.prepared(1, 11, 1));
        assert!(!t.prepared(2, 11, 2));
        assert!(t.prepared(1, 11, 2));
        assert!(t.snapshot(2).unwrap().active);
        assert_eq!(t.crash(11, 12), ServiceAction::Deny);
    }
    #[test]
    fn exhausted_retries_deny_and_cannot_spoof_generation() {
        let mut t = ServiceTable::<1>::new();
        assert!(t.configure(1, 10, 0));
        assert_eq!(t.start(1), ServiceAction::Start { pid: 10, generation: 1 });
        assert_eq!(t.crash(10, 11), ServiceAction::Deny);
        assert!(!t.prepared(1, 11, 2));
        assert_eq!(t.snapshot(1).unwrap().restarts, 0);
    }
}
