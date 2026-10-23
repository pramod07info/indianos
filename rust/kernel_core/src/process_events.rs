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
