//! Bounded, allocation-free process lifecycle event queue (R023).
//!
//! This is a single-owner kernel primitive. Interrupt handlers must not
//! access it without a separately established synchronization policy.
//! The producer must authenticate PIDs against the scheduler's PCB table.
//! Events are not currently exposed as syscalls or a desktop event service.

pub const MAX_PROCESS_EVENTS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventKind { Started, Exited, Faulted, Message }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessEvent {
    pub pid: u32,
    pub kind: EventKind,
    pub value: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventError { InvalidPid, Full, Empty }

/// A FIFO event ring. Zero capacity is valid but cannot store any events.
pub struct ProcessEvents<const CAPACITY: usize> {
    entries: [Option<ProcessEvent>; CAPACITY],
    head: usize,
    count: usize,
}

impl<const CAPACITY: usize> Default for ProcessEvents<CAPACITY> {
    fn default() -> Self { Self::new() }
}

impl<const CAPACITY: usize> ProcessEvents<CAPACITY> {
    pub const fn new() -> Self {
        Self { entries: [None; CAPACITY], head: 0, count: 0 }
    }
    pub fn len(&self) -> usize { self.count }
    pub fn capacity(&self) -> usize { CAPACITY }
    pub fn is_empty(&self) -> bool { self.count == 0 }

    pub fn enqueue(&mut self, event: ProcessEvent) -> Result<(), EventError> {
        if event.pid == 0 { return Err(EventError::InvalidPid); }
        if self.count == CAPACITY { return Err(EventError::Full); }
        let tail = (self.head + self.count) % CAPACITY;
        self.entries[tail] = Some(event);
        self.count += 1;
        Ok(())
    }

    pub fn peek(&self) -> Option<ProcessEvent> {
        if self.count == 0 { return None; }
        self.entries[self.head]
    }

    pub fn dequeue(&mut self) -> Result<ProcessEvent, EventError> {
        if self.count == 0 { return Err(EventError::Empty); }
        let event = self.entries[self.head].take().ok_or(EventError::Empty)?;
        self.head = (self.head + 1) % CAPACITY;
        self.count -= 1;
        Ok(event)
    }

    /// Remove a terminated process's pending events while preserving FIFO
    /// order for all other PIDs. Does not alter global scheduling state.
    pub fn clear_pid(&mut self, pid: u32) -> usize {
        if pid == 0 || self.count == 0 { return 0; }
        let original = self.count;
        let mut removed = 0usize;
        // Dequeue/re-enqueue exactly the original number of entries:
        // surviving events retain their relative order, even across wrap.
        for _ in 0..original {
            let event = self.dequeue().expect("event ring invariant");
            if event.pid == pid {
                removed += 1;
            } else {
                self.enqueue(event).expect("event ring capacity invariant");
            }
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(pid:u32,kind:EventKind,value:u64)->ProcessEvent {
        ProcessEvent{pid,kind,value}
    }

    #[test]
    fn fifo_wraparound_and_backpressure() {
        let mut q=ProcessEvents::<3>::new();
        assert_eq!(q.dequeue(),Err(EventError::Empty));
        q.enqueue(event(1,EventKind::Started,1)).unwrap();
        q.enqueue(event(2,EventKind::Message,2)).unwrap();
        q.enqueue(event(3,EventKind::Faulted,3)).unwrap();
        assert_eq!(q.len(),3);
        assert_eq!(q.enqueue(event(4,EventKind::Exited,4)),Err(EventError::Full));
        assert_eq!(q.peek().unwrap().pid,1);
        assert_eq!(q.dequeue().unwrap().value,1);
        q.enqueue(event(4,EventKind::Exited,4)).unwrap();
        for expected in [2,3,4] {
            assert_eq!(q.dequeue().unwrap().pid,expected);
        }
        assert!(q.is_empty());
        assert_eq!(q.dequeue(),Err(EventError::Empty));
    }

    #[test]
    fn invalid_pid_capacity_one_and_zero_capacity() {
        let mut none=ProcessEvents::<0>::new();
        assert_eq!(none.enqueue(event(1,EventKind::Started,0)),Err(EventError::Full));
        assert_eq!(none.dequeue(),Err(EventError::Empty));
        assert_eq!(none.clear_pid(1),0);
        let mut one=ProcessEvents::<1>::new();
        assert_eq!(one.enqueue(event(0,EventKind::Started,0)),Err(EventError::InvalidPid));
        for value in 0..5 {
            one.enqueue(event(7,EventKind::Message,value)).unwrap();
            assert_eq!(one.dequeue().unwrap().value,value);
        }
        assert_eq!(one.len(),0);
        assert_eq!(one.capacity(),1);
    }

    #[test]
    fn teardown_preserves_other_process_order_after_wraparound() {
        let mut q=ProcessEvents::<4>::new();
        for pid in [1,2,1,3] { q.enqueue(event(pid,EventKind::Message,pid as u64)).unwrap(); }
        assert_eq!(q.dequeue().unwrap().pid,1);
        q.enqueue(event(2,EventKind::Exited,20)).unwrap();
        // FIFO now: 2,1,3,2, with wrapped head/tail.
        assert_eq!(q.clear_pid(1),1);
        assert_eq!(q.clear_pid(0),0);
        assert_eq!(q.clear_pid(99),0);
        assert_eq!(q.len(),3);
        assert_eq!(q.dequeue().unwrap().value,2);
        assert_eq!(q.dequeue().unwrap().value,3);
        assert_eq!(q.dequeue().unwrap().value,20);
        assert!(q.is_empty());
        q.enqueue(event(9,EventKind::Started,100)).unwrap();
        assert_eq!(q.clear_pid(9),1);
        assert!(q.is_empty());
    }

    #[test]
    fn every_other_event_can_be_cleared_without_reordering_survivors() {
        let mut q=ProcessEvents::<6>::new();
        for pid in [9,3,9,4,9,5] {
            q.enqueue(event(pid,EventKind::Started,pid as u64)).unwrap();
        }
        assert_eq!(q.clear_pid(9),3);
        assert_eq!(q.len(),3);
        for pid in [3,4,5] { assert_eq!(q.dequeue().unwrap().pid,pid); }
        assert_eq!(q.dequeue(),Err(EventError::Empty));
    }
}

 
/// R009 multi-core scheduler foundation. Separate single-owner queues are
/// supplied by a trusted CPU dispatcher; a real SMP implementation must
/// provide locking, IPIs, TLB shootdown and hardware context switching.
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum DispatchError { InvalidCpu, InvalidTask, Duplicate, Full, Empty }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Runnable { pub pid:u32, pub generation:u64, pub priority:u8 }
pub struct CoreRunQueues<const C:usize,const N:usize>{
 queues:[[Option<Runnable>;N];C], length:[usize;C], cursor:[usize;C],
}
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub const fn new()->Self{
  Self{queues:[[None;N];C],length:[0;C],cursor:[0;C]}
 }
 pub fn queue_len(&self,cpu:usize)->Result<usize,DispatchError>{
  self.length.get(cpu).copied().ok_or(DispatchError::InvalidCpu)
 }
 pub fn enqueue(&mut self,cpu:usize,task:Runnable)->Result<(),DispatchError>{
  if task.pid==0||task.generation==0{return Err(DispatchError::InvalidTask);}
  if cpu>=C{return Err(DispatchError::InvalidCpu);}
  if self.queues.iter().flatten().flatten().any(|t|
       t.pid==task.pid&&t.generation==task.generation){return Err(DispatchError::Duplicate);}
  if self.length[cpu]==N{return Err(DispatchError::Full);}
  self.queues[cpu][self.length[cpu]]=Some(task);self.length[cpu]+=1;Ok(())
 }
 pub fn next(&mut self,cpu:usize)->Result<Runnable,DispatchError>{
  if cpu>=C{return Err(DispatchError::InvalidCpu);}
  let count=self.length[cpu];
  if count==0{return Err(DispatchError::Empty);}
  let index=self.cursor[cpu]%count;
  let task=self.queues[cpu][index].ok_or(DispatchError::Empty)?;
  self.cursor[cpu]=(index+1)%count;Ok(task)
 }
 pub fn evict(&mut self,pid:u32,generation:u64)->usize{
  if pid==0||generation==0{return 0;}
  let mut removed=0;
  for cpu in 0..C{
   let mut i=0;
   while i<self.length[cpu]{
    if self.queues[cpu][i].is_some_and(|t|t.pid==pid&&t.generation==generation){
     for j in i..self.length[cpu]-1{self.queues[cpu][j]=self.queues[cpu][j+1];}
     self.length[cpu]-=1;
     self.queues[cpu][self.length[cpu]]=None;
     if self.length[cpu]==0{self.cursor[cpu]=0;}
     else if self.cursor[cpu]>i{self.cursor[cpu]-=1;}
     else{self.cursor[cpu]%=self.length[cpu];}
     removed+=1;
    }else{i+=1;}
   }
  }
  removed
 }
}
#[cfg(test)]
mod r009_core_run_queue_tests{
 use super::*;
 fn task(pid:u32,generation:u64)->Runnable{Runnable{pid,generation,priority:1}}
 #[test]fn fair_rotation_for_each_core(){
  let mut q=CoreRunQueues::<2,3>::new();
  q.enqueue(0,task(1,1)).unwrap();q.enqueue(0,task(2,1)).unwrap();
  q.enqueue(1,task(3,1)).unwrap();
  assert_eq!(q.next(0),Ok(task(1,1)));
  assert_eq!(q.next(0),Ok(task(2,1)));
  assert_eq!(q.next(0),Ok(task(1,1)));
  assert_eq!(q.next(1),Ok(task(3,1)));
 }
 #[test]fn reuse_and_cross_core_duplicate_fail_closed(){
  let mut q=CoreRunQueues::<2,2>::new();
  q.enqueue(0,task(7,1)).unwrap();
  assert_eq!(q.enqueue(1,task(7,1)),Err(DispatchError::Duplicate));
  assert_eq!(q.enqueue(2,task(8,1)),Err(DispatchError::InvalidCpu));
  assert_eq!(q.evict(7,1),1);
  q.enqueue(1,task(7,2)).unwrap();
  assert_eq!(q.evict(7,1),0);
  assert_eq!(q.next(1),Ok(task(7,2)));
 }
 #[test]fn queue_capacity_and_zero_cores_do_not_panic(){
  let mut q=CoreRunQueues::<0,0>::new();
  assert_eq!(q.next(0),Err(DispatchError::InvalidCpu));
  assert_eq!(q.enqueue(0,task(1,1)),Err(DispatchError::InvalidCpu));
  let mut one=CoreRunQueues::<1,1>::new();
  one.enqueue(0,task(1,1)).unwrap();
  assert_eq!(one.enqueue(0,task(2,1)),Err(DispatchError::Full));
  assert_eq!(one.evict(1,1),1);
  assert_eq!(one.next(0),Err(DispatchError::Empty));
 }
}

 
/// R010 safe single-dispatcher queue migration. Kernel SMP callers MUST
/// hold both run-queue locks and arrange IPI/TLB synchronization separately.
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn migrate(&mut self,from:usize,to:usize,pid:u32,
  generation:u64)->Result<(),DispatchError>{
  if from>=C||to>=C{return Err(DispatchError::InvalidCpu);}
  if from==to||pid==0||generation==0{return Err(DispatchError::InvalidTask);}
  let position=self.queues[from][..self.length[from]].iter().position(|s|
    s.is_some_and(|task|task.pid==pid&&task.generation==generation))
    .ok_or(DispatchError::InvalidTask)?;
  if self.length[to]>=N{return Err(DispatchError::Full);}
  let task=self.queues[from][position].ok_or(DispatchError::InvalidTask)?;
  for i in position..self.length[from]-1{self.queues[from][i]=self.queues[from][i+1];}
  self.length[from]-=1;self.queues[from][self.length[from]]=None;
  if self.length[from]==0{self.cursor[from]=0;}
  else if self.cursor[from]>position{self.cursor[from]-=1;}
  else{self.cursor[from]%=self.length[from];}
  self.queues[to][self.length[to]]=Some(task);self.length[to]+=1;
  Ok(())
 }
 pub fn steal_for_idle(&mut self,idle:usize)->Result<Option<Runnable>,DispatchError>{
  if idle>=C{return Err(DispatchError::InvalidCpu);}
  if self.length[idle]!=0{return Ok(None);}
  let mut donor=None;
  for cpu in 0..C{
   if cpu!=idle&&self.length[cpu]>1&&donor.is_none_or(|d:usize|
      self.length[cpu]>self.length[d]){donor=Some(cpu);}
  }
  let Some(from)=donor else{return Ok(None)};
  let task=self.queues[from][self.length[from]-1].ok_or(DispatchError::Empty)?;
  self.migrate(from,idle,task.pid,task.generation)?;
  Ok(Some(task))
 }
 pub fn snapshot_core(&self,cpu:usize,out:&mut[Runnable])
  ->Result<usize,DispatchError>{
  if cpu>=C{return Err(DispatchError::InvalidCpu);}
  if out.len()<self.length[cpu]{return Err(DispatchError::Full);}
  for (i,entry) in self.queues[cpu][..self.length[cpu]].iter().enumerate(){
   out[i]=entry.ok_or(DispatchError::Empty)?;
  }
  Ok(self.length[cpu])
 }
}
#[cfg(test)]
mod r010_smp_migration_tests{
 use super::*;
 fn job(pid:u32,generation:u64)->Runnable{Runnable{pid,generation,priority:1}}
 #[test]fn migrate_preserves_affinity_and_no_duplicate(){
  let mut q=CoreRunQueues::<2,3>::new();
  q.enqueue(0,job(4,2)).unwrap();q.enqueue(0,job(5,2)).unwrap();
  q.migrate(0,1,job(5,2).pid,2).unwrap();
  assert_eq!(q.queue_len(0),Ok(1));assert_eq!(q.queue_len(1),Ok(1));
  assert_eq!(q.next(1),Ok(job(5,2)));
  assert_eq!(q.enqueue(0,job(5,2)),Err(DispatchError::Duplicate));
 }
 #[test]fn steal_from_busy_core_is_generation_preserving(){
  let mut q=CoreRunQueues::<2,3>::new();
  q.enqueue(0,job(1,5)).unwrap();q.enqueue(0,job(2,6)).unwrap();
  assert_eq!(q.steal_for_idle(1),Ok(Some(job(2,6))));
  assert_eq!(q.next(0),Ok(job(1,5)));
  assert_eq!(q.next(1),Ok(job(2,6)));
  assert_eq!(q.steal_for_idle(1),Ok(None));
 }
 #[test]fn full_destination_or_stale_generation_is_atomic(){
  let mut q=CoreRunQueues::<2,2>::new();
  q.enqueue(0,job(1,3)).unwrap();q.enqueue(1,job(2,3)).unwrap();
  q.enqueue(1,job(3,3)).unwrap();
  assert_eq!(q.migrate(0,1,1,3),Err(DispatchError::Full));
  assert_eq!(q.migrate(0,1,1,4),Err(DispatchError::InvalidTask));
  assert_eq!(q.next(0),Ok(job(1,3)));
  let mut tiny=[job(99,1);1];
  assert_eq!(q.snapshot_core(1,&mut tiny),Err(DispatchError::Full));
  assert_eq!(tiny,[job(99,1)]);
 }
}

 
/// R011 affinity-aware admission. CPU masks represent slots 0..63 and are
/// selected only by a trusted scheduler after authenticating the process.
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn enqueue_affine(&mut self,task:Runnable,allowed:u64)
  ->Result<usize,DispatchError>{
  if task.pid==0||task.generation==0||allowed==0||C==0{
   return Err(DispatchError::InvalidTask);
  }
  if self.queues.iter().flatten().flatten().any(|known|
     known.pid==task.pid&&known.generation==task.generation){
   return Err(DispatchError::Duplicate);
  }
  let mut chosen=None;
  for cpu in 0..C.min(64){
   if allowed&(1u64<<cpu)==0||self.length[cpu]>=N{continue;}
   if chosen.is_none_or(|current:usize|self.length[cpu]<self.length[current]){
    chosen=Some(cpu);
   }
  }
  let cpu=chosen.ok_or(DispatchError::Full)?;
  self.enqueue(cpu,task)?;Ok(cpu)
 }
 /// Restrict cross-core migration by an explicit policy mask.
 pub fn migrate_affine(&mut self,from:usize,to:usize,
  task:Runnable,allowed:u64)->Result<(),DispatchError>{
  if to>=64||allowed&(1u64<<to)==0{return Err(DispatchError::InvalidTask);}
  self.migrate(from,to,task.pid,task.generation)
 }
 /// Fair round-robin scan for tasks meeting a priority threshold. Avoids
 /// starvation by allowing trusted scheduler to lower the threshold.
 pub fn next_eligible(&mut self,cpu:usize,minimum_priority:u8)
  ->Result<Runnable,DispatchError>{
  if cpu>=C{return Err(DispatchError::InvalidCpu);}
  let count=self.length[cpu];
  if count==0{return Err(DispatchError::Empty);}
  for offset in 0..count{
   let i=(self.cursor[cpu]+offset)%count;
   if let Some(task)=self.queues[cpu][i]{
    if task.priority>=minimum_priority{
     self.cursor[cpu]=(i+1)%count;return Ok(task);
    }
   }
  }
  Err(DispatchError::Empty)
 }
 pub fn retire_all_epochs(&mut self,pid:u32)->usize{
  if pid==0{return 0;}
  let mut ids=[(0u32,0u64);0];let _=&mut ids;
  let mut removed=0;
  for cpu in 0..C{
   let mut i=0;
   while i<self.length[cpu]{
    if self.queues[cpu][i].is_some_and(|task|task.pid==pid){
     let generation=self.queues[cpu][i].unwrap().generation;
     removed+=self.evict(pid,generation);
     i=0;
   }else{i+=1;}
   }
  }
  removed
 }
}
#[cfg(test)]
mod r011_affinity_tests{
 use super::*;
 fn j(pid:u32,p:u8)->Runnable{Runnable{pid,generation:1,priority:p}}
 #[test]fn least_loaded_eligible_cpu_is_selected_without_duplicate(){
  let mut q=CoreRunQueues::<3,2>::new();
  assert_eq!(q.enqueue_affine(j(1,2),0b011),Ok(0));
  assert_eq!(q.enqueue_affine(j(2,3),0b011),Ok(1));
  assert_eq!(q.enqueue_affine(j(3,4),0b100),Ok(2));
  assert_eq!(q.enqueue_affine(j(3,4),0b100),Err(DispatchError::Duplicate));
 }
 #[test]fn invalid_affinity_migration_and_capacity_preserve_state(){
  let mut q=CoreRunQueues::<2,1>::new();
  q.enqueue(0,j(1,2)).unwrap();q.enqueue(1,j(2,2)).unwrap();
  assert_eq!(q.enqueue_affine(j(3,2),0b11),Err(DispatchError::Full));
  assert_eq!(q.migrate_affine(0,1,j(1,2),0b01),Err(DispatchError::InvalidTask));
  assert_eq!(q.next(0),Ok(j(1,2)));
 }
 #[test]fn eligible_rr_threshold_and_all_epoch_cleanup(){
  let mut q=CoreRunQueues::<2,3>::new();
  q.enqueue(0,j(4,1)).unwrap();
  q.enqueue(0,j(5,3)).unwrap();
  q.enqueue(1,Runnable{pid:4,generation:2,priority:3}).unwrap();
  assert_eq!(q.next_eligible(0,3),Ok(j(5,3)));
  assert_eq!(q.next_eligible(0,4),Err(DispatchError::Empty));
  assert_eq!(q.retire_all_epochs(4),2);
  assert_eq!(q.next(0),Ok(j(5,3)));
  assert_eq!(q.next(1),Err(DispatchError::Empty));
 }
}
