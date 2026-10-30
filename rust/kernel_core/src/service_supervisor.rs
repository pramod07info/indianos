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
// Private authorization state: `active` remains a public snapshot field only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServicePhase { Configured, Stopped, Active, Pending, Exhausted }
pub struct ServiceTable<const N: usize> {
    phases: [ServicePhase; N],
    slots: [Option<ServiceSlot>; N],
}
impl<const N: usize> ServiceTable<N> {
    pub const fn new() -> Self { Self { slots: [None; N], phases: [ServicePhase::Configured; N] } }

    /// Trusted init configures a service before handing it to the scheduler.
    pub fn configure(&mut self, service_id: u32, pid: u32, max_restarts: u8) -> bool {
        if service_id == 0 || pid == 0 || self.slots.iter().flatten()
            .any(|s| s.service_id == service_id || s.pid == pid) {
            return false;
        }
        let Some((index, free)) = self.slots.iter_mut().enumerate().find(|(_, s)| s.is_none()) else { return false; };
        self.phases[index] = ServicePhase::Configured;
        *free = Some(ServiceSlot {
            service_id, pid, generation: 0, restarts: 0, max_restarts, active: false,
        });
        true
    }

    /// Start only a configured or deliberately stopped service. Pending
    /// replacements require `prepared`; exhausted services stay inactive.
    pub fn start(&mut self, service_id: u32) -> ServiceAction {
        let Some(index) = self.slots.iter().position(|s| s.as_ref().is_some_and(|s| s.service_id == service_id)) else { return ServiceAction::Deny; };
        if !matches!(self.phases[index], ServicePhase::Configured | ServicePhase::Stopped) {
            return ServiceAction::Deny;
        }
        let slot = self.slots[index].as_mut().unwrap();
        let Some(generation) = slot.generation.checked_add(1) else {
            self.phases[index] = ServicePhase::Exhausted;
            return ServiceAction::Deny;
        };
        self.phases[index] = ServicePhase::Active;
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
        let Some(index) = self.slots.iter().position(|s| s.as_ref().is_some_and(|s| s.pid == pid && s.active)) else { return ServiceAction::Deny; };
        if self.phases[index] != ServicePhase::Active { return ServiceAction::Deny; }
        let slot = self.slots[index].as_mut().unwrap();
        if slot.restarts >= slot.max_restarts { slot.active = false; self.phases[index] = ServicePhase::Exhausted; return ServiceAction::Deny; }
        let (Some(restarts), Some(generation)) =
            (slot.restarts.checked_add(1), slot.generation.checked_add(1)) else {
            slot.active = false;
            self.phases[index] = ServicePhase::Exhausted;
            return ServiceAction::Deny;
        };
        self.phases[index] = ServicePhase::Pending;
        slot.pid = replacement_pid;
        slot.restarts = restarts;
        slot.generation = generation;
        slot.active = false; // no grants until the replacement is prepared
        ServiceAction::Restart { pid: replacement_pid, generation }
    }

    /// Only the replacement for the *same* service/generation may become active.
    pub fn prepared(&mut self, service_id: u32, pid: u32, generation: u32) -> bool {
        if let Some(index) = self.slots.iter().position(|s| s.as_ref().is_some_and(|s| s.service_id == service_id && s.pid == pid && s.generation == generation)) {
            if self.phases[index] != ServicePhase::Pending { return false; }
            self.phases[index] = ServicePhase::Active;
            let slot = self.slots[index].as_mut().unwrap();
            slot.active = true;
            return true;
        }
        false
    }

    /// Stop an active service, invalidating its preparation token. Pending
    /// reservations are not cancelled; a stopped service may explicitly start.
    pub fn stop(&mut self, service_id: u32, pid: u32) -> bool {
        if let Some(index) = self.slots.iter().position(|s| s.as_ref().is_some_and(|s| s.service_id == service_id && s.pid == pid && s.active)) {
            self.phases[index] = ServicePhase::Stopped;
            self.slots[index].as_mut().unwrap().active = false;
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


/// Batch 1: stop an active service only for the exact authenticated process
/// generation. Stale PID reuse and supervisor identity confusion fail closed.
impl<const N: usize> ServiceTable<N> {
    pub fn stop_exact(&mut self, service_id: u32, authenticated_pid: u32,
        generation: u32) -> bool {
        if service_id == 0 || authenticated_pid == 0 || generation == 0 {
            return false;
        }
        let Some(index) = self.slots.iter().position(|s| s.as_ref().is_some_and(|s|
            s.service_id == service_id && s.pid == authenticated_pid &&
            s.generation == generation && s.active)) else { return false; };
        self.phases[index] = ServicePhase::Stopped;
        self.slots[index].as_mut().unwrap().active = false;
        true
    }
}
#[cfg(test)]
mod batch1_supervisor_generation_tests {
    use super::*;
    #[test]
    fn stale_generation_cannot_stop_replacement_service() {
        let mut services = ServiceTable::<2>::new();
        assert!(services.configure(1, 7, 2));
        assert!(services.configure(2, 8, 1));
        assert_eq!(services.start(1), ServiceAction::Start{pid:7,generation:1});
        assert_eq!(services.start(2), ServiceAction::Start{pid:8,generation:1});
        assert_eq!(services.crash(7,9),ServiceAction::Restart{pid:9,generation:2});
        assert!(services.prepared(1,9,2));
        assert!(!services.stop_exact(1,9,1));
        assert!(!services.stop_exact(1,7,2));
        assert!(services.snapshot(1).unwrap().active);
        assert!(services.stop_exact(1,9,2));
        assert!(services.snapshot(2).unwrap().active);
    }
    #[test]
    fn zero_and_mismatched_service_identity_is_rejected() {
        let mut services=ServiceTable::<1>::new();
        services.configure(1, 10, 1);
        services.start(1);
        assert!(!services.stop_exact(1, 10, 0));
        assert!(!services.stop_exact(2, 10, 1));
        assert!(services.stop_exact(1, 10, 1));
        assert!(!services.stop_exact(1, 10, 1));
    }
}



/// R017 workstation process panel and monitored service start group.
/// Only the trusted supervisor can call start_batch or view all services.
impl<const N:usize> ServiceTable<N>{
 pub fn snapshot_all(&self,out:&mut[ServiceSlot])->Option<usize>{
  let count=self.slots.iter().flatten().count();
  if count>out.len(){return None;}
  let mut at=0;
  for item in self.slots.iter().flatten(){out[at]=*item;at+=1;}
  Some(at)
 }
 pub fn start_batch(&mut self,services:&[u32])->bool{
  if services.is_empty()||services.len()>N{return false;}
  let mut shadow=Self{slots:self.slots,phases:self.phases};
  for (i,&service) in services.iter().enumerate(){
   if service==0||services[..i].contains(&service)||
      shadow.start(service)==ServiceAction::Deny{
    return false;
   }
  }
  self.slots=shadow.slots;self.phases=shadow.phases;true
 }
 /// Reconcile several trusted PID faults into one atomic restart plan.
 /// Any exhausted, duplicate or unknown request leaves every service
 /// in its previous phase and leaves caller output untouched.
 pub fn crash_batch(&mut self,failures:&[(u32,u32)],out:&mut[ServiceAction])->bool{
  if failures.is_empty()||failures.len()>N||out.len()<failures.len(){
   return false;
  }
  let mut staged=Self{phases:self.phases,slots:self.slots};
  let mut decisions=[ServiceAction::Deny;N];
  for (i,&(failed_pid,replacement_pid)) in failures.iter().enumerate(){
   if failed_pid==0||replacement_pid==0||
     failures[..i].iter().any(|&(old,new)|old==failed_pid||new==replacement_pid){
    return false;
   }
   let action=staged.crash(failed_pid,replacement_pid);
   if action==ServiceAction::Deny{return false;}
   decisions[i]=action;
  }
  self.phases=staged.phases;self.slots=staged.slots;
  out[..failures.len()].copy_from_slice(&decisions[..failures.len()]);
  true
 }
 /// Stop several active allowlisted services as one generation-pinned
 /// transaction. A stale PID, reused generation, duplicate service or
 /// missing service leaves every service's state unchanged.
 pub fn stop_batch_exact(&mut self,requests:&[(u32,u32,u32)])->bool{
  if requests.is_empty()||requests.len()>N{return false;}
  let mut staged=Self{phases:self.phases,slots:self.slots};
  for (i,&(service_id,pid,generation)) in requests.iter().enumerate(){
   if requests[..i].iter().any(|&(id,_,_)|id==service_id)||
      !staged.stop_exact(service_id,pid,generation){
    return false;
   }
  }
  self.phases=staged.phases;self.slots=staged.slots;true
 }
 pub fn active_services(&self)->usize{
  self.slots.iter().flatten().filter(|s|s.active).count()
 }
}
#[cfg(test)]mod r017_service_panel_tests{
 use super::*;
 #[test]fn m1_r052_multi_service_restarts_require_exact_preparation(){
  let mut t=ServiceTable::<3>::new();
  assert!(t.configure(1,10,2));assert!(t.configure(2,20,2));
  assert!(t.start_batch(&[1,2]));
  let mut actions=[ServiceAction::Deny;2];
  assert!(t.crash_batch(&[(10,11),(20,21)],&mut actions));
  assert_eq!(actions,[ServiceAction::Restart{pid:11,generation:2},
                      ServiceAction::Restart{pid:21,generation:2}]);
  assert_eq!(t.active_services(),0);
  assert!(!t.prepared(1,11,1));
  assert!(t.prepared(1,11,2));assert!(t.prepared(2,21,2));
  assert_eq!(t.active_services(),2);
 }
 #[test]fn m1_r052_bad_service_failover_batch_is_fail_atomic(){
  let mut t=ServiceTable::<2>::new();
  assert!(t.configure(1,10,1));assert!(t.configure(2,20,0));
  assert!(t.start_batch(&[1,2]));
  let original1=t.snapshot(1).unwrap();let original2=t.snapshot(2).unwrap();
  let mut output=[ServiceAction::Start{pid:77,generation:77};2];
  for inputs in [[(10,11),(20,21)],[(10,11),(10,22)],[(10,11),(20,11)]]{
   assert!(!t.crash_batch(&inputs,&mut output));
   assert_eq!(t.snapshot(1),Some(original1));
   assert_eq!(t.snapshot(2),Some(original2));
   assert_eq!(output,[ServiceAction::Start{pid:77,generation:77};2]);
  }
  assert!(!t.crash_batch(&[(10,11),(20,21)],&mut output[..1]));
  assert_eq!(t.active_services(),2);
 }
 #[test]fn m1_r053_authenticated_multi_service_stop_is_transactional(){
  let mut svc=ServiceTable::<3>::new();
  assert!(svc.configure(1,10,2));assert!(svc.configure(2,20,2));
  assert!(svc.start_batch(&[1,2]));
  assert!(svc.stop_batch_exact(&[(1,10,1),(2,20,1)]));
  assert_eq!(svc.active_services(),0);
  assert_eq!(svc.start(1),ServiceAction::Start{pid:10,generation:2});
  assert_eq!(svc.start(2),ServiceAction::Start{pid:20,generation:2});
  assert!(!svc.stop_batch_exact(&[(1,10,1),(2,20,2)]));
  assert_eq!(svc.active_services(),2);
 }
 #[test]fn m1_r053_stale_peer_generation_prevents_partial_shutdown(){
  let mut svc=ServiceTable::<2>::new();
  assert!(svc.configure(1,10,1));assert!(svc.configure(2,20,1));
  assert!(svc.start_batch(&[1,2]));
  let original_a=svc.snapshot(1);let original_b=svc.snapshot(2);
  for req in [
   [(1,10,1),(2,20,9)],
   [(1,10,1),(1,10,1)],
   [(1,10,1),(2,21,1)],
  ]{
   assert!(!svc.stop_batch_exact(&req));
   assert_eq!(svc.snapshot(1),original_a);
   assert_eq!(svc.snapshot(2),original_b);
   assert_eq!(svc.active_services(),2);
  }
 }
 #[test]fn two_dev_services_start_in_one_transaction(){
  let mut t=ServiceTable::<2>::new();
  assert!(t.configure(1,10,1));assert!(t.configure(2,20,1));
  assert!(t.start_batch(&[1,2]));assert_eq!(t.active_services(),2);
  let mut out=[t.snapshot(1).unwrap();2];
  assert_eq!(t.snapshot_all(&mut out),Some(2));
  assert_eq!(out[1].service_id,2);
 }
 #[test]fn bad_second_service_does_not_start_first(){
  let mut t=ServiceTable::<2>::new();t.configure(1,10,1);
  assert!(!t.start_batch(&[1,99]));assert_eq!(t.active_services(),0);
 }
 #[test]fn snapshot_too_short_never_publishes_partial_state(){
  let mut t=ServiceTable::<2>::new();t.configure(1,10,1);t.configure(2,20,1);
  let mut out=[t.snapshot(1).unwrap();1];
  assert_eq!(t.snapshot_all(&mut out),None);
  assert_eq!(out[0].service_id,1);
 }
}

#[cfg(test)]
mod restart_phase_tests {
    use super::*;

    #[test]
    fn pending_restart_cannot_bypass_preparation_or_disturb_peer() {
        let mut t = ServiceTable::<2>::new();
        t.configure(1, 10, 1); t.configure(2, 20, 1);
        assert!(t.start_batch(&[1, 2]));
        let peer = t.snapshot(2);
        assert_eq!(t.crash(10, 11), ServiceAction::Restart { pid: 11, generation: 2 });
        let pending = t.snapshot(1);
        assert_eq!(t.start(1), ServiceAction::Deny);
        assert!(!t.stop(1, 11));
        assert!(!t.stop_exact(1, 11, 2));
        assert_eq!(t.snapshot(1), pending);
        assert!(!t.prepared(1, 10, 2));
        assert!(t.prepared(1, 11, 2));
        assert!(!t.prepared(1, 11, 2));
        assert_eq!(t.snapshot(2), peer);
    }

    #[test]
    fn stop_invalidates_prepared_but_allows_explicit_new_start() {
        for exact in [false, true] {
            let mut t = ServiceTable::<1>::new();
            t.configure(1, 10, 2); t.start(1); t.crash(10, 11);
            assert!(t.prepared(1, 11, 2));
            assert!(if exact { t.stop_exact(1, 11, 2) } else { t.stop(1, 11) });
            assert!(!t.prepared(1, 11, 2));
            assert_eq!(t.start(1), ServiceAction::Start { pid: 11, generation: 3 });
            assert!(!t.prepared(1, 11, 2));
        }
    }

    #[test]
    fn retry_exhaustion_is_terminal_for_all_activation_paths() {
        let mut t = ServiceTable::<1>::new();
        t.configure(1, 10, 1); t.start(1); t.crash(10, 11);
        assert!(t.prepared(1, 11, 2));
        assert_eq!(t.crash(11, 12), ServiceAction::Deny);
        assert!(!t.prepared(1, 11, 2));
        assert_eq!(t.start(1), ServiceAction::Deny);
        assert!(!t.start_batch(&[1]));
        assert!(!t.snapshot(1).unwrap().active);
    }

    #[test]
    fn failed_batch_rolls_back_slot_and_phase_state() {
        let mut t = ServiceTable::<2>::new();
        t.configure(1, 10, 1); t.configure(2, 20, 1);
        t.start(2); t.crash(20, 21);
        let before = t.slots;
        let phases = t.phases;
        assert!(!t.start_batch(&[1, 2]));
        assert_eq!(t.slots, before); assert_eq!(t.phases, phases);
        assert_eq!(t.start(1), ServiceAction::Start { pid: 10, generation: 1 });
        assert!(t.prepared(2, 21, 2));
    }

    #[test]
    fn generation_overflow_fails_closed_for_start_and_crash() {
        let mut t = ServiceTable::<2>::new();
        t.configure(1, 10, 2); t.configure(2, 20, 2);
        t.slots[0].as_mut().unwrap().generation = u32::MAX;
        assert_eq!(t.start(1), ServiceAction::Deny);
        assert_eq!(t.phases[0], ServicePhase::Exhausted);
        assert!(!t.prepared(1, 10, u32::MAX));
        t.start(2);
        t.slots[1].as_mut().unwrap().generation = u32::MAX;
        assert_eq!(t.crash(20, 21), ServiceAction::Deny);
        assert!(!t.snapshot(2).unwrap().active);
        assert_eq!(t.start(2), ServiceAction::Deny);
        assert!(!t.prepared(2, 20, u32::MAX));
        assert_eq!(t.snapshot(2).unwrap().pid, 20);
        assert_eq!(t.snapshot(2).unwrap().restarts, 0);
    }
}
