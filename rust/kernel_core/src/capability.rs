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
