//! Fixed-capacity, allocation-free kernel capability registry.
//!
//! Callers must obtain the owner PID from authenticated scheduler state; a
//! user-supplied PID is not sufficient proof of ownership. Tokens are
//! monotonically issued and never reused, but are not cryptographic secrets.
//! Kernel code must serialize access; this type is intentionally not atomic.

pub const MAX_CAPABILITIES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityError {
    InvalidOwner,
    InvalidToken,
    InvalidRights,
    UnknownToken,
    Denied,
    Full,
    TokenExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistryEntry {
    owner: u32,
    token: u64,
    resource: u64,
    rights: u32,
}

impl RegistryEntry {
    pub fn owner(&self) -> u32 { self.owner }
    pub fn token(&self) -> u64 { self.token }
    pub fn resource(&self) -> u64 { self.resource }
    pub fn rights(&self) -> u32 { self.rights }
}

/// No heap, no global state, and no token reuse, including after revocation.
/// Each Registry instance is owned and synchronized by its kernel caller.
pub struct Registry<const CAPACITY: usize> {
    entries: [Option<RegistryEntry>; CAPACITY],
    next_token: u64,
}

impl<const CAPACITY: usize> Default for Registry<CAPACITY> {
    fn default() -> Self { Self::new() }
}

impl<const CAPACITY: usize> Registry<CAPACITY> {
    pub const fn new() -> Self {
        Self { entries: [None; CAPACITY], next_token: 1 }
    }

    pub fn capacity(&self) -> usize { CAPACITY }

    pub fn len(&self) -> usize {
        self.entries.iter().filter(|entry| entry.is_some()).count()
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }

    /// The caller authenticates owner against the current process context.
    /// This method issues a fresh token and never reuses revoked tokens.
    pub fn grant(
        &mut self, owner: u32, resource: u64, rights: u32,
    ) -> Result<u64, CapabilityError> {
        if owner == 0 { return Err(CapabilityError::InvalidOwner); }
        if rights == 0 { return Err(CapabilityError::InvalidRights); }
        if self.next_token == 0 { return Err(CapabilityError::TokenExhausted); }
        let slot = self.entries.iter_mut().find(|entry| entry.is_none())
            .ok_or(CapabilityError::Full)?;
        let token = self.next_token;
        // On overflow, permanently stop issuing tokens rather than reuse one.
        self.next_token = self.next_token.checked_add(1).unwrap_or(0);
        *slot = Some(RegistryEntry { owner, token, resource, rights });
        Ok(token)
    }

    /// Trusted kernel lookup. Untrusted requests must use authorize().
    pub fn lookup(&self, token: u64) -> Option<RegistryEntry> {
        if token == 0 { return None; }
        self.entries.iter().flatten().copied()
            .find(|entry| entry.token == token)
    }

    /// All requested rights must be present; no zero-rights authorization.
    pub fn authorize(
        &self, owner: u32, token: u64, resource: u64, requested_rights: u32,
    ) -> Result<(), CapabilityError> {
        if owner == 0 { return Err(CapabilityError::InvalidOwner); }
        if token == 0 { return Err(CapabilityError::InvalidToken); }
        if requested_rights == 0 { return Err(CapabilityError::InvalidRights); }
        let entry = self.lookup(token).ok_or(CapabilityError::UnknownToken)?;
        if entry.owner != owner || entry.resource != resource ||
           (entry.rights & requested_rights) != requested_rights {
            return Err(CapabilityError::Denied);
        }
        Ok(())
    }

    pub fn revoke(&mut self, owner: u32, token: u64) -> Result<(), CapabilityError> {
        if owner == 0 { return Err(CapabilityError::InvalidOwner); }
        if token == 0 { return Err(CapabilityError::InvalidToken); }
        let slot = self.entries.iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|entry| entry.token == token))
            .ok_or(CapabilityError::UnknownToken)?;
        if slot.as_ref().is_some_and(|entry| entry.owner != owner) {
            return Err(CapabilityError::Denied);
        }
        *slot = None;
        Ok(())
    }

    /// Drop all capabilities for a terminated process without touching peers.
    pub fn revoke_all(&mut self, owner: u32) -> usize {
        if owner == 0 { return 0; }
        let mut revoked = 0;
        for slot in &mut self.entries {
            if slot.as_ref().is_some_and(|entry| entry.owner == owner) {
                *slot = None;
                revoked += 1;
            }
        }
        revoked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_lookup_and_all_rights_required() {
        let mut registry = Registry::<4>::new();
        let token = registry.grant(10, 500, 0b1011).unwrap();
        assert_ne!(token, 0);
        assert_eq!(registry.lookup(token).unwrap().owner(), 10);
        assert_eq!(registry.lookup(token).unwrap().resource(), 500);
        assert_eq!(registry.lookup(token).unwrap().rights(), 0b1011);
        assert_eq!(registry.lookup(token).unwrap().token(), token);
        assert_eq!(registry.authorize(10, token, 500, 0b0011), Ok(()));
        assert_eq!(registry.authorize(10, token, 500, 0b0100), Err(CapabilityError::Denied));
        assert_eq!(registry.authorize(11, token, 500, 0b0001), Err(CapabilityError::Denied));
        assert_eq!(registry.authorize(10, token, 501, 0b0001), Err(CapabilityError::Denied));
        assert_eq!(registry.authorize(10, token, 500, 0), Err(CapabilityError::InvalidRights));
    }

    #[test]
    fn invalid_tokens_owners_and_revocation() {
        let mut registry = Registry::<2>::new();
        assert_eq!(registry.grant(0, 1, 1), Err(CapabilityError::InvalidOwner));
        assert_eq!(registry.grant(1, 1, 0), Err(CapabilityError::InvalidRights));
        let first = registry.grant(1, 1, 0b11).unwrap();
        assert_eq!(registry.authorize(1, 0, 1, 1), Err(CapabilityError::InvalidToken));
        assert_eq!(registry.revoke(2, first), Err(CapabilityError::Denied));
        assert_eq!(registry.revoke(1, first), Ok(()));
        assert_eq!(registry.lookup(first), None);
        assert_eq!(registry.authorize(1, first, 1, 1), Err(CapabilityError::UnknownToken));
        assert_eq!(registry.revoke(1, first), Err(CapabilityError::UnknownToken));
        let second = registry.grant(1, 1, 1).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn capacity_and_pid_isolation() {
        let mut registry = Registry::<3>::new();
        let a = registry.grant(1, 11, 1).unwrap();
        registry.grant(1, 12, 2).unwrap();
        let b = registry.grant(2, 13, 3).unwrap();
        assert_eq!(registry.len(), 3);
        assert_eq!(registry.grant(3, 14, 1), Err(CapabilityError::Full));
        assert_eq!(registry.revoke_all(1), 2);
        assert_eq!(registry.revoke_all(1), 0);
        assert_eq!(registry.lookup(a), None);
        assert_eq!(registry.authorize(2, b, 13, 3), Ok(()));
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.capacity(), 3);
        assert!(!registry.is_empty());
        assert_eq!(registry.revoke_all(2), 1);
        assert!(registry.is_empty());
    }

    #[test]
    fn zero_capacity_and_token_overflow_fail_closed() {
        let mut empty = Registry::<0>::new();
        assert_eq!(empty.grant(1, 1, 1), Err(CapabilityError::Full));
        let mut registry = Registry::<1>::new();
        registry.next_token = u64::MAX;
        assert_eq!(registry.grant(1, 1, 1), Ok(u64::MAX));
        registry.revoke(1, u64::MAX).unwrap();
        assert_eq!(registry.grant(1, 1, 1), Err(CapabilityError::TokenExhausted));
    }
}


/// Generation-bound capability ticket for a single authenticated process
/// incarnation. This prepares IPC restart checks; the scheduler must provide
/// actual PID/generation, not syscall arguments from userspace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EpochTicket { pub token: u64, pub pid: u32, pub generation: u64 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EpochError {
    InvalidPid, AlreadyActive, NotActive, Full, Exhausted, Denied, Registry(CapabilityError),
}
#[derive(Clone, Copy)]
struct ProcessEpoch { pid: u32, generation: u64, active: bool }

pub struct EpochRegistry<const N: usize> {
    epochs: [Option<ProcessEpoch>; N],
    grants: Registry<N>,
}
impl<const N: usize> EpochRegistry<N> {
    pub const fn new() -> Self {
        Self { epochs: [None; N], grants: Registry::new() }
    }
    fn process(&self, pid: u32) -> Option<ProcessEpoch> {
        self.epochs.iter().flatten().copied().find(|entry| entry.pid == pid)
    }
    pub fn start(&mut self, pid: u32) -> Result<u64, EpochError> {
        if pid == 0 { return Err(EpochError::InvalidPid); }
        if let Some(entry) = self.epochs.iter_mut().flatten().find(|e| e.pid == pid) {
            if entry.active { return Err(EpochError::AlreadyActive); }
            entry.generation = entry.generation.checked_add(1).ok_or(EpochError::Exhausted)?;
            entry.active = true;
            return Ok(entry.generation);
        }
        let slot = self.epochs.iter_mut().find(|e| e.is_none()).ok_or(EpochError::Full)?;
        *slot = Some(ProcessEpoch { pid, generation: 1, active: true });
        Ok(1)
    }

    pub fn grant(&mut self, pid: u32, resource: u64, rights: u32)
        -> Result<EpochTicket, EpochError>
    {
        let epoch = self.process(pid).filter(|e| e.active).ok_or(EpochError::NotActive)?;
        let token = self.grants.grant(pid, resource, rights).map_err(EpochError::Registry)?;
        Ok(EpochTicket { token, pid, generation: epoch.generation })
    }

    pub fn authorize(&self, authenticated_pid: u32, authenticated_generation: u64,
                     ticket: EpochTicket, resource: u64, rights: u32)
        -> Result<(), EpochError>
    {
        let epoch = self.process(authenticated_pid).ok_or(EpochError::NotActive)?;
        if !epoch.active || epoch.generation != authenticated_generation {
            return Err(EpochError::NotActive);
        }
        if ticket.pid != authenticated_pid || ticket.generation != epoch.generation {
            return Err(EpochError::Denied);
        }
        self.grants.authorize(authenticated_pid, ticket.token, resource, rights)
            .map_err(EpochError::Registry)
    }

    /// Process fault/exit: revoke all tickets *before* any PID can restart.
    pub fn retire(&mut self, authenticated_pid: u32, authenticated_generation: u64)
        -> Result<usize, EpochError>
    {
        let entry = self.epochs.iter_mut().flatten()
            .find(|e| e.pid == authenticated_pid && e.active &&
                e.generation == authenticated_generation)
            .ok_or(EpochError::NotActive)?;
        entry.active = false;
        Ok(self.grants.revoke_all(authenticated_pid))
    }
}
#[cfg(test)]
mod generation_tests {
    use super::*;
    #[test]
    fn pid_reuse_invalidates_original_grants() {
        let mut r = EpochRegistry::<4>::new();
        let gen1 = r.start(10).unwrap();
        let old = r.grant(10, 500, 3).unwrap();
        assert_eq!(r.authorize(10, gen1, old, 500, 1), Ok(()));
        assert_eq!(r.retire(10, gen1), Ok(1));
        let gen2 = r.start(10).unwrap();
        assert_ne!(gen1, gen2);
        assert_eq!(r.authorize(10, gen2, old, 500, 1), Err(EpochError::Denied));
        let fresh = r.grant(10, 500, 1).unwrap();
        assert_ne!(old.token, fresh.token);
        assert_eq!(r.authorize(10, gen2, fresh, 500, 1), Ok(()));
    }
    #[test]
    fn wrong_pid_or_generation_cannot_spoof_owner() {
        let mut r = EpochRegistry::<3>::new();
        r.start(4).unwrap(); r.start(5).unwrap();
        let token = r.grant(4, 90, 1).unwrap();
        assert_eq!(r.authorize(5, 1, token, 90, 1), Err(EpochError::Denied));
        assert_eq!(r.authorize(4, 99, token, 90, 1), Err(EpochError::NotActive));
        assert_eq!(r.authorize(4, 1, token, 91, 1),
            Err(EpochError::Registry(CapabilityError::Denied)));
    }
    #[test]
    fn retirement_is_exact_and_does_not_clear_peer_grants() {
        let mut r = EpochRegistry::<4>::new();
        r.start(1).unwrap(); r.start(2).unwrap();
        let a = r.grant(1, 1, 1).unwrap();
        let b = r.grant(2, 2, 1).unwrap();
        assert_eq!(r.retire(1, 99), Err(EpochError::NotActive));
        assert_eq!(r.retire(1, 1), Ok(1));
        assert_eq!(r.retire(1, 1), Err(EpochError::NotActive));
        assert_eq!(r.authorize(1, 1, a, 1, 1), Err(EpochError::NotActive));
        assert_eq!(r.authorize(2, 1, b, 2, 1), Ok(()));
    }
    #[test]
    fn zero_pid_duplicate_and_capacity_are_denied() {
        let mut r = EpochRegistry::<1>::new();
        assert_eq!(r.start(0), Err(EpochError::InvalidPid));
        assert_eq!(r.start(1), Ok(1));
        assert_eq!(r.start(1), Err(EpochError::AlreadyActive));
        assert_eq!(r.start(2), Err(EpochError::Full));
    }
}
