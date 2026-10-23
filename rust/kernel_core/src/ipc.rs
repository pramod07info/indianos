//! Bounded kernel-owned IPC mailboxes for the future userspace ABI.
//!
//! No heap allocation, interrupts, blocking, or untrusted user pointers.
//! The privileged caller must authenticate the sender against the current PCB;
//! this is *not* yet exposed as a syscall or scheduler-integrated IPC service.
//! Each mailbox has a fixed receiver PID, and teardown discards queued messages.

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

    /// A retired PID must not receive messages after its resources are freed.
    pub fn unregister(&mut self, pid: u32) -> Result<(), IpcError> {
        let slot = self.mailboxes.iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|m| m.receiver == pid))
            .ok_or(IpcError::UnknownReceiver)?;
        *slot = None;
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
    fn payload_is_owned_and_zero_padded() {
        let mut source = [7u8, 8, 9];
        let message = Message::new(1, &source).unwrap();
        source.fill(0);
        assert_eq!(message.payload(), &[7, 8, 9]);
        assert!(message.bytes[3..].iter().all(|&byte| byte == 0));
    }
}
