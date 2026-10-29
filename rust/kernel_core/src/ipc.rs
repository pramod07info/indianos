//! Bounded kernel-owned IPC mailboxes for the future userspace ABI.
//!
//! No heap allocation, interrupts, blocking, or untrusted user pointers.
//! The privileged caller must authenticate the sender against the current PCB;
//! this is *not* yet exposed as a syscall or scheduler-integrated IPC service.
//! Each mailbox has a fixed receiver PID. Teardown discards its received
//! messages and any pending messages it sent to peers.

pub const MAX_PAYLOAD: usize = 64;
pub const MAX_MAILBOX_DEPTH: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IpcError {
    InvalidPid,
    InvalidConfiguration,
    DuplicateReceiver,
    NoFreeMailbox,
    UnknownSender,
    UnknownReceiver,
    PayloadTooLarge,
    Full,
    Empty,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Message {
    sender: u32,
    length: u8,
    bytes: [u8; MAX_PAYLOAD],
}

impl Message {
    pub fn sender(&self) -> u32 { self.sender }
    pub fn payload(&self) -> &[u8] { &self.bytes[..self.length as usize] }

    fn new(sender: u32, payload: &[u8]) -> Result<Self, IpcError> {
        if sender == 0 { return Err(IpcError::InvalidPid); }
        if payload.len() > MAX_PAYLOAD { return Err(IpcError::PayloadTooLarge); }
        let mut bytes = [0u8; MAX_PAYLOAD];
        bytes[..payload.len()].copy_from_slice(payload);
        Ok(Self { sender, length: payload.len() as u8, bytes })
    }
}

pub struct Mailbox<const DEPTH: usize> {
    receiver: u32,
    slots: [Option<Message>; DEPTH],
    head: usize,
    count: usize,
}

impl<const DEPTH: usize> Mailbox<DEPTH> {
    pub fn new(receiver: u32) -> Result<Self, IpcError> {
        if receiver == 0 { return Err(IpcError::InvalidPid); }
        if DEPTH == 0 || DEPTH > MAX_MAILBOX_DEPTH {
            return Err(IpcError::InvalidConfiguration);
        }
        Ok(Self { receiver, slots: [None; DEPTH], head: 0, count: 0 })
    }

    pub fn receiver(&self) -> u32 { self.receiver }
    pub fn pending(&self) -> usize { self.count }
    pub fn capacity(&self) -> usize { DEPTH }

    fn enqueue(&mut self, message: Message) -> Result<(), IpcError> {
        if self.count == DEPTH { return Err(IpcError::Full); }
        let tail = (self.head + self.count) % DEPTH;
        self.slots[tail] = Some(message);
        self.count += 1;
        Ok(())
    }

    pub fn receive(&mut self) -> Result<Message, IpcError> {
        if self.count == 0 { return Err(IpcError::Empty); }
        let message = self.slots[self.head].take().ok_or(IpcError::Empty)?;
        self.head = (self.head + 1) % DEPTH;
        self.count -= 1;
        Ok(message)
    }

    // Compact in logical FIFO order. The write cursor never passes the read
    // cursor, including when the ring wraps, so unread messages stay intact.
    fn discard_sender(&mut self, sender: u32) {
        let original_count = self.count;
        let mut kept = 0;
        for offset in 0..original_count {
            let read = (self.head + offset) % DEPTH;
            if let Some(message) = self.slots[read].take() {
                if message.sender != sender {
                    let write = (self.head + kept) % DEPTH;
                    self.slots[write] = Some(message);
                    kept += 1;
                }
            }
        }
        self.count = kept;
    }

    pub fn clear(&mut self) {
        self.slots.fill(None);
        self.head = 0;
        self.count = 0;
    }
}

/// Fixed-capacity per-PID routing table. This is a non-locking primitive:
/// it must be used with appropriate scheduler/IRQ exclusion on one CPU.
/// Callers must authorize the sender PID before invoking send().
pub struct IpcRouter<const MAILBOXES: usize, const DEPTH: usize> {
    mailboxes: [Option<Mailbox<DEPTH>>; MAILBOXES],
}

impl<const MAILBOXES: usize, const DEPTH: usize> IpcRouter<MAILBOXES, DEPTH> {
    pub fn new() -> Result<Self, IpcError> {
        if MAILBOXES == 0 || DEPTH == 0 || DEPTH > MAX_MAILBOX_DEPTH {
            return Err(IpcError::InvalidConfiguration);
        }
        Ok(Self { mailboxes: core::array::from_fn(|_| None) })
    }

    pub fn register(&mut self, pid: u32) -> Result<(), IpcError> {
        if pid == 0 { return Err(IpcError::InvalidPid); }
        if self.mailboxes.iter().flatten().any(|box_| box_.receiver == pid) {
            return Err(IpcError::DuplicateReceiver);
        }
        let slot = self.mailboxes.iter_mut().find(|slot| slot.is_none())
            .ok_or(IpcError::NoFreeMailbox)?;
        *slot = Some(Mailbox::new(pid)?);
        Ok(())
    }

    /// Drop the receiver mailbox and its pending outbound traffic before PID
    /// reuse. An unknown PID leaves every mailbox unchanged.
    pub fn unregister(&mut self, pid: u32) -> Result<(), IpcError> {
        let slot = self.mailboxes.iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|m| m.receiver == pid))
            .ok_or(IpcError::UnknownReceiver)?;
        *slot = None;
        for mailbox in self.mailboxes.iter_mut().flatten() {
            mailbox.discard_sender(pid);
        }
        Ok(())
    }

    pub fn send(&mut self, sender: u32, receiver: u32, data: &[u8])
        -> Result<(), IpcError>
    {
        if sender == 0 || receiver == 0 { return Err(IpcError::InvalidPid); }
        if data.len() > MAX_PAYLOAD { return Err(IpcError::PayloadTooLarge); }
        if !self.mailboxes.iter().flatten().any(|m| m.receiver == sender) {
            return Err(IpcError::UnknownSender);
        }
        let target = self.mailboxes.iter_mut().flatten()
            .find(|m| m.receiver == receiver).ok_or(IpcError::UnknownReceiver)?;
        target.enqueue(Message::new(sender, data)?)
    }

    /// Only the named receiver can consume its queue. The future syscall
    /// handler must derive this identity from the current PCB, not user input.
    pub fn receive(&mut self, receiver: u32) -> Result<Message, IpcError> {
        let target = self.mailboxes.iter_mut().flatten()
            .find(|m| m.receiver == receiver).ok_or(IpcError::UnknownReceiver)?;
        target.receive()
    }

    pub fn pending(&self, receiver: u32) -> Result<usize, IpcError> {
        self.mailboxes.iter().flatten().find(|m| m.receiver == receiver)
            .map(|m| m.pending()).ok_or(IpcError::UnknownReceiver)
    }
}

/// Exact-process-identity IPC routing. The prior PID-only router is kept
/// for legacy callers; new services must use this generation-aware variant.
/// All methods require scheduler-authenticated identities and serialized
/// kernel access (not yet an SMP-safe live syscall implementation).
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct TaggedMessage{
 sender:Identity,length:u8,bytes:[u8;MAX_PAYLOAD],
}
impl TaggedMessage{
 pub fn sender(&self)->Identity{self.sender}
 pub fn payload(&self)->&[u8]{&self.bytes[..self.length as usize]}
}
struct ScopedMailbox<const D:usize>{
 owner:Identity,queue:[Option<TaggedMessage>;D],count:usize,
}
pub struct ScopedRouter<const M:usize,const D:usize>{
 boxes:[Option<ScopedMailbox<D>>;M],
}
impl<const M:usize,const D:usize> ScopedRouter<M,D>{
 pub fn new()->Result<Self,IpcError>{
  if M==0||D==0||D>MAX_MAILBOX_DEPTH{
   return Err(IpcError::InvalidConfiguration);
  }
  Ok(Self{boxes:core::array::from_fn(|_|None)})
 }
 pub fn register(&mut self,owner:Identity)->Result<(),IpcError>{
  if owner.pid==0||owner.generation==0{return Err(IpcError::InvalidPid);}
  if self.boxes.iter().flatten().any(|b|b.owner.pid==owner.pid){
   return Err(IpcError::DuplicateReceiver);
  }
  let slot=self.boxes.iter_mut().find(|x|x.is_none())
    .ok_or(IpcError::NoFreeMailbox)?;
  *slot=Some(ScopedMailbox{owner,queue:[None;D],count:0});
  Ok(())
 }
 /// Drop both received and not-yet-read messages sent by the exiting
 /// process: a recycled PID must never inherit or impersonate old traffic.
 pub fn reap(&mut self,owner:Identity)->Result<(),IpcError>{
  let found=self.boxes.iter().any(|b|b.as_ref().is_some_and(|x|x.owner==owner));
  if !found{return Err(IpcError::UnknownReceiver);}
  for slot in &mut self.boxes{
   if slot.as_ref().is_some_and(|b|b.owner==owner){*slot=None;continue;}
   if let Some(b)=slot{
    let mut write=0;
    for i in 0..b.count{
     let msg=b.queue[i];
     if !msg.is_some_and(|m|m.sender==owner){
      b.queue[write]=msg;write+=1;
     }
    }
    for i in write..b.count{b.queue[i]=None;}
    b.count=write;
   }
  }
  Ok(())
 }
 pub fn send(&mut self,sender:Identity,receiver:Identity,
  payload:&[u8])->Result<(),IpcError>{
  if sender.pid==0||sender.generation==0||receiver.pid==0||
     receiver.generation==0{return Err(IpcError::InvalidPid);}
  if payload.len()>MAX_PAYLOAD{return Err(IpcError::PayloadTooLarge);}
  if !self.boxes.iter().flatten().any(|b|b.owner==sender){
   return Err(IpcError::UnknownSender);
  }
  let target=self.boxes.iter_mut().flatten().find(|b|b.owner==receiver)
   .ok_or(IpcError::UnknownReceiver)?;
  if target.count==D{return Err(IpcError::Full);}
  let mut bytes=[0u8;MAX_PAYLOAD];
  bytes[..payload.len()].copy_from_slice(payload);
  target.queue[target.count]=Some(TaggedMessage{
   sender,length:payload.len() as u8,bytes,
  });
  target.count+=1;Ok(())
 }
 /// Send to a set of distinct authenticated receivers as one
 /// serialized operation. Any stale recipient or full queue rejects the
 /// complete fanout without delivering a partial message.
 pub fn multicast(&mut self,sender:Identity,recipients:&[Identity],
  payload:&[u8])->Result<(),IpcError>{
  if sender.pid==0||sender.generation==0||recipients.is_empty(){
   return Err(IpcError::InvalidPid);
  }
  if payload.len()>MAX_PAYLOAD{return Err(IpcError::PayloadTooLarge);}
  if !self.boxes.iter().flatten().any(|b|b.owner==sender){
   return Err(IpcError::UnknownSender);
  }
  for (index,recipient) in recipients.iter().enumerate(){
   if recipient.pid==0||recipient.generation==0{
    return Err(IpcError::InvalidPid);
   }
   if recipients[..index].contains(recipient){
    return Err(IpcError::DuplicateReceiver);
   }
   let target=self.boxes.iter().flatten().find(|b|b.owner==*recipient)
     .ok_or(IpcError::UnknownReceiver)?;
   if target.count==D{return Err(IpcError::Full);}
  }
  // Serialized syscall context: no other mutator may run between preflight
  // and publication. This is not yet a multicore-safe lockless queue.
  for &recipient in recipients{
   self.send(sender,recipient,payload)?;
  }
  Ok(())
 }
 pub fn receive(&mut self,receiver:Identity)->Result<TaggedMessage,IpcError>{
  let b=self.boxes.iter_mut().flatten().find(|b|b.owner==receiver)
    .ok_or(IpcError::UnknownReceiver)?;
  if b.count==0{return Err(IpcError::Empty);}
  let value=b.queue[0].take().ok_or(IpcError::Empty)?;
  for i in 1..b.count{b.queue[i-1]=b.queue[i];}
  b.count-=1;b.queue[b.count]=None;Ok(value)
 }
 pub fn pending(&self,receiver:Identity)->Result<usize,IpcError>{
  self.boxes.iter().flatten().find(|b|b.owner==receiver)
   .map(|b|b.count).ok_or(IpcError::UnknownReceiver)
 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_fifo_wraparound_and_sender_identity() {
        let mut router = IpcRouter::<3, 2>::new().unwrap();
        router.register(10).unwrap();
        router.register(20).unwrap();
        router.register(30).unwrap();
        router.send(10, 20, b"hello").unwrap();
        router.send(30, 20, b"world").unwrap();
        assert_eq!(router.pending(20), Ok(2));
        assert_eq!(router.send(10, 20, b"full"), Err(IpcError::Full));
        let first = router.receive(20).unwrap();
        assert_eq!(first.sender(), 10);
        assert_eq!(first.payload(), b"hello");
        router.send(10, 20, b"again").unwrap();
        assert_eq!(router.receive(20).unwrap().payload(), b"world");
        assert_eq!(router.receive(20).unwrap().payload(), b"again");
        assert_eq!(router.receive(20), Err(IpcError::Empty));
    }

    #[test]
    fn ipc_rejects_unknown_identity_oversize_and_overcommit() {
        assert!(matches!(IpcRouter::<0, 2>::new(), Err(IpcError::InvalidConfiguration)));
        assert!(matches!(IpcRouter::<1, 0>::new(), Err(IpcError::InvalidConfiguration)));
        let mut router = IpcRouter::<2, 1>::new().unwrap();
        assert_eq!(router.register(0), Err(IpcError::InvalidPid));
        router.register(1).unwrap();
        assert_eq!(router.register(1), Err(IpcError::DuplicateReceiver));
        router.register(2).unwrap();
        assert_eq!(router.register(3), Err(IpcError::NoFreeMailbox));
        assert_eq!(router.send(3, 2, b"x"), Err(IpcError::UnknownSender));
        assert_eq!(router.send(1, 3, b"x"), Err(IpcError::UnknownReceiver));
        assert_eq!(router.send(1, 2, &[42; MAX_PAYLOAD + 1]), Err(IpcError::PayloadTooLarge));
        assert_eq!(router.send(0, 2, b"x"), Err(IpcError::InvalidPid));
        router.send(1, 2, b"").unwrap();
        assert_eq!(router.receive(2).unwrap().payload(), b"");
    }

    #[test]
    fn ipc_teardown_disposes_queued_data_and_isolates_receivers() {
        let mut router = IpcRouter::<3, 4>::new().unwrap();
        router.register(1).unwrap();
        router.register(2).unwrap();
        router.send(1, 2, b"secret").unwrap();
        assert_eq!(router.pending(1), Ok(0));
        router.unregister(2).unwrap();
        assert_eq!(router.pending(2), Err(IpcError::UnknownReceiver));
        router.register(2).unwrap();
        assert_eq!(router.receive(2), Err(IpcError::Empty));
        assert_eq!(router.unregister(3), Err(IpcError::UnknownReceiver));
    }

    #[test]
    fn ipc_unregister_recycled_sender_purges_all_peers() {
        let mut router = IpcRouter::<4, 4>::new().unwrap();
        for pid in 1..=4 { router.register(pid).unwrap(); }
        for peer in 2..=4 {
            router.send(1, peer, b"old").unwrap();
            router.send(2, peer, b"keep").unwrap();
            router.send(1, peer, b"old again").unwrap();
        }
        router.send(1, 1, b"self").unwrap();
        router.send(2, 1, b"inbound").unwrap();
        router.unregister(1).unwrap();
        assert_eq!(router.send(1, 2, b"absent"), Err(IpcError::UnknownSender));
        router.register(1).unwrap();
        assert_eq!(router.receive(1), Err(IpcError::Empty));
        for peer in 2..=4 {
            assert_eq!(router.pending(peer), Ok(1));
            router.send(1, peer, b"fresh").unwrap();
            let retained = router.receive(peer).unwrap();
            assert_eq!(retained.sender(), 2);
            assert_eq!(retained.payload(), b"keep");
            assert_eq!(router.receive(peer).unwrap().payload(), b"fresh");
            assert_eq!(router.receive(peer), Err(IpcError::Empty));
        }
    }

    // Enumerate every sender arrangement and head position in a full ring.
    // Refill to capacity after teardown to catch holes and stale tail slots.
    fn check_teardown_rings<const D: usize>() {
        for head in 0..D {
            for mask in 0..(1usize << D) {
                let mut router = IpcRouter::<3, D>::new().unwrap();
                for pid in 1..=3 { router.register(pid).unwrap(); }
                for _ in 0..head {
                    router.send(3, 2, b"advance").unwrap();
                    router.receive(2).unwrap();
                }
                let mut expected = [None; D];
                let mut kept = 0;
                for index in 0..D {
                    let sender = if mask & (1 << index) == 0 { 1 } else { 3 };
                    let mut payload = [index as u8; MAX_PAYLOAD];
                    payload[0] = 0;
                    router.send(sender, 2, &payload).unwrap();
                    if sender == 3 {
                        expected[kept] = Some(Message::new(sender, &payload).unwrap());
                        kept += 1;
                    }
                }
                // Failure must preserve exact wrapped queue contents.
                assert_eq!(router.unregister(99), Err(IpcError::UnknownReceiver));
                assert_eq!(router.unregister(0), Err(IpcError::UnknownReceiver));
                assert_eq!(router.pending(2), Ok(D));
                router.unregister(1).unwrap();
                assert_eq!(router.pending(2), Ok(kept));
                for _ in kept..D { router.send(3, 2, b"").unwrap(); }
                assert_eq!(router.send(3, 2, b"full"), Err(IpcError::Full));
                for message in expected.iter().take(kept) {
                    assert_eq!(router.receive(2), Ok(message.unwrap()));
                }
                for _ in kept..D {
                    let message = router.receive(2).unwrap();
                    assert_eq!(message.sender(), 3);
                    assert_eq!(message.payload(), b"");
                }
                assert_eq!(router.receive(2), Err(IpcError::Empty));
            }
        }
    }

    #[test]
    fn ipc_teardown_preserves_wrapped_fifo_and_reclaims_capacity() {
        check_teardown_rings::<1>();
        check_teardown_rings::<2>();
        check_teardown_rings::<5>();
    }

    #[test]
    fn ipc_teardown_empty_maximum_queue_and_invalid_depth() {
        assert!(matches!(IpcRouter::<3, 0>::new(), Err(IpcError::InvalidConfiguration)));
        let mut router = IpcRouter::<3, MAX_MAILBOX_DEPTH>::new().unwrap();
        for pid in 1..=3 { router.register(pid).unwrap(); }
        router.unregister(1).unwrap();
        assert_eq!(router.pending(2), Ok(0));
        router.register(1).unwrap();
        for _ in 0..MAX_MAILBOX_DEPTH { router.send(1, 2, b"old").unwrap(); }
        router.unregister(1).unwrap();
        for _ in 0..MAX_MAILBOX_DEPTH { router.send(3, 2, b"new").unwrap(); }
        assert_eq!(router.send(3, 2, b"full"), Err(IpcError::Full));
        for _ in 0..MAX_MAILBOX_DEPTH {
            assert_eq!(router.receive(2).unwrap().payload(), b"new");
        }
    }

    #[test]
    fn payload_is_owned_and_zero_padded() {
        let mut source = [7u8, 8, 9];
        let message = Message::new(1, &source).unwrap();
        source.fill(0);
        assert_eq!(message.payload(), &[7, 8, 9]);
        assert!(message.bytes[3..].iter().all(|&byte| byte == 0));
    }

 #[test]fn scoped_ipc_reaps_old_sender_and_disallows_pid_reuse(){
  let mut router=ScopedRouter::<3,3>::new().unwrap();
  let a=Identity{pid:2,generation:1};
  let b=Identity{pid:3,generation:1};
  router.register(a).unwrap();router.register(b).unwrap();
  router.send(a,b,b"old").unwrap();
  router.reap(a).unwrap();
  assert_eq!(router.pending(b),Ok(0));
  let reuse=Identity{pid:2,generation:2};
  router.register(reuse).unwrap();
  assert_eq!(router.send(a,b,b"spoof"),Err(IpcError::UnknownSender));
  router.send(reuse,b,b"fresh").unwrap();
  let m=router.receive(b).unwrap();
  assert_eq!(m.sender(),reuse);
  assert_eq!(m.payload(),b"fresh");
 }
 #[test]fn scoped_ipc_receiver_generation_and_fifo_cannot_be_bypassed(){
  let mut router=ScopedRouter::<2,2>::new().unwrap();
  let a=Identity{pid:2,generation:1};let b=Identity{pid:3,generation:4};
  router.register(a).unwrap();router.register(b).unwrap();
  router.send(a,b,b"one").unwrap();
  router.send(a,b,b"two").unwrap();
  assert_eq!(router.send(a,b,b"full"),Err(IpcError::Full));
  assert_eq!(router.receive(Identity{generation:5,..b}),
    Err(IpcError::UnknownReceiver));
  assert_eq!(router.receive(b).unwrap().payload(),b"one");
  assert_eq!(router.receive(b).unwrap().payload(),b"two");
  router.reap(b).unwrap();
  assert_eq!(router.receive(b),Err(IpcError::UnknownReceiver));
 }


 #[test]fn atomic_multicast_fanout_delivers_only_if_every_mailbox_has_capacity(){
  let mut router=ScopedRouter::<3,1>::new().unwrap();
  let a=Identity{pid:1,generation:1};
  let b=Identity{pid:2,generation:1};
  let c=Identity{pid:3,generation:1};
  for owner in [a,b,c]{router.register(owner).unwrap();}
  router.send(a,b,b"busy").unwrap();
  assert_eq!(router.multicast(a,&[b,c],b"news"),Err(IpcError::Full));
  assert_eq!(router.pending(c),Ok(0));
  assert_eq!(router.receive(b).unwrap().payload(),b"busy");
  router.multicast(a,&[b,c],b"news").unwrap();
  assert_eq!(router.receive(b).unwrap().payload(),b"news");
  assert_eq!(router.receive(c).unwrap().payload(),b"news");
 }
 #[test]fn multicast_rejects_duplicate_or_stale_receiver_before_publication(){
  let mut router=ScopedRouter::<2,2>::new().unwrap();
  let a=Identity{pid:1,generation:1};
  let b=Identity{pid:2,generation:1};
  router.register(a).unwrap();router.register(b).unwrap();
  assert_eq!(router.multicast(a,&[b,b],b"x"),Err(IpcError::DuplicateReceiver));
  assert_eq!(router.multicast(a,&[b,Identity{generation:2,..b}],b"x"),
      Err(IpcError::UnknownReceiver));
  assert_eq!(router.pending(b),Ok(0));
 }

}
