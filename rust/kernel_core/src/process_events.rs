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

/// Kernel-only one-time acknowledgement proof. The sequence does not derive
/// from an untrusted guest-provided PID, kind or message payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventReceipt {
    pub sequence:u64,
    pub event:ProcessEvent,
}

/// A FIFO event ring. Zero capacity is valid but cannot store any events.
#[derive(Clone)]
pub struct ProcessEvents<const CAPACITY: usize> {
    entries: [Option<ProcessEvent>; CAPACITY],
    sequences: [u64; CAPACITY],
    next_sequence:u64,
    head: usize,
    count: usize,
}

impl<const CAPACITY: usize> Default for ProcessEvents<CAPACITY> {
    fn default() -> Self { Self::new() }
}

impl<const CAPACITY: usize> ProcessEvents<CAPACITY> {
    pub const fn new() -> Self {
        Self { entries: [None; CAPACITY], sequences:[0;CAPACITY],
            next_sequence:1, head: 0, count: 0 }
    }
    pub fn len(&self) -> usize { self.count }
    pub fn capacity(&self) -> usize { CAPACITY }
    pub fn is_empty(&self) -> bool { self.count == 0 }

    pub fn enqueue(&mut self, event: ProcessEvent) -> Result<(), EventError> {
        if event.pid == 0 { return Err(EventError::InvalidPid); }
        if self.count == CAPACITY { return Err(EventError::Full); }
        // Never wrap to sequence 0 or reuse an earlier acknowledgement ID.
        let next=self.next_sequence.checked_add(1).ok_or(EventError::Full)?;
        let tail = (self.head + self.count) % CAPACITY;
        self.entries[tail] = Some(event);
        self.sequences[tail]=self.next_sequence;
        self.next_sequence=next;
        self.count += 1;
        Ok(())
    }

    pub fn peek(&self) -> Option<ProcessEvent> {
        if self.count == 0 { return None; }
        self.entries[self.head]
    }

    /// Read the current, immutable head event and its unique enqueue ID.
    /// A consumer must retain this full receipt, not just the event value.
    pub fn peek_receipt(&self)->Option<EventReceipt>{
        if self.count==0{return None;}
        let event=self.entries[self.head]?;
        let sequence=self.sequences[self.head];
        if sequence==0{return None;}
        Some(EventReceipt{sequence,event})
    }

    /// A receipt from an earlier dequeue, identical message, or ring slot
    /// reuse cannot consume this event. The ring is single-owner BSP only.
    pub fn acknowledge_receipt(
        &mut self,expected:EventReceipt
    )->Option<ProcessEvent>{
        if expected.sequence==0||self.peek_receipt()!=Some(expected){
            return None;
        }
        self.dequeue().ok()
    }

    /// Copy an ordered, immutable receipt snapshot without consuming events.
    /// Insufficient space or a malformed entry leaves the caller buffer intact.
    pub fn snapshot_receipts(&self,out:&mut[EventReceipt])
        ->Result<usize,EventError>{
        if out.len()<self.count{return Err(EventError::Full);}
        for i in 0..self.count{
            let index=(self.head+i)%CAPACITY;
            if self.entries[index].is_none()||self.sequences[index]==0{
                return Err(EventError::Empty);
            }
        }
        for i in 0..self.count{
            let index=(self.head+i)%CAPACITY;
            out[i]=EventReceipt{sequence:self.sequences[index],
                event:self.entries[index].ok_or(EventError::Empty)?};
        }
        Ok(self.count)
    }
    pub fn dequeue(&mut self) -> Result<ProcessEvent, EventError> {
        if self.count == 0 { return Err(EventError::Empty); }
        let event = self.entries[self.head].take().ok_or(EventError::Empty)?;
        self.sequences[self.head]=0;
        self.head = (self.head + 1) % CAPACITY;
        self.count -= 1;
        Ok(event)
    }

    /// Compaction preserves FIFO order and existing receipt sequence IDs.
    /// It must not allocate new IDs, especially after counter exhaustion.
    fn remove_matching(&mut self,mut discard:impl FnMut(ProcessEvent)->bool)->usize{
        let original=self.count;
        let mut removed=0;
        for _ in 0..original{
            let at=self.head;
            let event=self.entries[at].take().expect("validated FIFO entry");
            let sequence=self.sequences[at];
            self.sequences[at]=0;
            self.head=(self.head+1)%CAPACITY;
            self.count-=1;
            if discard(event){removed+=1;}
            else{
                let tail=(self.head+self.count)%CAPACITY;
                self.entries[tail]=Some(event);
                self.sequences[tail]=sequence;
                self.count+=1;
            }
        }
        removed
    }
    /// Remove the specified PID without invalidating peer acknowledgements.
    pub fn clear_pid(&mut self,pid:u32)->usize{
        if pid==0{return 0;}
        self.remove_matching(|event|event.pid==pid)
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


/// R012: bounded queue-level CPU balancing. The real SMP runtime must hold
/// runqueue locks and coordinate IPIs and memory barriers before using this.
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn rebalance_once(&mut self,allowed:u64)
  ->Result<Option<Runnable>,DispatchError>{
  if C==0||allowed==0{return Err(DispatchError::InvalidCpu);}
  let mut fullest=None;let mut emptiest=None;
  for cpu in 0..C.min(64){
   if allowed&(1u64<<cpu)==0{continue;}
   if fullest.is_none_or(|idx:usize|self.length[cpu]>self.length[idx]){
    fullest=Some(cpu);
   }
   if emptiest.is_none_or(|idx:usize|self.length[cpu]<self.length[idx]){
    emptiest=Some(cpu);
   }
  }
  let from=fullest.ok_or(DispatchError::InvalidCpu)?;
  let to=emptiest.ok_or(DispatchError::InvalidCpu)?;
  if from==to||self.length[from]<=self.length[to]+1{return Ok(None);}
  let task=self.queues[from][self.length[from]-1]
    .ok_or(DispatchError::Empty)?;
  self.migrate(from,to,task.pid,task.generation)?;
  Ok(Some(task))
 }
 pub fn remove_epoch(&mut self,task:Runnable)->Result<usize,DispatchError>{
  if task.pid==0||task.generation==0{return Err(DispatchError::InvalidTask);}
  Ok(self.evict(task.pid,task.generation))
 }
 pub fn total_runnable(&self)->usize{
  self.length.iter().copied().sum()
 }
}
#[cfg(test)]
mod r012_rebalance_tests{
 use super::*;
 fn t(pid:u32,gen:u64)->Runnable{Runnable{pid,generation:gen,priority:1}}
 #[test]fn rebalancing_migrates_only_one_task_per_iteration(){
  let mut q=CoreRunQueues::<3,4>::new();
  q.enqueue(0,t(1,1)).unwrap();q.enqueue(0,t(2,1)).unwrap();
  q.enqueue(0,t(3,1)).unwrap();
  assert_eq!(q.total_runnable(),3);
  assert_eq!(q.rebalance_once(0b111),Ok(Some(t(3,1))));
  assert_eq!(q.total_runnable(),3);
  assert_eq!(q.queue_len(0),Ok(2));assert_eq!(q.queue_len(1),Ok(1));
  assert_eq!(q.rebalance_once(0b111),Ok(Some(t(2,1))));
  assert_eq!(q.total_runnable(),3);
 }
 #[test]fn restricted_affinity_does_not_touch_disallowed_core(){
  let mut q=CoreRunQueues::<2,3>::new();
  q.enqueue(0,t(1,1)).unwrap();q.enqueue(0,t(2,1)).unwrap();
  assert_eq!(q.rebalance_once(0b01),Ok(None));
  assert_eq!(q.queue_len(1),Ok(0));
 }
 #[test]fn process_epoch_teardown_removes_only_old_process(){
  let mut q=CoreRunQueues::<2,3>::new();
  q.enqueue(0,t(5,1)).unwrap();q.enqueue(1,t(5,2)).unwrap();
  assert_eq!(q.remove_epoch(t(5,1)),Ok(1));
  assert_eq!(q.next(1),Ok(t(5,2)));
  assert_eq!(q.total_runnable(),1);
 }
}

/// R013 load-balancing admission with a maximum migration budget and
/// deterministic CPU mask; actual cross-core synchronization/IPIs remain
/// the responsibility of the trusted SMP scheduler.
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn balance_budgeted(&mut self,allowed:u64,max_moves:usize,
   out:&mut[Runnable])->Result<usize,DispatchError>{
  if allowed==0||C==0{return Err(DispatchError::InvalidCpu);}
  if out.len()<max_moves{return Err(DispatchError::Full);}
  let mut staged=Self{queues:self.queues,length:self.length,cursor:self.cursor};
  let mut n=0;
  while n<max_moves{
   match staged.rebalance_once(allowed)?{
    Some(task)=>{out[n]=task;n+=1;}
    None=>break,
   }
  }
  self.queues=staged.queues;self.length=staged.length;self.cursor=staged.cursor;
  Ok(n)
 }
 /// Reclaim exactly one authenticated task incarnation across all CPUs.
 /// Never treat numeric PID reuse as permission to kill a newer epoch.
 pub fn evict_exact(&mut self,pid:u32,generation:u64)
   ->Result<usize,DispatchError>{
  if pid==0||generation==0{return Err(DispatchError::InvalidTask);}
  Ok(self.evict(pid,generation))
 }
 /// Validate each per-CPU work queue's basic invariants before SMP
 /// rendezvous or QEMU scheduling acceptance is claimed.
 pub fn validate_queues(&self)->Result<usize,DispatchError>{
  let mut total=0usize;
  for cpu in 0..C{
   if self.length[cpu]>N{return Err(DispatchError::Full);}
   for i in 0..self.length[cpu]{
    let task=self.queues[cpu][i].ok_or(DispatchError::InvalidTask)?;
    if task.pid==0||task.generation==0{return Err(DispatchError::InvalidTask);}
    for j in i+1..self.length[cpu]{
     if self.queues[cpu][j].is_some_and(|t|
       t.pid==task.pid&&t.generation==task.generation){
      return Err(DispatchError::Duplicate);
     }
    }
    total+=1;
   }
  }
  Ok(total)
 }
}
#[cfg(test)]
mod r013_cpu_fairness_tests{
 use super::*;
 fn t(pid:u32,gen:u64)->Runnable{Runnable{pid,generation:gen,priority:1}}
 #[test]fn multiple_moves_preserve_all_tasks_and_balance_cores(){
  let mut q=CoreRunQueues::<3,5>::new();
  for pid in 1..=5{q.enqueue(0,t(pid,1)).unwrap();}
  let mut moved=[t(99,1);3];
  let n=q.balance_budgeted(0b111,3,&mut moved).unwrap();
  assert!(n>0);assert_eq!(q.total_runnable(),5);
  assert_eq!(q.validate_queues(),Ok(5));
 }
 #[test]fn short_move_output_never_mutates_queues(){
  let mut q=CoreRunQueues::<2,3>::new();
  q.enqueue(0,t(1,1)).unwrap();q.enqueue(0,t(2,1)).unwrap();
  let mut short=[t(99,1);1];
  assert_eq!(q.balance_budgeted(3,2,&mut short),Err(DispatchError::Full));
  assert_eq!(q.queue_len(0),Ok(2));assert_eq!(short,[t(99,1)]);
 }
 #[test]fn kill_old_epoch_preserves_new_epoch_after_pid_reuse(){
  let mut q=CoreRunQueues::<2,3>::new();
  q.enqueue(0,t(10,1)).unwrap();q.enqueue(1,t(10,2)).unwrap();
  assert_eq!(q.evict_exact(10,1),Ok(1));
  assert_eq!(q.next(1),Ok(t(10,2)));
  assert_eq!(q.validate_queues(),Ok(1));
 }
}

/// R014 all-or-nothing scheduler admission for groups of service workers.
/// The caller supplies CPU masks authenticated by the process supervisor.
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn enqueue_batch_affine(&mut self,
  requests:&[(Runnable,u64)],placed:&mut[usize])->Result<usize,DispatchError>{
  if requests.is_empty()||placed.len()<requests.len()||
     requests.len()>C.saturating_mul(N){return Err(DispatchError::Full);}
  let mut shadow=Self{queues:self.queues,length:self.length,cursor:self.cursor};
  let mut targets=[0usize;64];
  // Keep transient bookkeeping bounded and reject an unexpectedly
  // oversized request without changing any scheduler queues.
  if requests.len()>targets.len(){return Err(DispatchError::Full);}
  for (i,&(task,mask)) in requests.iter().enumerate(){
   targets[i]=shadow.enqueue_affine(task,mask)?;
  }
  self.queues=shadow.queues;self.length=shadow.length;self.cursor=shadow.cursor;
  placed[..requests.len()].copy_from_slice(&targets[..requests.len()]);
  Ok(requests.len())
 }
 pub fn queue_occupancy(&self,cpu:usize)->Result<(usize,usize),DispatchError>{
  if cpu>=C{return Err(DispatchError::InvalidCpu);}
  Ok((self.length[cpu],N.saturating_sub(self.length[cpu])))
 }
 pub fn contains_epoch(&self,pid:u32,generation:u64)->bool{
  if pid==0||generation==0{return false;}
  self.queues.iter().flatten().flatten().any(|t|
   t.pid==pid&&t.generation==generation)
 }
}
#[cfg(test)]
mod r014_group_admission_tests{
 use super::*;
 fn t(pid:u32,gen:u64)->Runnable{Runnable{pid,generation:gen,priority:1}}
 #[test]fn group_admission_distributes_workers_atomically(){
  let mut q=CoreRunQueues::<2,2>::new();let mut cpus=[99;3];
  assert_eq!(q.enqueue_batch_affine(&[
    (t(1,1),3),(t(2,1),3),(t(3,1),3)],&mut cpus),Ok(3));
  assert_eq!(q.total_runnable(),3);
  assert_eq!(cpus,[0,1,0]);assert!(q.contains_epoch(3,1));
 }
 #[test]fn duplicate_in_second_batch_item_rolls_back_first(){
  let mut q=CoreRunQueues::<2,2>::new();let mut cpus=[99;2];
  assert_eq!(q.enqueue_batch_affine(&[(t(1,1),3),(t(1,1),3)],
    &mut cpus),Err(DispatchError::Duplicate));
  assert_eq!(q.total_runnable(),0);assert_eq!(cpus,[99;2]);
 }
 #[test]fn capacity_or_invalid_mask_keeps_existing_service(){
  let mut q=CoreRunQueues::<1,1>::new();q.enqueue(0,t(4,1)).unwrap();
  let mut cpus=[99;1];assert_eq!(q.enqueue_batch_affine(
    &[(t(5,1),1)],&mut cpus),Err(DispatchError::Full));
  assert_eq!(q.queue_occupancy(0),Ok((1,0)));
  assert!(q.contains_epoch(4,1));assert!(!q.contains_epoch(5,1));
 }
}

/// R016: scheduler-only atomic cleanup for several authenticated process
/// incarnations. No SMP inter-core locking is implied by this helper.
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn evict_epochs_atomic(&mut self,epochs:&[(u32,u64)])
  ->Result<usize,DispatchError>{
  if epochs.is_empty()||epochs.len()>C.saturating_mul(N){
   return Err(DispatchError::InvalidTask);
  }
  for (i,&(pid,generation)) in epochs.iter().enumerate(){
   if pid==0||generation==0||epochs[..i].contains(&(pid,generation)){
    return Err(DispatchError::InvalidTask);
   }
   if !self.contains_epoch(pid,generation){
    return Err(DispatchError::InvalidTask);
   }
  }
  let mut staged=Self{queues:self.queues,length:self.length,cursor:self.cursor};
  let mut removed=0usize;
  for &(pid,generation) in epochs{
   removed+=staged.evict(pid,generation);
  }
  self.queues=staged.queues;self.length=staged.length;self.cursor=staged.cursor;
  Ok(removed)
 }
 pub fn capacity_snapshot(&self,output:&mut[(usize,usize)])
  ->Result<usize,DispatchError>{
  if output.len()<C{return Err(DispatchError::Full);}
  for (i,item) in output[..C].iter_mut().enumerate(){
   *item=(self.length[i],N.saturating_sub(self.length[i]));
  }
  Ok(C)
 }
}
#[cfg(test)]mod r016_epoch_teardown_tests{
 use super::*;
 fn task(pid:u32,g:u64)->Runnable{Runnable{pid,generation:g,priority:1}}
 #[test]fn batch_teardown_preserves_unrelated_epoch(){
  let mut q=CoreRunQueues::<2,3>::new();
  q.enqueue(0,task(1,1)).unwrap();q.enqueue(1,task(2,1)).unwrap();
  q.enqueue(0,task(1,2)).unwrap();
  assert_eq!(q.evict_epochs_atomic(&[(1,1),(2,1)]),Ok(2));
  assert_eq!(q.total_runnable(),1);assert!(q.contains_epoch(1,2));
 }
 #[test]fn invalid_second_epoch_cannot_partially_evict(){
  let mut q=CoreRunQueues::<2,2>::new();
  q.enqueue(0,task(1,1)).unwrap();q.enqueue(1,task(2,1)).unwrap();
  assert_eq!(q.evict_epochs_atomic(&[(1,1),(3,1)]),
    Err(DispatchError::InvalidTask));
  assert_eq!(q.total_runnable(),2);
  assert_eq!(q.evict_epochs_atomic(&[(1,1),(1,1)]),
    Err(DispatchError::InvalidTask));
 }
 #[test]fn short_capacity_snapshot_preserves_output(){
  let mut q=CoreRunQueues::<2,2>::new();
  q.enqueue(0,task(1,1)).unwrap();
  let mut short=[(7,7);1];
  assert_eq!(q.capacity_snapshot(&mut short),Err(DispatchError::Full));
  assert_eq!(short,[(7,7)]);
  let mut both=[(0,0);2];assert_eq!(q.capacity_snapshot(&mut both),Ok(2));
  assert_eq!(both,[(1,1),(0,2)]);
 }
}



/// R017 developer process inspector. The supervisor can list runnable
/// process generations without modifying the live run queues or cursor.
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn snapshot_runnable(&self,out:&mut[(usize,Runnable)])
  ->Result<usize,DispatchError>{
  let count=self.total_runnable();
  if out.len()<count{return Err(DispatchError::Full);}
  let mut at=0;
  for cpu in 0..C{
   for task in self.queues[cpu][..self.length[cpu]].iter().flatten(){
    out[at]=(cpu,*task);at+=1;
   }
  }
  Ok(at)
 }
 pub fn terminate_workstation_group(&mut self,epochs:&[(u32,u64)])
  ->Result<usize,DispatchError>{
  self.evict_epochs_atomic(epochs)
 }
}
#[cfg(test)]mod r017_process_inspector_tests{
 use super::*;
 fn task(pid:u32,g:u64)->Runnable{Runnable{pid,generation:g,priority:1}}
 #[test]fn captures_all_runnable_workers_and_cpu_affinity(){
  let mut q=CoreRunQueues::<2,2>::new();
  q.enqueue(0,task(1,1)).unwrap();q.enqueue(1,task(2,1)).unwrap();
  let mut out=[(99,task(9,9));2];
  assert_eq!(q.snapshot_runnable(&mut out),Ok(2));
  assert_eq!(out[0].0,0);assert_eq!(out[1].0,1);
 }
 #[test]fn too_small_view_does_not_mutate_queue_or_output(){
  let mut q=CoreRunQueues::<2,2>::new();
  q.enqueue(0,task(1,1)).unwrap();q.enqueue(1,task(2,1)).unwrap();
  let mut out=[(99,task(9,9));1];
  assert_eq!(q.snapshot_runnable(&mut out),Err(DispatchError::Full));
  assert_eq!(out[0].0,99);assert_eq!(q.total_runnable(),2);
 }
 #[test]fn old_generation_teardown_does_not_kill_reused_pid(){
  let mut q=CoreRunQueues::<2,2>::new();
  q.enqueue(0,task(1,1)).unwrap();q.enqueue(1,task(1,2)).unwrap();
  assert_eq!(q.terminate_workstation_group(&[(1,1)]),Ok(1));
  assert!(q.contains_epoch(1,2));assert!(!q.contains_epoch(1,1));
 }
}


// R114: atomic bounded process-event operations for trusted lifecycle
// coordinators. PID authentication and synchronization remain caller duties.
impl<const CAPACITY: usize> ProcessEvents<CAPACITY> {
    /// Validate the whole batch before publishing any event.
    pub fn enqueue_batch(&mut self, events: &[ProcessEvent])
        -> Result<usize, EventError>
    {
        if events.iter().any(|event| event.pid == 0) {
            return Err(EventError::InvalidPid);
        }
        if events.len() > CAPACITY - self.count ||
           (events.len() as u64)>u64::MAX-self.next_sequence {
            return Err(EventError::Full);
        }
        for &event in events { self.enqueue(event)?; }
        Ok(events.len())
    }

    /// Copy the entire FIFO without consuming it; insufficient destination
    /// capacity leaves the destination unchanged.
    pub fn snapshot(&self, out: &mut [ProcessEvent])
        -> Result<usize, EventError>
    {
        if out.len() < self.count { return Err(EventError::Full); }
        for i in 0..self.count {
            out[i] = self.entries[(self.head + i) % CAPACITY]
                .ok_or(EventError::Empty)?;
        }
        Ok(self.count)
    }

    pub fn count_pid(&self, pid: u32) -> usize {
        if pid == 0 { return 0; }
        (0..self.count).filter(|&i|
            self.entries[(self.head + i) % CAPACITY]
                .is_some_and(|e| e.pid == pid)).count()
    }

    pub fn clear_kind(&mut self,kind:EventKind)->usize{
        self.remove_matching(|event|event.kind==kind)
    }

    /// All-or-nothing validation of a multi-process teardown request.
    pub fn clear_pids(&mut self,pids:&[u32])->Result<usize,EventError>{
        if pids.contains(&0){return Err(EventError::InvalidPid);}
        Ok(self.remove_matching(|event|pids.contains(&event.pid)))
    }
}

#[cfg(test)]
mod r114_process_event_integrity {
    use super::*;
    fn event(pid: u32, value: u64) -> ProcessEvent {
        ProcessEvent { pid, value, kind: EventKind::Message }
    }
    #[test]
    fn batch_is_atomic_for_invalid_pid_and_capacity() {
        let mut q = ProcessEvents::<2>::new();
        assert_eq!(q.enqueue_batch(&[event(1,1),event(0,2)]), Err(EventError::InvalidPid));
        assert_eq!(q.len(), 0);
        assert_eq!(q.enqueue_batch(&[event(1,1),event(2,2)]), Ok(2));
        assert_eq!(q.enqueue_batch(&[event(3,3)]), Err(EventError::Full));
        assert_eq!(q.peek(), Some(event(1,1)));
    }
    #[test]
    fn snapshot_preserves_destination_on_short_buffer_and_wrap() {
        let mut q = ProcessEvents::<3>::new();
        q.enqueue_batch(&[event(1,1),event(2,2),event(3,3)]).unwrap();
        q.dequeue().unwrap();
        q.enqueue(event(4,4)).unwrap();
        let mut short = [event(9,9); 2];
        assert_eq!(q.snapshot(&mut short), Err(EventError::Full));
        assert_eq!(short, [event(9,9); 2]);
        let mut all = [event(0,0); 3];
        assert_eq!(q.snapshot(&mut all), Ok(3));
        assert_eq!(all, [event(2,2),event(3,3),event(4,4)]);
    }
    #[test]
    fn teardown_and_kind_cleanup_keep_other_events_in_order() {
        let mut q = ProcessEvents::<6>::new();
        q.enqueue_batch(&[event(1,1),event(2,2),event(3,3),event(2,4)]).unwrap();
        q.enqueue(ProcessEvent{pid:9,kind:EventKind::Faulted,value:5}).unwrap();
        assert_eq!(q.count_pid(2), 2);
        assert_eq!(q.clear_pids(&[1,0]), Err(EventError::InvalidPid));
        assert_eq!(q.len(), 5);
        assert_eq!(q.clear_kind(EventKind::Faulted), 1);
        assert_eq!(q.clear_pids(&[1,3]), Ok(2));
        assert_eq!(q.dequeue().unwrap().value, 2);
        assert_eq!(q.dequeue().unwrap().value, 4);
    }
    #[test]
    fn empty_capacity_is_safe_for_every_new_operation() {
        let mut q = ProcessEvents::<0>::new();
        assert_eq!(q.enqueue_batch(&[]), Ok(0));
        assert_eq!(q.enqueue_batch(&[event(1,1)]), Err(EventError::Full));
        assert_eq!(q.snapshot(&mut []), Ok(0));
        assert_eq!(q.clear_kind(EventKind::Started), 0);
        assert_eq!(q.clear_pids(&[1]), Ok(0));
        assert_eq!(q.count_pid(1), 0);
    }
}


// R116: exact-incarnation exit notices cannot be replayed into the PID1
// lifecycle consumer, and stale delayed acknowledgements must never delete
// a different generation's event. The value field is the generation ONLY
// for the Exited kind; Message values remain opaque.
impl<const N:usize> ProcessEvents<N>{
 pub fn enqueue_exit_once(&mut self,pid:u32,generation:u64)
   ->Result<bool,EventError>{
  if pid==0||generation==0{return Err(EventError::InvalidPid)}
  let found=(0..self.count).any(|index|{
   self.entries[(self.head+index)%N].is_some_and(|entry|
    entry.pid==pid&&entry.kind==EventKind::Exited&&
    entry.value==generation)
  });
  if found{return Ok(false)}
  self.enqueue(ProcessEvent{pid,kind:EventKind::Exited,value:generation})?;
  Ok(true)
 }
 pub fn clear_exact_exit(&mut self,pid:u32,generation:u64)->usize{
  if pid==0||generation==0{return 0;}
  self.remove_matching(|event|event.pid==pid&&
     event.kind==EventKind::Exited&&event.value==generation)
 }
}
#[cfg(test)]
mod r116_exit_event_generation_tests{
 use super::*;
 #[test]fn same_pid_generation_produces_exactly_one_exit_notice(){
  let mut e=ProcessEvents::<3>::new();
  assert_eq!(e.enqueue_exit_once(2,10),Ok(true));
  assert_eq!(e.enqueue_exit_once(2,10),Ok(false));
  assert_eq!(e.len(),1);
  assert_eq!(e.enqueue_exit_once(2,11),Ok(true));
  assert_eq!(e.len(),2);
  assert_eq!(e.enqueue_exit_once(1,10),Ok(true));
  assert_eq!(e.enqueue_exit_once(3,10),Err(EventError::Full));
 }
 #[test]fn old_pid_epoch_clear_never_steals_new_exit_or_messages(){
  let mut e=ProcessEvents::<5>::new();
  e.enqueue_exit_once(2,10).unwrap();
  e.enqueue(ProcessEvent{pid:2,kind:EventKind::Message,value:10}).unwrap();
  e.enqueue_exit_once(2,11).unwrap();
  e.enqueue_exit_once(1,3).unwrap();
  assert_eq!(e.clear_exact_exit(2,10),1);
  assert_eq!(e.len(),3);
  assert_eq!(e.dequeue().unwrap().kind,EventKind::Message);
  assert_eq!(e.dequeue().unwrap().value,11);
  assert_eq!(e.dequeue().unwrap().pid,1);
 }
 #[test]fn ring_wrap_and_duplicate_detect_only_live_entries(){
  let mut e=ProcessEvents::<3>::new();
  e.enqueue_exit_once(2,2).unwrap();
  e.enqueue_exit_once(3,3).unwrap();
  e.dequeue().unwrap();
  e.enqueue_exit_once(4,4).unwrap();
  e.enqueue_exit_once(2,2).unwrap();
  assert_eq!(e.enqueue_exit_once(2,2),Ok(false));
  assert_eq!(e.clear_exact_exit(3,3),1);
  assert_eq!(e.dequeue().unwrap().pid,4);
  assert_eq!(e.dequeue().unwrap().pid,2);
 }
 #[test]fn invalid_empty_and_zero_capacity_are_pure(){
  let mut e=ProcessEvents::<0>::new();
  assert_eq!(e.enqueue_exit_once(0,1),Err(EventError::InvalidPid));
  assert_eq!(e.enqueue_exit_once(1,0),Err(EventError::InvalidPid));
  assert_eq!(e.enqueue_exit_once(1,1),Err(EventError::Full));
  assert_eq!(e.clear_exact_exit(1,1),0);
  let mut e=ProcessEvents::<2>::new();
  assert_eq!(e.clear_exact_exit(3,3),0);
  assert_eq!(e.enqueue_exit_once(3,3),Ok(true));
  assert_eq!(e.clear_exact_exit(0,3),0);
  assert_eq!(e.clear_exact_exit(3,0),0);
  assert_eq!(e.len(),1);
 }
}


/// Legacy value-only acknowledgement for compatibility with existing tests.
/// Privileged consumers MUST use acknowledge_receipt (unique enqueue ID):
/// equal consecutive ProcessEvent payloads are not replay-safe here.
/// R117: only a byte-for-byte matching head may be acknowledged; a stale
/// process-incarnation receipt cannot dequeue a peer message or successor.
/// Authentication of the consumer itself remains in the runtime supervisor.
impl<const N:usize> ProcessEvents<N>{
 pub fn acknowledge_exact_head(
  &mut self,expected:ProcessEvent
 )->Option<ProcessEvent>{
  if expected.pid==0 || self.peek()!=Some(expected){return None;}
  self.dequeue().ok()
 }
}
#[cfg(test)]
mod r117_exact_event_ack_tests {
 use super::*;
 fn e(pid:u32,kind:EventKind,value:u64)->ProcessEvent{
  ProcessEvent{pid,kind,value}
 }
 #[test]fn exact_head_only_and_duplicate_ack_is_idempotent(){
  let mut q=ProcessEvents::<2>::new();
  let first=e(1,EventKind::Exited,5);
  assert_eq!(q.acknowledge_exact_head(first),None);
  q.enqueue(first).unwrap();
  assert_eq!(q.acknowledge_exact_head(e(1,EventKind::Exited,6)),None);
  assert_eq!(q.len(),1);
  assert_eq!(q.acknowledge_exact_head(first),Some(first));
  assert_eq!(q.acknowledge_exact_head(first),None);
 }
 #[test]fn stale_pid_message_and_successor_remain_untouched(){
  let mut q=ProcessEvents::<3>::new();
  let old=e(2,EventKind::Exited,9);
  let newest=e(2,EventKind::Exited,10);
  let peer=e(1,EventKind::Message,77);
  q.enqueue(newest).unwrap();q.enqueue(peer).unwrap();
  assert_eq!(q.acknowledge_exact_head(old),None);
  assert_eq!(q.acknowledge_exact_head(peer),None);
  assert_eq!(q.len(),2);
  assert_eq!(q.acknowledge_exact_head(newest),Some(newest));
  assert_eq!(q.acknowledge_exact_head(peer),Some(peer));
 }
 #[test]fn wrapped_queue_ack_preserves_fifo_and_capacity(){
  let mut q=ProcessEvents::<2>::new();
  let a=e(1,EventKind::Started,1);
  let b=e(2,EventKind::Exited,2);
  let c=e(1,EventKind::Exited,3);
  q.enqueue(a).unwrap();q.enqueue(b).unwrap();
  assert_eq!(q.acknowledge_exact_head(a),Some(a));
  q.enqueue(c).unwrap();
  assert_eq!(q.acknowledge_exact_head(c),None);
  assert_eq!(q.acknowledge_exact_head(b),Some(b));
  assert_eq!(q.acknowledge_exact_head(c),Some(c));
 }
 #[test]fn zero_capacity_and_invalid_pid_fail_closed(){
  let mut zero=ProcessEvents::<0>::new();
  assert_eq!(zero.acknowledge_exact_head(e(1,EventKind::Exited,1)),None);
  let mut q=ProcessEvents::<1>::new();
  q.enqueue(e(1,EventKind::Exited,8)).unwrap();
  assert_eq!(q.acknowledge_exact_head(e(0,EventKind::Exited,8)),None);
  assert_eq!(q.len(),1);
 }
}


#[cfg(test)]
mod r118_sequence_bound_lifecycle_tests {
 use super::*;
 fn item()->ProcessEvent{ProcessEvent{pid:2,kind:EventKind::Message,value:55}}
 #[test]
 fn replay_cannot_consume_identical_successor_or_reused_ring_slot(){
  let mut q=ProcessEvents::<2>::new();
  q.enqueue(item()).unwrap();
  let old=q.peek_receipt().unwrap();
  q.enqueue(item()).unwrap();
  assert_eq!(q.acknowledge_receipt(old),Some(item()));
  let second=q.peek_receipt().unwrap();
  assert_eq!(second.event,old.event);
  assert_ne!(second.sequence,old.sequence);
  assert_eq!(q.acknowledge_receipt(old),None);
  assert_eq!(q.len(),1);
  assert_eq!(q.acknowledge_receipt(second),Some(item()));
  q.enqueue(item()).unwrap(); // wraps to previously reused ring slot
  let wrapped=q.peek_receipt().unwrap();
  assert_ne!(wrapped.sequence,old.sequence);
  assert_eq!(q.acknowledge_receipt(old),None);
  assert_eq!(q.acknowledge_receipt(wrapped),Some(item()));
 }
 #[test]
 fn peer_or_forged_receipt_must_not_dequeue_head(){
  let mut q=ProcessEvents::<2>::new();
  q.enqueue(item()).unwrap();
  let exact=q.peek_receipt().unwrap();
  let wrong_sequence=EventReceipt{sequence:exact.sequence+1,..exact};
  let wrong_pid=EventReceipt{event:ProcessEvent{pid:1,..item()},..exact};
  assert_eq!(q.acknowledge_receipt(wrong_sequence),None);
  assert_eq!(q.acknowledge_receipt(wrong_pid),None);
  assert_eq!(q.acknowledge_receipt(EventReceipt{sequence:0,..exact}),None);
  assert_eq!(q.len(),1);
  assert_eq!(q.acknowledge_receipt(exact),Some(item()));
 }
 #[test]
 fn capacity_and_sequence_exhaustion_leave_batch_unchanged(){
  let mut q=ProcessEvents::<2>::new();
  q.enqueue(item()).unwrap();
  let head=q.peek_receipt().unwrap();
  q.next_sequence=u64::MAX-1;
  assert_eq!(q.enqueue_batch(&[item(),item()]),Err(EventError::Full));
  assert_eq!(q.peek_receipt(),Some(head));
  assert_eq!(q.len(),1);
  assert_eq!(q.enqueue(item()),Ok(()));
  assert_eq!(q.enqueue(item()),Err(EventError::Full));
  assert_eq!(q.acknowledge_receipt(head),Some(item()));
  assert_eq!(q.enqueue(item()),Err(EventError::Full));
  let zero=ProcessEvents::<0>::new();
  assert_eq!(zero.peek_receipt(),None);
 }
 #[test]
 fn filtering_preserves_fifo_and_survivor_receipts(){
  let mut q=ProcessEvents::<3>::new();
  q.enqueue(ProcessEvent{pid:1,kind:EventKind::Started,value:1}).unwrap();
  q.enqueue(item()).unwrap();
  let removed=q.peek_receipt().unwrap();
  assert_eq!(q.clear_pid(1),1);
  assert_eq!(q.acknowledge_receipt(removed),None);
  let survivor=q.peek_receipt().unwrap();
  assert_eq!(survivor.event,item());
  assert_eq!(q.acknowledge_receipt(survivor),Some(item()));
 }
 #[test]
 fn sequence_exhaustion_keeps_survivor_receipt_across_filters(){
  let mut q=ProcessEvents::<3>::new();
  q.enqueue(item()).unwrap();
  let receipt=q.peek_receipt().unwrap();
  q.enqueue(ProcessEvent{pid:1,kind:EventKind::Exited,value:33}).unwrap();
  q.enqueue(ProcessEvent{pid:9,kind:EventKind::Faulted,value:99}).unwrap();
  q.next_sequence=u64::MAX;
  assert_eq!(q.clear_pid(1),1);
  assert_eq!(q.peek_receipt(),Some(receipt));
  assert_eq!(q.clear_kind(EventKind::Faulted),1);
  assert_eq!(q.peek_receipt(),Some(receipt));
  assert_eq!(q.clear_pids(&[0]),Err(EventError::InvalidPid));
  assert_eq!(q.clear_exact_exit(1,33),0);
  assert_eq!(q.acknowledge_receipt(receipt),Some(item()));
  assert_eq!(q.enqueue(item()),Err(EventError::Full));
 }
}

/// R119: issue exact-generation exit notices transactionally. Invalid
/// members, capacity pressure and receipt exhaustion never publish a prefix.
/// This bounded single-owner queue still requires external SMP synchronization.
impl<const N:usize> ProcessEvents<N>{
 pub fn enqueue_exit_batch_once(&mut self,owners:&[(u32,u64)])
     ->Result<usize,EventError>{
  let mut staged=Self{
    entries:self.entries,sequences:self.sequences,
    next_sequence:self.next_sequence,head:self.head,count:self.count
  };
  let mut inserted=0usize;
  for &(pid,generation) in owners{
   if staged.enqueue_exit_once(pid,generation)?{inserted+=1;}
  }
  *self=staged;
  Ok(inserted)
 }
}
#[cfg(test)]
mod r119_atomic_exit_batch_tests{
 use super::*;
 fn e(pid:u32,g:u64)->ProcessEvent{
  ProcessEvent{pid,kind:EventKind::Exited,value:g}
 }
 #[test]fn duplicate_and_prior_exit_are_idempotent(){
  let mut q=ProcessEvents::<4>::new();q.enqueue_exit_once(1,1).unwrap();
  let saved=q.peek_receipt().unwrap();
  assert_eq!(q.enqueue_exit_batch_once(&[(1,1),(2,2),(2,2)]),Ok(1));
  assert_eq!(q.len(),2);
  assert_eq!(q.acknowledge_receipt(saved),Some(e(1,1)));
  assert_eq!(q.dequeue(),Ok(e(2,2)));
 }
 #[test]fn invalid_or_full_late_member_never_adds_prefix(){
  let mut q=ProcessEvents::<2>::new();
  assert_eq!(q.enqueue_exit_batch_once(&[(1,1),(0,2)]),Err(EventError::InvalidPid));
  assert_eq!(q.len(),0);
  q.enqueue_exit_once(4,4).unwrap();
  let original=q.peek_receipt().unwrap();
  assert_eq!(q.enqueue_exit_batch_once(&[(1,1),(2,2)]),Err(EventError::Full));
  assert_eq!(q.peek_receipt(),Some(original));
  assert_eq!(q.len(),1);
 }
 #[test]fn sequence_exhaustion_and_zero_capacity_are_safe(){
  let mut q=ProcessEvents::<3>::new();
  q.enqueue_exit_once(3,3).unwrap();
  let original=q.peek_receipt().unwrap();
  q.next_sequence=u64::MAX-1;
  assert_eq!(q.enqueue_exit_batch_once(&[(4,4),(5,5)]),Err(EventError::Full));
  assert_eq!(q.peek_receipt(),Some(original));
  assert_eq!(q.len(),1);
  let mut zero=ProcessEvents::<0>::new();
  assert_eq!(zero.enqueue_exit_batch_once(&[]),Ok(0));
  assert_eq!(zero.enqueue_exit_batch_once(&[(1,1)]),Err(EventError::Full));
 }
 #[test]fn distinct_generations_keep_independent_receipts(){
  let mut q=ProcessEvents::<4>::new();
  assert_eq!(q.enqueue_exit_batch_once(&[(2,8),(2,9),(3,1)]),Ok(3));
  let old=q.peek_receipt().unwrap();
  assert_eq!(q.acknowledge_receipt(old),Some(e(2,8)));
  let fresh=q.peek_receipt().unwrap();
  assert_ne!(old.sequence,fresh.sequence);
  assert_eq!(q.acknowledge_receipt(old),None);
  assert_eq!(q.acknowledge_receipt(fresh),Some(e(2,9)));
  assert_eq!(q.dequeue(),Ok(e(3,1)));
 }
}

/// R120: exact process-incarnation fault receipt, separate from normal Exited.
/// The caller must authenticate the owner against the kernel scheduler.
impl<const N: usize> ProcessEvents<N> {
    pub fn enqueue_fault_once(&mut self, pid: u32, generation: u64)
        -> Result<bool, EventError>
    {
        if pid == 0 || generation == 0 {
            return Err(EventError::InvalidPid);
        }
        let duplicate = (0..self.count).any(|index| {
            self.entries[(self.head + index) % N].is_some_and(|event|
                event.pid == pid && event.kind == EventKind::Faulted &&
                event.value == generation)
        });
        if duplicate { return Ok(false); }
        self.enqueue(ProcessEvent { pid, kind: EventKind::Faulted, value: generation })?;
        Ok(true)
    }
}
#[cfg(test)]
mod r120_fault_receipt_tests {
    use super::*;
    #[test]
    fn fault_receipts_are_idempotent_and_distinguish_process_generations() {
        let mut events = ProcessEvents::<3>::new();
        assert_eq!(events.enqueue_fault_once(7, 1), Ok(true));
        assert_eq!(events.enqueue_fault_once(7, 1), Ok(false));
        assert_eq!(events.enqueue_fault_once(7, 2), Ok(true));
        assert_eq!(events.len(), 2);
        let old = events.peek_receipt().unwrap();
        assert_eq!(old.event.kind, EventKind::Faulted);
        assert_eq!(events.acknowledge_receipt(old), Some(old.event));
        assert_eq!(events.acknowledge_receipt(old), None);
        let new_receipt = events.peek_receipt().unwrap();
        assert_ne!(old.sequence, new_receipt.sequence);
        assert_eq!(new_receipt.event.value, 2);
    }
    #[test]
    fn zero_capacity_and_bad_fault_identities_never_publish() {
        let mut events = ProcessEvents::<0>::new();
        assert_eq!(events.enqueue_fault_once(0, 1), Err(EventError::InvalidPid));
        assert_eq!(events.enqueue_fault_once(1, 0), Err(EventError::InvalidPid));
        assert_eq!(events.enqueue_fault_once(1, 1), Err(EventError::Full));
        assert_eq!(events.len(), 0);
    }
}

#[cfg(test)]
mod m1_ws14_receipt_snapshot_tests {
 use super::*;
 #[test] fn snapshot_preserves_fifo_identity_after_wrap_and_capacity_failure() {
  let mut q=ProcessEvents::<2>::new();
  let e=|pid|ProcessEvent{pid,kind:EventKind::Message,value:pid as u64};
  q.enqueue(e(1)).unwrap();q.enqueue(e(2)).unwrap();
  let first=q.peek_receipt().unwrap();
  q.dequeue().unwrap();q.enqueue(e(3)).unwrap();
  let sentinel=EventReceipt{sequence:0,event:e(99)};
  let mut short=[sentinel;1];
  assert_eq!(q.snapshot_receipts(&mut short),Err(EventError::Full));
  assert_eq!(short,[sentinel]);
  let mut all=[sentinel;2];
  assert_eq!(q.snapshot_receipts(&mut all),Ok(2));
  assert_eq!((all[0].event.pid,all[1].event.pid),(2,3));
  assert!(all[0].sequence>first.sequence);
  assert_eq!(q.acknowledge_receipt(first),None);
  assert_eq!(q.acknowledge_receipt(all[0]),Some(e(2)));
  assert_eq!(q.peek_receipt(),Some(all[1]));
  let none=ProcessEvents::<0>::new();
  assert_eq!(none.snapshot_receipts(&mut []),Ok(0));
 }
}


// PR782 WS59-61: process-scoped FIFO observation and exact consumption.
// The scheduler must supply an authenticated PID: this is not a guest API.
impl<const CAPACITY:usize> ProcessEvents<CAPACITY>{
    /// WS59: Read only a given process's receipts, preserving order and
    /// destination on short capacity or a malformed kernel ring.
    pub fn snapshot_pid_receipts(&self,pid:u32,out:&mut[EventReceipt])
        ->Result<usize,EventError>{
        if pid==0{return Err(EventError::InvalidPid);}
        let mut count=0;
        for i in 0..self.count{
            let at=(self.head+i)%CAPACITY;
            let event=self.entries[at].ok_or(EventError::Empty)?;
            if self.sequences[at]==0{return Err(EventError::Empty);}
            if event.pid==pid{count+=1;}
        }
        if count>out.len(){return Err(EventError::Full);}
        let mut index=0;
        for i in 0..self.count{
            let at=(self.head+i)%CAPACITY;
            if let Some(event)=self.entries[at]{
                if event.pid==pid{
                    out[index]=EventReceipt{sequence:self.sequences[at],event};
                    index+=1;
                }
            }
        }
        Ok(index)
    }
    /// WS60: Require both an authenticated owner and exact, single-use head
    /// receipt before removing an event. A peer's matching payload is not
    /// sufficient to drain a FIFO item.
    pub fn acknowledge_pid_head(&mut self,pid:u32,receipt:EventReceipt)
        ->Result<ProcessEvent,EventError>{
        if pid==0{return Err(EventError::InvalidPid);}
        if receipt.event.pid!=pid||self.peek_receipt()!=Some(receipt){
            return Err(EventError::Empty);
        }
        self.dequeue()
    }
    /// WS61: Bounded cursor paging never consumes events. An empty page means
    /// no retained matching events after cursor; external callers should not
    /// infer that earlier ring data is still available after eviction.
    pub fn page_pid_after(&self,pid:u32,after:u64,
        out:&mut[EventReceipt])->Result<(usize,Option<u64>),EventError>{
        if pid==0{return Err(EventError::InvalidPid);}
        if out.is_empty(){return Err(EventError::Full);}
        // Complete validation before exposing any paged data.
        for i in 0..self.count{
            let at=(self.head+i)%CAPACITY;
            if self.entries[at].is_none()||self.sequences[at]==0{
                return Err(EventError::Empty);
            }
        }
        let mut written=0;let mut last=None;
        for i in 0..self.count{
            let at=(self.head+i)%CAPACITY;
            if let Some(event)=self.entries[at]{
                let seq=self.sequences[at];
                if event.pid==pid&&seq>after{
                    out[written]=EventReceipt{sequence:seq,event};
                    last=Some(seq);written+=1;
                    if written==out.len(){break;}
                }
            }
        }
        Ok((written,last))
    }
}
#[cfg(test)]
mod pr782_ws59_61_tests{
    use super::*;
    fn e(pid:u32,value:u64)->ProcessEvent{
        ProcessEvent{pid,kind:EventKind::Message,value}
    }
    fn empty()->EventReceipt{
        EventReceipt{sequence:0,event:e(99,999)}
    }
    #[test]fn ws59_owner_snapshot_is_atomic_and_fifo_ordered(){
        let mut q=ProcessEvents::<3>::new();
        q.enqueue(e(1,10)).unwrap();q.enqueue(e(2,20)).unwrap();
        q.enqueue(e(1,30)).unwrap();
        let mut small=[empty();1];
        assert_eq!(q.snapshot_pid_receipts(1,&mut small),Err(EventError::Full));
        assert_eq!(small,[empty();1]);
        let mut out=[empty();2];
        assert_eq!(q.snapshot_pid_receipts(1,&mut out),Ok(2));
        assert_eq!([out[0].event.value,out[1].event.value],[10,30]);
        assert!(out[0].sequence<out[1].sequence);
    }
    #[test]fn ws60_peer_or_replayed_receipt_cannot_consume_head(){
        let mut q=ProcessEvents::<2>::new();q.enqueue(e(1,10)).unwrap();
        let receipt=q.peek_receipt().unwrap();
        assert_eq!(q.acknowledge_pid_head(2,receipt),Err(EventError::Empty));
        assert_eq!(q.len(),1);
        assert_eq!(q.acknowledge_pid_head(1,receipt),Ok(e(1,10)));
        q.enqueue(e(1,10)).unwrap();
        assert_eq!(q.acknowledge_pid_head(1,receipt),Err(EventError::Empty));
        assert_eq!(q.len(),1);
    }
    #[test]fn ws61_pages_skip_peer_events_with_stable_sequence_cursor(){
        let mut q=ProcessEvents::<4>::new();
        for (p,v) in [(1,1),(2,2),(1,3),(1,4)]{
            q.enqueue(e(p,v)).unwrap();
        }
        let mut out=[empty();1];
        let (n,last)=q.page_pid_after(1,0,&mut out).unwrap();
        assert_eq!((n,out[0].event.value),(1,1));
        let (_,last2)=q.page_pid_after(1,last.unwrap(),&mut out).unwrap();
        assert_eq!(out[0].event.value,3);
        let (_,last3)=q.page_pid_after(1,last2.unwrap(),&mut out).unwrap();
        assert_eq!(out[0].event.value,4);
        assert_eq!(q.page_pid_after(1,last3.unwrap(),&mut out),Ok((0,None)));
        assert_eq!(q.len(),4);
    }
}

/// A non-starving scheduler selection primitive: prefer authenticated
/// priority-threshold work when it exists, but fall back to ordinary round
/// robin rather than leaving a runnable low-priority task behind forever.
/// Policy-only until wired to actual per-CPU timer dispatch.
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn next_with_priority_fallback(&mut self,cpu:usize,minimum:u8)
    ->Result<(Runnable,bool),DispatchError>{
  match self.next_eligible(cpu,minimum){
   Ok(task)=>Ok((task,false)),
   Err(DispatchError::Empty)=>self.next(cpu).map(|task|(task,true)),
   Err(error)=>Err(error),
  }
 }
}
#[cfg(test)]
mod m1_gate20_w5_next_dispatch_tests{
 use super::*;
 #[test]fn priority_fallback_can_run_low_priority_peer_without_starvation(){
  let mut q=CoreRunQueues::<1,2>::new();
  let low=Runnable{pid:5,generation:1,priority:1};
  let high=Runnable{pid:6,generation:1,priority:9};
  q.enqueue(0,low).unwrap();
  assert_eq!(q.next_with_priority_fallback(0,8),Ok((low,true)));
  q.enqueue(0,high).unwrap();
  assert_eq!(q.next_with_priority_fallback(0,8),Ok((high,false)));
  assert_eq!(q.next_with_priority_fallback(3,8),Err(DispatchError::InvalidCpu));
 }
}

impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 /// Bounded protected-PID OOM candidate selection, without killing tasks.
 pub fn lowest_priority_victim(&self,protected_pid:u32)
  ->Option<(usize,Runnable)>{
  let mut chosen:Option<(usize,Runnable)>=None;
  for cpu in 0..C{
   for &task in self.queues[cpu][..self.length[cpu]].iter().flatten(){
    if task.pid==0||task.generation==0||task.pid==protected_pid{continue;}
    if chosen.is_none_or(|(_,prior)|task.priority<prior.priority){
     chosen=Some((cpu,task));
    }
   }
  }
  chosen
 }
 /// Exact authenticated incarnation only; no physical frame reclaim here.
 pub fn evict_pressure_victim_exact(&mut self,cpu:usize,pid:u32,epoch:u64)
  ->Result<Runnable,DispatchError>{
  if cpu>=C{return Err(DispatchError::InvalidCpu);}
  let chosen=self.queues[cpu][..self.length[cpu]].iter().flatten()
   .find(|task|task.pid==pid&&task.generation==epoch)
   .copied().ok_or(DispatchError::InvalidTask)?;
  if self.evict(pid,epoch)!=1{return Err(DispatchError::InvalidTask);}
  Ok(chosen)
 }
}
#[cfg(test)]mod m1_batch03_w4_cpu_tests{
 use super::*;
 #[test]fn memory_pressure_protects_pid1_and_generation(){
  let mut q=CoreRunQueues::<2,2>::new();
  let protected=Runnable{pid:1,generation:1,priority:0};
  let low=Runnable{pid:2,generation:3,priority:1};
  q.enqueue(0,protected).unwrap();q.enqueue(0,low).unwrap();
  q.enqueue(1,Runnable{pid:3,generation:2,priority:9}).unwrap();
  assert_eq!(q.lowest_priority_victim(1),Some((0,low)));
  assert_eq!(q.evict_pressure_victim_exact(0,2,2),Err(DispatchError::InvalidTask));
  assert_eq!(q.evict_pressure_victim_exact(0,2,3),Ok(low));
  assert!(q.contains_epoch(1,1));
 }
 #[test]fn pressure_denies_unowned_cpu(){
  let mut q=CoreRunQueues::<1,1>::new();
  let r=Runnable{pid:5,generation:8,priority:1};q.enqueue(0,r).unwrap();
  assert_eq!(q.evict_pressure_victim_exact(1,5,8),Err(DispatchError::InvalidCpu));
  assert_eq!(q.next(0),Ok(r));
 }
}


/// Task Manager: snapshot only from scheduler-authenticated PID1 epoch.
/// It does NOT expose task control or a kill syscall to an untrusted GUI.
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn snapshot_pid1_task_manager(&self,authenticated:Runnable,
  out:&mut[(usize,Runnable)])->Result<usize,DispatchError>{
  if authenticated.pid!=1||authenticated.generation==0||
     !self.contains_epoch(1,authenticated.generation){
   return Err(DispatchError::InvalidTask);
  }
  self.snapshot_runnable(out)
 }
}
#[cfg(test)]
mod m1_batch05_w1_task_manager_tests{
 use super::*;
 #[test]fn only_current_pid1_may_snapshot_bounded_task_inventory(){
  let mut s=CoreRunQueues::<2,2>::new();
  let pid1=Runnable{pid:1,generation:10,priority:0};
  let peer=Runnable{pid:2,generation:4,priority:1};
  s.enqueue(0,pid1).unwrap();s.enqueue(1,peer).unwrap();
  let sentinel=(7,Runnable{pid:9,generation:9,priority:9});
  let mut output=[sentinel;2];
  assert_eq!(s.snapshot_pid1_task_manager(Runnable{generation:9,..pid1},
    &mut output),Err(DispatchError::InvalidTask));
  assert_eq!(output,[sentinel;2]);
  assert_eq!(s.snapshot_pid1_task_manager(pid1,&mut output[..1]),
    Err(DispatchError::Full));
  assert_eq!(output,[sentinel;2]);
  assert_eq!(s.snapshot_pid1_task_manager(pid1,&mut output),Ok(2));
  assert_eq!(output,[(0,pid1),(1,peer)]);
 }
}


// P0 kernel workstream W1: exact AP acknowledgement gates a real, atomic
// scheduler runqueue eviction. These are the existing software runqueues,
// not a substitute for physical IPI delivery or MMU TLB invalidation.
#[derive(Debug,Clone,Copy,Eq,PartialEq)]
pub enum ReclaimError{
 InvalidOwner,
 Ap(crate::smp_lifecycle_fence::FenceError),
 Queue(DispatchError),
}
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn retire_after_ap_reclaim_barrier(&mut self,supervisor:Runnable,
  victims:&[(u32,u64)],
  acknowledgements:&[(&crate::smp_lifecycle_fence::LifecycleFence,
                      u32,crate::smp_lifecycle_fence::Receipt)])
  ->Result<usize,ReclaimError>{
  if supervisor.pid!=1||supervisor.generation==0||
     !self.contains_epoch(supervisor.pid,supervisor.generation)||
     victims.is_empty()||acknowledgements.is_empty()||
     acknowledgements.len()>64||
     victims.iter().any(|&(pid,epoch)|pid==1||pid==0||epoch==0){
    return Err(ReclaimError::InvalidOwner);
  }
  // One AP receipt must not masquerade as several independent CPUs.
  for (i,&(fence,_,_)) in acknowledgements.iter().enumerate(){
   if acknowledgements[..i].iter().any(|&(prior,_,_)|
     core::ptr::eq(prior,fence)){return Err(ReclaimError::InvalidOwner);}
  }
  crate::smp_lifecycle_fence::require_all_acks_for_reclaim(acknowledgements)
   .map_err(ReclaimError::Ap)?;
  // Build a private scheduler snapshot. Invalid later process generations
  // leave the real queues, cursors, and all other tasks unchanged.
  let mut staged=Self{queues:self.queues,length:self.length,cursor:self.cursor};
  let removed=staged.evict_epochs_atomic(victims).map_err(ReclaimError::Queue)?;
  // Recheck before publishing after fallible staging. The caller must
  // serialize the actual scheduler and AP publication under SMP locks.
  crate::smp_lifecycle_fence::require_all_acks_for_reclaim(acknowledgements)
   .map_err(ReclaimError::Ap)?;
  *self=staged;
  Ok(removed)
 }
}

/// P0 W2: starvation-bounded priority quantum for the existing per-core
/// software runqueues. Timer-ISR and preemptive context-switch wiring is
/// separately required for a genuinely preemptive SMP scheduler.
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct FairQuantum{priority_streak:u8,maximum_streak:u8}
impl FairQuantum{
 pub const fn new(maximum_streak:u8)->Self{
  Self{priority_streak:0,maximum_streak}
 }
 pub fn high_streak(&self)->u8{self.priority_streak}
 pub fn reset(&mut self){self.priority_streak=0;}
}
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn select_fair_quantum(&mut self,cpu:usize,high_threshold:u8,
  quantum:&mut FairQuantum)->Result<Runnable,DispatchError>{
  if cpu>=C{return Err(DispatchError::InvalidCpu);}
  if quantum.maximum_streak==0{return Err(DispatchError::InvalidTask);}
  let count=self.length[cpu];
  if count==0{return Err(DispatchError::Empty);}
  if quantum.priority_streak>=quantum.maximum_streak{
   // Prefer the oldest next low-priority slot in this CPU's circular order.
   for offset in 0..count{
    let index=(self.cursor[cpu]+offset)%count;
    if let Some(task)=self.queues[cpu][index]{
     if task.priority<high_threshold{
      self.cursor[cpu]=(index+1)%count;
      quantum.priority_streak=0;
      return Ok(task);
     }
    }
   }
  }
  match self.next_eligible(cpu,high_threshold){
   Ok(task)=>{
    quantum.priority_streak=quantum.priority_streak.saturating_add(1);
    Ok(task)
   },
   Err(DispatchError::Empty)=>{
    let task=self.next(cpu)?;
    quantum.priority_streak=0;
    Ok(task)
   },
   Err(err)=>Err(err),
  }
 }
}
#[cfg(test)]
mod kernel_p0_w1_smp_reclaim_tests{
 use super::*;
 use crate::smp_lifecycle_fence::{LifecycleFence,FenceError};
 fn t(pid:u32,generation:u64)->Runnable{
  Runnable{pid,generation,priority:3}
 }
 #[test]fn two_ap_acknowledgements_are_required_before_exact_process_reclaim(){
  let mut queues=CoreRunQueues::<2,3>::new();
  queues.enqueue(0,t(1,4)).unwrap();
  queues.enqueue(1,t(2,7)).unwrap();
  let first=LifecycleFence::new(0,1);
  let second=LifecycleFence::new(2,3);
  let a=first.publish(0,10).unwrap();
  let b=second.publish(2,10).unwrap();
  first.observe_and_ack_after(1,0,|_|true).unwrap();
  let receipts=[(&first,0,a),(&second,2,b)];
  assert_eq!(queues.retire_after_ap_reclaim_barrier(t(1,4),&[(2,7)],&receipts),
   Err(ReclaimError::Ap(FenceError::NoProgress)));
  assert!(queues.contains_epoch(2,7));
  second.observe_and_ack_after(3,0,|_|true).unwrap();
  assert_eq!(queues.retire_after_ap_reclaim_barrier(t(1,4),&[(2,8)],&receipts),
   Err(ReclaimError::Queue(DispatchError::InvalidTask)));
  assert!(queues.contains_epoch(2,7));
  assert_eq!(queues.retire_after_ap_reclaim_barrier(t(1,4),&[(2,7)],&receipts),Ok(1));
  assert!(queues.contains_epoch(1,4));assert!(!queues.contains_epoch(2,7));
 }
 #[test]fn no_reclaim_of_pid1_or_repeated_ap_receipts(){
  let mut q=CoreRunQueues::<1,2>::new();
  q.enqueue(0,t(1,3)).unwrap();q.enqueue(0,t(2,4)).unwrap();
  let fence=LifecycleFence::new(0,1);
  let token=fence.publish(0,4).unwrap();
  fence.observe_and_ack_after(1,0,|_|true).unwrap();
  let once=[(&fence,0,token)];
  assert_eq!(q.retire_after_ap_reclaim_barrier(t(1,3),&[(1,3)],&once),
    Err(ReclaimError::InvalidOwner));
  assert_eq!(q.retire_after_ap_reclaim_barrier(t(1,3),&[(2,4)],&[once[0],once[0]]),
    Err(ReclaimError::InvalidOwner));
  assert_eq!(q.total_runnable(),2);
 }
}
#[cfg(test)]
mod kernel_p0_w2_scheduler_tests{
 use super::*;
 #[test]fn low_priority_makes_progress_with_continuous_high_priority_work(){
  let mut q=CoreRunQueues::<1,3>::new();
  let high=Runnable{pid:2,generation:2,priority:10};
  let low=Runnable{pid:3,generation:2,priority:1};
  q.enqueue(0,high).unwrap();q.enqueue(0,low).unwrap();
  let mut budget=FairQuantum::new(2);
  assert_eq!(q.select_fair_quantum(0,5,&mut budget),Ok(high));
  assert_eq!(q.select_fair_quantum(0,5,&mut budget),Ok(high));
  assert_eq!(q.select_fair_quantum(0,5,&mut budget),Ok(low));
  assert_eq!(budget.high_streak(),0);
  assert_eq!(q.select_fair_quantum(99,5,&mut budget),
     Err(DispatchError::InvalidCpu));
  assert_eq!(q.total_runnable(),2);
 }
 #[test]fn invalid_quantum_does_not_change_cursor_or_process_state(){
  let mut q=CoreRunQueues::<1,2>::new();
  let a=Runnable{pid:2,generation:4,priority:1};
  let b=Runnable{pid:3,generation:4,priority:9};
  q.enqueue(0,a).unwrap();q.enqueue(0,b).unwrap();
  let mut zero=FairQuantum::new(0);
  assert_eq!(q.select_fair_quantum(0,5,&mut zero),Err(DispatchError::InvalidTask));
  assert_eq!(q.next(0),Ok(a));
  assert_eq!(q.total_runnable(),2);
 }
}


/// P0 follow-up W1: exact process-identity and affinity-checked atomic
/// multi-core migration. The trusted caller still owns actual scheduler
/// locks, IPI, TLB shootdown and CPU-local saved contexts.
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn migrate_group_exact(&mut self,
  requests:&[(usize,usize,Runnable,u64)])->Result<usize,DispatchError>{
  if requests.is_empty()||requests.len()>64||
     requests.len()>C.saturating_mul(N){return Err(DispatchError::InvalidTask);}
  let mut staged=Self{queues:self.queues,length:self.length,cursor:self.cursor};
  for (i,&(from,to,task,allowed)) in requests.iter().enumerate(){
   if from>=C||to>=C{return Err(DispatchError::InvalidCpu);}
   if from==to||to>=64||task.pid<=1||task.generation==0||
      allowed&(1u64<<to)==0||
      requests[..i].iter().any(|&(_,_,other,_)|
        other.pid==task.pid&&other.generation==task.generation){
    return Err(DispatchError::InvalidTask);
   }
   if !staged.queues[from][..staged.length[from]]
       .iter().any(|slot|slot.is_some_and(|known|known==task)){
    return Err(DispatchError::InvalidTask);
   }
   staged.migrate(from,to,task.pid,task.generation)?;
  }
  staged.validate_queues()?;
  *self=staged;Ok(requests.len())
 }
}
#[cfg(test)]
mod kernel_p0_next_w1_migration_tests{
 use super::*;
 fn t(pid:u32,epoch:u64)->Runnable{Runnable{pid,generation:epoch,priority:2}}
 #[test]fn four_cross_cpu_migrations_are_atomic_across_late_capacity_fault(){
  let mut q=CoreRunQueues::<2,3>::new();
  q.enqueue(0,t(2,1)).unwrap();q.enqueue(0,t(3,1)).unwrap();
  q.enqueue(1,t(4,1)).unwrap();q.enqueue(1,t(5,1)).unwrap();
  let before=q.total_runnable();
  let bad=[(0,1,t(2,1),2),(0,1,t(3,1),2)];
  assert_eq!(q.migrate_group_exact(&bad),Err(DispatchError::Full));
  assert_eq!(q.total_runnable(),before);
  assert!(q.contains_epoch(2,1));assert!(q.contains_epoch(3,1));
  let good=[(0,1,t(2,1),2),(1,0,t(4,1),1)];
  assert_eq!(q.migrate_group_exact(&good),Ok(2));
  let mut first=[t(99,99);3];let mut second=[t(99,99);3];
  assert_eq!(q.snapshot_core(0,&mut first),Ok(2));
  assert_eq!(&first[..2],&[t(3,1),t(4,1)]);
  assert_eq!(q.snapshot_core(1,&mut second),Ok(2));
  assert_eq!(&second[..2],&[t(5,1),t(2,1)]);
 }
 #[test]fn pid1_recycling_affinity_and_stale_generation_all_fail_closed(){
  let mut q=CoreRunQueues::<2,2>::new();
  q.enqueue(0,t(1,4)).unwrap();q.enqueue(0,t(2,4)).unwrap();
  assert_eq!(q.migrate_group_exact(&[(0,1,t(1,4),2)]),
    Err(DispatchError::InvalidTask));
  assert_eq!(q.migrate_group_exact(&[(0,1,t(2,3),2)]),
    Err(DispatchError::InvalidTask));
  assert_eq!(q.migrate_group_exact(&[(0,1,t(2,4),1)]),
    Err(DispatchError::InvalidTask));
  assert_eq!(q.migrate_group_exact(&[(0,1,t(2,4),2),(0,1,t(2,4),2)]),
    Err(DispatchError::InvalidTask));
  assert_eq!(q.queue_len(0),Ok(2));
  assert_eq!(q.queue_len(1),Ok(0));
 }
}


/// P0 batch three W2: atomic runqueue + exact-once process-exit publication.
/// The privileged scheduler must hold the same exclusive retirement lock
/// while staging, publishing, and revoking the corresponding PCB/handles.
/// Hardware IPIs/TLB acknowledgement are checked by the prior gate.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ExitPublicationError{
 Reclaim(ReclaimError),
 Notice(EventError),
 DuplicateNotice,
}
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn retire_and_notify_exact<const E:usize>(&mut self,
  supervisor:Runnable,victims:&[(u32,u64)],
  receipts:&[(&crate::smp_lifecycle_fence::LifecycleFence,
             u32,crate::smp_lifecycle_fence::Receipt)],
  notices:&mut ProcessEvents<E>)
  ->Result<usize,ExitPublicationError>{
  let mut scheduled=Self{queues:self.queues,length:self.length,cursor:self.cursor};
  let mut staged_notices=notices.clone();
  let removed=scheduled.retire_after_ap_reclaim_barrier(
    supervisor,victims,receipts).map_err(ExitPublicationError::Reclaim)?;
  let count=staged_notices.enqueue_exit_batch_once(victims)
     .map_err(ExitPublicationError::Notice)?;
  if count!=victims.len(){return Err(ExitPublicationError::DuplicateNotice);}
  crate::smp_lifecycle_fence::require_all_acks_for_reclaim(receipts)
    .map_err(|error|ExitPublicationError::Reclaim(ReclaimError::Ap(error)))?;
  *self=scheduled;
  *notices=staged_notices;
  Ok(removed)
 }
}
#[cfg(test)]
mod kernel_p0_batch3_w2_exit_publication_tests{
 use super::*;
 use crate::smp_lifecycle_fence::{LifecycleFence,FenceError};
 fn t(pid:u32,epoch:u64)->Runnable{Runnable{pid,generation:epoch,priority:5}}
 #[test]fn full_exit_queue_rolls_back_actual_runqueue_retirement(){
  let mut q=CoreRunQueues::<1,3>::new();
  q.enqueue(0,t(1,4)).unwrap();q.enqueue(0,t(2,6)).unwrap();
  let mut events=ProcessEvents::<1>::new();
  events.enqueue(ProcessEvent{pid:9,kind:EventKind::Message,value:99}).unwrap();
  let ap=LifecycleFence::new(0,1);let receipt=ap.publish(0,7).unwrap();
  ap.observe_and_ack_after(1,0,|_|true).unwrap();
  assert_eq!(q.retire_and_notify_exact(t(1,4),&[(2,6)],&[(&ap,0,receipt)],
    &mut events),Err(ExitPublicationError::Notice(EventError::Full)));
  assert!(q.contains_epoch(2,6));
  assert_eq!(events.count_pid(9),1);
 }
 #[test]fn missing_ap_ack_or_duplicate_exit_receipt_never_retires_process(){
  let mut q=CoreRunQueues::<1,3>::new();
  q.enqueue(0,t(1,4)).unwrap();q.enqueue(0,t(2,6)).unwrap();
  let mut events=ProcessEvents::<3>::new();
  let ap=LifecycleFence::new(0,1);let receipt=ap.publish(0,7).unwrap();
  let refs=[(&ap,0,receipt)];
  assert_eq!(q.retire_and_notify_exact(t(1,4),&[(2,6)],&refs,&mut events),
    Err(ExitPublicationError::Reclaim(ReclaimError::Ap(FenceError::NoProgress))));
  assert_eq!(events.count_pid(2),0);
  ap.observe_and_ack_after(1,0,|_|true).unwrap();
  events.enqueue_exit_once(2,6).unwrap();
  assert_eq!(q.retire_and_notify_exact(t(1,4),&[(2,6)],&refs,&mut events),
    Err(ExitPublicationError::DuplicateNotice));
  assert!(q.contains_epoch(2,6));
  assert_eq!(events.count_pid(2),1);
  events.clear_exact_exit(2,6);
  assert_eq!(q.retire_and_notify_exact(t(1,4),&[(2,6)],&refs,&mut events),Ok(1));
  assert!(!q.contains_epoch(2,6));assert!(q.contains_epoch(1,4));
  assert_eq!(events.count_pid(2),1);
 }
}

/// P0-65..68: process-eviction+exit-notification transaction requires a
/// distinct acknowledged AP proof before mutating either owned queue.
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn retire_and_notify_distinct_aps<const E:usize>(&mut self,
  supervisor:Runnable,victims:&[(u32,u64)],
  receipts:&[(&crate::smp_lifecycle_fence::LifecycleFence,u32,
   crate::smp_lifecycle_fence::Receipt)],notices:&mut ProcessEvents<E>)
  ->Result<usize,ExitPublicationError>{
  // Never retire two incarnations of one PID in a single transaction:
  // a recycled PID cannot inherit the first generation's exit authority.
  for (i,&(pid,_)) in victims.iter().enumerate(){
   if victims[..i].iter().any(|&(previous,_)|previous==pid){
    return Err(ExitPublicationError::Reclaim(ReclaimError::InvalidOwner));
   }
  }
  crate::smp_lifecycle_fence::require_distinct_acks_for_reclaim(receipts)
   .map_err(|e|ExitPublicationError::Reclaim(ReclaimError::Ap(e)))?;
  self.retire_and_notify_exact(supervisor,victims,receipts,notices)
 }
}
#[cfg(test)]
mod kernel_p0_batch4_w2_retirement_tests{
 use super::*;
 use crate::smp_lifecycle_fence::{LifecycleFence,FenceError};
 fn t(pid:u32)->Runnable{Runnable{pid,generation:7,priority:3}}
 #[test]fn duplicate_or_pending_ack_never_evicts_a_live_process(){
  let mut q=CoreRunQueues::<1,3>::new();
  q.enqueue(0,t(1)).unwrap();q.enqueue(0,t(2)).unwrap();
  let mut e=ProcessEvents::<2>::new();
  let a=LifecycleFence::new(0,1);let x=a.publish(0,8).unwrap();
  let same=[(&a,0,x),(&a,0,x)];
  assert_eq!(q.retire_and_notify_distinct_aps(t(1),&[(2,7)],&same,&mut e),
   Err(ExitPublicationError::Reclaim(ReclaimError::Ap(FenceError::NoProgress))));
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  assert_eq!(q.retire_and_notify_distinct_aps(t(1),&[(2,7)],&same,&mut e),
   Err(ExitPublicationError::Reclaim(ReclaimError::Ap(FenceError::StaleReceipt))));
  assert!(q.contains_epoch(2,7));assert_eq!(e.count_pid(2),0);
 }
 #[test]fn full_event_queue_rolls_back_and_success_notifies_once(){
  let mut q=CoreRunQueues::<1,3>::new();
  q.enqueue(0,t(1)).unwrap();q.enqueue(0,t(2)).unwrap();
  let mut e=ProcessEvents::<1>::new();
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(0,2);
  let x=a.publish(0,8).unwrap();let y=b.publish(0,9).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  b.observe_and_ack_after(2,0,|_|true).unwrap();
  e.enqueue(ProcessEvent{pid:9,kind:EventKind::Message,value:1}).unwrap();
  let proof=[(&a,0,x),(&b,0,y)];
  assert_eq!(q.retire_and_notify_distinct_aps(t(1),&[(2,7)],&proof,&mut e),
   Err(ExitPublicationError::Notice(EventError::Full)));
  assert!(q.contains_epoch(2,7));assert_eq!(e.count_pid(9),1);
  e.dequeue().unwrap();
  assert_eq!(q.retire_and_notify_distinct_aps(t(1),&[(2,7)],&proof,&mut e),Ok(1));
  assert!(!q.contains_epoch(2,7));assert_eq!(e.count_pid(2),1);
 }
}

#[cfg(test)]
mod kernel_p0_batch5_w2_generation_teardown_tests {
 use super::*;
 use crate::smp_lifecycle_fence::{LifecycleFence,FenceError};
 fn t(pid:u32,generation:u64)->Runnable{Runnable{pid,generation,priority:3}}
 #[test] fn mixed_incarnations_are_rejected_without_queue_or_notice_change(){
  let mut q=CoreRunQueues::<1,4>::new();
  for x in [t(1,7),t(2,8),t(2,9)]{q.enqueue(0,x).unwrap();}
  let mut notices=ProcessEvents::<3>::new();
  let ap=LifecycleFence::new(0,1);
  let proof=ap.publish(0,15).unwrap();
  ap.observe_and_ack_after(1,0,|_|true).unwrap();
  assert_eq!(q.retire_and_notify_distinct_aps(t(1,7),
   &[(2,8),(2,9)],&[(&ap,0,proof)],&mut notices),
   Err(ExitPublicationError::Reclaim(ReclaimError::InvalidOwner)));
  assert_eq!(q.total_runnable(),3);
  assert!(q.contains_epoch(2,8)&&q.contains_epoch(2,9));
  assert_eq!(notices.count_pid(2),0);
 }
 #[test] fn foreign_bsp_barrier_does_not_remove_live_victim(){
  let mut q=CoreRunQueues::<1,3>::new();
  q.enqueue(0,t(1,7)).unwrap();q.enqueue(0,t(2,8)).unwrap();
  let mut notices=ProcessEvents::<3>::new();
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(3,4);
  let x=a.publish(0,5).unwrap();let y=b.publish(3,6).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  b.observe_and_ack_after(4,0,|_|true).unwrap();
  assert_eq!(q.retire_and_notify_distinct_aps(t(1,7),
   &[(2,8)],&[(&a,0,x),(&b,3,y)],&mut notices),
   Err(ExitPublicationError::Reclaim(ReclaimError::Ap(FenceError::InvalidCpu))));
  assert!(q.contains_epoch(2,8));
  assert_eq!(notices.count_pid(2),0);
 }
}
/// P0-105..108: the scheduler may publish a group exit only if the
/// privileged topology inventory is covered by distinct AP receipts.
/// Performs all checks before the existing transactional queue/notice swap.
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn retire_and_notify_required_topology<const E:usize>(&mut self,
  supervisor:Runnable,victims:&[(u32,u64)],
  receipts:&[(&crate::smp_lifecycle_fence::LifecycleFence,
              u32,crate::smp_lifecycle_fence::Receipt)],
  expected_aps:&[u32],notices:&mut ProcessEvents<E>)
  ->Result<usize,ExitPublicationError>{
  crate::smp_lifecycle_fence::require_topology_acks_for_reclaim(
   receipts,expected_aps)
   .map_err(|e|ExitPublicationError::Reclaim(ReclaimError::Ap(e)))?;
  self.retire_and_notify_distinct_aps(supervisor,victims,receipts,notices)
 }
}
#[cfg(test)]
mod kernel_p0_batch6_w2_topology_exit_tests{
 use super::*;
 use crate::smp_lifecycle_fence::{FenceError,LifecycleFence};
 fn t(pid:u32,g:u64)->Runnable{Runnable{pid,generation:g,priority:4}}
 #[test] fn missing_topology_ack_keeps_exiting_process_and_events_intact(){
  let mut queues=CoreRunQueues::<1,4>::new();
  queues.enqueue(0,t(1,1)).unwrap();queues.enqueue(0,t(2,2)).unwrap();
  let mut events=ProcessEvents::<2>::new();
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(0,2);
  let ra=a.publish(0,8).unwrap();let rb=b.publish(0,9).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  let receipts=[(&a,0,ra),(&b,0,rb)];
  assert_eq!(queues.retire_and_notify_required_topology(t(1,1),
   &[(2,2)],&receipts,&[1,2],&mut events),
   Err(ExitPublicationError::Reclaim(ReclaimError::Ap(FenceError::NoProgress))));
  assert!(queues.contains_epoch(2,2));assert_eq!(events.len(),0);
  b.observe_and_ack_after(2,0,|_|true).unwrap();
  assert_eq!(queues.retire_and_notify_required_topology(t(1,1),
   &[(2,2)],&receipts,&[1,3],&mut events),
   Err(ExitPublicationError::Reclaim(ReclaimError::Ap(FenceError::StaleReceipt))));
  assert!(queues.contains_epoch(2,2));assert_eq!(events.len(),0);
  assert_eq!(queues.retire_and_notify_required_topology(t(1,1),
   &[(2,2)],&receipts,&[2,1],&mut events),Ok(1));
  assert!(!queues.contains_epoch(2,2));assert_eq!(events.count_pid(2),1);
 }
 #[test] fn capacity_failure_still_rolls_back_after_full_topology_proof(){
  let mut queues=CoreRunQueues::<1,4>::new();
  queues.enqueue(0,t(1,1)).unwrap();queues.enqueue(0,t(2,2)).unwrap();
  let mut notices=ProcessEvents::<1>::new();
  notices.enqueue(ProcessEvent{pid:8,kind:EventKind::Message,value:5}).unwrap();
  let a=LifecycleFence::new(0,1);let r=a.publish(0,3).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  assert_eq!(queues.retire_and_notify_required_topology(t(1,1),
   &[(2,2)],&[(&a,0,r)],&[1],&mut notices),
   Err(ExitPublicationError::Notice(EventError::Full)));
  assert!(queues.contains_epoch(2,2));
  assert_eq!(notices.count_pid(8),1);assert_eq!(notices.count_pid(2),0);
 }
}


/// P0-125..128: make the exact AP topology AND the release epoch part of
/// the process exit transaction's authority. Atomic runqueue/notice changes
/// are delegated to the existing staging transaction; a stale topology or
/// failed notice publication must not evict any incarnation.
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn retire_and_notify_epoch_topology<const E:usize>(&mut self,
  supervisor:Runnable,victims:&[(u32,u64)],
  receipts:&[(&crate::smp_lifecycle_fence::LifecycleFence,u32,
    crate::smp_lifecycle_fence::Receipt)],
  expected_aps:&[u32],expected_generation:u64,
  notices:&mut ProcessEvents<E>)->Result<usize,ExitPublicationError>{
  crate::smp_lifecycle_fence::require_epoch_topology_acks_for_reclaim(
   receipts,expected_aps,expected_generation)
   .map_err(|e|ExitPublicationError::Reclaim(ReclaimError::Ap(e)))?;
  self.retire_and_notify_required_topology(supervisor,victims,
   receipts,expected_aps,notices)
 }
}
#[cfg(test)]
mod kernel_p0_batch7_w2_exit_epoch_tests{
 use super::*;
 use crate::smp_lifecycle_fence::{FenceError,LifecycleFence};
 fn task(pid:u32,generation:u64)->Runnable{
  Runnable{pid,generation,priority:5}
 }
 #[test]fn mixing_valid_receipts_of_different_epochs_cannot_retire(){
  let mut q=CoreRunQueues::<1,4>::new();
  q.enqueue(0,task(1,8)).unwrap();q.enqueue(0,task(2,9)).unwrap();
  let mut events=ProcessEvents::<2>::new();
  let a=LifecycleFence::new(0,1);let b=LifecycleFence::new(0,2);
  let x=a.publish(0,20).unwrap();let y=b.publish(0,21).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  b.observe_and_ack_after(2,0,|_|true).unwrap();
  let refs=[(&a,0,x),(&b,0,y)];
  assert_eq!(q.retire_and_notify_epoch_topology(task(1,8),
   &[(2,9)],&refs,&[1,2],20,&mut events),
   Err(ExitPublicationError::Reclaim(ReclaimError::Ap(FenceError::StaleReceipt))));
  assert!(q.contains_epoch(2,9));assert_eq!(events.len(),0);
 }
 #[test]fn matching_epoch_still_obeys_atomic_event_capacity_and_pid1_guard(){
  let mut q=CoreRunQueues::<1,4>::new();
  q.enqueue(0,task(1,8)).unwrap();q.enqueue(0,task(2,9)).unwrap();
  let mut events=ProcessEvents::<1>::new();
  let a=LifecycleFence::new(0,1);let x=a.publish(0,20).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  let refs=[(&a,0,x)];
  events.enqueue(ProcessEvent{pid:7,kind:EventKind::Message,value:1}).unwrap();
  assert_eq!(q.retire_and_notify_epoch_topology(task(1,8),
   &[(2,9)],&refs,&[1],20,&mut events),
   Err(ExitPublicationError::Notice(EventError::Full)));
  assert!(q.contains_epoch(2,9));assert_eq!(events.count_pid(7),1);
  events.dequeue().unwrap();
  assert!(q.retire_and_notify_epoch_topology(task(1,8),
   &[(1,8)],&refs,&[1],20,&mut events).is_err());
  assert!(q.contains_epoch(1,8));
  assert_eq!(q.retire_and_notify_epoch_topology(task(1,8),
   &[(2,9)],&refs,&[1],20,&mut events),Ok(1));
  assert!(!q.contains_epoch(2,9));assert_eq!(events.count_pid(2),1);
 }
}


/// P0-145..148: a running process may migrate to another CPU only after
/// the entire trusted AP inventory acknowledged the same local lifecycle
/// epoch. Stage every scheduler edit privately and recheck the barrier before
/// publication. Actual runtime must hold scheduler/topology locks throughout.
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn migrate_group_after_epoch_acks(
  &mut self,requests:&[(usize,usize,Runnable,u64)],
  receipts:&[(&crate::smp_lifecycle_fence::LifecycleFence,u32,
   crate::smp_lifecycle_fence::Receipt)],
  expected_aps:&[u32],expected_generation:u64
 )->Result<usize,ReclaimError>{
  crate::smp_lifecycle_fence::require_epoch_topology_acks_for_reclaim(
   receipts,expected_aps,expected_generation).map_err(ReclaimError::Ap)?;
  let mut staged=Self{queues:self.queues,length:self.length,cursor:self.cursor};
  let count=staged.migrate_group_exact(requests).map_err(ReclaimError::Queue)?;
  crate::smp_lifecycle_fence::require_epoch_topology_acks_for_reclaim(
   receipts,expected_aps,expected_generation).map_err(ReclaimError::Ap)?;
  *self=staged;
  Ok(count)
 }
}
#[cfg(test)]
mod kernel_p0_batch8_w2_topology_migration_tests{
 use super::*;
 use crate::smp_lifecycle_fence::{FenceError,LifecycleFence};
 fn task(pid:u32,g:u64)->Runnable{Runnable{pid,generation:g,priority:2}}
 #[test]fn acknowledged_same_epoch_topology_permits_atomic_cross_core_migration(){
  let mut queues=CoreRunQueues::<2,3>::new();
  queues.enqueue(0,task(1,1)).unwrap();
  queues.enqueue(0,task(2,3)).unwrap();queues.enqueue(1,task(3,4)).unwrap();
  let a=LifecycleFence::new(9,1);let b=LifecycleFence::new(9,2);
  let x=a.publish(9,20).unwrap();let y=b.publish(9,20).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  b.observe_and_ack_after(2,0,|_|true).unwrap();
  let proofs=[(&a,9,x),(&b,9,y)];
  assert_eq!(queues.migrate_group_after_epoch_acks(
   &[(0,1,task(2,3),2),(1,0,task(3,4),1)],
   &proofs,&[2,1],20),Ok(2));
  let mut cpu0=[task(99,99);3];let mut cpu1=[task(99,99);3];
  assert_eq!(queues.snapshot_core(0,&mut cpu0),Ok(2));
  assert_eq!(&cpu0[..2],&[task(1,1),task(3,4)]);
  assert_eq!(queues.snapshot_core(1,&mut cpu1),Ok(1));
  assert_eq!(cpu1[0],task(2,3));
 }
 #[test]fn pending_mixed_epoch_or_invalid_affinity_never_changes_queue(){
  let mut queues=CoreRunQueues::<2,2>::new();
  queues.enqueue(0,task(1,1)).unwrap();queues.enqueue(0,task(2,3)).unwrap();
  let a=LifecycleFence::new(9,1);let b=LifecycleFence::new(9,2);
  let x=a.publish(9,20).unwrap();let y=b.publish(9,21).unwrap();
  a.observe_and_ack_after(1,0,|_|true).unwrap();
  let proofs=[(&a,9,x),(&b,9,y)];
  let moves=[(0,1,task(2,3),2)];
  assert_eq!(queues.migrate_group_after_epoch_acks(&moves,&proofs,&[1,2],20),
   Err(ReclaimError::Ap(FenceError::StaleReceipt)));
  assert_eq!(queues.queue_len(0),Ok(2));assert_eq!(queues.queue_len(1),Ok(0));
  let proof=[(&a,9,x)];
  assert_eq!(queues.migrate_group_after_epoch_acks(
   &[(0,1,task(2,3),1)],&proof,&[1],20),
   Err(ReclaimError::Queue(DispatchError::InvalidTask)));
  assert_eq!(queues.queue_len(0),Ok(2));assert_eq!(queues.queue_len(1),Ok(0));
 }
}


/// P0-165..168: one kernel-locked transaction for process scheduler exit,
/// exact-generation event publication and IPC mailbox/sent-message revocation.
/// A failing IPC generation or full event ring leaves ALL structures intact.
/// Real runtime must hold scheduler, IPC and topology locks in fixed order.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ExitReapError{
 Exit(ExitPublicationError),
 Ipc(crate::ipc::IpcError),
}
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn retire_notify_and_reap_ipc<const E:usize,const M:usize,const D:usize>(
  &mut self,supervisor:Runnable,victims:&[(u32,u64)],
  receipts:&[(&crate::smp_lifecycle_fence::LifecycleFence,u32,
   crate::smp_lifecycle_fence::Receipt)],
  expected_aps:&[u32],epoch:u64,
  events:&mut ProcessEvents<E>,router:&mut crate::ipc::ScopedRouter<M,D>
 )->Result<usize,ExitReapError>{
  if victims.is_empty()||victims.len()>M{
   return Err(ExitReapError::Ipc(crate::ipc::IpcError::InvalidConfiguration));
  }
  let mut ids=[crate::batch5_session::Identity{pid:0,generation:0};M];
  for (i,&(pid,generation)) in victims.iter().enumerate(){
   ids[i]=crate::batch5_session::Identity{pid,generation};
  }
  let mut staged=Self{queues:self.queues,length:self.length,cursor:self.cursor};
  let mut notices=events.clone();
  let mut mailboxes=router.clone();
  let n=staged.retire_and_notify_epoch_topology(
   supervisor,victims,receipts,expected_aps,epoch,&mut notices)
   .map_err(ExitReapError::Exit)?;
  mailboxes.reap_group_exact(&ids[..victims.len()])
   .map_err(ExitReapError::Ipc)?;
  crate::smp_lifecycle_fence::require_epoch_topology_acks_for_reclaim(
   receipts,expected_aps,epoch)
   .map_err(|e|ExitReapError::Exit(
    ExitPublicationError::Reclaim(ReclaimError::Ap(e))))?;
  *self=staged;
  *events=notices;
  *router=mailboxes;
  Ok(n)
 }
}
#[cfg(test)]
mod kernel_p0_batch9_w2_exit_ipc_atomic_tests{
 use super::*;
 use crate::batch5_session::Identity;
 use crate::ipc::{IpcError,ScopedRouter};
 use crate::smp_lifecycle_fence::LifecycleFence;
 fn t(pid:u32,g:u64)->Runnable{Runnable{pid,generation:g,priority:3}}
 fn id(pid:u32,g:u64)->Identity{Identity{pid,generation:g}}
 #[test]fn successful_exit_reaps_outgoing_ipc_and_publishes_one_notice(){
  let mut q=CoreRunQueues::<1,3>::new();
  q.enqueue(0,t(1,1)).unwrap();q.enqueue(0,t(2,4)).unwrap();
  let mut events=ProcessEvents::<2>::new();
  let mut ipc=ScopedRouter::<3,2>::new().unwrap();
  for x in [id(1,1),id(2,4),id(3,2)]{ipc.register(x).unwrap();}
  ipc.send(id(2,4),id(3,2),b"stale").unwrap();
  let f=LifecycleFence::new(0,7);let r=f.publish(0,10).unwrap();
  f.observe_and_ack_after(7,0,|_|true).unwrap();
  assert_eq!(q.retire_notify_and_reap_ipc(t(1,1),&[(2,4)],
   &[(&f,0,r)],&[7],10,&mut events,&mut ipc),Ok(1));
  assert!(!q.contains_epoch(2,4));
  assert_eq!(events.count_pid(2),1);
  assert_eq!(ipc.pending(id(2,4)),Err(IpcError::UnknownReceiver));
  assert_eq!(ipc.pending(id(3,2)),Ok(0));
 }
 #[test]fn stale_mailbox_or_full_event_ring_rolls_back_everything(){
  let mut q=CoreRunQueues::<1,3>::new();
  q.enqueue(0,t(1,1)).unwrap();q.enqueue(0,t(2,4)).unwrap();
  let mut events=ProcessEvents::<1>::new();
  let mut ipc=ScopedRouter::<2,2>::new().unwrap();
  ipc.register(id(1,1)).unwrap();ipc.register(id(2,5)).unwrap();
  let f=LifecycleFence::new(0,7);let r=f.publish(0,10).unwrap();
  f.observe_and_ack_after(7,0,|_|true).unwrap();
  assert_eq!(q.retire_notify_and_reap_ipc(t(1,1),&[(2,4)],
   &[(&f,0,r)],&[7],10,&mut events,&mut ipc),
   Err(ExitReapError::Ipc(IpcError::UnknownReceiver)));
  assert!(q.contains_epoch(2,4));assert_eq!(events.len(),0);
  assert_eq!(ipc.pending(id(2,5)),Ok(0));
  ipc.reap(id(2,5)).unwrap();ipc.register(id(2,4)).unwrap();
  events.enqueue(ProcessEvent{pid:8,kind:EventKind::Message,value:1}).unwrap();
  assert_eq!(q.retire_notify_and_reap_ipc(t(1,1),&[(2,4)],
   &[(&f,0,r)],&[7],10,&mut events,&mut ipc),
   Err(ExitReapError::Exit(ExitPublicationError::Notice(EventError::Full))));
  assert!(q.contains_epoch(2,4));assert_eq!(events.count_pid(8),1);
  assert_eq!(ipc.pending(id(2,4)),Ok(0));
 }
}


/// P0-185..188: prevent phantom process exits and PID-reuse confusion before
/// an all-or-none scheduler/event/IPC retirement. Every victim must have
/// exactly one runnable incarnation, no other generation of that PID can be
/// running, and PID1 is immutable. Caller serializes the three stores.
impl<const C:usize,const N:usize> CoreRunQueues<C,N>{
 pub fn retire_notify_and_reap_live_exact<
  const E:usize,const M:usize,const D:usize>(
  &mut self,supervisor:Runnable,victims:&[(u32,u64)],
  receipts:&[(&crate::smp_lifecycle_fence::LifecycleFence,u32,
   crate::smp_lifecycle_fence::Receipt)],
  expected_aps:&[u32],epoch:u64,
  events:&mut ProcessEvents<E>,router:&mut crate::ipc::ScopedRouter<M,D>
 )->Result<usize,ExitReapError>{
  let invalid=ExitReapError::Exit(
   ExitPublicationError::Reclaim(ReclaimError::InvalidOwner));
  if victims.is_empty()||victims.len()>M||
     supervisor.pid!=1||supervisor.generation==0||
     !self.contains_epoch(supervisor.pid,supervisor.generation){
   return Err(invalid);
  }
  for (index,&(pid,generation)) in victims.iter().enumerate(){
   if pid<=1||generation==0||
      victims[..index].iter().any(|&(old,_)|old==pid){
    return Err(invalid);
   }
   let mut exact=0usize;
   for task in self.queues.iter().flatten().flatten(){
    if task.pid==pid{
     if task.generation!=generation{return Err(invalid);}
     exact+=1;
    }
   }
   if exact!=1{return Err(invalid);}
  }
  self.retire_notify_and_reap_ipc(supervisor,victims,receipts,
   expected_aps,epoch,events,router)
 }
}
#[cfg(test)]
mod kernel_p0_batch10_w2_live_exit_tests{
 use super::*;
 use crate::batch5_session::Identity;
 use crate::ipc::ScopedRouter;
 use crate::smp_lifecycle_fence::LifecycleFence;
 fn task(pid:u32,generation:u64)->Runnable{Runnable{pid,generation,priority:2}}
 fn owner(pid:u32,generation:u64)->Identity{Identity{pid,generation}}
 #[test]fn phantom_or_recycled_victim_cannot_publish_exit_or_reap_messages(){
  let mut q=CoreRunQueues::<2,3>::new();
  q.enqueue(0,task(1,1)).unwrap();q.enqueue(1,task(2,8)).unwrap();
  let mut events=ProcessEvents::<3>::new();
  let mut ipc=ScopedRouter::<2,2>::new().unwrap();
  ipc.register(owner(1,1)).unwrap();ipc.register(owner(2,8)).unwrap();
  let f=LifecycleFence::new(0,7);let r=f.publish(0,9).unwrap();
  f.observe_and_ack_after(7,0,|_|true).unwrap();
  for victims in [&[(3,2)][..],&[(2,7)][..],&[(1,1)][..],
    &[(2,8),(2,8)][..]]{
   assert_eq!(q.retire_notify_and_reap_live_exact(
    task(1,1),victims,&[(&f,0,r)],&[7],9,&mut events,&mut ipc),
    Err(ExitReapError::Exit(ExitPublicationError::Reclaim(
     ReclaimError::InvalidOwner))));
   assert!(q.contains_epoch(2,8));assert_eq!(events.len(),0);
   assert_eq!(ipc.pending(owner(2,8)),Ok(0));
  }
 }
 #[test]fn one_real_incarnation_exits_all_three_stores(){
  let mut q=CoreRunQueues::<2,3>::new();
  q.enqueue(0,task(1,1)).unwrap();q.enqueue(1,task(2,8)).unwrap();
  let mut events=ProcessEvents::<3>::new();
  let mut ipc=ScopedRouter::<2,2>::new().unwrap();
  ipc.register(owner(1,1)).unwrap();ipc.register(owner(2,8)).unwrap();
  let f=LifecycleFence::new(0,7);let r=f.publish(0,9).unwrap();
  f.observe_and_ack_after(7,0,|_|true).unwrap();
  assert_eq!(q.retire_notify_and_reap_live_exact(task(1,1),&[(2,8)],
   &[(&f,0,r)],&[7],9,&mut events,&mut ipc),Ok(1));
  assert!(!q.contains_epoch(2,8));assert_eq!(events.count_pid(2),1);
  assert_eq!(ipc.pending(owner(2,8)),Err(crate::ipc::IpcError::UnknownReceiver));
 }
}


/// P0-285..287: a supervisor may acknowledge an exact ordered prefix of
/// event receipts atomically. A stale, duplicated or out-of-order receipt
/// cannot partially drain the lifecycle FIFO. Caller owns the event lock.
impl<const C:usize> ProcessEvents<C>{
 pub fn acknowledge_exact_prefix(&mut self,receipts:&[EventReceipt])
  ->Result<usize,EventError>{
  if receipts.is_empty()||receipts.len()>self.count{
   return Err(EventError::Empty);
  }
  let mut staged=self.clone();
  for &receipt in receipts{
   if receipt.sequence==0||staged.peek_receipt()!=Some(receipt){
    return Err(EventError::Empty);
   }
   staged.dequeue()?;
  }
  *self=staged;
  Ok(receipts.len())
 }
}
#[cfg(test)]
mod kernel_p0_batch14_w2_atomic_event_ack_tests{
 use super::*;
 fn event(pid:u32)->ProcessEvent{
  ProcessEvent{pid,kind:EventKind::Exited,value:1}
 }
 #[test]fn exact_receipt_prefix_is_single_use_and_fifo(){
  let mut q=ProcessEvents::<3>::new();
  q.enqueue(event(2)).unwrap();q.enqueue(event(2)).unwrap();
  q.enqueue(event(3)).unwrap();
  let mut receipts=[EventReceipt{sequence:0,event:event(1)};3];
  assert_eq!(q.snapshot_receipts(&mut receipts),Ok(3));
  assert_ne!(receipts[0].sequence,receipts[1].sequence);
  assert_eq!(q.acknowledge_exact_prefix(&receipts[..2]),Ok(2));
  assert_eq!(q.peek_receipt(),Some(receipts[2]));
  assert_eq!(q.acknowledge_exact_prefix(&receipts[..1]),Err(EventError::Empty));
  assert_eq!(q.acknowledge_exact_prefix(&receipts[2..3]),Ok(1));
 }
 #[test]fn later_stale_receipt_cannot_consume_valid_prefix(){
  let mut q=ProcessEvents::<3>::new();
  q.enqueue(event(2)).unwrap();q.enqueue(event(3)).unwrap();
  let mut receipts=[EventReceipt{sequence:0,event:event(1)};2];
  q.snapshot_receipts(&mut receipts).unwrap();
  let mut bad=receipts;
  bad[1].sequence+=10;
  assert_eq!(q.acknowledge_exact_prefix(&bad),Err(EventError::Empty));
  assert_eq!(q.len(),2);
  assert_eq!(q.peek_receipt(),Some(receipts[0]));
  assert_eq!(q.acknowledge_exact_prefix(&[receipts[0],receipts[0]]),
   Err(EventError::Empty));
  assert_eq!(q.len(),2);
  assert_eq!(q.acknowledge_exact_prefix(&[]),Err(EventError::Empty));
 }
}



/// P0-306..310: deliver the same exact receipt prefix into supervisor
/// memory atomically with dequeuing. On any replay, short buffer or
/// malformed prefix BOTH destination and lifecycle FIFO remain unchanged.
impl<const C:usize> ProcessEvents<C>{
 pub fn acknowledge_exact_prefix_into(
  &mut self,receipts:&[EventReceipt],out:&mut[ProcessEvent]
 )->Result<usize,EventError>{
  if receipts.is_empty(){return Err(EventError::Empty);}
  if out.len()<receipts.len(){return Err(EventError::Full);}
  let mut staged=self.clone();
  staged.acknowledge_exact_prefix(receipts)?;
  for (destination,receipt) in out.iter_mut().zip(receipts.iter()){
   *destination=receipt.event;
  }
  *self=staged;
  Ok(receipts.len())
 }
}
#[cfg(test)]
mod kernel_p0_batch15_w2_atomic_delivery_tests{
 use super::*;
 fn event(pid:u32)->ProcessEvent{
  ProcessEvent{pid,kind:EventKind::Exited,value:pid as u64}
 }
 fn receipt()->EventReceipt{EventReceipt{sequence:0,event:event(9)}}
 #[test]fn all_receipts_copy_and_dequeue_together(){
  let mut q=ProcessEvents::<3>::new();
  q.enqueue(event(2)).unwrap();q.enqueue(event(3)).unwrap();
  let mut receipts=[receipt();2];
  assert_eq!(q.snapshot_receipts(&mut receipts),Ok(2));
  let mut out=[event(9);2];
  assert_eq!(q.acknowledge_exact_prefix_into(&receipts,&mut out),Ok(2));
  assert_eq!(out,[event(2),event(3)]);
  assert_eq!(q.len(),0);
  assert_eq!(q.acknowledge_exact_prefix_into(&receipts,&mut out),
   Err(EventError::Empty));
 }
 #[test]fn late_stale_receipt_cannot_publish_even_first_event(){
  let mut q=ProcessEvents::<3>::new();
  q.enqueue(event(2)).unwrap();q.enqueue(event(3)).unwrap();
  let mut r=[receipt();2];q.snapshot_receipts(&mut r).unwrap();
  let mut wrong=r;wrong[1].sequence+=1;
  let mut out=[event(9);2];
  assert_eq!(q.acknowledge_exact_prefix_into(&wrong,&mut out),
   Err(EventError::Empty));
  assert_eq!(out,[event(9);2]);
  assert_eq!(q.len(),2);
  assert_eq!(q.peek_receipt(),Some(r[0]));
 }
 #[test]fn short_destination_and_empty_prefix_never_drain(){
  let mut q=ProcessEvents::<1>::new();q.enqueue(event(2)).unwrap();
  let mut r=[receipt();1];q.snapshot_receipts(&mut r).unwrap();
  let mut no_output:[ProcessEvent;0]=[];
  assert_eq!(q.acknowledge_exact_prefix_into(&r,&mut no_output),
   Err(EventError::Full));
  let mut output=[event(9);1];
  assert_eq!(q.acknowledge_exact_prefix_into(&[],&mut output),
   Err(EventError::Empty));
  assert_eq!(output,[event(9)]);
  assert_eq!(q.len(),1);
 }
 #[test]fn duplicate_receipts_are_not_a_valid_prefix(){
  let mut q=ProcessEvents::<2>::new();
  q.enqueue(event(2)).unwrap();q.enqueue(event(2)).unwrap();
  let r=q.peek_receipt().unwrap();let mut out=[event(9);2];
  assert_eq!(q.acknowledge_exact_prefix_into(&[r,r],&mut out),
   Err(EventError::Empty));
  assert_eq!(q.len(),2);assert_eq!(out,[event(9);2]);
 }
}



/// P0-336..340: transfer an exact lifecycle notification prefix into a
/// supervisor event queue. Both source dequeue and destination enqueue are
/// staged and committed together; forwarding creates new destination receipt
/// sequence IDs, preventing a source receipt replay from removing a peer.
/// The kernel caller must hold exclusive ownership of BOTH queues.
impl<const C:usize> ProcessEvents<C>{
 pub fn forward_receipt_prefix_to<const N:usize>(
  &mut self,destination:&mut ProcessEvents<N>,
  receipts:&[EventReceipt],allowed_pids:&[u32]
 )->Result<usize,EventError>{
  if receipts.is_empty(){return Err(EventError::Empty);}
  if allowed_pids.is_empty()||allowed_pids.contains(&0){
   return Err(EventError::InvalidPid);
  }
  let mut source_stage=self.clone();
  let mut target_stage=destination.clone();
  source_stage.acknowledge_exact_prefix(receipts)?;
  for item in receipts{
   if !allowed_pids.contains(&item.event.pid){return Err(EventError::InvalidPid);}
   target_stage.enqueue(item.event)?;
  }
  *self=source_stage;
  *destination=target_stage;
  Ok(receipts.len())
 }
}
#[cfg(test)]
mod kernel_p0_batch16_w2_supervisor_forward_tests{
 use super::*;
 fn event(pid:u32)->ProcessEvent{
  ProcessEvent{pid,kind:EventKind::Exited,value:pid as u64}
 }
 fn prefix<const N:usize>(events:&ProcessEvents<N>,count:usize)->[EventReceipt;2]{
  let mut r=[EventReceipt{sequence:0,event:event(9)};2];
  assert_eq!(events.snapshot_receipts(&mut r),Ok(count));r
 }
 #[test]fn forwarded_notifications_get_new_destination_sequence(){
  let mut from=ProcessEvents::<2>::new();
  let mut to=ProcessEvents::<3>::new();
  from.enqueue(event(2)).unwrap();from.enqueue(event(3)).unwrap();
  to.enqueue(event(7)).unwrap();
  let receipt=prefix(&from,2);
  assert_eq!(from.forward_receipt_prefix_to(&mut to,&receipt,&[2,3]),Ok(2));
  assert_eq!(from.len(),0);assert_eq!(to.len(),3);
  let mut delivered=[EventReceipt{sequence:0,event:event(9)};3];
  to.snapshot_receipts(&mut delivered).unwrap();
  assert_eq!(delivered[0].event,event(7));
  assert_eq!(delivered[1].event,event(2));
  assert_eq!(delivered[2].event,event(3));
  // Sequence spaces are independent; test fresh destination progression.
  assert_eq!(delivered[1].sequence,delivered[0].sequence+1);
  assert_eq!(delivered[2].sequence,delivered[1].sequence+1);
  assert_ne!(delivered[2].sequence,receipt[1].sequence);
  assert_eq!(from.forward_receipt_prefix_to(&mut to,&receipt,&[2,3]),
   Err(EventError::Empty));
 }
 #[test]fn late_capacity_failure_keeps_both_rings_unchanged(){
  let mut from=ProcessEvents::<2>::new();let mut to=ProcessEvents::<2>::new();
  from.enqueue(event(2)).unwrap();from.enqueue(event(3)).unwrap();
  to.enqueue(event(7)).unwrap();
  let r=prefix(&from,2);
  assert_eq!(from.forward_receipt_prefix_to(&mut to,&r,&[2,3]),
   Err(EventError::Full));
  assert_eq!(from.len(),2);assert_eq!(to.len(),1);
  assert_eq!(from.peek_receipt(),Some(r[0]));
  assert_eq!(to.peek(),Some(event(7)));
 }
 #[test]fn unauthorized_or_stale_second_receipt_is_all_or_none(){
  let mut from=ProcessEvents::<2>::new();let mut to=ProcessEvents::<2>::new();
  from.enqueue(event(2)).unwrap();from.enqueue(event(3)).unwrap();
  let r=prefix(&from,2);
  assert_eq!(from.forward_receipt_prefix_to(&mut to,&r,&[2]),
   Err(EventError::InvalidPid));
  let mut wrong=r;wrong[1].sequence+=1;
  assert_eq!(from.forward_receipt_prefix_to(&mut to,&wrong,&[2,3]),
   Err(EventError::Empty));
  assert_eq!(from.len(),2);assert_eq!(to.len(),0);
  assert_eq!(from.peek_receipt(),Some(r[0]));
 }
 #[test]fn duplicate_receipts_and_empty_allowlist_are_denied(){
  let mut from=ProcessEvents::<2>::new();let mut to=ProcessEvents::<2>::new();
  from.enqueue(event(2)).unwrap();from.enqueue(event(3)).unwrap();
  let r=prefix(&from,2);
  assert_eq!(from.forward_receipt_prefix_to(&mut to,&[r[0],r[0]],&[2,3]),
   Err(EventError::Empty));
  assert_eq!(from.forward_receipt_prefix_to(&mut to,&r,&[]),
   Err(EventError::InvalidPid));
  assert_eq!(from.len(),2);assert_eq!(to.len(),0);
 }
}
