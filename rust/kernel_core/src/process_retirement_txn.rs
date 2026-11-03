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
use crate::network::{EpochUdpEndpoints, IngressError, SocketError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Owner { pub pid: u32, pub generation: u64 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetirementError {
    Capabilities(EpochError),
    Udp(UdpError),
    Sockets(SocketError),
    Ingress(IngressError),
    Events(EventError),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetirementReceipt {
    pub capabilities_revoked: usize,
    pub udp_ports_released: usize,
    pub notices_enqueued: usize,
}

#[derive(Clone)]
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

    /// Trust only the generation already bound by the privileged live PCB
    /// and FileHandles service; never derive it from ring-3 syscall fields.
    pub fn start_exact(&mut self, owner: Owner)
        -> Result<(), RetirementError>
    {
        self.capabilities.start_exact(owner.pid,owner.generation)
            .map_err(RetirementError::Capabilities)
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

    /// Read-only postcondition for a privileged scheduler/QEMU acceptance
    /// path; never treats a bare socket port as an authoritative PID epoch.
    pub fn live_sockets_consistent<const P:usize,const Q:usize>(
        &self,sockets:&EpochUdpEndpoints<P,Q>
    )->bool{
        sockets.matches_registry(&self.udp)
    }
    pub fn queued_notices(&self) -> usize { self.events.len() }
    pub fn peek_notice(&self) -> Option<EventReceipt> { self.events.peek_receipt() }

    /// The one-time sequence in EventReceipt prevents replay or head skipping.
    pub fn acknowledge_notice(&mut self, receipt: EventReceipt)
        -> Option<ProcessEvent>
    {
        self.events.acknowledge_receipt(receipt)
    }

    /// R120: bind the actual kernel packet-queue/socket table and generation
    /// registry as a single atomic update. This is not merely the policy
    /// registry: real endpoint state and queued datagrams are included.
    pub fn bind_live_udp<const P:usize,const Q:usize>(
        &mut self,sockets:&mut EpochUdpEndpoints<P,Q>,owner:Owner,port:u16
    )->Result<(),RetirementError>{
        if !sockets.matches_registry(&self.udp){
            return Err(RetirementError::Sockets(SocketError::Denied));
        }
        let mut staged=self.clone();
        let mut staged_sockets=sockets.clone();
        staged.bind_udp(owner,port)?;
        staged_sockets.bind(owner.pid,owner.generation,port)
            .map_err(RetirementError::Sockets)?;
        if !staged_sockets.matches_registry(&staged.udp){
            return Err(RetirementError::Sockets(SocketError::Denied));
        }
        *self=staged;
        *sockets=staged_sockets;
        Ok(())
    }

    /// The network receive path must validate the scheduler-supplied exact
    /// owner epoch before frame parsing and UDP FIFO admission. A packet
    /// cannot use a grant left behind by a retired or recycled PID.
    /// This is a polled virtio-net guest path, not an IRQ/SMP hardware proof.
    pub fn ingest_live_udp<const P:usize,const Q:usize>(
        &self,sockets:&mut EpochUdpEndpoints<P,Q>,owner:Owner,port:u16,
        frame:&[u8],mac:[u8;6]
    )->Result<(),RetirementError>{
        self.require_active(owner)?;
        if !sockets.matches_registry(&self.udp){
            return Err(RetirementError::Sockets(SocketError::Denied));
        }
        self.udp.authorize(owner.pid,owner.generation,port)
            .map_err(RetirementError::Udp)?;
        sockets.ingest_registered_frame(&self.udp,owner.pid,owner.generation,
            port,frame,mac).map_err(RetirementError::Ingress)
    }

    /// Kernel NIC ingress dispatch: the packet is validated and its
    /// destination port determines an exact currently live process generation.
    /// No caller-supplied user PID or packet header is used as PID authority.
    /// A non-Send BSP permit must cover the entire parser/queue transition.
    /// This is ready for a future IRQ dispatcher, but is currently usable by
    /// the exclusive polled-NIC supervisor only: NOT physical IRQ/IOMMU proof.
    pub fn ingest_kernel_nic_frame<const P:usize,const Q:usize>(
        &self,_permit:&crate::retirement_gate::Permit<'_>,
        sockets:&mut EpochUdpEndpoints<P,Q>,
        frame:&[u8],local_mac:[u8;6],
    )->Result<Owner,RetirementError>{
        let (port,_payload)=crate::network::authenticated_udp_frame(frame,local_mac)
            .map_err(RetirementError::Ingress)?;
        let grant=self.udp.owner_of(port)
            .ok_or(RetirementError::Udp(UdpError::Denied))?;
        let owner=Owner{pid:grant.pid,generation:grant.generation};
        self.ingest_live_udp(sockets,owner,port,frame,local_mac)?;
        Ok(owner)
    }

    /// Authenticated dequeue also requires the process to remain live.
    /// A denied read must leave both the output and queued packets untouched.
    pub fn receive_live_udp<const P:usize,const Q:usize>(
        &self,sockets:&mut EpochUdpEndpoints<P,Q>,owner:Owner,port:u16,
        out:&mut[u8]
    )->Result<usize,RetirementError>{
        self.require_active(owner)?;
        if !sockets.matches_registry(&self.udp){
            return Err(RetirementError::Sockets(SocketError::Denied));
        }
        sockets.receive_registered(&self.udp,owner.pid,owner.generation,port,out)
            .map_err(RetirementError::Sockets)
    }

    /// R120 ring-3 syscall: paired close is a transaction across policy and
    /// the real socket endpoint / pending receive queue.
    pub fn close_live_udp<const P:usize,const Q:usize>(
        &mut self,sockets:&mut EpochUdpEndpoints<P,Q>,
        owner:Owner,port:u16
    )->Result<(),RetirementError>{
        self.require_active(owner)?;
        if !sockets.matches_registry(&self.udp){
            return Err(RetirementError::Sockets(SocketError::Denied));
        }
        let mut staged=self.clone();
        let mut staged_sockets=sockets.clone();
        staged.udp.unbind(owner.pid,owner.generation,port)
            .map_err(RetirementError::Udp)?;
        staged_sockets.close(owner.pid,owner.generation,port)
            .map_err(RetirementError::Sockets)?;
        if !staged_sockets.matches_registry(&staged.udp){
            return Err(RetirementError::Sockets(SocketError::Denied));
        }
        *self=staged; *sockets=staged_sockets;
        Ok(())
    }

    /// R120 ring-3 syscall: sender port must belong to the exact live epoch,
    /// with matching capability generation and registered endpoint state.
    pub fn send_live_udp<const P:usize,const Q:usize>(
        &self,sockets:&mut EpochUdpEndpoints<P,Q>,owner:Owner,
        source:u16,destination:u16,data:&[u8]
    )->Result<(),RetirementError>{
        self.require_active(owner)?;
        if !sockets.matches_registry(&self.udp){
            return Err(RetirementError::Sockets(SocketError::Denied));
        }
        self.udp.authorize(owner.pid,owner.generation,source)
            .map_err(RetirementError::Udp)?;
        sockets.send_local(owner.pid,owner.generation,source,destination,data)
            .map_err(RetirementError::Sockets)
    }

    pub fn retire_live_exited_batch<const P:usize,const Q:usize>(
        &mut self,sockets:&mut EpochUdpEndpoints<P,Q>,owners:&[Owner]
    )->Result<RetirementReceipt,RetirementError>{
        self.retire_live_batch(sockets,owners,EventKind::Exited)
    }

    pub fn retire_live_faulted_batch<const P:usize,const Q:usize>(
        &mut self,sockets:&mut EpochUdpEndpoints<P,Q>,owners:&[Owner]
    )->Result<RetirementReceipt,RetirementError>{
        self.retire_live_batch(sockets,owners,EventKind::Faulted)
    }

    fn retire_live_batch<const P:usize,const Q:usize>(
        &mut self,sockets:&mut EpochUdpEndpoints<P,Q>,owners:&[Owner],
        kind:EventKind
    )->Result<RetirementReceipt,RetirementError>{
        if !sockets.matches_registry(&self.udp){
            return Err(RetirementError::Sockets(SocketError::Denied));
        }
        // Stage *all four* structures. A failed later owner, orphan grant,
        // stale port, or full event queue must preserve every live authority.
        let mut staged=self.clone();
        let mut staged_sockets=sockets.clone();
        let receipt=staged.retire_batch(owners,kind)?;
        for owner in owners{
            // Registry entries were staged as revoked above. This exact
            // generation check also purges the physical in-kernel UDP
            // endpoint table and its queued datagrams, never peer messages.
            staged_sockets.retire_exact_kernel(
                &mut staged.udp,owner.pid,owner.generation
            ).map_err(RetirementError::Sockets)?;
        }
        if !staged_sockets.matches_registry(&staged.udp){
            return Err(RetirementError::Sockets(SocketError::Denied));
        }
        *self=staged;
        *sockets=staged_sockets;
        Ok(receipt)
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

/// R120: integration regressions through the real in-kernel UDP endpoint
/// table (not the simulated policy registry alone). Host tests do not prove
/// physical NIC IRQ and IOMMU ownership.
#[cfg(test)]
mod r120_live_socket_integration_tests {
    use super::*;
    #[test]
    fn atomic_exit_clears_actual_socket_queue_but_preserves_peer_traffic() {
        let mut k=ProcessRetirement::<3,3,4>::new();
        let mut live=EpochUdpEndpoints::<3,5>::new();
        let a=k.start(7).unwrap();
        let peer=k.start(8).unwrap();
        k.bind_live_udp(&mut live,a,5001).unwrap();
        k.bind_live_udp(&mut live,peer,5002).unwrap();
        let ticket=k.grant(a,11,1).unwrap();
        assert_eq!(live.send_local(7,a.generation,5001,5002,b"peer"),Ok(()));
        assert_eq!(live.send_local(8,peer.generation,5002,5001,b"stale"),Ok(()));
        assert_eq!(k.retire_live_exited_batch(&mut live,&[a]),
            Ok(RetirementReceipt{
                capabilities_revoked:1,udp_ports_released:1,notices_enqueued:1
            }));
        assert!(k.authorize_ticket(a,ticket,11,1).is_err());
        assert_eq!(k.active_udp_ports(),1);
        assert_eq!(live.receive(a.pid,a.generation,5001,&mut [0u8;8]),
            Err(SocketError::Denied));
        let mut out=[0u8;8];
        assert_eq!(live.receive(peer.pid,peer.generation,5002,&mut out),Ok(4));
        assert_eq!(&out[..4],b"peer");
        let replacement=k.start(a.pid).unwrap();
        assert!(k.bind_live_udp(&mut live,a,6001).is_err());
        k.bind_live_udp(&mut live,replacement,5001).unwrap();
        assert_eq!(live.receive(7,replacement.generation,5001,&mut out),
            Err(SocketError::Empty));
        assert_eq!(k.queued_notices(),1);
    }

    #[test]
    fn second_stale_owner_or_event_full_cannot_retire_first_live_socket() {
        let mut k=ProcessRetirement::<3,3,1>::new();
        let mut live=EpochUdpEndpoints::<3,4>::new();
        let a=k.start(1).unwrap();let b=k.start(2).unwrap();
        k.bind_live_udp(&mut live,a,5501).unwrap();
        k.bind_live_udp(&mut live,b,5502).unwrap();
        let stale=Owner{pid:b.pid,generation:b.generation+1};
        assert_eq!(k.retire_live_exited_batch(&mut live,&[a,stale]),
            Err(RetirementError::Capabilities(EpochError::NotActive)));
        assert_eq!(k.authorize_udp(a,5501),Ok(()));
        assert!(live.matches_registry(&k.udp));
        k.events.enqueue_exit_once(99,1).unwrap();
        assert_eq!(k.retire_live_exited_batch(&mut live,&[a]),
            Err(RetirementError::Events(EventError::Full)));
        assert_eq!(k.authorize_udp(a,5501),Ok(()));
        assert_eq!(live.send_local(a.pid,a.generation,5501,5502,b"alive"),Ok(()));
    }

    #[test]
    fn direct_socket_divergence_is_denied_before_any_policy_mutation() {
        let mut k=ProcessRetirement::<3,3,3>::new();
        let mut live=EpochUdpEndpoints::<3,3>::new();
        let a=k.start(1).unwrap();
        k.bind_live_udp(&mut live,a,7001).unwrap();
        // Simulate a mutation bypassing the paired authorized APIs.
        live.bind(a.pid,a.generation,7002).unwrap();
        assert!(!live.matches_registry(&k.udp));
        assert_eq!(k.retire_live_faulted_batch(&mut live,&[a]),
            Err(RetirementError::Sockets(SocketError::Denied)));
        assert_eq!(k.authorize_udp(a,7001),Ok(()));
        assert_eq!(k.active_generation(a.pid),Some(a.generation));
    }
    #[test]
    fn live_fault_retirement_releases_ports_and_reports_fault_generation() {
        let mut k=ProcessRetirement::<2,2,2>::new();
        let mut live=EpochUdpEndpoints::<2,4>::new();
        let a=k.start(1).unwrap();
        k.bind_live_udp(&mut live,a,7001).unwrap();
        assert_eq!(k.retire_live_faulted_batch(&mut live,&[a]),
            Ok(RetirementReceipt{
                capabilities_revoked:0,udp_ports_released:1,notices_enqueued:1
            }));
        let event=k.peek_notice().unwrap();
        assert_eq!(event.event.kind,EventKind::Faulted);
        assert_eq!(event.event.value,a.generation);
        assert_eq!(k.acknowledge_notice(event),Some(event.event));
        assert!(k.authorize_udp(a,7001).is_err());
        assert!(live.matches_registry(&k.udp));
    }
}

#[cfg(test)]
mod r120_polled_ingress_epoch_tests {
    use super::*;

    #[test]
    fn stale_epoch_is_denied_before_frame_parse_or_queue_mutation(){
        let mut k=ProcessRetirement::<2,2,3>::new();
        let mut s=EpochUdpEndpoints::<2,3>::new();
        let old=k.start(1).unwrap();
        k.bind_live_udp(&mut s,old,6001).unwrap();
        let stale=Owner{pid:old.pid,generation:old.generation+1};
        let mut dest=[0x55u8;8];
        assert_eq!(k.ingest_live_udp(&mut s,stale,6001,&[0u8;2],[0;6]),
            Err(RetirementError::Capabilities(EpochError::NotActive)));
        assert_eq!(k.receive_live_udp(&mut s,stale,6001,&mut dest),
            Err(RetirementError::Capabilities(EpochError::NotActive)));
        assert_eq!(dest,[0x55;8]);
        assert_eq!(s.pending_owned(old.pid,old.generation),Ok(0));
    }

    #[test]
    fn retired_udp_owner_cannot_reenter_network_queue_after_pid_recycle(){
        let mut k=ProcessRetirement::<2,2,3>::new();
        let mut s=EpochUdpEndpoints::<2,3>::new();
        let first=k.start(1).unwrap();
        k.bind_live_udp(&mut s,first,6001).unwrap();
        k.retire_live_exited_batch(&mut s,&[first]).unwrap();
        assert_eq!(k.ingest_live_udp(&mut s,first,6001,&[],[0;6]),
            Err(RetirementError::Capabilities(EpochError::NotActive)));
        let second=k.start(1).unwrap();
        k.bind_live_udp(&mut s,second,6001).unwrap();
        assert_eq!(k.ingest_live_udp(&mut s,first,6001,&[],[0;6]),
            Err(RetirementError::Capabilities(EpochError::NotActive)));
        assert!(matches!(
            k.ingest_live_udp(&mut s,second,6001,&[],[0;6]),
            Err(RetirementError::Ingress(IngressError::Malformed))
        ));
        assert_eq!(s.pending_owned(second.pid,second.generation),Ok(0));
        assert!(s.matches_registry(&k.udp));
    }
}

/// R120: real-socket lifetime and generation closure regression.
#[cfg(test)]
mod r120_scheduler_network_boundary_tests{
    use super::*;
    #[test]fn global_file_generation_can_drive_scheduler_socket_lifecycle(){
        let mut k=ProcessRetirement::<2,2,4>::new();
        let mut s=EpochUdpEndpoints::<2,4>::new();
        let a=Owner{pid:1,generation:1};
        let b=Owner{pid:2,generation:2};
        k.start_exact(a).unwrap();k.start_exact(b).unwrap();
        k.bind_live_udp(&mut s,a,5501).unwrap();
        k.bind_live_udp(&mut s,b,5502).unwrap();
        assert_eq!(k.send_live_udp(&mut s,a,5501,5502,b"ping"),Ok(()));
        let mut out=[0u8;8];
        assert_eq!(k.receive_live_udp(&mut s,b,5502,&mut out),Ok(4));
        assert_eq!(&out[..4],b"ping");
        assert_eq!(k.send_live_udp(&mut s,b,5501,5502,b"forge"),
            Err(RetirementError::Udp(UdpError::Denied)));
        assert_eq!(k.close_live_udp(&mut s,a,5501),Ok(()));
        assert!(k.authorize_udp(a,5501).is_err());
        assert!(s.matches_registry(&k.udp));
        assert_eq!(k.retire_live_faulted_batch(&mut s,&[b]).unwrap().notices_enqueued,1);
        assert_eq!(k.peek_notice().unwrap().event.kind,EventKind::Faulted);
        let next=Owner{pid:2,generation:4};
        k.start_exact(next).unwrap();
        assert_eq!(k.close_live_udp(&mut s,b,5502),
            Err(RetirementError::Capabilities(EpochError::NotActive)));
        k.bind_live_udp(&mut s,next,5502).unwrap();
        assert_eq!(s.pending_owned(next.pid,next.generation),Ok(0));
    }

    #[test]fn stale_or_foreign_close_cannot_discard_peers_pending_packet(){
        let mut k=ProcessRetirement::<2,2,4>::new();
        let mut s=EpochUdpEndpoints::<2,4>::new();
        let a=k.start(1).unwrap();let b=k.start(2).unwrap();
        k.bind_live_udp(&mut s,a,6001).unwrap();
        k.bind_live_udp(&mut s,b,6002).unwrap();
        k.send_live_udp(&mut s,a,6001,6002,b"still here").unwrap();
        assert!(k.close_live_udp(&mut s,a,6002).is_err());
        assert_eq!(s.pending_owned(b.pid,b.generation),Ok(1));
        assert!(k.close_live_udp(&mut s,Owner{pid:2,generation:2},6002).is_err());
        let mut out=[0u8;16];
        assert_eq!(k.receive_live_udp(&mut s,b,6002,&mut out),Ok(10));
        assert_eq!(&out[..10],b"still here");
        assert!(s.matches_registry(&k.udp));
    }
}
