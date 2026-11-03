// Agent X generated, safety-gated module: udp_owner_epoch
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error { Invalid, Duplicate, Denied, Full }

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Grant { pub pid:u32, pub generation:u64, pub port:u16 }

/// Bounded UDP port ownership proposal; not yet wired to live sockets.
/// PID+generation must come from authenticated kernel process state.
#[derive(Clone)]
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

    /// Read-only exact-generation collision proof for a trusted teardown
    /// coordinator. A stale task must never retire its successor's ports.
    pub fn has_foreign_generation(&self,pid:u32,generation:u64)->bool{
        self.entries.iter().flatten().any(|g|
            g.pid==pid && g.generation!=generation)
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


// R114: transactional owner operations. This remains a proposed policy
// registry; live sockets must authenticate and serialize callers.
impl<const N:usize> Registry<N> {
    pub fn active_count(&self) -> usize {
        self.entries.iter().flatten().count()
    }
    pub fn owner_of(&self, port:u16) -> Option<Grant> {
        if port==0 { return None; }
        self.entries.iter().flatten().find(|grant|grant.port==port).copied()
    }
    /// Roll back the entire group if any member is stale, collides, or full.
    pub fn bind_batch(&mut self, grants:&[Grant]) -> Result<usize,Error> {
        let mut staged=Self { entries:self.entries };
        for &g in grants {
            staged.bind(g.pid,g.generation,g.port)?;
        }
        self.entries=staged.entries;
        Ok(grants.len())
    }
    /// Exact-owner release; wrong epochs cannot unbind another process.
    pub fn unbind(&mut self, pid:u32, generation:u64, port:u16)
        -> Result<(),Error>
    {
        if pid==0||generation==0||port==0{return Err(Error::Invalid);}
        let slot=self.entries.iter_mut().find(|slot|
            slot.as_ref().is_some_and(|g|
                g.pid==pid&&g.generation==generation&&g.port==port))
            .ok_or(Error::Denied)?;
        *slot=None;
        Ok(())
    }
    /// Atomic move of a grant, retaining the old port on any failure.
    pub fn move_port(&mut self,pid:u32,generation:u64,
        old_port:u16,new_port:u16)->Result<(),Error>
    {
        if pid==0||generation==0||old_port==0||new_port==0 {
            return Err(Error::Invalid);
        }
        if old_port==new_port { return self.authorize(pid,generation,old_port); }
        let mut staged=Self { entries:self.entries };
        staged.unbind(pid,generation,old_port)?;
        staged.bind(pid,generation,new_port)?;
        self.entries=staged.entries;
        Ok(())
    }
}

#[cfg(test)]
mod r114_udp_epoch_tests {
    use super::*;
    fn grant(pid:u32, generation:u64, port:u16)->Grant{
        Grant{pid,generation,port}
    }
    #[test]
    fn batch_admission_never_partially_binds() {
        let mut ports=Registry::<3>::new();
        ports.bind(1,1,10).unwrap();
        assert_eq!(ports.bind_batch(&[grant(2,1,20),grant(3,1,10)]),
            Err(Error::Duplicate));
        assert_eq!(ports.active_count(),1);
        assert_eq!(ports.owner_of(20),None);
        assert_eq!(ports.bind_batch(&[grant(2,1,20),grant(3,1,30)]),Ok(2));
        assert_eq!(ports.active_count(),3);
        assert_eq!(ports.bind_batch(&[grant(4,1,40)]),Err(Error::Full));
    }
    #[test]
    fn wrong_generation_never_releases_or_moves_a_port() {
        let mut ports=Registry::<2>::new();
        ports.bind(7,11,2500).unwrap();
        assert_eq!(ports.unbind(7,12,2500),Err(Error::Denied));
        assert_eq!(ports.move_port(7,12,2500,2600),Err(Error::Denied));
        assert_eq!(ports.authorize(7,11,2500),Ok(()));
        assert_eq!(ports.unbind(7,11,2500),Ok(()));
        assert_eq!(ports.authorize(7,11,2500),Err(Error::Denied));
    }
    #[test]
    fn failed_move_preserves_both_owners() {
        let mut ports=Registry::<2>::new();
        ports.bind(7,1,500).unwrap();
        ports.bind(8,1,600).unwrap();
        assert_eq!(ports.move_port(7,1,500,600),Err(Error::Duplicate));
        assert_eq!(ports.owner_of(500),Some(grant(7,1,500)));
        assert_eq!(ports.owner_of(600),Some(grant(8,1,600)));
        assert_eq!(ports.move_port(7,1,500,500),Ok(()));
        ports.unbind(8,1,600).unwrap();
        assert_eq!(ports.move_port(7,1,500,600),Ok(()));
        assert_eq!(ports.owner_of(500),None);
        assert_eq!(ports.owner_of(600),Some(grant(7,1,600)));
    }
    #[test]
    fn zero_capacity_and_invalid_batch_fail_closed() {
        let mut empty=Registry::<0>::new();
        assert_eq!(empty.bind_batch(&[]),Ok(0));
        assert_eq!(empty.bind_batch(&[grant(1,1,1)]),Err(Error::Full));
        assert_eq!(empty.active_count(),0);
        let mut one=Registry::<1>::new();
        assert_eq!(one.bind_batch(&[grant(1,0,1)]),Err(Error::Invalid));
        assert_eq!(one.owner_of(0),None);
        assert_eq!(one.move_port(0,1,1,2),Err(Error::Invalid));
    }
}



/// R115: exact-owner, all-or-nothing UDP grant withdrawal. The scheduler must
/// authenticate each PID/generation; this is a policy-table operation, not a
/// network-device shutdown.
impl<const N: usize> Registry<N> {
    pub fn unbind_batch(&mut self, grants: &[Grant]) -> Result<usize, Error> {
        let mut candidate = Self { entries: self.entries };
        for (i, grant) in grants.iter().enumerate() {
            if grant.pid == 0 || grant.generation == 0 || grant.port == 0 {
                return Err(Error::Invalid);
            }
            if grants[..i].iter().any(|prior| prior.port == grant.port) {
                return Err(Error::Duplicate);
            }
            candidate.unbind(grant.pid, grant.generation, grant.port)?;
        }
        self.entries = candidate.entries;
        Ok(grants.len())
    }
}

#[cfg(test)]
mod r115_atomic_udp_release_tests {
    use super::*;
    fn g(pid: u32, generation: u64, port: u16) -> Grant {
        Grant { pid, generation, port }
    }

    #[test]
    fn exact_multi_owner_release_is_atomic_and_preserves_peer() {
        let mut p = Registry::<4>::new();
        for (pid, port) in [(1, 6001), (1, 6002), (2, 6003), (3, 6004)] {
            p.bind(pid, 7, port).unwrap();
        }
        assert_eq!(p.unbind_batch(&[g(1, 7, 6001), g(2, 7, 6003)]),
            Ok(2));
        assert_eq!(p.active_count(), 2);
        assert_eq!(p.authorize(1, 7, 6001), Err(Error::Denied));
        assert_eq!(p.authorize(2, 7, 6003), Err(Error::Denied));
        assert_eq!(p.authorize(1, 7, 6002), Ok(()));
        assert_eq!(p.authorize(3, 7, 6004), Ok(()));
    }

    #[test]
    fn stale_or_forged_owner_cannot_partially_withdraw() {
        let mut p = Registry::<3>::new();
        p.bind(1, 7, 6001).unwrap();
        p.bind(2, 9, 6002).unwrap();
        for bad in [g(2, 8, 6002), g(7, 9, 6002), g(2, 9, 65535)] {
            assert_eq!(p.unbind_batch(&[g(1, 7, 6001), bad]),
                Err(Error::Denied));
            assert_eq!(p.authorize(1, 7, 6001), Ok(()));
            assert_eq!(p.authorize(2, 9, 6002), Ok(()));
        }
    }

    #[test]
    fn duplicates_and_invalid_grants_preserve_existing_authority() {
        let mut p = Registry::<2>::new();
        p.bind(1, 1, 88).unwrap();
        for invalid in [g(0, 1, 90), g(2, 0, 90), g(2, 1, 0)] {
            assert_eq!(p.unbind_batch(&[g(1, 1, 88), invalid]),
                Err(Error::Invalid));
            assert_eq!(p.authorize(1, 1, 88), Ok(()));
        }
        assert_eq!(p.unbind_batch(&[g(1, 1, 88), g(1, 1, 88)]),
            Err(Error::Duplicate));
        assert_eq!(p.authorize(1, 1, 88), Ok(()));
    }

    #[test]
    fn empty_and_zero_capacity_are_safe() {
        let mut empty = Registry::<0>::new();
        assert_eq!(empty.unbind_batch(&[]), Ok(0));
        assert_eq!(empty.unbind_batch(&[g(1, 1, 1)]), Err(Error::Denied));
        assert_eq!(empty.active_count(), 0);
        let mut one = Registry::<1>::new();
        one.bind(1, 1, 1).unwrap();
        assert_eq!(one.unbind_batch(&[]), Ok(0));
        assert_eq!(one.active_count(), 1);
    }
}

#[cfg(test)]
mod r119_inherited_port_generation_regressions{
 use super::*;
 #[test]
 fn delayed_retirement_cannot_revoke_new_generation_owner(){
  let mut r=Registry::<3>::new();
  r.bind(2,41,5300).unwrap();
  assert!(r.has_foreign_generation(2,40));
  assert!(!r.has_foreign_generation(2,41));
  assert_eq!(r.revoke_owner(2,40),Ok(0));
  assert_eq!(r.authorize(2,41,5300),Ok(()));
  r.revoke_owner(2,41).unwrap();
  r.bind(2,42,5300).unwrap();
  assert_eq!(r.authorize(2,41,5300),Err(Error::Denied));
  assert!(r.has_foreign_generation(2,41));
  assert_eq!(r.revoke_owner(2,41),Ok(0));
  assert_eq!(r.authorize(2,42,5300),Ok(()));
 }
}

/// R119: atomic retirement of generation-authenticated UDP registry owners.
/// The live socket table must be retired by the same kernel supervisor;
/// this helper alone is not an IRQ or physical NIC safety proof.
impl<const N:usize> Registry<N>{
 pub fn revoke_owners_batch(&mut self,owners:&[(u32,u64)])
     ->Result<usize,Error>{
  let mut candidate=Self{entries:self.entries};
  for (i,&(pid,generation)) in owners.iter().enumerate(){
   if pid==0||generation==0{return Err(Error::Invalid);}
   if owners[..i].iter().any(|&(previous,_)|previous==pid){
    return Err(Error::Duplicate);
   }
   if candidate.has_foreign_generation(pid,generation){
    return Err(Error::Denied);
   }
   candidate.revoke_owner(pid,generation)?;
  }
  let removed=self.active_count()-candidate.active_count();
  self.entries=candidate.entries;
  Ok(removed)
 }
}
#[cfg(test)]
mod r119_udp_owner_batch_tests{
 use super::*;
 #[test]fn exact_multi_owner_retirement_keeps_peer(){
  let mut r=Registry::<4>::new();
  r.bind(1,9,5001).unwrap();r.bind(1,9,5002).unwrap();
  r.bind(2,9,5003).unwrap();r.bind(3,9,5004).unwrap();
  assert_eq!(r.revoke_owners_batch(&[(1,9),(2,9)]),Ok(3));
  assert_eq!(r.authorize(3,9,5004),Ok(()));
  assert_eq!(r.active_count(),1);
 }
 #[test]fn recycled_pid_cannot_revoke_valid_earlier_owner(){
  let mut r=Registry::<3>::new();
  r.bind(1,2,5001).unwrap();r.bind(2,20,5002).unwrap();
  assert_eq!(r.revoke_owners_batch(&[(1,2),(2,19)]),Err(Error::Denied));
  assert_eq!(r.authorize(1,2,5001),Ok(()));
  assert_eq!(r.authorize(2,20,5002),Ok(()));
 }
 #[test]fn invalid_and_duplicate_group_members_are_atomic(){
  let mut r=Registry::<2>::new();r.bind(1,2,55).unwrap();
  for bad in [(0,2),(1,0),(1,3)]{
   assert!(r.revoke_owners_batch(&[(1,2),bad]).is_err());
   assert_eq!(r.authorize(1,2,55),Ok(()));
  }
  assert_eq!(r.revoke_owners_batch(&[(1,2),(1,2)]),Err(Error::Duplicate));
  assert_eq!(r.authorize(1,2,55),Ok(()));
 }
 #[test]fn empty_and_zero_capacity_retirement_are_safe(){
  let mut zero=Registry::<0>::new();
  assert_eq!(zero.revoke_owners_batch(&[]),Ok(0));
  assert_eq!(zero.revoke_owners_batch(&[(1,2)]),Ok(0));
  let mut r=Registry::<1>::new();r.bind(1,4,70).unwrap();
  assert_eq!(r.revoke_owners_batch(&[]),Ok(0));
  assert_eq!(r.revoke_owners_batch(&[(1,4)]),Ok(1));
  assert_eq!(r.revoke_owners_batch(&[(1,4)]),Ok(0));
 }
}
