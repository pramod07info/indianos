// Agent X generated, safety-gated module: udp_owner_epoch
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error { Invalid, Duplicate, Denied, Full }

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Grant { pub pid:u32, pub generation:u64, pub port:u16 }

/// Bounded UDP port ownership proposal; not yet wired to live sockets.
/// PID+generation must come from authenticated kernel process state.
pub struct Registry<const N:usize> { entries:[Option<Grant>;N] }
impl<const N:usize> Registry<N> {
    pub const fn new()->Self { Self { entries:[None;N] } }

    pub fn bind(&mut self,pid:u32,generation:u64,port:u16)
        ->Result<(),Error> {
        if pid==0||generation==0||port==0 { return Err(Error::Invalid); }
        for item in self.entries.iter().flatten() {
            if item.port==port { return Err(Error::Duplicate); }
            // A reused PID cannot bind until old generation is revoked.
            if item.pid==pid && item.generation!=generation {
                return Err(Error::Denied);
            }
        }
        let slot=self.entries.iter_mut().find(|slot|slot.is_none())
            .ok_or(Error::Full)?;
        *slot=Some(Grant{pid,generation,port});
        Ok(())
    }

    pub fn authorize(&self,pid:u32,generation:u64,port:u16)
        ->Result<(),Error> {
        if pid==0||generation==0||port==0 { return Err(Error::Invalid); }
        if self.entries.iter().flatten().any(|item|
            item.pid==pid && item.generation==generation &&
            item.port==port) { Ok(()) } else { Err(Error::Denied) }
    }

    pub fn revoke_owner(&mut self,pid:u32,generation:u64)
        ->Result<usize,Error> {
        if pid==0||generation==0 { return Err(Error::Invalid); }
        let mut revoked=0usize;
        for slot in &mut self.entries {
            if slot.as_ref().is_some_and(|item|
                item.pid==pid && item.generation==generation) {
                *slot=None;
                revoked+=1;
            }
        }
        Ok(revoked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binds_and_authorizes_only_current_generation() {
        let mut ports=Registry::<2>::new();
        assert_eq!(ports.bind(7,1,5353),Ok(()));
        assert_eq!(ports.authorize(7,1,5353),Ok(()));
        assert_eq!(ports.authorize(7,2,5353),Err(Error::Denied));
        assert_eq!(ports.authorize(8,1,5353),Err(Error::Denied));
        assert_eq!(ports.bind(8,1,5353),Err(Error::Duplicate));
        assert_eq!(ports.bind(7,2,6000),Err(Error::Denied));
        assert_eq!(ports.revoke_owner(7,1),Ok(1));
        assert_eq!(ports.bind(7,2,6000),Ok(()));
        assert_eq!(ports.authorize(7,1,5353),Err(Error::Denied));
    }
    #[test]
    fn refuses_invalid_owners_ports_and_capacity_exhaustion() {
        let mut ports=Registry::<1>::new();
        assert_eq!(ports.bind(0,1,53),Err(Error::Invalid));
        assert_eq!(ports.bind(1,0,53),Err(Error::Invalid));
        assert_eq!(ports.bind(1,1,0),Err(Error::Invalid));
        assert_eq!(ports.bind(1,1,53),Ok(()));
        assert_eq!(ports.bind(2,1,54),Err(Error::Full));
        assert_eq!(ports.revoke_owner(2,1),Ok(0));
        assert_eq!(ports.revoke_owner(1,1),Ok(1));
        assert_eq!(ports.bind(2,1,54),Ok(()));
    }
}
