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
 /// Retry an unprepared replacement after the scheduler has torn down
 /// and revoked its old mappings, grants and mailbox. The new PID must
 /// be unused and gets a fresh generation AND a charged retry budget.
 /// Pending remains non-active until prepared() is called.
 pub fn retry_pending_exact(&mut self,service_id:u32,failed_pid:u32,
   failed_generation:u32,fresh_pid:u32)->ServiceAction{
  if service_id==0||failed_pid==0||failed_generation==0||
     fresh_pid==0||fresh_pid==failed_pid||
     self.slots.iter().flatten().any(|s|s.pid==fresh_pid){
   return ServiceAction::Deny;
  }
  let Some(index)=self.slots.iter().position(|s|
   s.as_ref().is_some_and(|s|s.service_id==service_id&&
    s.pid==failed_pid&&s.generation==failed_generation))
  else{return ServiceAction::Deny};
  if self.phases[index]!=ServicePhase::Pending{
   return ServiceAction::Deny;
  }
  let slot=self.slots[index].as_mut().unwrap();
  if slot.restarts>=slot.max_restarts{return ServiceAction::Deny;}
  let (Some(retries),Some(generation))=
   (slot.restarts.checked_add(1),slot.generation.checked_add(1))
  else{return ServiceAction::Deny};
  slot.pid=fresh_pid;slot.restarts=retries;slot.generation=generation;
  ServiceAction::Restart{pid:fresh_pid,generation}
 }
 /// Multiple pending restart retries are planned against a shadow copy.
 /// A duplicate, stale or over-budget request leaves state and output
 /// unchanged; peer services cannot be partially redirected.
 pub fn retry_pending_batch_exact(&mut self,
   requests:&[(u32,u32,u32,u32)],out:&mut[ServiceAction])->bool{
  if requests.is_empty()||requests.len()>N||out.len()<requests.len(){
   return false;
  }
  let mut staged=Self{phases:self.phases,slots:self.slots};
  let mut actions=[ServiceAction::Deny;N];
  for (i,&(id,pid,generation,replacement)) in requests.iter().enumerate(){
   if requests[..i].iter().any(|&(prior_id,_,_,_)|prior_id==id){
    return false;
   }
   let action=staged.retry_pending_exact(id,pid,generation,replacement);
   if action==ServiceAction::Deny{return false;}
   actions[i]=action;
  }
  self.phases=staged.phases;self.slots=staged.slots;
  out[..requests.len()].copy_from_slice(&actions[..requests.len()]);
  true
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
 #[test]fn r054_retry_pending_requires_new_pid_epoch_and_budget(){
  let mut t=ServiceTable::<2>::new();
  assert!(t.configure(1,10,2));assert!(t.configure(2,20,1));
  assert!(t.start_batch(&[1,2]));
  assert_eq!(t.crash(10,11),ServiceAction::Restart{pid:11,generation:2});
  assert_eq!(t.retry_pending_exact(1,11,1,12),ServiceAction::Deny);
  assert_eq!(t.retry_pending_exact(1,11,2,20),ServiceAction::Deny);
  assert_eq!(t.retry_pending_exact(1,11,2,12),
   ServiceAction::Restart{pid:12,generation:3});
  assert!(!t.prepared(1,11,2));
  assert_eq!(t.active_services(),1);
  assert!(t.prepared(1,12,3));
  assert_eq!(t.snapshot(1).unwrap().restarts,2);
  assert_eq!(t.crash(12,13),ServiceAction::Deny);
  assert_eq!(t.retry_pending_exact(1,12,3,14),ServiceAction::Deny);
  assert!(t.snapshot(2).unwrap().active);
 }
 #[test]fn r054_batch_retry_has_no_partial_pid_reassignment(){
  let mut t=ServiceTable::<2>::new();
  assert!(t.configure(1,10,3));assert!(t.configure(2,20,1));
  assert!(t.start_batch(&[1,2]));
  assert!(t.crash_batch(&[(10,11),(20,21)],
   &mut[ServiceAction::Deny;2]));
  let before1=t.snapshot(1);let before2=t.snapshot(2);
  let mut out=[ServiceAction::Start{pid:90,generation:90};2];
  assert!(!t.retry_pending_batch_exact(
   &[(1,11,2,12),(2,21,2,22)],&mut out));
  assert_eq!(t.snapshot(1),before1);
  assert_eq!(t.snapshot(2),before2);
  assert_eq!(out,[ServiceAction::Start{pid:90,generation:90};2]);
  assert_eq!(t.retry_pending_exact(1,11,2,12),
   ServiceAction::Restart{pid:12,generation:3});
  assert!(!t.prepared(1,11,2));
  assert_eq!(t.active_services(),0);
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


// R084 / process-supervision workstream: trustworthy lifecycle inventory,
// generation-pinned transitions, and removal of stopped configurations.
// No untrusted userspace command can call these methods directly.
impl<const N:usize> ServiceTable<N>{
    /// Count configured service records regardless of phase.
    pub fn configured_count(&self)->usize{
        self.slots.iter().flatten().count()
    }
    /// Count pending restarts requiring real scheduler preparation.
    pub fn pending_count(&self)->usize{
        (0..N).filter(|&i|self.slots[i].is_some()&&self.phases[i]==ServicePhase::Pending).count()
    }
    /// Count terminally exhausted slots that require an operator decision.
    pub fn exhausted_count(&self)->usize{
        (0..N).filter(|&i|self.slots[i].is_some()&&self.phases[i]==ServicePhase::Exhausted).count()
    }
    /// Remaining generations of restart budget for the named service.
    pub fn remaining_restarts(&self,id:u32)->Option<u8>{
        let s=self.snapshot(id)?;
        // An exhausted generation cannot be renewed even when the
        // configured retry budget still has unused credits.
        Some(if s.generation==u32::MAX{0}
             else{s.max_restarts.saturating_sub(s.restarts)})
    }
    /// Lookup real service ID only for a nonzero, uniquely allocated PID.
    pub fn owner_of_pid(&self,pid:u32)->Option<u32>{
        if pid==0{return None;}
        self.slots.iter().flatten().find(|s|s.pid==pid).map(|s|s.service_id)
    }
    /// Check exact pending replacement, not just PID equality.
    pub fn pending_exact(&self,id:u32,pid:u32,generation:u32)->bool{
        id!=0&&pid!=0&&generation!=0&&self.slots.iter().enumerate().any(|(i,s)|
            self.phases[i]==ServicePhase::Pending&&s.is_some_and(|slot|
                slot.service_id==id&&slot.pid==pid&&slot.generation==generation))
    }
    /// Snapshot only pending services, fail atomically on a short output.
    pub fn snapshot_pending(&self,out:&mut[ServiceSlot])->Option<usize>{
        let required=self.pending_count();
        if out.len()<required{return None;}
        let mut at=0;
        for i in 0..N{
            if self.phases[i]==ServicePhase::Pending{
                if let Some(s)=self.slots[i]{out[at]=s;at+=1;}
            }
        }
        Some(at)
    }
    /// Snapshot exhausted services without leaking partial output.
    pub fn snapshot_exhausted(&self,out:&mut[ServiceSlot])->Option<usize>{
        let required=self.exhausted_count();
        if out.len()<required{return None;}
        let mut at=0;
        for i in 0..N{
            if self.phases[i]==ServicePhase::Exhausted{
                if let Some(s)=self.slots[i]{out[at]=s;at+=1;}
            }
        }
        Some(at)
    }
    /// Generation-conditional service start to reject stale admin actions.
    pub fn start_if_generation(&mut self,id:u32,expected:u32)->ServiceAction{
        if id==0||self.snapshot(id).is_none_or(|s|s.generation!=expected){
            return ServiceAction::Deny;
        }
        self.start(id)
    }
    /// Unconfigure only a never-started or deliberately stopped service;
    /// pending/active/exhausted services need trusted higher-level cleanup.
    pub fn remove_stopped_config(&mut self,id:u32,pid:u32,generation:u32)->bool{
        if id==0||pid==0{return false;}
        let Some(i)=self.slots.iter().position(|s|s.is_some_and(|v|
           v.service_id==id&&v.pid==pid&&v.generation==generation))
           else{return false;};
        if !matches!(self.phases[i],ServicePhase::Configured|ServicePhase::Stopped){
            return false;
        }
        self.slots[i]=None;self.phases[i]=ServicePhase::Configured;true
    }
}
#[cfg(test)]
mod r084_supervisor_inventory_tests{
    use super::*;
    #[test]
    fn inventories_are_phase_correct_and_never_release_unprepared_service(){
        let mut t=ServiceTable::<3>::new();
        assert_eq!(t.configured_count(),0);
        assert!(t.configure(1,10,1));
        assert!(t.configure(2,20,0));
        assert!(t.configure(3,30,2));
        assert_eq!(t.configured_count(),3);
        assert_eq!(t.start_if_generation(1,9),ServiceAction::Deny);
        assert_eq!(t.start_if_generation(1,0),ServiceAction::Start{pid:10,generation:1});
        assert_eq!(t.start_if_generation(2,0),ServiceAction::Start{pid:20,generation:1});
        assert_eq!(t.crash(10,11),ServiceAction::Restart{pid:11,generation:2});
        assert_eq!(t.crash(20,21),ServiceAction::Deny);
        assert_eq!(t.pending_count(),1);
        assert_eq!(t.exhausted_count(),1);
        assert_eq!(t.active_services(),0);
        assert_eq!(t.remaining_restarts(1),Some(0));
        assert_eq!(t.owner_of_pid(11),Some(1));
        assert_eq!(t.owner_of_pid(0),None);
        assert!(t.pending_exact(1,11,2));
        assert!(!t.pending_exact(1,10,2));
        let empty=[ServiceSlot{service_id:99,pid:99,generation:99,
            restarts:99,max_restarts:99,active:false};0];
        let mut tiny=empty;
        assert_eq!(t.snapshot_pending(&mut tiny),None);
        assert_eq!(t.snapshot_exhausted(&mut tiny),None);
        let filler=t.snapshot(1).unwrap();let mut out=[filler;3];
        assert_eq!(t.snapshot_pending(&mut out),Some(1));
        assert_eq!(out[0].pid,11);
        assert_eq!(t.snapshot_exhausted(&mut out),Some(1));
        assert_eq!(out[0].service_id,2);
        assert!(!t.remove_stopped_config(1,11,2));
        assert!(!t.remove_stopped_config(2,20,1));
        assert!(t.remove_stopped_config(3,30,0));
        assert_eq!(t.configured_count(),2);
        assert!(t.configure(3,30,3));
    }
    #[test]
    fn retired_exact_stopped_process_cannot_be_reused_by_stale_pid(){
        let mut t=ServiceTable::<1>::new();
        assert!(t.configure(7,70,3));
        assert_eq!(t.start(7),ServiceAction::Start{pid:70,generation:1});
        assert!(t.stop_exact(7,70,1));
        assert!(!t.remove_stopped_config(7,70,0));
        assert!(!t.remove_stopped_config(7,71,1));
        assert!(t.remove_stopped_config(7,70,1));
        assert!(t.configure(8,70,0));
        assert_eq!(t.start_if_generation(7,1),ServiceAction::Deny);
        assert_eq!(t.owner_of_pid(70),Some(8));
    }
}


// R084 extension: generation-bound service lifecycle transactions.
// Only trusted scheduler/init code holds a mutable ServiceTable reference.
impl<const N:usize> ServiceTable<N>{
    /// 1. Count deliberately stopped service configurations.
    pub fn stopped_count(&self)->usize{
        (0..N).filter(|&i|self.slots[i].is_some()&&
            self.phases[i]==ServicePhase::Stopped).count()
    }
    /// 2. Count configured services never previously activated.
    pub fn never_started_count(&self)->usize{
        (0..N).filter(|&i|self.slots[i].is_some()&&
            self.phases[i]==ServicePhase::Configured).count()
    }
    /// 3. Services which may be started without pending-restart preparation.
    pub fn runnable_count(&self)->usize{
        (0..N).filter(|&i|self.slots[i].is_some()&&
            matches!(self.phases[i],ServicePhase::Configured|ServicePhase::Stopped)).count()
    }
    /// 4. Pending services with remaining authenticated restart budgets.
    pub fn retryable_pending_count(&self)->usize{
        (0..N).filter(|&i|self.phases[i]==ServicePhase::Pending&&
            self.slots[i].is_some_and(|s|
                s.restarts<s.max_restarts&&s.generation<u32::MAX)).count()
    }
    /// 5. Lookup the current generation associated with an exact owned PID.
    pub fn generation_for_pid(&self,pid:u32)->Option<u32>{
        if pid==0{return None;}
        self.slots.iter().flatten().find(|s|s.pid==pid).map(|s|s.generation)
    }
    /// 6. Active process identity never aliases an unprepared replacement.
    pub fn active_pid_for_service(&self,service:u32)->Option<u32>{
        if service==0{return None;}
        self.slots.iter().enumerate().find(|(i,s)|
            self.phases[*i]==ServicePhase::Active&&
            s.is_some_and(|v|v.service_id==service&&v.active))
            .and_then(|(_,s)|s.map(|v|v.pid))
    }
    /// 7. Exact generation-bound active process verification.
    pub fn is_active_exact(&self,id:u32,pid:u32,generation:u32)->bool{
        generation!=0&&pid!=0&&
            self.active_pid_for_service(id)==Some(pid)&&
            self.snapshot(id).is_some_and(|s|s.generation==generation)
    }
    /// 8. Pending service replacement is never marked active early.
    pub fn is_pending_service(&self,id:u32)->bool{
        id!=0&&self.slots.iter().enumerate().any(|(i,s)|
            self.phases[i]==ServicePhase::Pending&&
            s.is_some_and(|v|v.service_id==id))
    }
    /// 9. Explicitly distinguish exhausted state from no such ID.
    pub fn is_exhausted_service(&self,id:u32)->bool{
        id!=0&&self.slots.iter().enumerate().any(|(i,s)|
            self.phases[i]==ServicePhase::Exhausted&&
            s.is_some_and(|v|v.service_id==id))
    }
    /// 10. Only active services with a remaining restart budget may recover.
    pub fn can_restart_service(&self,id:u32)->bool{
        id!=0&&self.slots.iter().enumerate().any(|(i,s)|
            self.phases[i]==ServicePhase::Active&&s.is_some_and(|v|
                v.service_id==id&&v.active&&v.restarts<v.max_restarts&&
                v.generation<u32::MAX))
    }
    /// 11. Snapshot all active processes; short buffer publishes nothing.
    pub fn snapshot_active(&self,out:&mut[ServiceSlot])->Option<usize>{
        let count=self.active_services();if out.len()<count{return None;}
        let mut at=0;
        for (i,s) in self.slots.iter().enumerate(){
            if self.phases[i]==ServicePhase::Active{
                if let Some(v)=s{out[at]=*v;at+=1;}
            }
        }
        Some(at)
    }
    /// 12. Snapshot deliberately stopped services for a trusted process UI.
    pub fn snapshot_stopped(&self,out:&mut[ServiceSlot])->Option<usize>{
        let count=self.stopped_count();if out.len()<count{return None;}
        let mut at=0;
        for (i,s) in self.slots.iter().enumerate(){
            if self.phases[i]==ServicePhase::Stopped{
                if let Some(v)=s{out[at]=*v;at+=1;}
            }
        }
        Some(at)
    }
    /// 13. Snapshot runnable services without disclosing pending PIDs as active.
    pub fn snapshot_runnable(&self,out:&mut[ServiceSlot])->Option<usize>{
        let count=self.runnable_count();if out.len()<count{return None;}
        let mut at=0;
        for (i,s) in self.slots.iter().enumerate(){
            if matches!(self.phases[i],ServicePhase::Configured|ServicePhase::Stopped){
                if let Some(v)=s{out[at]=*v;at+=1;}
            }
        }
        Some(at)
    }
    /// 14. Real remaining service capacity (not number of free PIDs).
    pub fn available_service_slots(&self)->usize{
        self.slots.iter().filter(|s|s.is_none()).count()
    }
    /// 15. Provision distinct service IDs/PIDs with an all-or-nothing plan.
    pub fn configure_batch(&mut self,requests:&[(u32,u32,u8)])->bool{
        if requests.is_empty()||requests.len()>N{return false;}
        let mut staged=Self{slots:self.slots,phases:self.phases};
        for &(id,pid,budget) in requests{
            if !staged.configure(id,pid,budget){return false;}
        }
        self.slots=staged.slots;self.phases=staged.phases;true
    }
    /// 16. Start distinct services only at the requested observed generation.
    pub fn start_batch_if_generations(&mut self,requests:&[(u32,u32)])->bool{
        if requests.is_empty()||requests.len()>N{return false;}
        let mut staged=Self{slots:self.slots,phases:self.phases};
        for (i,&(id,expected)) in requests.iter().enumerate(){
            if requests[..i].iter().any(|(old,_)|*old==id)||
                staged.start_if_generation(id,expected)==ServiceAction::Deny{
                return false;
            }
        }
        self.slots=staged.slots;self.phases=staged.phases;true
    }
    /// 17. Prepare a complete set of replacement services atomically.
    pub fn prepare_batch_exact(&mut self,requests:&[(u32,u32,u32)])->bool{
        if requests.is_empty()||requests.len()>N{return false;}
        let mut staged=Self{slots:self.slots,phases:self.phases};
        for (i,&(id,pid,generation)) in requests.iter().enumerate(){
            if requests[..i].iter().any(|(old,_,_)|*old==id)||
                !staged.prepared(id,pid,generation){return false;}
        }
        self.slots=staged.slots;self.phases=staged.phases;true
    }
    /// 18. Retire only exact stopped/configured identities as one unit.
    pub fn remove_stopped_batch(&mut self,requests:&[(u32,u32,u32)])->bool{
        if requests.is_empty()||requests.len()>N{return false;}
        let mut staged=Self{slots:self.slots,phases:self.phases};
        for (i,&(id,pid,generation)) in requests.iter().enumerate(){
            if requests[..i].iter().any(|(old,_,_)|*old==id)||
                !staged.remove_stopped_config(id,pid,generation){return false;}
        }
        self.slots=staged.slots;self.phases=staged.phases;true
    }
    /// 19. A deliberately exhausted service may be unconfigured by trusted
    /// init, but never automatically restarted with a cleared retry budget.
    pub fn retire_exhausted_exact(&mut self,id:u32,pid:u32,generation:u32)->bool{
        if id==0||pid==0||generation==0{return false;}
        let Some(i)=self.slots.iter().position(|s|s.is_some_and(|v|
            v.service_id==id&&v.pid==pid&&v.generation==generation))
            else{return false;};
        if self.phases[i]!=ServicePhase::Exhausted{return false;}
        self.slots[i]=None;self.phases[i]=ServicePhase::Configured;true
    }
    /// 20. Reclaim multiple exhausted services with exact generation pins
    /// and transactional rollback if even one peer is not exhausted.
    pub fn retire_exhausted_batch(&mut self,requests:&[(u32,u32,u32)])->bool{
        if requests.is_empty()||requests.len()>N{return false;}
        let mut staged=Self{slots:self.slots,phases:self.phases};
        for (i,&(id,pid,generation)) in requests.iter().enumerate(){
            if requests[..i].iter().any(|(old,_,_)|*old==id)||
                !staged.retire_exhausted_exact(id,pid,generation){return false;}
        }
        self.slots=staged.slots;self.phases=staged.phases;true
    }
}
#[cfg(test)]
mod r084_extended_supervisor_tests{
    use super::*;
    #[test]
    fn twenty_lifecycle_services_require_generation_pins_and_no_partial_edits(){
        let mut t=ServiceTable::<4>::new();
        assert!(t.configure_batch(&[(1,11,1),(2,22,0),(3,33,2)]));
        assert_eq!(t.available_service_slots(),1);
        assert_eq!(t.never_started_count(),3);
        assert_eq!(t.runnable_count(),3);
        let filler=t.snapshot(1).unwrap();let mut short=[filler;2];
        assert_eq!(t.snapshot_runnable(&mut short),None);
        assert_eq!(short,[filler;2]);
        assert!(!t.start_batch_if_generations(&[(1,0),(2,9)]));
        assert_eq!(t.never_started_count(),3);
        assert!(t.start_batch_if_generations(&[(1,0),(2,0)]));
        assert_eq!(t.active_pid_for_service(1),Some(11));
        assert!(t.is_active_exact(1,11,1));
        assert!(t.can_restart_service(1));
        assert!(!t.can_restart_service(2));
        assert_eq!(t.generation_for_pid(22),Some(1));
        assert_eq!(t.crash(11,12),ServiceAction::Restart{pid:12,generation:2});
        assert_eq!(t.crash(22,23),ServiceAction::Deny);
        assert_eq!(t.pending_count(),1);
        assert_eq!(t.retryable_pending_count(),0);
        assert!(t.is_pending_service(1));assert!(t.is_exhausted_service(2));
        assert_eq!(t.active_pid_for_service(1),None);
        assert_eq!(t.exhausted_count(),1);
        let mut out=[filler;4];
        assert_eq!(t.snapshot_active(&mut out),Some(0));
        assert_eq!(t.snapshot_stopped(&mut out),Some(0));
        assert_eq!(t.snapshot_runnable(&mut out),Some(1));
        assert!(!t.prepare_batch_exact(&[(1,12,2),(2,23,2)]));
        assert!(t.is_pending_service(1));
        assert!(t.prepare_batch_exact(&[(1,12,2)]));
        assert!(t.is_active_exact(1,12,2));
        assert!(t.stop_exact(1,12,2));
        assert_eq!(t.stopped_count(),1);
        assert!(!t.remove_stopped_batch(&[(1,12,2),(2,22,1)]));
        assert_eq!(t.stopped_count(),1);
        assert!(t.remove_stopped_batch(&[(1,12,2),(3,33,0)]));
        assert_eq!(t.available_service_slots(),3);
        assert!(!t.retire_exhausted_exact(2,22,0));
        assert!(t.retire_exhausted_batch(&[(2,22,1)]));
        assert_eq!(t.configured_count(),0);
    }
    #[test]
    fn stale_or_conflicting_setup_batches_leave_all_peer_services_untouched(){
        let mut t=ServiceTable::<2>::new();
        assert!(!t.configure_batch(&[(5,15,1),(6,15,1)]));
        assert_eq!(t.configured_count(),0);
        assert!(t.configure_batch(&[(5,15,1),(6,16,1)]));
        assert!(!t.start_batch_if_generations(&[(5,0),(5,0)]));
        assert_eq!(t.active_services(),0);
        assert_eq!(t.available_service_slots(),0);
        assert!(t.start_batch_if_generations(&[(5,0)]));
        assert_eq!(t.snapshot_active(&mut[t.snapshot(5).unwrap();1]),Some(1));
    }
}


// R084 extension III: scheduler-backed service lifecycle audits and
// generation-pinned administrative transitions. All reads use actual slots.
impl<const N:usize> ServiceTable<N> {
    /// 1. Configured maximum number of future service restarts.
    pub fn restart_capacity_total(&self)->u64{
        self.slots.iter().flatten().map(|s|s.max_restarts as u64).sum()
    }
    /// 2. Number of real retry-budget credits already consumed.
    pub fn restart_credits_used(&self)->u64{
        self.slots.iter().flatten().map(|s|s.restarts as u64).sum()
    }
    /// 3. Remaining retry credits across all process services.
    pub fn restart_credits_left(&self)->u64{
        self.slots.iter().flatten().map(|s|
            (if s.generation==u32::MAX{0}
              else{s.max_restarts.saturating_sub(s.restarts)}) as u64).sum()
    }
    /// 4. Exact current service epoch check.
    pub fn generation_matches(&self,id:u32,epoch:u32)->bool{
        id!=0&&self.snapshot(id).is_some_and(|s|s.generation==epoch)
    }
    /// 5. Detect PID collisions during new process admission.
    pub fn pid_is_reserved(&self,pid:u32)->bool{
        pid!=0&&self.slots.iter().flatten().any(|s|s.pid==pid)
    }
    /// 6. Count active verified service records, excluding pending.
    pub fn active_slot_count(&self)->usize{
        (0..N).filter(|&i|self.phases[i]==ServicePhase::Active&&
           self.slots[i].is_some_and(|s|s.active)).count()
    }
    /// 7. Snapshot configured service IDs without partial output.
    pub fn service_ids(&self,out:&mut[u32])->Option<usize>{
        let need=self.configured_count();
        if out.len()<need{return None;}
        let mut k=0;for s in self.slots.iter().flatten(){
            out[k]=s.service_id;k+=1;
        }
        Some(k)
    }
    /// 8. Snapshot active IDs; pending replacements are NEVER active.
    pub fn active_ids(&self,out:&mut[u32])->Option<usize>{
        let need=self.active_slot_count();
        if out.len()<need{return None;}
        let mut k=0;for i in 0..N{
            if self.phases[i]==ServicePhase::Active{
                if let Some(s)=self.slots[i]{
                    if s.active{out[k]=s.service_id;k+=1;}
                }
            }
        }
        Some(k)
    }
    /// 9. Snapshot pending service IDs.
    pub fn pending_ids(&self,out:&mut[u32])->Option<usize>{
        let need=self.pending_count();
        if out.len()<need{return None;}
        let mut k=0;for i in 0..N{
            if self.phases[i]==ServicePhase::Pending{
                if let Some(s)=self.slots[i]{out[k]=s.service_id;k+=1;}
            }
        }
        Some(k)
    }
    /// 10. Snapshot exhausted service IDs.
    pub fn exhausted_ids(&self,out:&mut[u32])->Option<usize>{
        let need=self.exhausted_count();
        if out.len()<need{return None;}
        let mut k=0;for i in 0..N{
            if self.phases[i]==ServicePhase::Exhausted{
                if let Some(s)=self.slots[i]{out[k]=s.service_id;k+=1;}
            }
        }
        Some(k)
    }
    /// 11. Snapshot currently stopped service IDs.
    pub fn stopped_ids(&self,out:&mut[u32])->Option<usize>{
        let need=self.stopped_count();
        if out.len()<need{return None;}
        let mut k=0;for i in 0..N{
            if self.phases[i]==ServicePhase::Stopped{
                if let Some(s)=self.slots[i]{out[k]=s.service_id;k+=1;}
            }
        }
        Some(k)
    }
    /// 12. Snapshot never-started and explicitly stopped services.
    pub fn runnable_ids(&self,out:&mut[u32])->Option<usize>{
        let need=self.runnable_count();
        if out.len()<need{return None;}
        let mut k=0;for i in 0..N{
            if matches!(self.phases[i],ServicePhase::Configured|ServicePhase::Stopped){
                if let Some(s)=self.slots[i]{out[k]=s.service_id;k+=1;}
            }
        }
        Some(k)
    }
    /// 13. Exact observed-epoch snapshot, reject recycled process records.
    pub fn snapshot_at_epoch(&self,id:u32,generation:u32)->Option<ServiceSlot>{
        if id==0||generation==0{return None;}
        self.snapshot(id).filter(|s|s.generation==generation)
    }
    /// 14. Resolve service ID by both PID and incarnation epoch.
    pub fn service_at_pid_epoch(&self,pid:u32,generation:u32)->Option<u32>{
        if pid==0||generation==0{return None;}
        self.slots.iter().flatten().find(|s|
            s.pid==pid&&s.generation==generation).map(|s|s.service_id)
    }
    /// 15. Exact service configuration is stopped, not pending/restarted.
    pub fn is_stopped_exact(&self,id:u32,pid:u32,generation:u32)->bool{
        id!=0&&pid!=0&&generation!=0&&self.slots.iter().enumerate().any(|(i,s)|
            self.phases[i]==ServicePhase::Stopped&&s.is_some_and(|v|
                v.service_id==id&&v.pid==pid&&v.generation==generation))
    }
    /// 16. Only generation-zero entries are not yet started.
    pub fn is_unstarted_service(&self,id:u32)->bool{
        id!=0&&self.slots.iter().enumerate().any(|(i,s)|
            self.phases[i]==ServicePhase::Configured&&s.is_some_and(|v|
                v.service_id==id&&v.generation==0))
    }
    /// 17. Trusted scheduler can start only if the phase is runnable.
    pub fn can_start_at_epoch(&self,id:u32,generation:u32)->bool{
        id!=0&&self.slots.iter().enumerate().any(|(i,s)|
            matches!(self.phases[i],ServicePhase::Configured|ServicePhase::Stopped)
            &&s.is_some_and(|v|v.service_id==id&&v.generation==generation)
            &&generation<u32::MAX)
    }
    /// 18. Stop only the current active process generation.
    pub fn stop_if_epoch(&mut self,id:u32,expected_generation:u32)->bool{
        if expected_generation==0{return false;}
        let Some(s)=self.snapshot_at_epoch(id,expected_generation)else{return false;};
        self.stop_exact(id,s.pid,expected_generation)
    }
    /// 19. Trusted supervisor shutdown: retire all active services but do
    /// not activate pending replacements, nor clear their restart counters.
    pub fn stop_all_active(&mut self)->usize{
        let mut stopped=0;
        for i in 0..N{
            if self.phases[i]==ServicePhase::Active{
                if let Some(s)=self.slots[i]{
                    if s.active{
                        self.phases[i]=ServicePhase::Stopped;
                        self.slots[i].as_mut().unwrap().active=false;
                        stopped+=1;
                    }
                }
            }
        }
        stopped
    }
    /// 20. Retire only stopped service entries after scheduler teardown.
    /// Does not reclaim active, pending or exhausted process identities.
    pub fn retire_all_stopped(&mut self)->usize{
        let mut retired=0;
        for i in 0..N{
            if self.phases[i]==ServicePhase::Stopped&&self.slots[i].is_some(){
                self.slots[i]=None;self.phases[i]=ServicePhase::Configured;retired+=1;
            }
        }
        retired
    }
}
#[cfg(test)]
mod r084_lifecycle_inventory_batch_tests{
    use super::*;
    #[test]
    fn exact_epoch_snapshots_batch_shutdown_and_retirement_guard_pending_pids(){
        let mut table=ServiceTable::<4>::new();
        assert!(table.configure_batch(&[(1,11,2),(2,22,0),(3,33,1)]));
        assert_eq!(table.restart_capacity_total(),3);
        assert_eq!(table.restart_credits_used(),0);
        assert_eq!(table.restart_credits_left(),3);
        assert!(table.generation_matches(1,0));
        assert!(table.pid_is_reserved(11));
        assert!(!table.pid_is_reserved(99));
        assert_eq!(table.active_slot_count(),0);
        let mut short=[91u32;2];
        assert_eq!(table.service_ids(&mut short),None);
        assert_eq!(short,[91,91]);
        let mut ids=[0u32;4];
        assert_eq!(table.service_ids(&mut ids),Some(3));
        assert_eq!(&ids[..3],&[1,2,3]);
        assert_eq!(table.runnable_ids(&mut ids),Some(3));
        assert!(table.is_unstarted_service(3));
        assert!(table.can_start_at_epoch(1,0));
        assert!(table.start_batch_if_generations(&[(1,0),(2,0)]));
        assert_eq!(table.active_ids(&mut ids),Some(2));
        assert_eq!(&ids[..2],&[1,2]);
        assert_eq!(table.snapshot_at_epoch(1,1).unwrap().pid,11);
        assert_eq!(table.service_at_pid_epoch(11,1),Some(1));
        assert_eq!(table.service_at_pid_epoch(11,9),None);
        assert!(!table.is_stopped_exact(1,11,1));
        assert_eq!(table.crash(11,12),ServiceAction::Restart{pid:12,generation:2});
        assert_eq!(table.crash(22,23),ServiceAction::Deny);
        assert_eq!(table.restart_credits_used(),1);
        assert_eq!(table.restart_credits_left(),2);
        assert_eq!(table.pending_ids(&mut ids),Some(1));
        assert_eq!(ids[0],1);
        assert_eq!(table.exhausted_ids(&mut ids),Some(1));
        assert_eq!(ids[0],2);
        assert_eq!(table.active_ids(&mut ids),Some(0));
        assert!(!table.can_start_at_epoch(1,2));
        assert!(!table.stop_if_epoch(1,2));
        assert_eq!(table.stop_all_active(),0);
        assert!(table.start_if_generation(3,0)!=ServiceAction::Deny);
        assert_eq!(table.stop_all_active(),1);
        assert_eq!(table.stopped_ids(&mut ids),Some(1));
        assert!(table.is_stopped_exact(3,33,1));
        assert_eq!(table.retire_all_stopped(),1);
        assert_eq!(table.configured_count(),2);
        assert_eq!(table.pending_count(),1);
        assert_eq!(table.exhausted_count(),1);
    }
}


// M1/R084 extension IV — 20 service budget, identity and shutdown operations.
// Scheduler must authenticate process identity before any mutable call.
impl<const N:usize> ServiceTable<N>{
    /// 1. How many processes have ever entered an active generation?
    pub fn ever_started_count(&self)->usize{
        self.slots.iter().flatten().filter(|s|s.generation>0).count()
    }
    /// 2. How many process generations have consumed restart budget?
    pub fn services_with_retries(&self)->usize{
        self.slots.iter().flatten().filter(|s|s.restarts>0).count()
    }
    /// 3. Count records with no more available restart budget.
    pub fn services_without_retry_budget(&self)->usize{
        self.slots.iter().flatten().filter(|s|s.restarts>=s.max_restarts).count()
    }
    /// 4. Highest generation of configured service PID incarnations.
    pub fn maximum_service_generation(&self)->Option<u32>{
        self.slots.iter().flatten().map(|s|s.generation).max()
    }
    /// 5. Lowest configured service identifier for deterministic init order.
    pub fn minimum_service_id(&self)->Option<u32>{
        self.slots.iter().flatten().map(|s|s.service_id).min()
    }
    /// 6. Highest configured service identifier, excluding empty records.
    pub fn maximum_service_id(&self)->Option<u32>{
        self.slots.iter().flatten().map(|s|s.service_id).max()
    }
    /// 7. Check service admission without changing a single process record.
    pub fn can_configure(&self,id:u32,pid:u32)->bool{
        id!=0&&pid!=0&&self.available_service_slots()>0&&
        !self.slots.iter().flatten().any(|s|s.service_id==id||s.pid==pid)
    }
    /// 8. Check an active exact caller PID and generation after authentication.
    pub fn is_active_pid_epoch(&self,pid:u32,generation:u32)->bool{
        if pid==0||generation==0{return false;}
        self.slots.iter().enumerate().any(|(i,s)|
            self.phases[i]==ServicePhase::Active&&s.is_some_and(|v|
                v.pid==pid&&v.generation==generation&&v.active))
    }
    /// 9. Count jobs with an exact observed process epoch.
    pub fn count_generation(&self,generation:u32)->usize{
        self.slots.iter().flatten().filter(|s|s.generation==generation).count()
    }
    /// 10. List services with depleted budgets, preflight caller capacity.
    pub fn depleted_service_ids(&self,out:&mut[u32])->Option<usize>{
        let need=self.services_without_retry_budget();
        if out.len()<need{return None;}
        let mut i=0;
        for s in self.slots.iter().flatten(){
            if s.restarts>=s.max_restarts{out[i]=s.service_id;i+=1;}
        }
        Some(i)
    }
    /// 11. List exact service identities with consumed retries.
    pub fn restarted_service_ids(&self,out:&mut[u32])->Option<usize>{
        let need=self.services_with_retries();
        if out.len()<need{return None;}
        let mut i=0;
        for s in self.slots.iter().flatten(){
            if s.restarts>0{out[i]=s.service_id;i+=1;}
        }
        Some(i)
    }
    /// 12. Snapshot all configured exact PID/generation pairs.
    pub fn snapshot_pid_epochs(&self,out:&mut[(u32,u32)])->Option<usize>{
        let need=self.configured_count();
        if out.len()<need{return None;}
        let mut i=0;
        for s in self.slots.iter().flatten(){
            out[i]=(s.pid,s.generation);i+=1;
        }
        Some(i)
    }
    /// 13. Compare-and-narrow restart capacity on an unstarted service.
    pub fn narrow_unstarted_budget(&mut self,id:u32,expected:u8,new:u8)->bool{
        let Some(i)=self.slots.iter().position(|x|x.is_some_and(|s|
            s.service_id==id&&s.generation==0&&s.max_restarts==expected))
            else{return false;};
        if self.phases[i]!=ServicePhase::Configured||new>expected{return false;}
        self.slots[i].as_mut().unwrap().max_restarts=new;true
    }
    /// 14. Compare-and-narrow restart budget of an exact stopped process.
    pub fn narrow_stopped_budget(&mut self,id:u32,pid:u32,generation:u32,
        expected:u8,new:u8)->bool{
        if !self.is_stopped_exact(id,pid,generation){return false;}
        let Some(i)=self.slots.iter().position(|x|x.is_some_and(|s|
            s.service_id==id&&s.pid==pid&&s.generation==generation))
            else{return false;};
        let s=self.slots[i].as_mut().unwrap();
        if s.max_restarts!=expected||new>expected||new<s.restarts{return false;}
        s.max_restarts=new;true
    }
    /// 15. Start a set of distinct services with full transaction rollback.
    pub fn start_services_batch(&mut self,ids:&[u32])->bool{
        if ids.is_empty()||ids.len()>N{return false;}
        let mut copy=Self{slots:self.slots,phases:self.phases};
        for (i,&id) in ids.iter().enumerate(){
            if id==0||ids[..i].contains(&id)||copy.start(id)==ServiceAction::Deny{
                return false;
            }
        }
        *self=copy;true
    }
    /// 16. Stop multiple exact active identities without partial commits.
    pub fn stop_exact_batch(&mut self,requests:&[(u32,u32,u32)])->bool{
        if requests.is_empty()||requests.len()>N{return false;}
        let mut copy=Self{slots:self.slots,phases:self.phases};
        for (i,&(id,pid,generation)) in requests.iter().enumerate(){
            if requests[..i].iter().any(|(other,_,_)|*other==id)||
                !copy.stop_exact(id,pid,generation){return false;}
        }
        *self=copy;true
    }
    /// 17. Stop currently active services only when supplied epochs match.
    pub fn stop_epochs_batch(&mut self,requests:&[(u32,u32)])->bool{
        if requests.is_empty()||requests.len()>N{return false;}
        let mut copy=Self{slots:self.slots,phases:self.phases};
        for (i,&(id,generation)) in requests.iter().enumerate(){
            if requests[..i].iter().any(|(other,_)|*other==id)||
               !copy.stop_if_epoch(id,generation){return false;}
        }
        *self=copy;true
    }
    /// 18. Narrow restart budgets across multiple *unstarted* services.
    pub fn narrow_unstarted_batch(&mut self,items:&[(u32,u8,u8)])->bool{
        if items.is_empty()||items.len()>N{return false;}
        let mut copy=Self{slots:self.slots,phases:self.phases};
        for (i,&(id,expected,next)) in items.iter().enumerate(){
            if items[..i].iter().any(|(old,_,_)|*old==id)||
                !copy.narrow_unstarted_budget(id,expected,next){return false;}
        }
        *self=copy;true
    }
    /// 19. Narrow budgets of multiple stopped exact process generations.
    pub fn narrow_stopped_batch(&mut self,items:&[(u32,u32,u32,u8,u8)])->bool{
        if items.is_empty()||items.len()>N{return false;}
        let mut copy=Self{slots:self.slots,phases:self.phases};
        for (i,&(id,pid,generation,expected,next)) in items.iter().enumerate(){
            if items[..i].iter().any(|(old,_,_,_,_)|*old==id)||
               !copy.narrow_stopped_budget(id,pid,generation,expected,next){return false;}
        }
        *self=copy;true
    }
    /// 20. Exact retired service configuration with preflighted generation.
    pub fn retire_stopped_if_budget(&mut self,id:u32,pid:u32,generation:u32,
        expected:u8)->bool{
        if !self.is_stopped_exact(id,pid,generation)||self.snapshot(id)
            .is_none_or(|s|s.max_restarts!=expected){return false;}
        self.remove_stopped_config(id,pid,generation)
    }
}
#[cfg(test)]
mod r084_service_budget_batch_tests{
    use super::*;
    #[test]
    fn service_restart_budget_is_narrowed_only_before_or_after_lifecycle(){
        let mut t=ServiceTable::<4>::new();
        assert!(t.configure_batch(&[(1,11,2),(2,22,2),(3,33,0)]));
        assert_eq!(t.ever_started_count(),0);
        assert_eq!(t.services_with_retries(),0);
        assert_eq!(t.services_without_retry_budget(),1);
        assert_eq!(t.maximum_service_generation(),Some(0));
        assert_eq!(t.minimum_service_id(),Some(1));
        assert_eq!(t.maximum_service_id(),Some(3));
        assert!(!t.can_configure(4,11));assert!(t.can_configure(4,44));
        assert_eq!(t.count_generation(0),3);
        let mut tiny=[77u32;0];assert_eq!(t.depleted_service_ids(&mut tiny),None);
        let mut ids=[0u32;4];
        assert_eq!(t.depleted_service_ids(&mut ids),Some(1));
        assert_eq!(ids[0],3);
        assert_eq!(t.restarted_service_ids(&mut ids),Some(0));
        let mut epochs=[(0u32,0u32);4];
        assert_eq!(t.snapshot_pid_epochs(&mut epochs),Some(3));
        assert_eq!(epochs[0],(11,0));
        assert!(!t.narrow_unstarted_budget(1,2,3));
        assert!(t.narrow_unstarted_budget(1,2,1));
        assert!(!t.narrow_unstarted_batch(&[(2,2,1),(3,9,0)]));
        assert_eq!(t.snapshot(2).unwrap().max_restarts,2);
        assert!(t.narrow_unstarted_batch(&[(2,2,1),(3,0,0)]));
        assert!(t.start_services_batch(&[1,2]));
        assert!(t.is_active_pid_epoch(11,1));
        assert!(!t.is_active_pid_epoch(11,2));
        assert!(!t.stop_epochs_batch(&[(1,1),(2,7)]));
        assert_eq!(t.active_services(),2);
        assert!(t.stop_exact_batch(&[(1,11,1),(2,22,1)]));
        assert!(t.narrow_stopped_budget(1,11,1,1,0));
        assert!(t.narrow_stopped_batch(&[(2,22,1,1,0)]));
        assert!(t.retire_stopped_if_budget(1,11,1,0));
        assert_eq!(t.configured_count(),2);
    }
    #[test]
    fn rejected_batch_must_not_leave_partial_scheduler_state(){
        let mut t=ServiceTable::<2>::new();
        assert!(t.configure_batch(&[(1,11,1),(2,22,2)]));
        assert!(!t.start_services_batch(&[1,1]));
        assert_eq!(t.active_services(),0);
        assert!(t.start_services_batch(&[1,2]));
        assert!(!t.stop_exact_batch(&[(1,11,1),(2,22,9)]));
        assert_eq!(t.active_services(),2);
    }
}


// R084 extension V: 20 generation/identity bound lifecycle controls and
// scheduler audit queries. No user-supplied identity is trusted here.
impl<const N:usize> ServiceTable<N>{
    /// Count configured IDs inclusively inside a service namespace partition.
    pub fn count_service_ids_in(&self,lo:u32,hi:u32)->usize{
        if lo==0||hi<lo{return 0;}
        self.slots.iter().flatten().filter(|s|
            s.service_id>=lo&&s.service_id<=hi).count()
    }
    /// Count exactly reserved PIDs inside a scheduler PID interval.
    pub fn count_pids_in(&self,lo:u32,hi:u32)->usize{
        if lo==0||hi<lo{return 0;}
        self.slots.iter().flatten().filter(|s|s.pid>=lo&&s.pid<=hi).count()
    }
    /// Find next configured ID with deterministic ID ordering.
    pub fn first_service_at_or_after(&self,threshold:u32)->Option<u32>{
        self.slots.iter().flatten().filter(|s|s.service_id>=threshold)
            .map(|s|s.service_id).min()
    }
    /// Find next current active ID, excluding prepared-but-pending records.
    pub fn first_active_at_or_after(&self,threshold:u32)->Option<u32>{
        self.slots.iter().enumerate().filter_map(|(i,s)|
            if self.phases[i]==ServicePhase::Active{
                s.filter(|v|v.active&&v.service_id>=threshold).map(|v|v.service_id)
            }else{None}).min()
    }
    /// Count live services with generations newer than an observed epoch.
    pub fn active_newer_than(&self,epoch:u32)->usize{
        self.slots.iter().enumerate().filter(|(i,s)|
            self.phases[*i]==ServicePhase::Active&&
            s.is_some_and(|v|v.active&&v.generation>epoch)).count()
    }
    /// Count stopped services whose generation is >= the observed epoch.
    pub fn stopped_since_epoch(&self,epoch:u32)->usize{
        self.slots.iter().enumerate().filter(|(i,s)|
            self.phases[*i]==ServicePhase::Stopped&&
            s.is_some_and(|v|v.generation>=epoch)).count()
    }
    /// Count pending restarts newer than a known incarnation epoch.
    pub fn pending_newer_than(&self,epoch:u32)->usize{
        self.slots.iter().enumerate().filter(|(i,s)|
            self.phases[*i]==ServicePhase::Pending&&
            s.is_some_and(|v|v.generation>epoch)).count()
    }
    /// Count the exact observed restarts of one existing service.
    pub fn restarts_for_service(&self,id:u32)->Option<u8>{
        self.snapshot(id).map(|s|s.restarts)
    }
    /// Return configured restart ceiling; cannot be changed by callers.
    pub fn max_restarts_for_service(&self,id:u32)->Option<u8>{
        self.snapshot(id).map(|s|s.max_restarts)
    }
    /// Resolve the PID for an exact service ID, even when pending.
    pub fn pid_for_service(&self,id:u32)->Option<u32>{
        self.snapshot(id).map(|s|s.pid)
    }
    /// Confirm an occupied PID belongs to a live generation.
    pub fn reserved_exact_pid_epoch(&self,pid:u32,epoch:u32)->bool{
        self.service_at_pid_epoch(pid,epoch).is_some()
    }
    /// Only an unallocated PID can become a replacement process.
    pub fn pid_is_available(&self,pid:u32)->bool{
        pid!=0&&!self.pid_is_reserved(pid)
    }
    /// Check a restart belongs to current exact service+PID incarnation.
    pub fn can_restart_at_epoch(&self,id:u32,pid:u32,epoch:u32)->bool{
        epoch!=0&&self.is_active_exact(id,pid,epoch)&&self.can_restart_service(id)
    }
    /// Prepare a replacement only with the original pending generation.
    pub fn prepare_if_pending_epoch(&mut self,id:u32,pid:u32,epoch:u32)->bool{
        self.pending_exact(id,pid,epoch)&&self.prepared(id,pid,epoch)
    }
    /// Start only an exact service/PID observed before the start transition.
    pub fn start_with_pid_epoch(&mut self,id:u32,pid:u32,epoch:u32)
        ->ServiceAction{
        if self.snapshot(id).is_none_or(|s|s.pid!=pid||s.generation!=epoch){
            return ServiceAction::Deny;
        }
        self.start_if_generation(id,epoch)
    }
    /// Stop an authenticated active PID epoch via scheduler's exact record.
    pub fn stop_pid_epoch(&mut self,pid:u32,epoch:u32)->bool{
        let Some(id)=self.service_at_pid_epoch(pid,epoch)else{return false;};
        self.stop_exact(id,pid,epoch)
    }
    /// Shutdown all active service generations > threshold without touching
    /// pending restarts or stopped services.
    pub fn stop_active_newer_than(&mut self,threshold:u32)->usize{
        let mut stopped=0;
        for i in 0..N{
            if self.phases[i]==ServicePhase::Active&&
              self.slots[i].is_some_and(|s|s.active&&s.generation>threshold){
                self.phases[i]=ServicePhase::Stopped;
                if let Some(slot)=self.slots[i].as_mut(){slot.active=false;}
                stopped+=1;
            }
        }
        stopped
    }
    /// Retire a never-started service only with its exact PID reservation.
    pub fn retire_unstarted_exact(&mut self,id:u32,pid:u32)->bool{
        if id==0||pid==0||!self.is_unstarted_service(id)||
           self.snapshot(id).is_none_or(|s|s.pid!=pid){return false;}
        self.remove_stopped_config(id,pid,0)
    }
    /// Narrow exact pre-start budgets of multiple independent services,
    /// preserving every current slot on any invalid request.
    pub fn narrow_unstarted_if_ids(&mut self,requests:&[(u32,u8)])
        ->bool{
        if requests.is_empty()||requests.len()>N{return false;}
        let mut stage=Self{slots:self.slots,phases:self.phases};
        for (i,&(id,limit)) in requests.iter().enumerate(){
            if requests[..i].iter().any(|(old,_)|*old==id){return false;}
            let Some(observed)=stage.max_restarts_for_service(id)else{return false;};
            if !stage.narrow_unstarted_budget(id,observed,limit){return false;}
        }
        *self=stage;true
    }
    /// Simultaneously crash independent running processes, with rollback
    /// if any source PID, replacement PID, or restart budget is invalid.
    pub fn crash_batch_without_output(&mut self,requests:&[(u32,u32)])->bool{
        if requests.is_empty()||requests.len()>N{return false;}
        let mut stage=Self{slots:self.slots,phases:self.phases};
        for (i,&(from,to)) in requests.iter().enumerate(){
            if requests[..i].iter().any(|(old,new)|*old==from||*new==to)||
                stage.crash(from,to)==ServiceAction::Deny{return false;}
        }
        *self=stage;true
    }
}
#[cfg(test)]
mod r084_v5_supervisor_tests{
    use super::*;
    #[test]
    fn scheduler_identity_range_and_restart_batch_are_generation_pinned(){
        let mut t=ServiceTable::<3>::new();
        assert!(t.configure_batch(&[(2,20,2),(4,40,1),(6,60,0)]));
        assert_eq!(t.count_service_ids_in(2,4),2);
        assert_eq!(t.count_pids_in(20,40),2);
        assert_eq!(t.first_service_at_or_after(3),Some(4));
        assert_eq!(t.first_active_at_or_after(3),None);
        assert_eq!(t.active_newer_than(0),0);
        assert_eq!(t.stopped_since_epoch(1),0);
        assert_eq!(t.pending_newer_than(1),0);
        assert_eq!(t.restarts_for_service(2),Some(0));
        assert_eq!(t.max_restarts_for_service(2),Some(2));
        assert_eq!(t.pid_for_service(2),Some(20));
        assert!(!t.reserved_exact_pid_epoch(20,1));
        assert!(!t.pid_is_available(20));
        assert!(t.pid_is_available(21));
        assert!(t.narrow_unstarted_if_ids(&[(2,1),(4,1)]));
        assert!(!t.narrow_unstarted_if_ids(&[(2,2)]));
        assert!(t.retire_unstarted_exact(6,60));
        assert_eq!(t.start_with_pid_epoch(2,20,0),ServiceAction::Start{pid:20,generation:1});
        assert_eq!(t.start_with_pid_epoch(4,40,0),ServiceAction::Start{pid:40,generation:1});
        assert!(t.reserved_exact_pid_epoch(20,1));
        assert!(t.can_restart_at_epoch(2,20,1));
        assert_eq!(t.first_active_at_or_after(3),Some(4));
        assert_eq!(t.active_newer_than(0),2);
        assert!(!t.crash_batch_without_output(&[(20,22),(40,22)]));
        assert_eq!(t.active_newer_than(0),2);
        assert!(t.crash_batch_without_output(&[(20,21),(40,41)]));
        assert_eq!(t.pending_newer_than(1),2);
        assert!(t.prepare_if_pending_epoch(2,21,2));
        assert!(!t.prepare_if_pending_epoch(4,41,1));
        assert!(t.stop_pid_epoch(21,2));
        assert_eq!(t.stopped_since_epoch(2),1);
        assert_eq!(t.stop_active_newer_than(1),0);
    }
}


// R084 batch VI: 20 bounded lifecycle inspection, retry-health and
// generation-pinned maintenance operations on live service slots.
impl<const N:usize> ServiceTable<N>{
    // 1. Count all services whose restart budget is actually exhausted.
    pub fn retry_budget_depleted(&self)->usize{
        self.slots.iter().flatten()
            .filter(|s|s.restarts>=s.max_restarts).count()
    }
    // 2. Count services capable of at least one more authorized restart.
    pub fn retry_budget_remaining_services(&self)->usize{
        self.slots.iter().flatten()
            .filter(|s|s.restarts<s.max_restarts).count()
    }
    // 3. Largest currently remaining retry budget of any service.
    pub fn largest_remaining_retries(&self)->Option<u8>{
        self.slots.iter().flatten()
            .map(|s|s.max_restarts.saturating_sub(s.restarts)).max()
    }
    // 4. Smallest currently remaining retry budget, including exhausted.
    pub fn smallest_remaining_retries(&self)->Option<u8>{
        self.slots.iter().flatten()
            .map(|s|s.max_restarts.saturating_sub(s.restarts)).min()
    }
    // 5. Service with greatest remaining retry headroom.
    pub fn most_recoverable_service(&self)->Option<u32>{
        self.slots.iter().flatten().max_by_key(|s|
            s.max_restarts.saturating_sub(s.restarts)).map(|s|s.service_id)
    }
    // 6. Service with greatest consumed restart budget.
    pub fn most_restarted_service(&self)->Option<u32>{
        self.slots.iter().flatten()
            .max_by_key(|s|s.restarts).map(|s|s.service_id)
    }
    // 7. Count configured records whose generation is exactly the observer's.
    pub fn services_at_generation(&self,epoch:u32)->usize{
        self.slots.iter().flatten().filter(|s|s.generation==epoch).count()
    }
    // 8. Count *active* services at one exact generation, ignoring pending.
    pub fn active_services_at_generation(&self,epoch:u32)->usize{
        self.slots.iter().enumerate().filter(|(i,s)|
            self.phases[*i]==ServicePhase::Active&&
            s.is_some_and(|v|v.active&&v.generation==epoch)).count()
    }
    // 9. Number of currently configured but never-started processes.
    pub fn services_without_first_start(&self)->usize{
        self.slots.iter().enumerate().filter(|(i,s)|
            self.phases[*i]==ServicePhase::Configured&&
            s.is_some_and(|v|v.generation==0)).count()
    }
    // 10. Snapshot a consistent service-epoch pair list, no partial output.
    pub fn snapshot_service_epochs(&self,out:&mut[(u32,u32)])->Option<usize>{
        let wanted=self.configured_count();
        if out.len()<wanted{return None;}
        let mut at=0;
        for s in self.slots.iter().flatten(){
            out[at]=(s.service_id,s.generation);at+=1;
        }
        Some(at)
    }
    // 11. Snapshot all exact active PID-epoch pairs, no pending PID leak.
    pub fn snapshot_active_pid_epochs(&self,out:&mut[(u32,u32)])->Option<usize>{
        let wanted=self.active_services();
        if out.len()<wanted{return None;}
        let mut at=0;
        for (i,s) in self.slots.iter().enumerate(){
            if self.phases[i]==ServicePhase::Active{
                if let Some(x)=s{if x.active{
                    out[at]=(x.pid,x.generation);at+=1;
                }}
            }
        }
        Some(at)
    }
    // 12. Snapshot only pending PID-epoch pairs, separate from active jobs.
    pub fn snapshot_pending_pid_epochs(&self,out:&mut[(u32,u32)])->Option<usize>{
        let wanted=self.pending_count();
        if out.len()<wanted{return None;}
        let mut at=0;
        for (i,s) in self.slots.iter().enumerate(){
            if self.phases[i]==ServicePhase::Pending{
                if let Some(x)=s{out[at]=(x.pid,x.generation);at+=1;}
            }
        }
        Some(at)
    }
    // 13. Confirm an observed epoch is the exact current stopped service.
    pub fn stopped_service_at_epoch(&self,id:u32,generation:u32)->Option<ServiceSlot>{
        let s=self.snapshot_at_epoch(id,generation)?;
        if self.is_stopped_exact(id,s.pid,generation){Some(s)}else{None}
    }
    // 14. Confirm an observed epoch is the exact pending replacement.
    pub fn pending_service_at_epoch(&self,id:u32,generation:u32)->Option<ServiceSlot>{
        let s=self.snapshot_at_epoch(id,generation)?;
        if self.pending_exact(id,s.pid,generation){Some(s)}else{None}
    }
    // 15. Reject attempts to retire service records for a recycled epoch.
    pub fn retire_stopped_epoch(&mut self,id:u32,generation:u32)->bool{
        let Some(s)=self.stopped_service_at_epoch(id,generation)else{return false;};
        self.remove_stopped_config(id,s.pid,generation)
    }
    // 16. Retire multiple stopped services with exact ID/epoch audit.
    pub fn retire_stopped_epochs_batch(&mut self,pairs:&[(u32,u32)])->bool{
        if pairs.is_empty()||pairs.len()>N{return false;}
        let mut stage=Self{slots:self.slots,phases:self.phases};
        for (i,&(id,epoch)) in pairs.iter().enumerate(){
            if pairs[..i].iter().any(|(seen,_)|*seen==id)||
                !stage.retire_stopped_epoch(id,epoch){return false;}
        }
        *self=stage;true
    }
    // 17. Stop active exact services in a bounded service-ID namespace.
    pub fn stop_active_in_range(&mut self,from:u32,to:u32)->usize{
        if from==0||to<from{return 0;}
        let mut count=0;
        for i in 0..N{
            if self.phases[i]==ServicePhase::Active&&
                self.slots[i].is_some_and(|s|s.service_id>=from&&
                    s.service_id<=to&&s.active){
                self.phases[i]=ServicePhase::Stopped;
                if let Some(s)=self.slots[i].as_mut(){s.active=false;}
                count+=1;
            }
        }
        count
    }
    // 18. Retire only *stopped* services in a given namespace partition.
    pub fn retire_stopped_in_range(&mut self,from:u32,to:u32)->usize{
        if from==0||to<from{return 0;}
        let mut count=0;
        for i in 0..N{
            if self.phases[i]==ServicePhase::Stopped&&
                self.slots[i].is_some_and(|s|s.service_id>=from&&s.service_id<=to){
                self.slots[i]=None;self.phases[i]=ServicePhase::Configured;count+=1;
            }
        }
        count
    }
    // 19. Restrict the configured retry limit of an exact unstarted process.
    pub fn narrow_unstarted_pid(&mut self,pid:u32,expected:u8,next:u8)->bool{
        if pid==0{return false;}
        let Some(id)=self.slots.iter().flatten().find(|s|
            s.pid==pid&&s.generation==0).map(|s|s.service_id)else{return false;};
        self.narrow_unstarted_budget(id,expected,next)
    }
    // 20. Verify service table invariants, duplicate PIDs and phase metadata.
    pub fn validate_service_table(&self)->bool{
        for i in 0..N{
            let Some(s)=self.slots[i]else{
                if self.phases[i]!=ServicePhase::Configured{return false;}
                continue;
            };
            if s.pid==0||s.service_id==0||s.restarts>s.max_restarts{return false;}
            if self.slots[..i].iter().flatten().any(|p|
                p.pid==s.pid||p.service_id==s.service_id){return false;}
            match self.phases[i]{
                ServicePhase::Configured=>if s.generation!=0||s.active{return false;},
                ServicePhase::Stopped=>if s.generation==0||s.active{return false;},
                ServicePhase::Active=>if s.generation==0||!s.active{return false;},
                ServicePhase::Pending|ServicePhase::Exhausted=>if s.active{return false;},
            }
        }
        true
    }
}
#[cfg(test)]
mod r084_batch_vi_supervisor_tests{
    use super::*;
    #[test]
    fn generation_pinned_health_and_namespace_retirement(){
        let mut t=ServiceTable::<4>::new();
        assert!(t.configure_batch(&[(1,10,2),(2,20,0),(3,30,1)]));
        assert_eq!(t.retry_budget_depleted(),1);
        assert_eq!(t.retry_budget_remaining_services(),2);
        assert_eq!(t.largest_remaining_retries(),Some(2));
        assert_eq!(t.smallest_remaining_retries(),Some(0));
        assert_eq!(t.most_recoverable_service(),Some(1));
        assert_eq!(t.most_restarted_service(),Some(3));
        assert_eq!(t.services_at_generation(0),3);
        assert_eq!(t.active_services_at_generation(0),0);
        assert_eq!(t.services_without_first_start(),3);
        let mut tiny=[(0u32,0u32);2];
        assert_eq!(t.snapshot_service_epochs(&mut tiny),None);
        let mut out=[(0u32,0u32);4];
        assert_eq!(t.snapshot_service_epochs(&mut out),Some(3));
        assert_eq!(t.snapshot_active_pid_epochs(&mut out),Some(0));
        assert_eq!(t.snapshot_pending_pid_epochs(&mut out),Some(0));
        assert!(t.narrow_unstarted_pid(10,2,1));
        assert!(t.validate_service_table());
        assert_eq!(t.start(1),ServiceAction::Start{pid:10,generation:1});
        assert_eq!(t.start(3),ServiceAction::Start{pid:30,generation:1});
        assert_eq!(t.active_services_at_generation(1),2);
        assert_eq!(t.crash(10,11),ServiceAction::Restart{pid:11,generation:2});
        assert_eq!(t.pending_service_at_epoch(1,2).unwrap().pid,11);
        assert_eq!(t.start(2),ServiceAction::Start{pid:20,generation:1});
        assert_eq!(t.stop_active_in_range(2,3),2);
        assert_eq!(t.stopped_service_at_epoch(3,1).unwrap().pid,30);
        assert!(!t.retire_stopped_epochs_batch(&[(2,7),(3,1)]));
        assert!(t.retire_stopped_epochs_batch(&[(2,1),(3,1)]));
        assert_eq!(t.retire_stopped_in_range(1,3),0);
        assert_eq!(t.configured_count(),1);
        assert!(t.validate_service_table());
    }
}



/// R100: scheduler-only exact process-incarnation crash authorization.
/// These APIs do not start an executable or grant user-process capabilities.
impl<const N:usize> ServiceTable<N>{
 pub fn crash_exact(&mut self,service:u32,pid:u32,generation:u32,
                    replacement:u32)->ServiceAction{
  if !self.is_active_exact(service,pid,generation){return ServiceAction::Deny;}
  self.crash(pid,replacement)
 }
 /// All-or-none group of verified scheduler faults. A failure cannot
 /// advance earlier services' generations or consume their restart quotas.
 /// The caller must publish new process mappings/grants only after successful
 /// returned actions and independent prepared() acknowledgement.
 pub fn crash_group_exact(&mut self,requests:&[(u32,u32,u32,u32)],
                           actions:&mut[ServiceAction])->bool{
  if requests.is_empty()||requests.len()>N||actions.len()<requests.len(){
   return false;
  }
  let mut staged=Self{phases:self.phases,slots:self.slots};
  let mut results=[ServiceAction::Deny;N];
  for (i,&(service,pid,generation,replacement)) in requests.iter().enumerate(){
   let outcome=staged.crash_exact(service,pid,generation,replacement);
   if !matches!(outcome,ServiceAction::Restart{..}){return false;}
   results[i]=outcome;
  }
  self.phases=staged.phases;self.slots=staged.slots;
  actions[..requests.len()].copy_from_slice(&results[..requests.len()]);
  true
 }
}
#[cfg(test)]
mod r100_exact_service_crash_tests{
 use super::*;
 #[test]fn reused_pid_or_wrong_service_generation_cannot_restart(){
  let mut t=ServiceTable::<2>::new();
  assert!(t.configure(7,70,1));
  assert_eq!(t.start(7),ServiceAction::Start{pid:70,generation:1});
  assert_eq!(t.crash_exact(8,70,1,71),ServiceAction::Deny);
  assert_eq!(t.crash_exact(7,70,0,71),ServiceAction::Deny);
  assert_eq!(t.crash_exact(7,70,1,71),ServiceAction::Restart{pid:71,generation:2});
  assert_eq!(t.crash_exact(7,70,1,72),ServiceAction::Deny);
  assert!(!t.is_active_exact(7,71,2));
  assert!(t.prepared(7,71,2));
  assert!(t.is_active_exact(7,71,2));
 }
 #[test]fn failed_group_has_zero_partial_restart_side_effects(){
  let mut t=ServiceTable::<2>::new();
  assert!(t.configure(7,70,1));assert!(t.configure(8,80,1));
  t.start(7);t.start(8);
  let before7=t.snapshot(7);let before8=t.snapshot(8);
  let mut out=[ServiceAction::Deny;2];
  assert!(!t.crash_group_exact(&[(7,70,1,71),(8,80,2,81)],&mut out));
  assert_eq!(t.snapshot(7),before7);assert_eq!(t.snapshot(8),before8);
  assert_eq!(out,[ServiceAction::Deny;2]);
 }
 #[test]fn valid_group_reserves_independent_replacement_pids(){
  let mut t=ServiceTable::<2>::new();
  assert!(t.configure(7,70,1));assert!(t.configure(8,80,1));
  t.start(7);t.start(8);
  let mut out=[ServiceAction::Deny;2];
  assert!(t.crash_group_exact(&[(7,70,1,71),(8,80,1,81)],&mut out));
  assert_eq!(out,[ServiceAction::Restart{pid:71,generation:2},
                  ServiceAction::Restart{pid:81,generation:2}]);
  assert!(t.prepared(7,71,2));assert!(t.prepared(8,81,2));
 }
 #[test]fn group_refuses_replacement_pid_collisions_without_mutation(){
  let mut t=ServiceTable::<2>::new();
  assert!(t.configure(7,70,1));assert!(t.configure(8,80,1));
  t.start(7);t.start(8);let a=t.snapshot(7);let b=t.snapshot(8);
  let mut out=[ServiceAction::Deny;2];
  assert!(!t.crash_group_exact(&[(7,70,1,71),(8,80,1,71)],&mut out));
  assert_eq!(t.snapshot(7),a);assert_eq!(t.snapshot(8),b);
 }
}


/// R101 trusted boot coordination: enforce exact and distinct service identities
/// before treating the compositor, Files and Terminal as active services.
impl<const N:usize> ServiceTable<N>{
 pub fn all_active_exact(&self,required:&[(u32,u32,u32)])->bool{
  if required.is_empty()||required.len()>N{return false;}
  for (i,&(service,pid,generation)) in required.iter().enumerate(){
   if service==0||pid==0||generation==0 ||
      required[..i].iter().any(|&(s,p,_)|s==service||p==pid) ||
      !self.is_active_exact(service,pid,generation){
    return false;
   }
  }
  true
 }
 /// Stop every currently active service, but only if the kernel supplies an
 /// exact, complete set of active service/PID/epoch triples. A partial or
 /// forged shutdown cannot terminate an unrelated service or leave some
 /// services stopped because of a later authorization error.
 pub fn shutdown_all_exact(&mut self,requests:&[(u32,u32,u32)])->bool{
  if requests.len()!=self.active_services()||!self.all_active_exact(requests){
   return false;
  }
  self.stop_batch_exact(requests)
 }
}
#[cfg(test)]
mod r101_boot_and_shutdown_checks{
 use super::*;
 #[test]fn boot_gate_requires_three_active_distinct_process_epochs(){
  let mut s=ServiceTable::<3>::new();
  for id in 1..=3{assert!(s.configure(id,id*10,1));}
  for id in 1..=3{assert_eq!(s.start(id),ServiceAction::Start{pid:id*10,generation:1});}
  assert!(s.all_active_exact(&[(1,10,1),(2,20,1),(3,30,1)]));
  assert!(!s.all_active_exact(&[(1,10,1),(2,20,2),(3,30,1)]));
  assert!(!s.all_active_exact(&[(1,10,1),(1,10,1)]));
  assert!(!s.all_active_exact(&[(1,10,1),(2,10,1)]));
 }
 #[test]fn full_shutdown_needs_complete_authenticated_live_set(){
  let mut s=ServiceTable::<3>::new();
  assert!(s.configure(1,10,1));assert!(s.configure(2,20,1));
  s.start(1);s.start(2);
  assert!(!s.shutdown_all_exact(&[(1,10,1)]));
  assert!(s.all_active_exact(&[(1,10,1),(2,20,1)]));
  assert!(!s.shutdown_all_exact(&[(1,10,1),(2,20,2)]));
  assert!(s.all_active_exact(&[(1,10,1),(2,20,1)]));
  assert!(s.shutdown_all_exact(&[(1,10,1),(2,20,1)]));
  assert_eq!(s.active_services(),0);
 }
 #[test]fn stale_shutdown_request_cannot_stop_replacement(){
  let mut s=ServiceTable::<1>::new();assert!(s.configure(1,10,1));
  s.start(1);assert_eq!(s.crash_exact(1,10,1,11),
                          ServiceAction::Restart{pid:11,generation:2});
  assert!(s.prepared(1,11,2));
  assert!(!s.shutdown_all_exact(&[(1,10,1)]));
  assert!(s.all_active_exact(&[(1,11,2)]));
 }
}


/// Kernel scheduler handoff for a crashed process: restart only after the
/// complete supervisor/service and file-handle PID incarnation agree.
/// On a successful replacement reservation, invalidate all old file grants
/// and compositor focus/queues BEFORE the new process is prepared.
#[cfg(feature="test-r031-vfs")]
pub fn restart_and_revoke_exact<const S:usize,const W:usize,const Q:usize>(
 supervisor:&mut ServiceTable<S>,
 handles:&mut crate::fs_handles::FileHandles,
 broker:&mut crate::gui_broker::Broker<W,Q>,
 desktop:&mut crate::gui_broker::DesktopInput,
 service:u32,
 crashed:crate::batch5_session::Identity,
 replacement_pid:u32
)->ServiceAction{
 if crashed.generation==0||crashed.generation>u32::MAX as u64||
    !handles.owns(crashed)||
    !supervisor.is_active_exact(service,crashed.pid,crashed.generation as u32)||
    !supervisor.can_restart_service(service){
   return ServiceAction::Deny;
 }
 let decision=supervisor.crash_exact(service,crashed.pid,
                   crashed.generation as u32,replacement_pid);
 if matches!(decision,ServiceAction::Restart{..}){
    // Neither call may fail after an authenticated preflight.
    let retired=handles.retire_process(crashed);
    debug_assert!(retired);
    desktop.process_exited(broker,crashed.pid);
 }
 decision
}
/// Kernel-only crash recovery with all three process-scoped capabilities:
/// file handles, GUI grants, and the authenticated IPC inbox/outbound
/// messages. A missing IPC ownership record rejects the entire restart
/// before any supervisor quota or capability mutation.
#[cfg(feature="test-r031-vfs")]
pub fn restart_and_revoke_full_exact<
 const S:usize,const W:usize,const Q:usize,const M:usize,const D:usize>(
 supervisor:&mut ServiceTable<S>,
 handles:&mut crate::fs_handles::FileHandles,
 broker:&mut crate::gui_broker::Broker<W,Q>,
 desktop:&mut crate::gui_broker::DesktopInput,
 ipc:&mut crate::ipc::ScopedRouter<M,D>,
 service:u32,crashed:crate::batch5_session::Identity,replacement_pid:u32
)->ServiceAction{
 if ipc.pending(crashed).is_err(){return ServiceAction::Deny;}
 let answer=restart_and_revoke_exact(supervisor,handles,broker,desktop,
                                     service,crashed,replacement_pid);
 if matches!(answer,ServiceAction::Restart{..}){
   // Exiting process IPC registration was validated before any mutations.
   // Reap removes both its receiver queue and old outbound messages in
   // surviving peer mailboxes before a new PID can be prepared.
   let result=ipc.reap(crashed);
   debug_assert!(result.is_ok());
 }
 answer
}
/// Full kernel-owned shutdown capability teardown. Validate the entire
/// exact active service list and every IPC/file owner *before* changing any
/// supervisor state. After shutdown, retire each process's file handles,
/// IPC receiver/outbound messages, keyboard/pointer focus, input grants,
/// framebuffer surface grants and unpublished frame damage. This does not
/// substitute for separately quiescing the platform's APIC/PIT/ATA hardware.
#[cfg(feature="test-r031-vfs")]
pub fn shutdown_and_revoke_capabilities_exact<
 const S:usize,const W:usize,const Q:usize,const M:usize,
 const D:usize,const L:usize,const F:usize>(
 supervisor:&mut ServiceTable<S>,
 handles:&mut crate::fs_handles::FileHandles,
 broker:&mut crate::gui_broker::Broker<W,Q>,
 desktop:&mut crate::gui_broker::DesktopInput,
 ipc:&mut crate::ipc::ScopedRouter<M,D>,
 layout:&mut crate::gui_broker::SurfaceLayout<L>,
 frames:&mut crate::gui_broker::DamageQueue<F>,
 required:&[(u32,u32,u32)]
)->bool{
 if required.is_empty()||required.len()!=supervisor.active_services()||
    !supervisor.all_active_exact(required){return false;}
 for &(_,pid,generation) in required{
  let identity=crate::batch5_session::Identity{
      pid,generation:generation as u64};
  if ipc.pending(identity).is_err()||
     ((pid==1||pid==2)&&!handles.owns(identity)){
    return false;
  }
 }
 if !supervisor.shutdown_all_exact(required){return false;}
 for &(_,pid,generation) in required{
  let identity=crate::batch5_session::Identity{
      pid,generation:generation as u64};
  if pid==1||pid==2{
    let result=handles.retire_process(identity);
    debug_assert!(result);
  }
  let result=ipc.reap(identity);
  debug_assert!(result.is_ok());
  desktop.process_exited(broker,pid);
  layout.revoke(pid);
  frames.invalidate_owner(pid);
 }
 true
}

/// R114: replacement-only shutdown boundary. Any unsigned, revoked,
/// tampered or otherwise untrusted candidate is denied BEFORE stopping a
/// service, releasing process handles or tearing down the compositor.
/// The caller must provide a genuine cryptographic Verify implementation
/// backed by an authenticated signing-key registry; this entry point does
/// not install an OS image, overwrite firmware or supply a trust root.
#[cfg(feature="test-r031-vfs")]
pub fn shutdown_for_signed_replacement_exact<
 V:crate::batch4_signature::Verify,
 const S:usize,const W:usize,const Q:usize,const M:usize,
 const D:usize,const L:usize,const F:usize>(
 roots:&[crate::batch4_signature::Root],
 verifier:&V,
 envelope:&crate::batch4_signature::Envelope,
 signed_manifest_bytes:&[u8],
 supervisor:&mut ServiceTable<S>,
 handles:&mut crate::fs_handles::FileHandles,
 broker:&mut crate::gui_broker::Broker<W,Q>,
 desktop:&mut crate::gui_broker::DesktopInput,
 ipc:&mut crate::ipc::ScopedRouter<M,D>,
 layout:&mut crate::gui_broker::SurfaceLayout<L>,
 frames:&mut crate::gui_broker::DamageQueue<F>,
 required:&[(u32,u32,u32)]
)->bool{
 if crate::batch4_signature::check(roots,verifier,envelope,signed_manifest_bytes).is_err(){
  return false;
 }
 shutdown_and_revoke_capabilities_exact(supervisor,handles,broker,desktop,
     ipc,layout,frames,required)
}

#[cfg(all(test,feature="test-r031-vfs"))]
mod r114_signed_replacement_shutdown {
 use super::*;
 use crate::batch4_signature::{Envelope,Root,Verify};
 use crate::fs_handles::FileHandles;
 use crate::ipc::ScopedRouter;
 use crate::gui_broker::{Broker,DesktopInput,SurfaceLayout,DamageQueue};
 use crate::batch5_session::Identity;
 struct Reject;
 impl Verify for Reject {
  fn verify_digest_and_signature(&self,_:&Envelope,_:&[u8])->bool{false}
 }
 // This verifier is ONLY a controlled test double. Production must supply
 // the pinned Ed25519 trust backend, never an unconditional true verifier.
 struct TestAccept;
 impl Verify for TestAccept {
  fn verify_digest_and_signature(&self,e:&Envelope,payload:&[u8])->bool{
   e.digest==[4;32]&&e.signature==[5;64]&&payload==b"known verified bytes"
  }
 }
 #[test]
 fn unsigned_and_revoked_image_never_stop_live_desktop() {
  let mut sup=ServiceTable::<1>::new();
  assert!(sup.configure(7,1,1));sup.start(7);
  let mut files=FileHandles::new();
  let owner=files.bind_process(1).unwrap();
  let mut events=ScopedRouter::<1,2>::new().unwrap();events.register(owner).unwrap();
  let mut broker=Broker::<1,2>::new();broker.grant(1,10).unwrap();
  let mut input=DesktopInput::new();input.focus(&mut broker,10).unwrap();
  let mut surfaces=SurfaceLayout::<1>::new();
  let mut damage=DamageQueue::<1>::new();
  let roots=[Root{signer:11,epoch:3,enabled:true}];
  let signed=Envelope{signer:11,epoch:3,digest:[4;32],signature:[5;64]};
  let unsigned=Envelope{signature:[0;64],..signed};
  let mut attempt=|roots:&[Root],envelope:&Envelope,verifier:&dyn Verify| {
   // Generic verifier has to stay statically sized at callsite: use a
   // locally defined adapter for the precise branch under test.
   let _=verifier;
   shutdown_for_signed_replacement_exact(roots,&Reject,envelope,
    b"known verified bytes",&mut sup,&mut files,&mut broker,&mut input,
    &mut events,&mut surfaces,&mut damage,&[(7,1,1)])
  };
  assert!(!attempt(&roots,&unsigned,&Reject));
  assert!(!attempt(&roots,&signed,&Reject));
  assert!(!attempt(&[Root{enabled:false,..roots[0]}],&signed,&Reject));
  assert!(sup.is_active_exact(7,1,1));
  assert!(files.owns(owner));
  assert!(broker.owns(1,10));
  assert!(input.focused().is_some());
  assert_eq!(events.pending(Identity{pid:1,generation:1}),Ok(0));
 }
 #[test]
 fn verified_image_can_invoke_actual_full_capability_teardown() {
  let mut sup=ServiceTable::<1>::new();sup.configure(7,1,1);sup.start(7);
  let mut files=FileHandles::new();
  let owner=files.bind_process(1).unwrap();
  let mut events=ScopedRouter::<1,2>::new().unwrap();events.register(owner).unwrap();
  let mut broker=Broker::<1,2>::new();broker.grant(1,10).unwrap();
  let mut input=DesktopInput::new();input.focus(&mut broker,10).unwrap();
  let mut surfaces=SurfaceLayout::<1>::new();
  let mut damage=DamageQueue::<1>::new();
  let roots=[Root{signer:11,epoch:3,enabled:true}];
  let signed=Envelope{signer:11,epoch:3,digest:[4;32],signature:[5;64]};
  assert!(!shutdown_for_signed_replacement_exact(&roots,&TestAccept,
     &signed,b"tampered",&mut sup,&mut files,&mut broker,&mut input,
     &mut events,&mut surfaces,&mut damage,&[(7,1,1)]));
  assert!(sup.is_active_exact(7,1,1));
  assert!(shutdown_for_signed_replacement_exact(&roots,&TestAccept,
     &signed,b"known verified bytes",&mut sup,&mut files,&mut broker,
     &mut input,&mut events,&mut surfaces,&mut damage,&[(7,1,1)]));
  assert_eq!(sup.active_services(),0);
  assert!(!files.owns(owner));assert!(!broker.owns(1,10));
 }
}

#[cfg(all(test,feature="test-r031-vfs"))]
mod r105_process_ipc_revocation_tests{
 use super::*;
 use crate::batch5_session::Identity;
 use crate::fs_handles::FileHandles;
 use crate::ipc::{ScopedRouter,IpcError};
 use crate::gui_broker::{Broker,DesktopInput};
 #[test]fn crash_reaps_old_ipc_inbound_and_peer_outbound_before_replacement(){
  let mut t=ServiceTable::<2>::new();assert!(t.configure(7,1,1));
  assert!(t.configure(8,2,0));t.start(7);t.start(8);
  let mut handles=FileHandles::new();
  let owner=handles.bind_process(1).unwrap();
  let peer=Identity{pid:2,generation:1};
  let mut bus=ScopedRouter::<2,4>::new().unwrap();
  bus.register(owner).unwrap();bus.register(peer).unwrap();
  bus.send(owner,peer,b"old sender").unwrap();
  bus.send(peer,owner,b"old inbound").unwrap();
  let mut gui=Broker::<2,6>::new();gui.grant(1,10).unwrap();gui.grant(2,20).unwrap();
  let mut desktop=DesktopInput::new();desktop.focus(&mut gui,10).unwrap();
  assert_eq!(restart_and_revoke_full_exact(&mut t,&mut handles,&mut gui,
            &mut desktop,&mut bus,7,owner,3),
            ServiceAction::Restart{pid:3,generation:2});
  assert!(!handles.owns(owner));
  assert_eq!(bus.pending(owner),Err(IpcError::UnknownReceiver));
  assert_eq!(bus.pending(peer),Ok(0));
  assert!(!gui.owns(1,10));assert!(gui.owns(2,20));
  assert_eq!(desktop.focused(),None);
  assert!(!t.is_active_exact(7,3,2));
 }
 #[test]fn unknown_ipc_owner_refuses_restart_and_preserves_all_capabilities(){
  let mut t=ServiceTable::<1>::new();t.configure(7,1,1);t.start(7);
  let mut handles=FileHandles::new();let owner=handles.bind_process(1).unwrap();
  let mut ipc=ScopedRouter::<1,2>::new().unwrap();
  let mut gui=Broker::<1,4>::new();gui.grant(1,10).unwrap();
  let mut desktop=DesktopInput::new();desktop.focus(&mut gui,10).unwrap();
  assert_eq!(restart_and_revoke_full_exact(&mut t,&mut handles,&mut gui,
            &mut desktop,&mut ipc,7,owner,3),ServiceAction::Deny);
  assert!(t.is_active_exact(7,1,1));
  assert!(handles.owns(owner));assert!(gui.owns(1,10));
  assert!(desktop.focused().is_some());
 }
 #[test]fn replayed_retired_pid_cannot_consume_second_restart(){
  let mut t=ServiceTable::<1>::new();t.configure(7,1,2);t.start(7);
  let mut handles=FileHandles::new();let owner=handles.bind_process(1).unwrap();
  let mut ipc=ScopedRouter::<1,2>::new().unwrap();ipc.register(owner).unwrap();
  let mut gui=Broker::<1,4>::new();gui.grant(1,10).unwrap();
  let mut desktop=DesktopInput::new();desktop.focus(&mut gui,10).unwrap();
  assert!(matches!(restart_and_revoke_full_exact(&mut t,&mut handles,&mut gui,
        &mut desktop,&mut ipc,7,owner,3),ServiceAction::Restart{..}));
  let snapshot=t.snapshot(7);
  assert_eq!(restart_and_revoke_full_exact(&mut t,&mut handles,&mut gui,
        &mut desktop,&mut ipc,7,owner,4),ServiceAction::Deny);
  assert_eq!(t.snapshot(7),snapshot);
 }
}
#[cfg(all(test,feature="test-r031-vfs"))]
mod r102_cross_service_failure_isolation{
 use super::*;
 use crate::fs_handles::{FileHandles,READ};
 use crate::gui_broker::{Broker,DesktopInput,Error as GuiError};
 use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};
 use crate::hier_vfs::HierVfs;
 struct Disk([[u8;SECTOR_BYTES];100]);
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,l:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.0.get(l as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,l:u64,bytes:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.0.get_mut(l as usize).ok_or(BlockError::OutOfRange)?=*bytes;Ok(())
  }
 }
 #[test]fn trusted_crash_retires_old_handles_gui_focus_and_pending_events(){
  let mut fs=HierVfs::format_blank(Disk([[0;SECTOR_BYTES];100])).unwrap();
  fs.mkdir("/shared").unwrap();fs.create("/shared/item",b"data").unwrap();
  let mut h=FileHandles::new();let owner=h.bind_process(1).unwrap();
  let token=h.dispatch_authenticated(1,owner,0,READ,"/shared/item",
       &mut[],&mut fs).unwrap();
  let mut b=Broker::<2,8>::new();b.grant(1,10).unwrap();b.grant(2,20).unwrap();
  let mut focus=DesktopInput::new();focus.focus(&mut b,10).unwrap();
  focus.capture_pointer(&b,1,10).unwrap();
  let mut t=ServiceTable::<2>::new();t.configure(7,1,1);t.start(7);
  let result=restart_and_revoke_exact(&mut t,&mut h,&mut b,&mut focus,7,owner,2);
  assert_eq!(result,ServiceAction::Restart{pid:2,generation:2});
  assert!(!h.owns(owner));
  assert_eq!(h.dispatch_authenticated(2,owner,token,0,"",
       &mut[0;3],&mut fs),Err(crate::durablefs::FsError::InvalidName));
  assert!(!b.owns(1,10));assert!(b.owns(2,20));
  assert_eq!(focus.focused(),None);assert_eq!(focus.captured(),None);
  assert_eq!(b.peek(1),Err(GuiError::Unauthorized));
  assert!(t.prepared(7,2,2));
 }
 #[test]fn stale_owner_must_not_revoke_replacement_or_consume_restart_budget(){
  let mut h=FileHandles::new();let original=h.bind_process(1).unwrap();
  let replacement=h.bind_process(1).unwrap();
  let mut b=Broker::<1,3>::new();b.grant(1,10).unwrap();
  let mut focus=DesktopInput::new();focus.focus(&mut b,10).unwrap();
  let mut t=ServiceTable::<1>::new();t.configure(7,1,1);t.start(7);
  assert_eq!(restart_and_revoke_exact(&mut t,&mut h,&mut b,&mut focus,
             7,original,2),ServiceAction::Deny);
  assert!(h.owns(replacement));assert!(b.owns(1,10));
  assert_eq!(focus.focused(),Some(crate::gui_broker::FocusTarget{owner:1,window:10}));
  assert!(t.is_active_exact(7,1,1));
 }
 #[test]fn service_epoch_mismatch_refuses_revocation(){
  let mut h=FileHandles::new();let first=h.bind_process(1).unwrap();
  let owner=h.bind_process(1).unwrap();assert!(!h.owns(first));
  let mut b=Broker::<1,3>::new();b.grant(1,10).unwrap();
  let mut focus=DesktopInput::new();focus.focus(&mut b,10).unwrap();
  let mut t=ServiceTable::<1>::new();t.configure(7,1,1);t.start(7);
  assert_eq!(restart_and_revoke_exact(&mut t,&mut h,&mut b,&mut focus,
             7,owner,2),ServiceAction::Deny);
  assert!(h.owns(owner));assert!(b.owns(1,10));
 }
}


/// Kernel-only supervisor activation gate. Pending process replacements
/// become active only after the exact process incarnation owns both an IPC
/// receiver mailbox and a fresh compositor window grant; stale userspace
/// "ready" messages cannot activate an unprepared process.
impl<const N:usize> ServiceTable<N>{
 pub fn prepared_with_ipc_and_window<
    const M:usize,const D:usize,const W:usize,const Q:usize>(
    &mut self,service:u32,owner:crate::batch5_session::Identity,
    router:&crate::ipc::ScopedRouter<M,D>,
    windows:&crate::gui_broker::Broker<W,Q>,
    window:u32,epoch:u64)->bool{
   if service==0||owner.pid==0||owner.generation==0||
      owner.generation>u32::MAX as u64||window==0||epoch==0{
      return false;
   }
   if router.pending(owner).is_err() ||
      windows.grant_epoch_for(owner.pid,window)!=Ok(epoch){
      return false;
   }
   self.prepared(service,owner.pid,owner.generation as u32)
 }
}
#[cfg(test)]
mod r103_ipc_and_window_prepared_tests{
 use super::*;
 use crate::batch5_session::Identity;
 use crate::gui_broker::Broker;
 use crate::ipc::ScopedRouter;
 #[test]fn replacement_becomes_ready_only_with_both_kernel_owned_grants(){
  let mut sup=ServiceTable::<1>::new();sup.configure(7,10,1);sup.start(7);
  assert_eq!(sup.crash_exact(7,10,1,11),ServiceAction::Restart{pid:11,generation:2});
  let new_owner=Identity{pid:11,generation:2};
  let mut ipc=ScopedRouter::<2,2>::new().unwrap();
  let mut gui=Broker::<2,4>::new();
  assert!(!sup.prepared_with_ipc_and_window(7,new_owner,&ipc,&gui,110,1));
  ipc.register(new_owner).unwrap();
  assert!(!sup.prepared_with_ipc_and_window(7,new_owner,&ipc,&gui,110,1));
  gui.grant(11,110).unwrap();
  let ep=gui.grant_epoch_for(11,110).unwrap();
  assert!(!sup.prepared_with_ipc_and_window(7,new_owner,&ipc,&gui,110,ep+1));
  assert!(sup.prepared_with_ipc_and_window(7,new_owner,&ipc,&gui,110,ep));
  assert!(sup.is_active_exact(7,11,2));
 }
 #[test]fn foreign_or_stale_mailbox_cannot_activate_pending_process(){
  let mut sup=ServiceTable::<1>::new();sup.configure(7,10,1);sup.start(7);
  sup.crash_exact(7,10,1,11);
  let bad=Identity{pid:11,generation:1};
  let good=Identity{pid:11,generation:2};
  let mut ipc=ScopedRouter::<1,2>::new().unwrap();ipc.register(bad).unwrap();
  let mut gui=Broker::<1,2>::new();gui.grant(11,110).unwrap();
  let ep=gui.grant_epoch_for(11,110).unwrap();
  assert!(!sup.prepared_with_ipc_and_window(7,good,&ipc,&gui,110,ep));
  assert!(!sup.prepared_with_ipc_and_window(7,bad,&ipc,&gui,110,ep));
  assert!(!sup.is_active_exact(7,11,2));
  ipc.reap(bad).unwrap();ipc.register(good).unwrap();
  assert!(sup.prepared_with_ipc_and_window(7,good,&ipc,&gui,110,ep));
 }
 #[test]fn grant_revocation_blocks_activation_of_reissued_window(){
  let mut sup=ServiceTable::<1>::new();sup.configure(7,10,1);sup.start(7);
  sup.crash_exact(7,10,1,11);
  let owner=Identity{pid:11,generation:2};
  let mut ipc=ScopedRouter::<1,2>::new().unwrap();ipc.register(owner).unwrap();
  let mut gui=Broker::<1,2>::new();gui.grant(11,110).unwrap();
  let old=gui.grant_epoch_for(11,110).unwrap();
  gui.revoke_window(11,110).unwrap();gui.grant(11,110).unwrap();
  let new_epoch=gui.grant_epoch_for(11,110).unwrap();
  assert_ne!(old,new_epoch);
  assert!(!sup.prepared_with_ipc_and_window(7,owner,&ipc,&gui,110,old));
  assert!(sup.prepared_with_ipc_and_window(7,owner,&ipc,&gui,110,new_epoch));
 }
}


#[cfg(test)]
mod r109_generation_and_restart_credit_admission {
 use super::*;
 #[test]fn active_generation_exhaustion_never_advertises_restart_credit(){
  let mut t=ServiceTable::<2>::new();
  assert!(t.configure(7,70,3));assert!(t.configure(8,80,2));
  assert_eq!(t.start(7),ServiceAction::Start{pid:70,generation:1});
  assert_eq!(t.start(8),ServiceAction::Start{pid:80,generation:1});
  let slot=t.slots.iter().position(|s|s.is_some_and(|s|s.service_id==7)).unwrap();
  t.slots[slot].as_mut().unwrap().generation=u32::MAX;
  assert!(!t.can_restart_service(7));
  assert_eq!(t.remaining_restarts(7),Some(0));
  assert_eq!(t.restart_credits_left(),2);
  assert_eq!(t.crash_exact(7,70,u32::MAX,71),ServiceAction::Deny);
  assert!(t.is_exhausted_service(7));
  assert_eq!(t.snapshot(7).unwrap().pid,70);
  assert!(t.is_active_exact(8,80,1));
 }
 #[test]fn pending_generation_exhaustion_does_not_recycle_replacement_identity(){
  let mut t=ServiceTable::<1>::new();
  assert!(t.configure(7,70,3));
  t.start(7);
  assert_eq!(t.crash_exact(7,70,1,71),
             ServiceAction::Restart{pid:71,generation:2});
  let index=t.slots.iter().position(|s|s.is_some_and(|s|s.service_id==7)).unwrap();
  t.slots[index].as_mut().unwrap().generation=u32::MAX;
  let prior=t.snapshot(7);
  assert_eq!(t.retryable_pending_count(),0);
  assert_eq!(t.retry_pending_exact(7,71,u32::MAX,72),ServiceAction::Deny);
  assert_eq!(t.snapshot(7),prior);
  assert_eq!(t.remaining_restarts(7),Some(0));
 }
 #[test]fn exact_restart_budget_stops_at_configured_limit_and_cannot_steal_peer(){
  let mut t=ServiceTable::<2>::new();
  assert!(t.configure(7,70,1));assert!(t.configure(8,80,2));
  t.start(7);t.start(8);
  assert!(t.can_restart_service(7));
  assert_eq!(t.crash_exact(7,70,1,71),ServiceAction::Restart{pid:71,generation:2});
  assert_eq!(t.remaining_restarts(7),Some(0));
  assert!(t.prepared(7,71,2));
  assert!(!t.can_restart_service(7));
  assert_eq!(t.crash_exact(7,71,2,72),ServiceAction::Deny);
  assert!(!t.snapshot(7).unwrap().active);
  assert!(t.is_active_exact(8,80,1));
  assert_eq!(t.remaining_restarts(8),Some(2));
 }
}


#[cfg(all(test,feature="test-r031-vfs"))]
mod r110_full_shutdown_capability_teardown_tests{
 use super::*;
 use crate::fs_handles::FileHandles;
 use crate::ipc::{ScopedRouter,IpcError};
 use crate::gui_broker::{Broker,DesktopInput,SurfaceLayout,DamageQueue};
 use crate::agent_05_surface_rect::Rectangle;
 use crate::batch5_session::Identity;
 fn rect()->Rectangle{Rectangle{x:0,y:0,width:32,height:24}}
 #[test]fn full_shutdown_retires_files_ipc_focus_surfaces_damage_and_services(){
  let mut supervisor=ServiceTable::<1>::new();
  assert!(supervisor.configure(7,1,1));
  assert_eq!(supervisor.start(7),ServiceAction::Start{pid:1,generation:1});
  let mut handles=FileHandles::new();
  let owner=handles.bind_process(1).unwrap();
  let mut ipc=ScopedRouter::<1,2>::new().unwrap();ipc.register(owner).unwrap();
  let mut broker=Broker::<1,4>::new();broker.grant(1,10).unwrap();
  let mut input=DesktopInput::new();
  input.focus(&mut broker,10).unwrap();
  input.capture_pointer(&broker,1,10).unwrap();
  let mut layout=SurfaceLayout::<1>::new();layout.grant(1,10,rect()).unwrap();
  let mut frames=DamageQueue::<2>::new();
  frames.submit(&layout,1,&[(10,0,0,2,2)]).unwrap();
  assert!(shutdown_and_revoke_capabilities_exact(&mut supervisor,
      &mut handles,&mut broker,&mut input,&mut ipc,
      &mut layout,&mut frames,&[(7,1,1)]));
  assert_eq!(supervisor.active_services(),0);
  assert!(!handles.owns(owner));
  assert_eq!(ipc.pending(owner),Err(IpcError::UnknownReceiver));
  assert!(!broker.owns(1,10));
  assert_eq!(input.focused(),None);assert_eq!(input.captured(),None);
  assert_eq!(layout.geometry(1,10),Err(crate::gui_broker::Error::Unauthorized));
  assert_eq!(frames.pending(),0);
 }
 #[test]fn stale_epoch_or_missing_ipc_rejects_entire_shutdown_without_mutation(){
  let mut supervisor=ServiceTable::<1>::new();supervisor.configure(7,1,1);
  supervisor.start(7);
  let mut handles=FileHandles::new();let owner=handles.bind_process(1).unwrap();
  let mut ipc=ScopedRouter::<1,2>::new().unwrap();ipc.register(owner).unwrap();
  let mut broker=Broker::<1,4>::new();broker.grant(1,10).unwrap();
  let mut input=DesktopInput::new();input.focus(&mut broker,10).unwrap();
  let mut layout=SurfaceLayout::<1>::new();layout.grant(1,10,rect()).unwrap();
  let mut frames=DamageQueue::<2>::new();
  let attempt=|required:&[(u32,u32,u32)],supervisor:&mut ServiceTable<1>,
      handles:&mut FileHandles,broker:&mut Broker<1,4>,
      input:&mut DesktopInput,ipc:&mut ScopedRouter<1,2>,
      layout:&mut SurfaceLayout<1>,frames:&mut DamageQueue<2>|{
       shutdown_and_revoke_capabilities_exact(supervisor,handles,broker,input,
           ipc,layout,frames,required)
      };
  assert!(!attempt(&[(7,1,2)],&mut supervisor,&mut handles,&mut broker,
       &mut input,&mut ipc,&mut layout,&mut frames));
  assert!(supervisor.is_active_exact(7,1,1));assert!(handles.owns(owner));
  assert!(broker.owns(1,10));assert!(input.focused().is_some());
  assert_eq!(layout.geometry(1,10),Ok(rect()));
  assert!(ipc.reap(owner).is_ok());
  assert!(!attempt(&[(7,1,1)],&mut supervisor,&mut handles,&mut broker,
       &mut input,&mut ipc,&mut layout,&mut frames));
  assert!(supervisor.is_active_exact(7,1,1));assert!(handles.owns(owner));
  assert!(broker.owns(1,10));assert!(input.focused().is_some());
 }
}


/// R114: trusted, exact-incarnation teardown coordinator for a Files-like
/// graphical kernel service. Validates ALL authority before releasing any
/// file descriptor, event, input capture, surface or queued damage.
/// The live scheduler must call this on an authenticated PCB death; no user
/// syscall or interrupt dispatcher has been wired by this helper alone.
#[cfg(feature="test-r031-vfs")]
pub fn retire_desktop_instance_exact<
    const W:usize,const Q:usize,const L:usize,const F:usize,const E:usize>(
    identity:crate::batch5_session::Identity,
    window:u32,
    grant_epoch:u64,
    handles:&mut crate::fs_handles::FileHandles,
    events:&mut crate::process_events::ProcessEvents<E>,
    input:&mut crate::gui_broker::DesktopInput,
    broker:&mut crate::gui_broker::Broker<W,Q>,
    layout:&mut crate::gui_broker::SurfaceLayout<L>,
    damage:&mut crate::gui_broker::DamageQueue<F>,
)->Result<(usize,usize),crate::gui_broker::Error>{
    use crate::gui_broker::{Error,retire_crashed_compositor_owner_exact};
    if identity.pid==0||identity.generation==0||
       !handles.owns(identity) {
        return Err(Error::Unauthorized);
    }
    // The broker and surface epoch are independently checked again by the
    // teardown routine before any mutation.
    if broker.grant_epoch_for(identity.pid,window)!=Ok(grant_epoch) ||
       layout.geometry(identity.pid,window).is_err() {
        return Err(Error::Unauthorized);
    }
    let removed_damage=retire_crashed_compositor_owner_exact(
        input,broker,layout,damage,identity.pid,window,grant_epoch)?;
    // A failed file retirement after exact ownership validation would violate
    // the serial kernel-owned lifecycle contract; no other agent may mutate
    // this table between verification and retirement.
    let retired=handles.retire_process(identity);
    debug_assert!(retired);
    let discarded_events=events.clear_pid(identity.pid);
    Ok((removed_damage,discarded_events))
}
#[cfg(all(test,feature="test-r031-vfs"))]
mod r114_kernel_desktop_reap_tests{
    use super::*;
    use crate::gui_broker::{Broker,SurfaceLayout,DesktopInput,DamageQueue,Error,
        grant_compositor_surface_exact};
    use crate::agent_05_surface_rect::Rectangle;
    use crate::fs_handles::FileHandles;
    use crate::process_events::{ProcessEvents,ProcessEvent,EventKind};
    fn rect()->Rectangle{Rectangle{x:1,y:2,width:30,height:40}}
    #[test]
    fn trusted_reap_removes_exact_dead_owner_across_real_tables(){
        let mut fs=FileHandles::new();
        let files=fs.bind_process(2).unwrap();
        let init=fs.bind_process(1).unwrap();
        let mut b=Broker::<3,8>::new();
        let mut l=SurfaceLayout::<3>::new();
        let mut d=DamageQueue::<4>::new();
        let mut i=DesktopInput::new();
        let mut e=ProcessEvents::<5>::new();
        let epoch=grant_compositor_surface_exact(&mut b,&mut l,files.pid,20,rect()).unwrap();
        grant_compositor_surface_exact(&mut b,&mut l,init.pid,10,rect()).unwrap();
        i.focus(&mut b,20).unwrap();
        d.submit(&l,files.pid,&[(20,0,0,10,10)]).unwrap();
        d.submit(&l,init.pid,&[(10,0,0,10,10)]).unwrap();
        e.enqueue(ProcessEvent{pid:files.pid,kind:EventKind::Faulted,value:1}).unwrap();
        e.enqueue(ProcessEvent{pid:init.pid,kind:EventKind::Message,value:2}).unwrap();
        assert_eq!(retire_desktop_instance_exact(files,20,epoch,
            &mut fs,&mut e,&mut i,&mut b,&mut l,&mut d),Ok((1,1)));
        assert!(!fs.owns(files));
        assert!(fs.owns(init));
        assert_eq!(d.pending(),1);
        assert_eq!(e.len(),1);
        assert_eq!(e.peek().unwrap().pid,init.pid);
        assert_eq!(i.focused(),None);
        assert!(!b.owns(files.pid,20));
        assert!(b.owns(init.pid,10));
    }
    #[test]
    fn stale_file_epoch_and_wrong_window_epoch_never_modify_peer_state(){
        let mut fs=FileHandles::new();
        let stale=fs.bind_process(2).unwrap();
        let fresh=fs.bind_process(2).unwrap();
        let mut b=Broker::<2,5>::new();
        let mut l=SurfaceLayout::<2>::new();
        let mut d=DamageQueue::<2>::new();
        let mut i=DesktopInput::new();
        let mut e=ProcessEvents::<2>::new();
        let epoch=grant_compositor_surface_exact(&mut b,&mut l,fresh.pid,20,rect()).unwrap();
        d.submit(&l,fresh.pid,&[(20,0,0,1,1)]).unwrap();
        assert_eq!(retire_desktop_instance_exact(stale,20,epoch,
            &mut fs,&mut e,&mut i,&mut b,&mut l,&mut d),Err(Error::Unauthorized));
        assert_eq!(retire_desktop_instance_exact(fresh,20,epoch+1,
            &mut fs,&mut e,&mut i,&mut b,&mut l,&mut d),Err(Error::Unauthorized));
        assert!(fs.owns(fresh));
        assert!(b.owns(fresh.pid,20));
        assert_eq!(d.pending(),1);
    }
}
