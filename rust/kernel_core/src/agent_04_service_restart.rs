//! Pure, bounded service restart transitions, not a process launcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State { Stopped, Running, WaitingRestart, Exhausted }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event { Start, Crash, Retry, Stop }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Service {
    pub state: State, pub retries: u8, pub retry_budget: u8, pub generation: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceError { InvalidTransition, CounterOverflow }

pub fn transition(mut service: Service, event: Event) -> Result<Service, ServiceError> {
    match (service.state, event) {
        (_, Event::Stop) => service.state = State::Stopped,
        (State::Stopped, Event::Start) => {
            service.generation = service.generation.checked_add(1)
                .ok_or(ServiceError::CounterOverflow)?;
            service.retries = 0;
            service.state = State::Running;
        }
        (State::Running, Event::Crash) => {
            service.state = if service.retries < service.retry_budget {
                State::WaitingRestart
            } else { State::Exhausted };
        }
        (State::WaitingRestart, Event::Retry) => {
            service.retries = service.retries.checked_add(1)
                .ok_or(ServiceError::CounterOverflow)?;
            service.generation = service.generation.checked_add(1)
                .ok_or(ServiceError::CounterOverflow)?;
            service.state = State::Running;
        }
        _ => return Err(ServiceError::InvalidTransition),
    }
    Ok(service)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn base() -> Service {
        Service { state: State::Stopped, retries: 0, retry_budget: 1, generation: 0 }
    }
    #[test]
    fn restart_is_bounded_and_increments_generation() {
        let a = transition(base(), Event::Start).unwrap();
        assert_eq!(a.generation, 1);
        let b = transition(a, Event::Crash).unwrap();
        assert_eq!(b.state, State::WaitingRestart);
        let c = transition(b, Event::Retry).unwrap();
        assert_eq!((c.retries, c.generation), (1, 2));
        assert_eq!(transition(c, Event::Crash).unwrap().state, State::Exhausted);
    }
    #[test]
    fn invalid_transition_is_rejected() {
        assert_eq!(transition(base(), Event::Retry), Err(ServiceError::InvalidTransition));
        assert_eq!(transition(base(), Event::Crash), Err(ServiceError::InvalidTransition));
    }
    #[test]
    fn generation_overflow_does_not_start_service() {
        let mut s = base(); s.generation = u32::MAX;
        assert_eq!(transition(s, Event::Start), Err(ServiceError::CounterOverflow));
        assert_eq!(transition(s, Event::Stop).unwrap().state, State::Stopped);
    }
}

/// Calculate the earliest retry time with linear backoff. A running/stopped
/// service cannot allocate a restart timer; overflow denies scheduling.
pub fn retry_deadline(service: Service, now_ticks: u64, interval_ticks: u64)
    -> Result<u64, ServiceError>
{
    if service.state != State::WaitingRestart || interval_ticks == 0 {
        return Err(ServiceError::InvalidTransition);
    }
    if service.retries >= service.retry_budget {
        return Err(ServiceError::InvalidTransition);
    }
    let next = u64::from(service.retries) + 1;
    let delay = interval_ticks.checked_mul(next).ok_or(ServiceError::CounterOverflow)?;
    now_ticks.checked_add(delay).ok_or(ServiceError::CounterOverflow)
}
#[cfg(test)]
mod backoff_tests {
    use super::*;
    fn ready() -> Service {
        Service { state: State::WaitingRestart, retries: 1, retry_budget: 3, generation: 4 }
    }
    #[test]
    fn backoff_grows_with_retry_count() {
        assert_eq!(retry_deadline(ready(), 100, 5), Ok(110));
        let mut s = ready(); s.retries = 2;
        assert_eq!(retry_deadline(s, 100, 5), Ok(115));
    }
    #[test]
    fn cannot_retry_stopped_or_exhausted_service() {
        let mut s = ready(); s.state = State::Stopped;
        assert_eq!(retry_deadline(s, 100, 5), Err(ServiceError::InvalidTransition));
        s.state = State::WaitingRestart; s.retries = 3;
        assert_eq!(retry_deadline(s, 100, 5), Err(ServiceError::InvalidTransition));
    }
    #[test]
    fn prevents_timestamp_wraparound() {
        assert_eq!(retry_deadline(ready(), u64::MAX, 1), Err(ServiceError::CounterOverflow));
        assert_eq!(retry_deadline(ready(), 100, 0), Err(ServiceError::InvalidTransition));
    }
}


/// Permit one bounded service restart only after an explicit scheduler
/// deadline and exact generation match; no real process is launched.
pub fn retry_if_due(service:Service,observed_generation:u32,
    now_ticks:u64,deadline_ticks:u64)->Result<Service,ServiceError>{
    if service.state!=State::WaitingRestart||
       service.generation==0||
       observed_generation!=service.generation||
       now_ticks<deadline_ticks||
       service.retries>=service.retry_budget{
        return Err(ServiceError::InvalidTransition)
    }
    transition(service,Event::Retry)
}
#[cfg(test)]
mod retry_gate_tests{
    use super::*;
    fn waiting()->Service{
        Service{state:State::WaitingRestart,retries:0,
                retry_budget:2,generation:11}
    }
    #[test]fn exact_deadline_rotates_generation(){
        let restarted=retry_if_due(waiting(),11,150,150).unwrap();
        assert_eq!(restarted.state,State::Running);
        assert_eq!(restarted.generation,12);
        assert_eq!(restarted.retries,1);
    }
    #[test]fn early_or_stale_restart_is_blocked(){
        assert_eq!(retry_if_due(waiting(),11,149,150),
                   Err(ServiceError::InvalidTransition));
        assert_eq!(retry_if_due(waiting(),10,150,150),
                   Err(ServiceError::InvalidTransition));
        assert_eq!(retry_if_due(waiting(),0,150,150),
                   Err(ServiceError::InvalidTransition));
    }
    #[test]fn exhausted_and_generation_wrap_are_rejected(){
        let mut s=waiting();s.retries=2;
        assert_eq!(retry_if_due(s,11,150,150),Err(ServiceError::InvalidTransition));
        s.retries=0;s.state=State::Exhausted;
        assert_eq!(retry_if_due(s,11,150,150),Err(ServiceError::InvalidTransition));
        s.state=State::WaitingRestart;s.generation=u32::MAX;
        assert_eq!(retry_if_due(s,u32::MAX,150,150),Err(ServiceError::CounterOverflow));
    }
}


/// Ensure the observed scheduler tick is strictly newer than the last
/// consumed retry tick, preventing replay of one observed deadline.
pub fn retry_fresh(service:Service,observed_generation:u32,
 now:u64,deadline:u64,last_consumed:u64)->Result<Service,ServiceError>{
 if now<=last_consumed{return Err(ServiceError::InvalidTransition)}
 retry_if_due(service,observed_generation,now,deadline)
}
#[cfg(test)]mod replay_retry_tests{
 use super::*;
 fn waiting()->Service{Service{state:State::WaitingRestart,retries:0,retry_budget:2,generation:8}}
 #[test]fn newer_deadline_allows_retry(){assert_eq!(retry_fresh(waiting(),8,100,99,90).unwrap().generation,9);}
 #[test]fn repeated_tick_cannot_retry(){assert_eq!(retry_fresh(waiting(),8,100,99,100),Err(ServiceError::InvalidTransition));}
 #[test]fn wrong_generation_cannot_retry(){assert_eq!(retry_fresh(waiting(),9,100,99,90),Err(ServiceError::InvalidTransition));}
}


// R007: deterministic supervisor policy, not an actual process scheduler.
// At most N services are tracked and retries require exact generation.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct TrackedService{pub id:u16,pub service:Service,pub deadline:u64}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum SupervisorError{Invalid,Full,Missing,Duplicate,Service(ServiceError)}
pub struct ServiceGroup<const N:usize>{entries:[Option<TrackedService>;N]}
impl<const N:usize> ServiceGroup<N>{
    pub const fn new()->Self{Self{entries:[None;N]}}
    fn index(&self,id:u16)->Option<usize>{
        self.entries.iter().position(|x|x.is_some_and(|s|s.id==id))
    }
    pub fn register(&mut self,id:u16,retry_budget:u8)
        ->Result<u32,SupervisorError>{
        if id==0||retry_budget==0{return Err(SupervisorError::Invalid);}
        if self.index(id).is_some(){return Err(SupervisorError::Duplicate);}
        let slot=self.entries.iter_mut().find(|e|e.is_none())
            .ok_or(SupervisorError::Full)?;
        let svc=transition(Service{state:State::Stopped,retries:0,
            retry_budget,generation:0},Event::Start).map_err(SupervisorError::Service)?;
        *slot=Some(TrackedService{id,service:svc,deadline:0});
        Ok(svc.generation)
    }
    pub fn status(&self,id:u16)->Option<TrackedService>{
        self.index(id).and_then(|i|self.entries[i])
    }
    pub fn crash(&mut self,id:u16,generation:u32,now:u64,interval:u64)
        ->Result<TrackedService,SupervisorError>{
        let i=self.index(id).ok_or(SupervisorError::Missing)?;
        let mut entry=self.entries[i].ok_or(SupervisorError::Missing)?;
        if entry.service.generation!=generation{
            return Err(SupervisorError::Invalid);
        }
        entry.service=transition(entry.service,Event::Crash)
            .map_err(SupervisorError::Service)?;
        entry.deadline=if entry.service.state==State::WaitingRestart{
            retry_deadline(entry.service,now,interval)
                .map_err(SupervisorError::Service)?
        }else{0};
        self.entries[i]=Some(entry);Ok(entry)
    }
    pub fn retry(&mut self,id:u16,generation:u32,now:u64)
        ->Result<TrackedService,SupervisorError>{
        let i=self.index(id).ok_or(SupervisorError::Missing)?;
        let mut entry=self.entries[i].ok_or(SupervisorError::Missing)?;
        entry.service=retry_if_due(entry.service,generation,now,entry.deadline)
            .map_err(SupervisorError::Service)?;
        entry.deadline=0;self.entries[i]=Some(entry);Ok(entry)
    }
    pub fn stop(&mut self,id:u16,generation:u32)->Result<(),SupervisorError>{
        let i=self.index(id).ok_or(SupervisorError::Missing)?;
        let entry=self.entries[i].ok_or(SupervisorError::Missing)?;
        if entry.service.generation!=generation{return Err(SupervisorError::Invalid);}
        self.entries[i]=None;Ok(())
    }
}
#[cfg(test)]mod r007_supervisor_tests{
    use super::*;
    #[test]fn crash_retry_exhaustion_and_manual_cleanup(){
        let mut g=ServiceGroup::<2>::new();
        let gen=g.register(2,1).unwrap();
        let crash=g.crash(2,gen,10,5).unwrap();
        assert_eq!(crash.deadline,15);
        assert_eq!(g.retry(2,gen,14),Err(SupervisorError::Service(ServiceError::InvalidTransition)));
        let running=g.retry(2,gen,15).unwrap();
        assert_eq!(running.service.generation,2);
        assert_eq!(g.crash(2,2,20,5).unwrap().service.state,State::Exhausted);
        assert_eq!(g.retry(2,2,30),Err(SupervisorError::Service(ServiceError::InvalidTransition)));
        assert_eq!(g.stop(2,2),Ok(()));
        assert!(g.status(2).is_none());
    }
    #[test]fn duplicate_capacity_and_stale_generation_fail_closed(){
        let mut g=ServiceGroup::<1>::new();
        g.register(1,2).unwrap();
        assert_eq!(g.register(1,2),Err(SupervisorError::Duplicate));
        assert_eq!(g.register(2,2),Err(SupervisorError::Full));
        assert_eq!(g.crash(1,99,0,3),Err(SupervisorError::Invalid));
        assert_eq!(g.status(1).unwrap().service.state,State::Running);
    }
}


// R008 dependency boot policy; does not spawn processes or recover a kernel.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct DependentService{pub id:u16,pub parent:u16,pub generation:u32,pub running:bool}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum DependencyFault{Invalid,Missing,Duplicate,Full,Blocked,Overflow}
#[derive(Clone,Copy)]
pub struct DependencySupervisor<const N:usize>{nodes:[Option<DependentService>;N]}
impl<const N:usize> DependencySupervisor<N>{
 pub const fn new()->Self{Self{nodes:[None;N]}}
 pub fn get(&self,id:u16)->Option<DependentService>{
  self.nodes.iter().flatten().find(|v|v.id==id).copied()
 }
 pub fn register(&mut self,id:u16,parent:u16)->Result<(),DependencyFault>{
  if id==0||id==parent{return Err(DependencyFault::Invalid)}
  if self.get(id).is_some(){return Err(DependencyFault::Duplicate)}
  if parent!=0&&self.get(parent).is_none(){return Err(DependencyFault::Blocked)}
  let slot=self.nodes.iter_mut().find(|x|x.is_none()).ok_or(DependencyFault::Full)?;
  *slot=Some(DependentService{id,parent,generation:0,running:false});Ok(())
 }
 pub fn start(&mut self,id:u16)->Result<u32,DependencyFault>{
  let service=self.get(id).ok_or(DependencyFault::Missing)?;
  if service.running{return Err(DependencyFault::Duplicate)}
  if service.parent!=0&&!self.get(service.parent).is_some_and(|p|p.running){
   return Err(DependencyFault::Blocked)
  }
  let generation=service.generation.checked_add(1).ok_or(DependencyFault::Overflow)?;
  let slot=self.nodes.iter_mut().find(|x|x.is_some_and(|v|v.id==id))
   .ok_or(DependencyFault::Missing)?;
  *slot=Some(DependentService{generation,running:true,..service});Ok(generation)
 }
 pub fn start_batch(&mut self,order:&[u16])->Result<usize,DependencyFault>{
  if order.is_empty()||order.len()>N{return Err(DependencyFault::Full)}
  let mut staged=*self;
  for id in order{staged.start(*id)?;}
  *self=staged;Ok(order.len())
 }
 pub fn crash_cascade(&mut self,id:u16,generation:u32)->Result<usize,DependencyFault>{
  let origin=self.get(id).ok_or(DependencyFault::Missing)?;
  if !origin.running||origin.generation!=generation{return Err(DependencyFault::Invalid)}
  let mut affected=[false;N];
  for (i,item) in self.nodes.iter().enumerate(){
   if item.is_some_and(|v|v.id==id){affected[i]=true;}
  }
  for _ in 0..N{
   for i in 0..N{
    if let Some(v)=self.nodes[i]{
     if v.parent!=0&&self.nodes.iter().enumerate().any(|(j,p)|
      affected[j]&&p.is_some_and(|p|p.id==v.parent)){affected[i]=true}
    }
   }
  }
  let mut stopped=0;
  for (i,item) in self.nodes.iter_mut().enumerate(){
   if affected[i]{
    if let Some(s)=item{
     if s.running{stopped+=1}
     s.running=false;
    }
   }
  }
  Ok(stopped)
 }
}
#[cfg(test)]mod r008_dependency_supervisor_tests{
 use super::*;
 #[test]fn starts_in_dependency_order_and_cascades_failure(){
  let mut s=DependencySupervisor::<3>::new();
  s.register(1,0).unwrap();s.register(2,1).unwrap();s.register(3,2).unwrap();
  assert_eq!(s.start(3),Err(DependencyFault::Blocked));
  assert_eq!(s.start_batch(&[1,2,3]),Ok(3));
  assert_eq!(s.crash_cascade(1,1),Ok(3));
  assert_eq!(s.get(3).unwrap().running,false);
  assert_eq!(s.start_batch(&[1,2,3]),Ok(3));
  assert_eq!(s.get(3).unwrap().generation,2);
 }
 #[test]fn stale_generation_cannot_crash_new_session(){
  let mut s=DependencySupervisor::<2>::new();
  s.register(1,0).unwrap();s.start(1).unwrap();
  assert_eq!(s.crash_cascade(1,0),Err(DependencyFault::Invalid));
  assert_eq!(s.get(1).unwrap().running,true);
 }
 #[test]fn failed_start_transaction_is_atomic(){
  let mut s=DependencySupervisor::<2>::new();
  s.register(1,0).unwrap();s.register(2,1).unwrap();
  assert_eq!(s.start_batch(&[1,1]),Err(DependencyFault::Duplicate));
  assert_eq!(s.get(1).unwrap().running,false);
  assert_eq!(s.start_batch(&[2,1]),Err(DependencyFault::Blocked));
 }
}
