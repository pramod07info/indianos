//! Single-owner, no_std, allocation-free lifecycle policy transaction.
//!
//! A trusted scheduler supplies the process incarnation and exclusively owns
//! these three tables. Do not use this as a replacement for real scheduler
//! teardown, socket-table shutdown, SMP locks or IRQ safe synchronization.
//!
//! A transaction stages the capability, UDP, and process-notice tables and
//! publishes all three only after every owner passes all checks.

use crate::capability::{EpochError, EpochRegistry, EpochTicket};
use crate::process_events::{EventError, EventKind, EventReceipt, ProcessEvent, ProcessEvents};
use crate::udp_owner_epoch::{Error as UdpError, Registry as UdpRegistry};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Owner { pub pid: u32, pub generation: u64 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetirementError {
    Capabilities(EpochError),
    Udp(UdpError),
    Events(EventError),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetirementReceipt {
    pub capabilities_revoked: usize,
    pub udp_ports_released: usize,
    pub notices_enqueued: usize,
}

pub struct ProcessRetirement<const C: usize, const U: usize, const E: usize> {
    capabilities: EpochRegistry<C>,
    udp: UdpRegistry<U>,
    events: ProcessEvents<E>,
}

impl<const C: usize, const U: usize, const E: usize> Default
    for ProcessRetirement<C, U, E>
{
    fn default() -> Self { Self::new() }
}

impl<const C: usize, const U: usize, const E: usize> ProcessRetirement<C, U, E> {
    pub const fn new() -> Self {
        Self { capabilities: EpochRegistry::new(),
               udp: UdpRegistry::new(), events: ProcessEvents::new() }
    }

    /// Only the privileged scheduler may authenticate and start a PID.
    pub fn start(&mut self, pid: u32) -> Result<Owner, RetirementError> {
        let generation = self.capabilities.start(pid)
            .map_err(RetirementError::Capabilities)?;
        Ok(Owner { pid, generation })
    }

    pub fn active_generation(&self, pid: u32) -> Option<u64> {
        self.capabilities.active_generation(pid)
    }

    fn require_active(&self, owner: Owner) -> Result<(), RetirementError> {
        if owner.pid == 0 || owner.generation == 0 ||
           self.active_generation(owner.pid) != Some(owner.generation)
        {
            return Err(RetirementError::Capabilities(EpochError::NotActive));
        }
        Ok(())
    }

    pub fn grant(&mut self, owner: Owner, resource: u64, rights: u32)
        -> Result<EpochTicket, RetirementError>
    {
        self.require_active(owner)?;
        self.capabilities.grant(owner.pid, resource, rights)
            .map_err(RetirementError::Capabilities)
    }

    pub fn authorize_ticket(&self, owner: Owner, ticket: EpochTicket,
        resource: u64, rights: u32) -> Result<(), RetirementError>
    {
        self.capabilities.authorize(owner.pid, owner.generation, ticket, resource, rights)
            .map_err(RetirementError::Capabilities)
    }

    pub fn bind_udp(&mut self, owner: Owner, port: u16) -> Result<(), RetirementError> {
        self.require_active(owner)?;
        self.udp.bind(owner.pid, owner.generation, port).map_err(RetirementError::Udp)
    }

    pub fn authorize_udp(&self, owner: Owner, port: u16)
        -> Result<(), RetirementError>
    {
        self.require_active(owner)?;
        self.udp.authorize(owner.pid, owner.generation, port).map_err(RetirementError::Udp)
    }

    pub fn active_udp_ports(&self) -> usize { self.udp.active_count() }
    pub fn queued_notices(&self) -> usize { self.events.len() }
    pub fn peek_notice(&self) -> Option<EventReceipt> { self.events.peek_receipt() }

    /// The one-time sequence in EventReceipt prevents replay or head skipping.
    pub fn acknowledge_notice(&mut self, receipt: EventReceipt)
        -> Option<ProcessEvent>
    {
        self.events.acknowledge_receipt(receipt)
    }

    pub fn retire_exited_batch(&mut self, owners: &[Owner])
        -> Result<RetirementReceipt, RetirementError>
    {
        self.retire_batch(owners, EventKind::Exited)
    }

    pub fn retire_faulted_batch(&mut self, owners: &[Owner])
        -> Result<RetirementReceipt, RetirementError>
    {
        self.retire_batch(owners, EventKind::Faulted)
    }

    fn retire_batch(&mut self, owners: &[Owner], kind: EventKind)
        -> Result<RetirementReceipt, RetirementError>
    {
        // Validate the full batch before staging any changes, including
        // repeated PID with distinct generations.
        for (i, owner) in owners.iter().enumerate() {
            if owner.pid == 0 {
                return Err(RetirementError::Capabilities(EpochError::InvalidPid));
            }
            self.require_active(*owner)?;
            if owners[..i].iter().any(|previous| previous.pid == owner.pid) {
                return Err(RetirementError::Capabilities(EpochError::Denied));
            }
        }

        let mut capabilities = self.capabilities.clone();
        let mut udp = self.udp.clone();
        let mut events = self.events.clone();
        let mut result = RetirementReceipt::default();
        for owner in owners {
            result.capabilities_revoked += capabilities
                .retire(owner.pid, owner.generation)
                .map_err(RetirementError::Capabilities)?;
            if udp.has_foreign_generation(owner.pid, owner.generation) {
                return Err(RetirementError::Udp(UdpError::Denied));
            }
            result.udp_ports_released += udp
                .revoke_owner(owner.pid, owner.generation)
                .map_err(RetirementError::Udp)?;
            let inserted = match kind {
                EventKind::Exited => events.enqueue_exit_once(owner.pid, owner.generation),
                EventKind::Faulted => events.enqueue_fault_once(owner.pid, owner.generation),
                _ => unreachable!("only scheduler exit or fault is permitted"),
            }.map_err(RetirementError::Events)?;
            if inserted { result.notices_enqueued += 1; }
        }

        // Only this publication touches the live policy tables. The kernel
        // caller must exclude competing mutators on all three tables.
        self.capabilities = capabilities;
        self.udp = udp;
        self.events = events;
        Ok(result)
    }
}

#[cfg(test)]
mod r120_atomic_coordinator_tests {
    use super::*;

    #[test]
    fn atomic_exit_retires_exact_owners_but_preserves_peers_and_receipts() {
        let mut k = ProcessRetirement::<5, 5, 5>::new();
        let a = k.start(10).unwrap();
        let b = k.start(20).unwrap();
        let peer = k.start(30).unwrap();
        let ta = k.grant(a, 100, 1).unwrap();
        let tb = k.grant(b, 200, 1).unwrap();
        let tp = k.grant(peer, 300, 1).unwrap();
        k.bind_udp(a, 4001).unwrap();
        k.bind_udp(a, 4002).unwrap();
        k.bind_udp(b, 4003).unwrap();
        k.bind_udp(peer, 4004).unwrap();
        assert_eq!(k.retire_exited_batch(&[a, b]), Ok(RetirementReceipt {
            capabilities_revoked: 2, udp_ports_released: 3, notices_enqueued: 2
        }));
        assert_eq!(k.active_generation(a.pid), None);
        assert!(k.authorize_ticket(a, ta, 100, 1).is_err());
        assert!(k.authorize_ticket(b, tb, 200, 1).is_err());
        assert!(k.authorize_udp(a, 4001).is_err());
        assert_eq!(k.authorize_ticket(peer, tp, 300, 1), Ok(()));
        assert_eq!(k.authorize_udp(peer, 4004), Ok(()));
        assert_eq!(k.active_udp_ports(), 1);
        let first = k.peek_notice().unwrap();
        assert_eq!((first.event.pid, first.event.value), (10, a.generation));
        assert_eq!(first.event.kind, EventKind::Exited);
        assert_eq!(k.acknowledge_notice(first), Some(first.event));
        assert_eq!(k.acknowledge_notice(first), None);
        let next = k.peek_notice().unwrap();
        assert_ne!(first.sequence, next.sequence);
        assert_eq!(next.event.pid, 20);
    }

    #[test]
    fn stale_later_owner_cannot_partially_revoke_any_peer() {
        let mut k = ProcessRetirement::<3, 3, 3>::new();
        let a = k.start(1).unwrap();
        let b = k.start(2).unwrap();
        let token = k.grant(a, 8, 1).unwrap();
        k.bind_udp(a, 4100).unwrap();
        let stale = Owner { pid: b.pid, generation: b.generation + 1 };
        assert_eq!(k.retire_exited_batch(&[a, stale]),
            Err(RetirementError::Capabilities(EpochError::NotActive)));
        assert_eq!(k.authorize_ticket(a, token, 8, 1), Ok(()));
        assert_eq!(k.authorize_udp(a, 4100), Ok(()));
        assert_eq!(k.queued_notices(), 0);
    }

    #[test]
    fn event_backpressure_aborts_capability_and_udp_retirement() {
        let mut k = ProcessRetirement::<2, 2, 1>::new();
        let a = k.start(1).unwrap();
        let token = k.grant(a, 1, 1).unwrap();
        k.bind_udp(a, 5123).unwrap();
        k.events.enqueue_exit_once(99, 1).unwrap();
        let before = k.peek_notice().unwrap();
        assert_eq!(k.retire_exited_batch(&[a]),
            Err(RetirementError::Events(EventError::Full)));
        assert_eq!(k.peek_notice(), Some(before));
        assert_eq!(k.authorize_ticket(a, token, 1, 1), Ok(()));
        assert_eq!(k.authorize_udp(a, 5123), Ok(()));
    }

    #[test]
    fn foreign_udp_generation_rolls_back_all_three_tables() {
        let mut k = ProcessRetirement::<2, 2, 2>::new();
        let a = k.start(1).unwrap();
        let token = k.grant(a, 1, 1).unwrap();
        // Corrupt simulated registry for adversarial internal-policy test.
        k.udp.bind(a.pid, a.generation + 1, 7001).unwrap();
        assert_eq!(k.retire_exited_batch(&[a]),
            Err(RetirementError::Udp(UdpError::Denied)));
        assert_eq!(k.authorize_ticket(a, token, 1, 1), Ok(()));
        assert_eq!(k.queued_notices(), 0);
    }

    #[test]
    fn fault_notice_is_exact_generation_and_replay_safe() {
        let mut k = ProcessRetirement::<2, 2, 2>::new();
        let a = k.start(7).unwrap();
        k.bind_udp(a, 7001).unwrap();
        assert_eq!(k.retire_faulted_batch(&[a]), Ok(RetirementReceipt {
            capabilities_revoked: 0, udp_ports_released: 1, notices_enqueued: 1
        }));
        let first = k.peek_notice().unwrap();
        assert_eq!(first.event.kind, EventKind::Faulted);
        assert_eq!(k.acknowledge_notice(first), Some(first.event));
        let second = k.start(7).unwrap();
        assert!(k.bind_udp(a, 7002).is_err());
        k.bind_udp(second, 7002).unwrap();
        assert_eq!(k.retire_faulted_batch(&[second]).unwrap().notices_enqueued, 1);
        let newer = k.peek_notice().unwrap();
        assert_ne!(first.sequence, newer.sequence);
        assert_eq!(k.acknowledge_notice(first), None);
        assert_eq!(newer.event.value, second.generation);
    }

    #[test]
    fn duplicate_generation_and_zero_capacity_fail_closed() {
        let mut k = ProcessRetirement::<1, 1, 1>::new();
        let a = k.start(1).unwrap();
        assert_eq!(k.retire_exited_batch(&[a, a]),
            Err(RetirementError::Capabilities(EpochError::Denied)));
        assert!(k.retire_exited_batch(&[Owner { pid: 0, generation: 1 }]).is_err());
        assert_eq!(k.retire_exited_batch(&[]), Ok(RetirementReceipt::default()));
        assert_eq!(k.retire_exited_batch(&[a]).unwrap().notices_enqueued, 1);
        assert!(k.retire_exited_batch(&[a]).is_err());
        let b = k.start(1).unwrap();
        assert_eq!(b.generation, a.generation + 1);
        assert_eq!(k.retire_faulted_batch(&[b]),
            Err(RetirementError::Events(EventError::Full)));
        assert_eq!(k.active_generation(1), Some(b.generation));
        let mut empty = ProcessRetirement::<0, 0, 0>::new();
        assert_eq!(empty.retire_exited_batch(&[]), Ok(RetirementReceipt::default()));
        assert_eq!(empty.start(1), Err(RetirementError::Capabilities(EpochError::Full)));
    }
}
