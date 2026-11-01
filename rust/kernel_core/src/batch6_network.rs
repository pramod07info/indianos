//! R040 task 14: owner-bound outbound port leases with deterministic
//! expiration and verified TLS callback. Does not implement sockets/TLS.
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,SOCKET};
use crate::batch3_network_security::{TlsCertificateVerifier,tls_peer_gate};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Full,Conflict,Denied,Expired,Unverified}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct PortLease{pub owner:Identity,pub port:u16,pub deadline:u64}
pub struct Ports<const N:usize>{leases:[Option<PortLease>;N]}
impl<const N:usize> Ports<N>{
 pub const fn new()->Self{Self{leases:[None;N]}}
 pub fn bind<const G:usize>(&mut self,auth:&Access<G>,owner:Identity,
  port:u16,now:u64,deadline:u64)->Result<(),Error>{
  auth.require(owner,SOCKET,now).map_err(|_|Error::Denied)?;
  if port<1024||deadline<=now{return Err(Error::Invalid);}
  if self.leases.iter().flatten().any(|s|s.port==port&&s.deadline>now){
   return Err(Error::Conflict);
  }
  let slot=self.leases.iter_mut().find(|s|s.is_none_or(|x|x.deadline<=now))
   .ok_or(Error::Full)?;
  *slot=Some(PortLease{owner,port,deadline});Ok(())
 }
 pub fn require(&self,owner:Identity,port:u16,now:u64)->Result<(),Error>{
  let lease=self.leases.iter().flatten().find(|x|x.port==port)
   .ok_or(Error::Denied)?;
  if lease.owner!=owner{return Err(Error::Denied);}
  if now>=lease.deadline{return Err(Error::Expired);}
  Ok(())
 }
 pub fn retire(&mut self,owner:Identity){
  for x in &mut self.leases{
   if x.is_some_and(|s|s.owner==owner){*x=None;}
  }
 }
}
pub fn verified_tls<V:TlsCertificateVerifier>(v:&V,
 name:&[u8],cert:&[u8])->Result<(),Error>{
 tls_peer_gate(v,name,cert).map_err(|_|Error::Unverified)
}
#[cfg(test)]mod tests{
 use super::*;use crate::batch6_security::SOCKET;
 #[test]fn ephemeral_socket_lease_cannot_cross_pid_epoch(){
  let a=Identity{pid:3,generation:1};
  let mut acl=Access::<1>::new();acl.grant(a,SOCKET,90).unwrap();
  let mut ports=Ports::<1>::new();ports.bind(&acl,a,8080,1,30).unwrap();
  assert_eq!(ports.bind(&acl,a,8080,2,40),Err(Error::Conflict));
  assert_eq!(ports.require(Identity{generation:2,..a},8080,2),Err(Error::Denied));
  assert_eq!(ports.require(a,8080,30),Err(Error::Expired));
  ports.bind(&acl,a,8080,31,60).unwrap();
  ports.retire(a);assert_eq!(ports.require(a,8080,32),Err(Error::Denied));
 }
}

/// Orbit R005 outbound lease lifecycle. This is not a TCP/IP or TLS
/// implementation: actual device transport and certificate verification
/// remain mandatory at the real network boundary.
impl<const N:usize> Ports<N>{
 /// Bind a set of ports as a transaction, preserving existing leases
 /// when any port is privileged, already held or exceeds capacity.
 pub fn bind_batch<const G:usize>(&mut self,auth:&Access<G>,
  owner:Identity,ports:&[u16],now:u64,deadline:u64)->Result<usize,Error>{
  if ports.is_empty()||ports.len()>N{return Err(Error::Invalid);}
  let mut staged=Self{leases:self.leases};
  for &port in ports{
   staged.bind(auth,owner,port,now,deadline)?;
  }
  self.leases=staged.leases;Ok(ports.len())
 }
 /// Release only an exact process incarnation's port; stale recycled
 /// PIDs cannot free another process's active network binding.
 pub fn release(&mut self,owner:Identity,port:u16)->Result<(),Error>{
  if owner.pid==0||owner.generation==0||port==0{return Err(Error::Invalid);}
  let item=self.leases.iter_mut().find(|item|
   item.is_some_and(|x|x.port==port)).ok_or(Error::Denied)?;
  if item.is_none_or(|x|x.owner!=owner){return Err(Error::Denied);}
  *item=None;Ok(())
 }
 /// Reclaim expired numeric leases; caller may retry a denied binding
 /// once the trusted monotonic clock has advanced.
 pub fn reap(&mut self,now:u64)->usize{
  let mut reclaimed=0;
  for item in &mut self.leases{
   if item.is_some_and(|x|now>=x.deadline){
    *item=None;reclaimed+=1;
   }
  }
  reclaimed
 }
 /// Copy only leases belonging to the authenticated process incarnation.
 /// Short buffers do not expose partial state.
 pub fn list_owner(&self,owner:Identity,now:u64,
  out:&mut[PortLease])->Result<usize,Error>{
  if owner.pid==0||owner.generation==0{return Err(Error::Invalid);}
  let count=self.leases.iter().flatten()
   .filter(|x|x.owner==owner&&x.deadline>now).count();
  if out.len()<count{return Err(Error::Full);}
  let mut pos=0;
  for entry in self.leases.iter().flatten(){
   if entry.owner==owner&&entry.deadline>now{
    out[pos]=*entry;pos+=1;
   }
  }
  Ok(pos)
 }
}
#[cfg(test)]
mod orbit_r005_port_tests{
 use super::*;
 fn owner()->Identity{Identity{pid:22,generation:9}}
 fn acl()->Access<1>{
  let mut a=Access::new();a.grant(owner(),SOCKET,100).unwrap();a
 }
 #[test]fn atomic_port_group_enforces_capacity_conflicts_and_scope(){
  let mut ports=Ports::<2>::new();
  assert_eq!(ports.bind_batch(&acl(),owner(),&[8080,8081],1,30),Ok(2));
  assert_eq!(ports.bind_batch(&acl(),owner(),&[8082],1,30),Err(Error::Full));
  assert_eq!(ports.release(Identity{generation:10,..owner()},8080),Err(Error::Denied));
  assert_eq!(ports.release(owner(),8080),Ok(()));
  assert_eq!(ports.require(owner(),8081,1),Ok(()));
 }
 #[test]fn failed_batch_has_no_published_side_effects(){
  let mut ports=Ports::<2>::new();
  assert_eq!(ports.bind_batch(&acl(),owner(),&[9000,9000],1,40),Err(Error::Conflict));
  let mut out=[PortLease{owner:owner(),port:0,deadline:0};2];
  assert_eq!(ports.list_owner(owner(),1,&mut out),Ok(0));
  assert_eq!(ports.bind_batch(&acl(),owner(),&[1000,9000],1,40),Err(Error::Invalid));
  assert_eq!(ports.list_owner(owner(),1,&mut out),Ok(0));
 }
 #[test]fn expiry_releases_ports_and_preserves_owner_privacy(){
  let mut ports=Ports::<2>::new();
  ports.bind(&acl(),owner(),8000,1,5).unwrap();
  let mut out=[PortLease{owner:owner(),port:0,deadline:0};1];
  assert_eq!(ports.list_owner(Identity{pid:7,generation:1},1,&mut out),Ok(0));
  assert_eq!(ports.reap(5),1);
  assert_eq!(ports.list_owner(owner(),5,&mut out),Ok(0));
 }
}


// R084 / network resource workstream: real policy state for the active
// owner/port lease table. These are NOT TCP sockets or a TLS implementation.
// All sensitive operations remain process-generation and capability bound.
impl<const N:usize> Ports<N>{
    /// Count live bound leases for all kernel-owned service records.
    pub fn active_count(&self,now:u64)->usize{
        self.leases.iter().flatten().filter(|s|s.deadline>now).count()
    }
    pub fn free_capacity(&self,now:u64)->usize{
        N-self.active_count(now)
    }
    pub fn count_owner(&self,owner:Identity,now:u64)->usize{
        if owner.pid==0||owner.generation==0{return 0;}
        self.leases.iter().flatten().filter(|s|
            s.owner==owner&&s.deadline>now).count()
    }
    /// Earliest future expiry for an exact process generation, no other PID.
    pub fn owner_next_expiry(&self,owner:Identity,now:u64)->Option<u64>{
        if owner.pid==0||owner.generation==0{return None;}
        self.leases.iter().flatten().filter(|s|s.owner==owner&&s.deadline>now)
            .map(|s|s.deadline).min()
    }
    /// Earliest expiry across all leases; kernel monotonic scheduler only.
    pub fn next_global_expiry(&self,now:u64)->Option<u64>{
        self.leases.iter().flatten().filter(|s|s.deadline>now)
            .map(|s|s.deadline).min()
    }
    /// Check whether the exact owner owns an active port, no port enumeration.
    pub fn has_owner_port(&self,owner:Identity,port:u16,now:u64)->bool{
        owner.pid!=0&&owner.generation!=0&&port>=1024&&
        self.leases.iter().flatten().any(|s|
            s.owner==owner&&s.port==port&&s.deadline>now)
    }
    /// Renew a matching active lease only through SOCKET-authorized identity;
    /// deadline must strictly grow and be in the future.
    pub fn renew_exact<const G:usize>(&mut self,auth:&Access<G>,
        owner:Identity,port:u16,now:u64,expected_deadline:u64,
        new_deadline:u64)->Result<(),Error>{
        auth.require(owner,SOCKET,now).map_err(|_|Error::Denied)?;
        if port<1024||new_deadline<=now||new_deadline<=expected_deadline{
            return Err(Error::Invalid);
        }
        let slot=self.leases.iter_mut().flatten().find(|x|x.port==port)
            .ok_or(Error::Denied)?;
        if slot.owner!=owner{return Err(Error::Denied);}
        if slot.deadline<=now{return Err(Error::Expired);}
        if slot.deadline!=expected_deadline{return Err(Error::Conflict);}
        slot.deadline=new_deadline;Ok(())
    }
    /// Atomic renewal of multiple distinct owner ports; no partial lease
    /// extension on a stale deadline, duplicate or unauthorized request.
    pub fn renew_batch<const G:usize>(&mut self,auth:&Access<G>,
        owner:Identity,updates:&[(u16,u64,u64)],now:u64)->Result<usize,Error>{
        if updates.is_empty()||updates.len()>N{return Err(Error::Invalid);}
        let mut staged=Self{leases:self.leases};
        for (i,&(port,expected,new_deadline)) in updates.iter().enumerate(){
            if updates[..i].iter().any(|(seen,_,_)|*seen==port){
                return Err(Error::Conflict);
            }
            staged.renew_exact(auth,owner,port,now,expected,new_deadline)?;
        }
        self.leases=staged.leases;Ok(updates.len())
    }
    /// Generation-pinned batch release: rejected elements leave all peers
    /// untouched, including leases of unrelated owners.
    pub fn release_batch(&mut self,owner:Identity,ports:&[u16])
        ->Result<usize,Error>{
        if ports.is_empty()||ports.len()>N{return Err(Error::Invalid);}
        let mut staged=Self{leases:self.leases};
        for (i,&port) in ports.iter().enumerate(){
            if ports[..i].contains(&port){return Err(Error::Conflict);}
            staged.release(owner,port)?;
        }
        self.leases=staged.leases;Ok(ports.len())
    }
    /// Owner-only snapshot of leases expiring within an inclusive time
    /// window; outputs are atomic on short destinations.
    pub fn expiring_owner(&self,owner:Identity,now:u64,until:u64,
        out:&mut[PortLease])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||until<now{
            return Err(Error::Invalid);
        }
        let count=self.leases.iter().flatten().filter(|s|
            s.owner==owner&&s.deadline>now&&s.deadline<=until).count();
        if out.len()<count{return Err(Error::Full);}
        let mut at=0;
        for item in self.leases.iter().flatten(){
            if item.owner==owner&&item.deadline>now&&item.deadline<=until{
                out[at]=*item;at+=1;
            }
        }
        Ok(at)
    }
}
#[cfg(test)]
mod r084_port_lease_tests{
    use super::*;
    fn owner()->Identity{Identity{pid:54,generation:9}}
    fn peer()->Identity{Identity{pid:55,generation:9}}
    fn acl()->Access<2>{
        let mut a=Access::new();a.grant(owner(),SOCKET,100).unwrap();
        a.grant(peer(),SOCKET,100).unwrap();a
    }
    #[test]
    fn lease_renewal_accounting_and_generation_pinned_release_are_atomic(){
        let mut p=Ports::<4>::new();
        p.bind_batch(&acl(),owner(),&[8080,8081],1,20).unwrap();
        p.bind(&acl(),peer(),8082,1,30).unwrap();
        assert_eq!(p.active_count(1),3);
        assert_eq!(p.free_capacity(1),1);
        assert_eq!(p.count_owner(owner(),1),2);
        assert_eq!(p.owner_next_expiry(owner(),1),Some(20));
        assert_eq!(p.next_global_expiry(1),Some(20));
        assert!(p.has_owner_port(owner(),8081,1));
        assert!(!p.has_owner_port(Identity{generation:10,..owner()},8081,1));
        assert_eq!(p.renew_batch(&acl(),owner(),&[
            (8080,20,40),(8081,20,60)],2),Ok(2));
        assert_eq!(p.owner_next_expiry(owner(),2),Some(40));
        assert_eq!(p.renew_exact(&acl(),owner(),8080,2,20,70),Err(Error::Conflict));
        assert_eq!(p.renew_batch(&acl(),owner(),&[
            (8080,40,80),(8081,20,100)],2),Err(Error::Conflict));
        assert_eq!(p.owner_next_expiry(owner(),2),Some(40));
        let dummy=PortLease{owner:owner(),port:0,deadline:0};
        let mut tiny=[dummy;0];
        assert_eq!(p.expiring_owner(owner(),2,65,&mut tiny),Err(Error::Full));
        let mut out=[dummy;4];
        assert_eq!(p.expiring_owner(owner(),2,65,&mut out),Ok(2));
        assert_eq!(out[0].port,8080);
        assert_eq!(p.release_batch(owner(),&[8080,8082]),Err(Error::Denied));
        assert!(p.has_owner_port(owner(),8080,2));
        assert_eq!(p.release_batch(owner(),&[8080,8081]),Ok(2));
        assert!(p.has_owner_port(peer(),8082,2));
        assert_eq!(p.active_count(2),1);
    }
    #[test]
    fn expired_lease_cannot_be_renewed_or_leaked_to_recycled_pid(){
        let mut p=Ports::<1>::new();
        p.bind(&acl(),owner(),9200,1,5).unwrap();
        assert_eq!(p.renew_exact(&acl(),owner(),9200,5,5,50),Err(Error::Expired));
        assert_eq!(p.renew_exact(&acl(),Identity{generation:10,..owner()},
            9200,1,5,10),Err(Error::Denied));
        assert_eq!(p.active_count(5),0);
        assert_eq!(p.free_capacity(5),1);
        assert_eq!(p.owner_next_expiry(owner(),5),None);
    }
}


// R084 extended network resource management. Owns only numeric port policy,
// never claims the physical NIC or completed TCP/TLS protocol support.
impl<const N:usize> Ports<N>{
    /// 1. Count occupied table records including expired but un-reaped slots.
    pub fn occupied_slot_count(&self)->usize{
        self.leases.iter().filter(|s|s.is_some()).count()
    }
    /// 2. Count expired but still represented leases for a time tick.
    pub fn expired_count(&self,now:u64)->usize{
        self.leases.iter().flatten().filter(|s|s.deadline<=now).count()
    }
    /// 3. Check a specific unprivileged port's numeric availability.
    pub fn port_available(&self,port:u16,now:u64)->bool{
        port>=1024&&!self.leases.iter().flatten()
            .any(|s|s.port==port&&s.deadline>now)
    }
    /// 4. Owner-only lookup of a live port's actual lease deadline.
    pub fn owner_port_deadline(&self,owner:Identity,port:u16,now:u64)->Option<u64>{
        if owner.pid==0||owner.generation==0{return None;}
        self.leases.iter().flatten().find(|s|s.owner==owner&&
            s.port==port&&s.deadline>now).map(|s|s.deadline)
    }
    /// 5. Longest active lease expiration for the exact process incarnation.
    pub fn owner_latest_expiry(&self,owner:Identity,now:u64)->Option<u64>{
        if owner.pid==0||owner.generation==0{return None;}
        self.leases.iter().flatten().filter(|s|s.owner==owner&&s.deadline>now)
            .map(|s|s.deadline).max()
    }
    /// 6. Minimum remaining lease duration (trusted monotonic tick units).
    pub fn owner_shortest_remaining(&self,owner:Identity,now:u64)->Option<u64>{
        self.owner_next_expiry(owner,now).map(|deadline|deadline-now)
    }
    /// 7. Maximum remaining lease duration, without overflow.
    pub fn owner_longest_remaining(&self,owner:Identity,now:u64)->Option<u64>{
        self.owner_latest_expiry(owner,now).map(|deadline|deadline-now)
    }
    /// 8. Lowest active bound port for a selected process.
    pub fn owner_lowest_port(&self,owner:Identity,now:u64)->Option<u16>{
        if owner.pid==0||owner.generation==0{return None;}
        self.leases.iter().flatten().filter(|s|s.owner==owner&&s.deadline>now)
            .map(|s|s.port).min()
    }
    /// 9. Highest active bound port for the same process.
    pub fn owner_highest_port(&self,owner:Identity,now:u64)->Option<u16>{
        if owner.pid==0||owner.generation==0{return None;}
        self.leases.iter().flatten().filter(|s|s.owner==owner&&s.deadline>now)
            .map(|s|s.port).max()
    }
    /// 10. Owner-scoped port listing; no partial output on short buffer.
    pub fn list_owner_port_numbers(&self,owner:Identity,now:u64,
        out:&mut[u16])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0{return Err(Error::Invalid);}
        let n=self.count_owner(owner,now);
        if out.len()<n{return Err(Error::Full);}
        let mut at=0;
        for s in self.leases.iter().flatten(){
            if s.owner==owner&&s.deadline>now{out[at]=s.port;at+=1;}
        }
        Ok(at)
    }
    /// 11. Reclaim expired records belonging only to a particular PID epoch.
    pub fn reap_expired_owner(&mut self,owner:Identity,now:u64)->usize{
        if owner.pid==0||owner.generation==0{return 0;}
        let mut removed=0;
        for slot in &mut self.leases{
            if slot.is_some_and(|s|s.owner==owner&&s.deadline<=now){
                *slot=None;removed+=1;
            }
        }
        removed
    }
    /// 12. Reap exactly one previously expired port record.
    pub fn reap_expired_port(&mut self,port:u16,now:u64)->bool{
        if port<1024{return false;}
        let Some(slot)=self.leases.iter_mut().find(|slot|
            slot.is_some_and(|s|s.port==port&&s.deadline<=now))else{return false;};
        *slot=None;true
    }
    /// 13. Bind the first available candidate port with actual SOCKET rights.
    pub fn bind_from_candidates<const G:usize>(&mut self,auth:&Access<G>,
        owner:Identity,candidates:&[u16],now:u64,deadline:u64)
        ->Result<u16,Error>{
        if candidates.is_empty()||candidates.len()>64{return Err(Error::Invalid);}
        auth.require(owner,SOCKET,now).map_err(|_|Error::Denied)?;
        if deadline<=now{return Err(Error::Invalid);}
        let mut found=false;
        for &port in candidates{
            if port<1024{return Err(Error::Invalid);}
            if self.port_available(port,now){
                found=true;
                match self.bind(auth,owner,port,now,deadline){
                    Ok(())=>return Ok(port),
                    Err(Error::Full)=>return Err(Error::Full),
                    Err(e)=>return Err(e),
                }
            }
        }
        if found{Err(Error::Full)}else{Err(Error::Conflict)}
    }
    /// 14. Allocate first unoccupied numeric port from a bounded port range.
    pub fn bind_first_in_range<const G:usize>(&mut self,auth:&Access<G>,
        owner:Identity,start:u16,end:u16,now:u64,deadline:u64)->Result<u16,Error>{
        if start<1024||end<start||end as u32-start as u32>1024{
            return Err(Error::Invalid);
        }
        auth.require(owner,SOCKET,now).map_err(|_|Error::Denied)?;
        for port in start..=end{
            if self.port_available(port,now){
                self.bind(auth,owner,port,now,deadline)?;return Ok(port);
            }
        }
        Err(Error::Conflict)
    }
    /// 15. Atomically bind consecutive ports for a multi-queue service.
    pub fn bind_contiguous_range<const G:usize>(&mut self,auth:&Access<G>,
        owner:Identity,start:u16,count:usize,now:u64,deadline:u64)
        ->Result<usize,Error>{
        if start<1024||count==0||count>N{return Err(Error::Invalid);}
        let mut staged=Self{leases:self.leases};
        for i in 0..count{
            let port=(start as u32).checked_add(i as u32)
                .filter(|p|*p<=u16::MAX as u32).ok_or(Error::Invalid)? as u16;
            staged.bind(auth,owner,port,now,deadline)?;
        }
        self.leases=staged.leases;Ok(count)
    }
    /// 16. Shorten a deadline with exact optimistic current-revision match.
    pub fn shorten_exact<const G:usize>(&mut self,auth:&Access<G>,
        owner:Identity,port:u16,now:u64,expected:u64,new_deadline:u64)
        ->Result<(),Error>{
        auth.require(owner,SOCKET,now).map_err(|_|Error::Denied)?;
        if port<1024||new_deadline<=now||new_deadline>=expected{
            return Err(Error::Invalid);
        }
        let lease=self.leases.iter_mut().flatten()
            .find(|s|s.port==port).ok_or(Error::Denied)?;
        if lease.owner!=owner{return Err(Error::Denied);}
        if lease.deadline<=now{return Err(Error::Expired);}
        if lease.deadline!=expected{return Err(Error::Conflict);}
        lease.deadline=new_deadline;Ok(())
    }
    /// 17. Atomic, unique-port renewal shortening for a service group.
    pub fn shorten_batch<const G:usize>(&mut self,auth:&Access<G>,
        owner:Identity,requests:&[(u16,u64,u64)],now:u64)->Result<usize,Error>{
        if requests.is_empty()||requests.len()>N{return Err(Error::Invalid);}
        let mut staged=Self{leases:self.leases};
        for (i,&(port,expected,new_deadline)) in requests.iter().enumerate(){
            if requests[..i].iter().any(|(seen,_,_)|*seen==port){
                return Err(Error::Conflict);
            }
            staged.shorten_exact(auth,owner,port,now,expected,new_deadline)?;
        }
        self.leases=staged.leases;Ok(requests.len())
    }
    /// 18. Optimistic extension of every live lease for one owner; all
    /// matching leases must share the exact observed old deadline.
    pub fn extend_owner_matching<const G:usize>(&mut self,auth:&Access<G>,
        owner:Identity,now:u64,expected:u64,until:u64)->Result<usize,Error>{
        auth.require(owner,SOCKET,now).map_err(|_|Error::Denied)?;
        if until<=expected||until<=now{return Err(Error::Invalid);}
        let count=self.count_owner(owner,now);
        if count==0{return Err(Error::Denied);}
        if self.leases.iter().flatten().any(|s|
            s.owner==owner&&s.deadline>now&&s.deadline!=expected){
            return Err(Error::Conflict);
        }
        for s in self.leases.iter_mut().flatten(){
            if s.owner==owner&&s.deadline>now{s.deadline=until;}
        }
        Ok(count)
    }
    /// 19. Release only the owner's current exact-version lease.
    pub fn release_if_deadline(&mut self,owner:Identity,port:u16,
        expected:u64)->Result<(),Error>{
        if owner.pid==0||owner.generation==0||port<1024{return Err(Error::Invalid);}
        let slot=self.leases.iter_mut().find(|slot|
            slot.is_some_and(|s|s.port==port)).ok_or(Error::Denied)?;
        if slot.is_none_or(|s|s.owner!=owner){return Err(Error::Denied);}
        if slot.is_none_or(|s|s.deadline!=expected){return Err(Error::Conflict);}
        *slot=None;Ok(())
    }
    /// 20. Validate a group of owner-held ports with no partial side effects.
    pub fn require_batch(&self,owner:Identity,ports:&[u16],now:u64)
        ->Result<usize,Error>{
        if ports.is_empty()||ports.len()>N{return Err(Error::Invalid);}
        for (i,&port) in ports.iter().enumerate(){
            if ports[..i].contains(&port){return Err(Error::Conflict);}
            self.require(owner,port,now)?;
        }
        Ok(ports.len())
    }
}
#[cfg(test)]
mod r084_extended_port_tests{
    use super::*;
    fn who()->Identity{Identity{pid:110,generation:3}}
    fn peer()->Identity{Identity{pid:111,generation:3}}
    fn acl()->Access<2>{
        let mut a=Access::new();a.grant(who(),SOCKET,100).unwrap();
        a.grant(peer(),SOCKET,100).unwrap();a
    }
    #[test]
    fn twenty_real_port_lifecycle_operations_guard_epoch_and_batch_atomicity(){
        let mut p=Ports::<6>::new();
        assert_eq!(p.bind_contiguous_range(&acl(),who(),8080,2,1,20),Ok(2));
        assert_eq!(p.bind_from_candidates(&acl(),peer(),&[8080,8082],1,30),Ok(8082));
        assert_eq!(p.bind_first_in_range(&acl(),who(),8080,8084,1,20),Ok(8083));
        assert_eq!(p.occupied_slot_count(),4);
        assert_eq!(p.expired_count(1),0);
        assert!(!p.port_available(8080,1));
        assert_eq!(p.owner_port_deadline(who(),8080,1),Some(20));
        assert_eq!(p.owner_latest_expiry(who(),1),Some(20));
        assert_eq!(p.owner_shortest_remaining(who(),1),Some(19));
        assert_eq!(p.owner_longest_remaining(who(),1),Some(19));
        assert_eq!(p.owner_lowest_port(who(),1),Some(8080));
        assert_eq!(p.owner_highest_port(who(),1),Some(8083));
        let mut short=[0xaau16;1];
        assert_eq!(p.list_owner_port_numbers(who(),1,&mut short),Err(Error::Full));
        assert_eq!(short,[0xaa]);
        assert_eq!(p.require_batch(who(),&[8080,8081],1),Ok(2));
        assert_eq!(p.shorten_batch(&acl(),who(),&[(8080,20,10),(8081,20,12)],2),Ok(2));
        assert_eq!(p.renew_exact(&acl(),who(),8080,2,10,25),Ok(()));
        assert_eq!(p.release_if_deadline(who(),8081,20),Err(Error::Conflict));
        assert_eq!(p.release_if_deadline(who(),8081,12),Ok(()));
        assert_eq!(p.shorten_exact(&acl(),who(),8083,2,20,8),Ok(()));
        assert_eq!(p.extend_owner_matching(&acl(),who(),2,25,40),Err(Error::Conflict));
        assert_eq!(p.reap_expired_port(8083,8),true);
        assert_eq!(p.expired_count(10),0);
        assert_eq!(p.reap_expired_owner(who(),10),0);
        assert_eq!(p.owner_port_deadline(peer(),8082,10),Some(30));
        assert_eq!(p.release_if_deadline(peer(),8082,30),Ok(()));
        assert_eq!(p.reap_expired_owner(who(),100),1);
        assert_eq!(p.occupied_slot_count(),0);
    }
    #[test]
    fn failed_range_and_stale_deadline_cannot_reserve_or_edit_peer_ports(){
        let mut p=Ports::<2>::new();
        assert_eq!(p.bind_contiguous_range(&acl(),who(),65535,2,1,20),Err(Error::Invalid));
        assert_eq!(p.occupied_slot_count(),0);
        p.bind(&acl(),who(),9000,1,10).unwrap();
        assert_eq!(p.bind_from_candidates(&acl(),peer(),&[9000],1,20),Err(Error::Conflict));
        assert_eq!(p.shorten_exact(&acl(),peer(),9000,1,10,5),Err(Error::Denied));
        assert_eq!(p.require_batch(who(),&[9000,9000],1),Err(Error::Conflict));
        assert_eq!(p.release_if_deadline(Identity{generation:4,..who()},9000,10),
            Err(Error::Denied));
        assert_eq!(p.owner_port_deadline(who(),9000,1),Some(10));
    }
}


// R084 extension III: 20 real port-range, expiry and owner-scoped lease
// operations on the existing process-generation resource table.
// No claim of physical NIC connectivity or TLS transport.
impl<const N:usize> Ports<N>{
    /// 1. Count the exact process incarnation's live ports within a range.
    pub fn owner_ports_between(&self,owner:Identity,from:u16,to:u16,now:u64)->usize{
        if owner.pid==0||owner.generation==0||to<from{return 0;}
        self.leases.iter().flatten().filter(|s|
            s.owner==owner&&s.deadline>now&&s.port>=from&&s.port<=to).count()
    }
    /// 2. Kernel view: count all live leased ports in a range.
    pub fn active_ports_between(&self,from:u16,to:u16,now:u64)->usize{
        if to<from{return 0;}
        self.leases.iter().flatten().filter(|s|
            s.deadline>now&&s.port>=from&&s.port<=to).count()
    }
    /// 3. Owner's ports that will expire within (now, until].
    pub fn owner_expiring_count(&self,owner:Identity,now:u64,until:u64)->usize{
        if owner.pid==0||owner.generation==0||until<now{return 0;}
        self.leases.iter().flatten().filter(|s|
            s.owner==owner&&s.deadline>now&&s.deadline<=until).count()
    }
    /// 4. Owner's leases continuing beyond an explicit deadline.
    pub fn owner_surviving_count(&self,owner:Identity,until:u64)->usize{
        if owner.pid==0||owner.generation==0{return 0;}
        self.leases.iter().flatten().filter(|s|
            s.owner==owner&&s.deadline>until).count()
    }
    /// 5. Exact owner's soonest expiring port, not just timestamp.
    pub fn owner_next_expiring_port(&self,owner:Identity,now:u64)->Option<u16>{
        if owner.pid==0||owner.generation==0{return None;}
        self.leases.iter().flatten().filter(|s|s.owner==owner&&s.deadline>now)
            .min_by_key(|s|s.deadline).map(|s|s.port)
    }
    /// 6. Owner's port with furthest deadline for health panel.
    pub fn owner_last_expiring_port(&self,owner:Identity,now:u64)->Option<u16>{
        if owner.pid==0||owner.generation==0{return None;}
        self.leases.iter().flatten().filter(|s|s.owner==owner&&s.deadline>now)
            .max_by_key(|s|s.deadline).map(|s|s.port)
    }
    /// 7. First available candidate (without acquiring).
    pub fn first_free_candidate(&self,ports:&[u16],now:u64)->Option<u16>{
        ports.iter().copied().find(|port|self.port_available(*port,now))
    }
    /// 8. First unallocated numeric port from a bounded range.
    pub fn first_free_between(&self,from:u16,to:u16,now:u64)->Option<u16>{
        if from<1024||to<from||to as u32-from as u32>1024{return None;}
        (from..=to).find(|port|self.port_available(*port,now))
    }
    /// 9. Last unallocated numeric port in a bounded range.
    pub fn last_free_between(&self,from:u16,to:u16,now:u64)->Option<u16>{
        if from<1024||to<from||to as u32-from as u32>1024{return None;}
        (from..=to).rev().find(|port|self.port_available(*port,now))
    }
    /// 10. Available numeric capacity within a bounded range.
    pub fn free_ports_between(&self,from:u16,to:u16,now:u64)->usize{
        if from<1024||to<from||to as u32-from as u32>1024{return 0;}
        (from..=to).filter(|port|self.port_available(*port,now)).count()
    }
    /// 11. Require at least one of a process's candidate port leases.
    pub fn require_any(&self,owner:Identity,ports:&[u16],now:u64)->Result<u16,Error>{
        if ports.is_empty()||owner.pid==0||owner.generation==0{return Err(Error::Invalid);}
        for &port in ports{
            if self.require(owner,port,now).is_ok(){return Ok(port);}
        }
        Err(Error::Denied)
    }
    /// 12. Check all owner's ports exist with no duplicate source request.
    pub fn require_distinct(&self,owner:Identity,ports:&[u16],now:u64)
        ->Result<(),Error>{
        self.require_batch(owner,ports,now).map(|_|())
    }
    /// 13. Preview a group binding with the actual capability check, without
    /// reserving any live port or publishing partial state.
    pub fn would_bind_batch<const G:usize>(&self,acl:&Access<G>,owner:Identity,
        ports:&[u16],now:u64,deadline:u64)->Result<(),Error>{
        let mut staged=Self{leases:self.leases};
        staged.bind_batch(acl,owner,ports,now,deadline).map(|_|())
    }
    /// 14. Reserve the *highest* available port of a bounded range.
    pub fn bind_last_in_range<const G:usize>(&mut self,acl:&Access<G>,owner:Identity,
        from:u16,to:u16,now:u64,deadline:u64)->Result<u16,Error>{
        acl.require(owner,SOCKET,now).map_err(|_|Error::Denied)?;
        if deadline<=now||from<1024||to<from||to as u32-from as u32>1024{
            return Err(Error::Invalid);
        }
        let port=self.last_free_between(from,to,now).ok_or(Error::Conflict)?;
        self.bind(acl,owner,port,now,deadline)?;
        Ok(port)
    }
    /// 15. Prefer one exact port, fall back to a different authorized port.
    pub fn bind_preferred_fallback<const G:usize>(&mut self,acl:&Access<G>,
        owner:Identity,preferred:u16,fallback:u16,now:u64,deadline:u64)
        ->Result<u16,Error>{
        acl.require(owner,SOCKET,now).map_err(|_|Error::Denied)?;
        if preferred<1024||fallback<1024||preferred==fallback||deadline<=now{
            return Err(Error::Invalid);
        }
        let port=if self.port_available(preferred,now){preferred}
          else if self.port_available(fallback,now){fallback}
          else{return Err(Error::Conflict)};
        self.bind(acl,owner,port,now,deadline)?;Ok(port)
    }
    /// 16. Atomic release of exact owner ports with observed deadline pins.
    pub fn release_deadline_batch(&mut self,owner:Identity,
        entries:&[(u16,u64)])->Result<usize,Error>{
        if entries.is_empty()||entries.len()>N{return Err(Error::Invalid);}
        let mut staged=Self{leases:self.leases};
        for (i,&(port,expected)) in entries.iter().enumerate(){
            if entries[..i].iter().any(|(p,_)|*p==port){return Err(Error::Conflict);}
            staged.release_if_deadline(owner,port,expected)?;
        }
        self.leases=staged.leases;Ok(entries.len())
    }
    /// 17. Reap expired numeric leases limited to one validated port range.
    pub fn reap_range(&mut self,from:u16,to:u16,now:u64)->usize{
        if from<1024||to<from{return 0;}
        let mut count=0;
        for slot in &mut self.leases{
            if slot.is_some_and(|s|s.port>=from&&s.port<=to&&s.deadline<=now){
                *slot=None;count+=1;
            }
        }
        count
    }
    /// 18. Move an owner's whole equal-deadline lease group to an earlier
    /// expiry, preserving all leases on a stale expected deadline.
    pub fn shorten_owner_group<const G:usize>(&mut self,acl:&Access<G>,
        owner:Identity,now:u64,expected:u64,until:u64)->Result<usize,Error>{
        acl.require(owner,SOCKET,now).map_err(|_|Error::Denied)?;
        if until<=now||until>=expected{return Err(Error::Invalid);}
        let count=self.count_owner(owner,now);
        if count==0{return Err(Error::Denied);}
        if self.leases.iter().flatten().any(|s|
            s.owner==owner&&s.deadline>now&&s.deadline!=expected){
            return Err(Error::Conflict);
        }
        for s in self.leases.iter_mut().flatten(){
            if s.owner==owner&&s.deadline>now{s.deadline=until;}
        }
        Ok(count)
    }
    /// 19. Snapshot only owner leases within inclusive port bounds.
    pub fn list_owner_in_range(&self,owner:Identity,from:u16,to:u16,
        now:u64,out:&mut[PortLease])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||from<1024||to<from{
            return Err(Error::Invalid);
        }
        let need=self.owner_ports_between(owner,from,to,now);
        if out.len()<need{return Err(Error::Full);}
        let mut at=0;
        for s in self.leases.iter().flatten(){
            if s.owner==owner&&s.port>=from&&s.port<=to&&s.deadline>now{
                out[at]=*s;at+=1;
            }
        }
        Ok(at)
    }
    /// 20. Snapshot port numbers expiring for one process before cutoff.
    pub fn expiring_owner_ports(&self,owner:Identity,now:u64,until:u64,
        out:&mut[u16])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||until<now{return Err(Error::Invalid);}
        let need=self.owner_expiring_count(owner,now,until);
        if out.len()<need{return Err(Error::Full);}
        let mut at=0;
        for s in self.leases.iter().flatten(){
            if s.owner==owner&&s.deadline>now&&s.deadline<=until{
                out[at]=s.port;at+=1;
            }
        }
        Ok(at)
    }
}
#[cfg(test)]
mod r084_port_range_batch_tests{
    use super::*;
    fn owner()->Identity{Identity{pid:130,generation:4}}
    fn peer()->Identity{Identity{pid:131,generation:4}}
    fn acl()->Access<2>{
        let mut a=Access::new();
        a.grant(owner(),SOCKET,100).unwrap();a.grant(peer(),SOCKET,100).unwrap();a
    }
    #[test]
    fn ranged_leases_compose_optimistic_owner_generation_transactions(){
        let mut p=Ports::<8>::new();
        assert_eq!(p.would_bind_batch(&acl(),owner(),&[8100,8101],1,20),Ok(()));
        assert_eq!(p.active_count(1),0);
        p.bind_batch(&acl(),owner(),&[8100,8101],1,20).unwrap();
        assert_eq!(p.bind_preferred_fallback(&acl(),peer(),8100,8102,1,30),Ok(8102));
        assert_eq!(p.bind_last_in_range(&acl(),owner(),8100,8104,1,30),Ok(8104));
        assert_eq!(p.owner_ports_between(owner(),8100,8104,1),3);
        assert_eq!(p.active_ports_between(8100,8104,1),4);
        assert_eq!(p.owner_expiring_count(owner(),1,20),2);
        assert_eq!(p.owner_surviving_count(owner(),20),1);
        assert_eq!(p.owner_next_expiring_port(owner(),1),Some(8100));
        assert_eq!(p.owner_last_expiring_port(owner(),1),Some(8104));
        assert_eq!(p.first_free_candidate(&[8100,8103],1),Some(8103));
        assert_eq!(p.first_free_between(8100,8104,1),Some(8103));
        assert_eq!(p.last_free_between(8100,8104,1),Some(8103));
        assert_eq!(p.free_ports_between(8100,8104,1),1);
        assert_eq!(p.require_any(owner(),&[8102,8101],1),Ok(8101));
        assert_eq!(p.require_distinct(owner(),&[8100,8101],1),Ok(()));
        let dummy=PortLease{owner:owner(),port:0,deadline:0};
        let mut tiny=[dummy;0];
        assert_eq!(p.list_owner_in_range(owner(),8100,8104,1,&mut tiny),Err(Error::Full));
        let mut out=[dummy;8];
        assert_eq!(p.list_owner_in_range(owner(),8100,8104,1,&mut out),Ok(3));
        let mut ports=[0u16;8];
        assert_eq!(p.expiring_owner_ports(owner(),1,20,&mut ports),Ok(2));
        assert_eq!(&ports[..2],&[8100,8101]);
        assert_eq!(p.shorten_owner_group(&acl(),owner(),2,20,15),Err(Error::Conflict));
        assert_eq!(p.release_deadline_batch(owner(),&[(8100,20),(8101,20)]),Ok(2));
        assert_eq!(p.shorten_owner_group(&acl(),owner(),2,30,10),Ok(1));
        assert_eq!(p.reap_range(8100,8104,10),1);
        assert_eq!(p.count_owner(owner(),11),0);
        assert_eq!(p.count_owner(peer(),11),1);
    }
    #[test]
    fn failed_reservation_or_expired_epoch_never_allocates_any_port(){
        let mut p=Ports::<2>::new();
        assert_eq!(p.bind_preferred_fallback(&acl(),owner(),100,9100,1,20),
          Err(Error::Invalid));
        assert_eq!(p.bind_last_in_range(&acl(),owner(),9000,8999,1,20),
          Err(Error::Invalid));
        assert_eq!(p.active_count(1),0);
        assert_eq!(p.would_bind_batch(&acl(),owner(),&[9100,9100],1,20),
          Err(Error::Conflict));
        assert_eq!(p.active_count(1),0);
        assert_eq!(p.list_owner_in_range(Identity{generation:0,..owner()},
          9000,9100,1,&mut[]),Err(Error::Invalid));
    }
}


// M1/R084 extension IV: twenty authenticated port-lease diagnostics,
// fail-closed revocation and migration policies (not a TCP/IP stack).
impl<const N:usize> Ports<N>{
    /// 1. Count expired reservations belonging only to the given PID epoch.
    pub fn expired_owner_count(&self,who:Identity,now:u64)->usize{
        if who.pid==0||who.generation==0{return 0;}
        self.leases.iter().flatten().filter(|s|s.owner==who&&s.deadline<=now).count()
    }
    /// 2. Find actual vacant or expired table slots for the current time.
    pub fn reusable_slot_count(&self,now:u64)->usize{
        self.leases.iter().filter(|slot|slot.is_none_or(|s|s.deadline<=now)).count()
    }
    /// 3. Count active lease records expiring before a monotonic cutoff.
    pub fn global_expiring_count(&self,now:u64,until:u64)->usize{
        if until<now{return 0;}
        self.leases.iter().flatten().filter(|s|s.deadline>now&&s.deadline<=until).count()
    }
    /// 4. Lookup the lowest live port across all kernel-managed owners.
    pub fn global_lowest_port(&self,now:u64)->Option<u16>{
        self.leases.iter().flatten().filter(|s|s.deadline>now).map(|s|s.port).min()
    }
    /// 5. Lookup highest live port in the bounded slot table.
    pub fn global_highest_port(&self,now:u64)->Option<u16>{
        self.leases.iter().flatten().filter(|s|s.deadline>now).map(|s|s.port).max()
    }
    /// 6. Numeric port of the earliest expiring active lease globally.
    pub fn global_next_expiring_port(&self,now:u64)->Option<u16>{
        self.leases.iter().flatten().filter(|s|s.deadline>now)
            .min_by_key(|s|s.deadline).map(|s|s.port)
    }
    /// 7. Exact count of owner's leases with matching observed expiry.
    pub fn owner_deadline_count(&self,who:Identity,deadline:u64,now:u64)->usize{
        if who.pid==0||who.generation==0||deadline<=now{return 0;}
        self.leases.iter().flatten().filter(|s|
            s.owner==who&&s.deadline==deadline).count()
    }
    /// 8. Snapshot all live leases expiring before cutoff, for kernel auditor.
    pub fn expiring_snapshot(&self,now:u64,until:u64,out:&mut[PortLease])
        ->Result<usize,Error>{
        if until<now{return Err(Error::Invalid);}
        let need=self.global_expiring_count(now,until);
        if out.len()<need{return Err(Error::Full);}
        let mut i=0;
        for s in self.leases.iter().flatten(){
            if s.deadline>now&&s.deadline<=until{out[i]=*s;i+=1;}
        }
        Ok(i)
    }
    /// 9. Snapshot exact deadline owner ports, rejecting short output.
    pub fn owner_deadline_ports(&self,who:Identity,deadline:u64,now:u64,
        out:&mut[u16])->Result<usize,Error>{
        if who.pid==0||who.generation==0{return Err(Error::Invalid);}
        let need=self.owner_deadline_count(who,deadline,now);
        if out.len()<need{return Err(Error::Full);}
        let mut i=0;for s in self.leases.iter().flatten(){
            if s.owner==who&&s.deadline==deadline&&s.deadline>now{
                out[i]=s.port;i+=1;
            }
        }
        Ok(i)
    }
    /// 10. Validate exact generation and deadline across unique port list.
    pub fn require_deadline_batch(&self,who:Identity,now:u64,
        ports:&[(u16,u64)])->Result<usize,Error>{
        if ports.is_empty()||ports.len()>N{return Err(Error::Invalid);}
        for (i,&(port,deadline)) in ports.iter().enumerate(){
            if ports[..i].iter().any(|(p,_)|*p==port){return Err(Error::Conflict);}
            if self.owner_port_deadline(who,port,now)!=Some(deadline){
                return Err(Error::Conflict);
            }
        }
        Ok(ports.len())
    }
    /// 11. Reap all expired lease records in one exact process incarnation.
    pub fn revoke_expired_owner(&mut self,who:Identity,now:u64)->usize{
        self.reap_expired_owner(who,now)
    }
    /// 12. Release all lease records with a specified exact owner/deadline.
    pub fn release_owner_deadline(&mut self,who:Identity,deadline:u64)->usize{
        if who.pid==0||who.generation==0{return 0;}
        let mut count=0;
        for slot in &mut self.leases{
            if slot.is_some_and(|s|s.owner==who&&s.deadline==deadline){
                *slot=None;count+=1;
            }
        }
        count
    }
    /// 13. Reclaim all leases in a process-scoped closed numeric port range.
    pub fn release_owner_range(&mut self,who:Identity,from:u16,to:u16)->usize{
        if who.pid==0||who.generation==0||from<1024||to<from{return 0;}
        let mut count=0;
        for slot in &mut self.leases{
            if slot.is_some_and(|s|s.owner==who&&s.port>=from&&s.port<=to){
                *slot=None;count+=1;
            }
        }
        count
    }
    /// 14. Release a port only if it is already expired at actual monotonic time.
    pub fn release_if_expired(&mut self,who:Identity,port:u16,now:u64)->bool{
        if who.pid==0||who.generation==0{return false;}
        let Some(slot)=self.leases.iter_mut().find(|slot|
            slot.is_some_and(|s|s.owner==who&&s.port==port&&s.deadline<=now))
            else{return false;};
        *slot=None;true
    }
    /// 15. Compare-and-swap one owned active port to a different free number.
    pub fn replace_owner_port<const G:usize>(&mut self,auth:&Access<G>,
        who:Identity,old:u16,new:u16,now:u64,expected:u64)->Result<(),Error>{
        auth.require(who,SOCKET,now).map_err(|_|Error::Denied)?;
        if old==new||new<1024||!self.port_available(new,now){
            return Err(Error::Conflict);
        }
        if self.owner_port_deadline(who,old,now)!=Some(expected){
            return Err(Error::Conflict);
        }
        let mut next=Self{leases:self.leases};
        next.release_if_deadline(who,old,expected)?;
        next.bind(auth,who,new,now,expected)?;
        self.leases=next.leases;Ok(())
    }
    /// 16. Swap two own port numbers with exact deadline versions.
    pub fn swap_owned_ports<const G:usize>(&mut self,auth:&Access<G>,
        who:Identity,a:u16,b:u16,now:u64,exp_a:u64,exp_b:u64)->Result<(),Error>{
        auth.require(who,SOCKET,now).map_err(|_|Error::Denied)?;
        if a==b||self.owner_port_deadline(who,a,now)!=Some(exp_a)||
            self.owner_port_deadline(who,b,now)!=Some(exp_b){return Err(Error::Conflict);}
        let pa=self.leases.iter().position(|s|s.is_some_and(|v|v.owner==who&&v.port==a))
            .ok_or(Error::Denied)?;
        let pb=self.leases.iter().position(|s|s.is_some_and(|v|v.owner==who&&v.port==b))
            .ok_or(Error::Denied)?;
        self.leases[pa].as_mut().unwrap().port=b;
        self.leases[pb].as_mut().unwrap().port=a;
        Ok(())
    }
    /// 17. Reserve a preferred free port from a unique candidate group.
    pub fn bind_from_unique_candidates<const G:usize>(&mut self,auth:&Access<G>,
        who:Identity,choices:&[u16],now:u64,deadline:u64)->Result<u16,Error>{
        if choices.is_empty()||choices.len()>64{return Err(Error::Invalid);}
        for (i,&port) in choices.iter().enumerate(){
            if port<1024||choices[..i].contains(&port){return Err(Error::Invalid);}
        }
        self.bind_from_candidates(auth,who,choices,now,deadline)
    }
    /// 18. Allocate the first owner-authorized free port above a floor.
    pub fn bind_above_floor<const G:usize>(&mut self,auth:&Access<G>,
        who:Identity,floor:u16,range:u16,now:u64,deadline:u64)->Result<u16,Error>{
        let start=floor.checked_add(1).ok_or(Error::Invalid)?;
        let end=start.checked_add(range).ok_or(Error::Invalid)?;
        self.bind_first_in_range(auth,who,start,end,now,deadline)
    }
    /// 19. Allocate the last available authorized port below a ceiling.
    pub fn bind_below_ceiling<const G:usize>(&mut self,auth:&Access<G>,
        who:Identity,ceiling:u16,range:u16,now:u64,deadline:u64)
        ->Result<u16,Error>{
        if ceiling<=1024{return Err(Error::Invalid);}
        let end=ceiling-1;let start=end.checked_sub(range).ok_or(Error::Invalid)?;
        self.bind_last_in_range(auth,who,start,end,now,deadline)
    }
    /// 20. Revoke distinct owner ports only after checking exact deadlines.
    pub fn revoke_exact_batch(&mut self,who:Identity,now:u64,
        ports:&[(u16,u64)])->Result<usize,Error>{
        self.require_deadline_batch(who,now,ports)?;
        self.release_deadline_batch(who,ports)
    }
}
#[cfg(test)]
mod r084_network_lease_reassignment_tests{
    use super::*;
    fn who()->Identity{Identity{pid:221,generation:7}}
    fn other()->Identity{Identity{pid:222,generation:7}}
    fn acl()->Access<2>{
        let mut a=Access::new();
        a.grant(who(),SOCKET,100).unwrap();
        a.grant(other(),SOCKET,100).unwrap();a
    }
    #[test]
    fn lease_reassignment_preserves_expiry_auth_and_all_or_nothing_ownership(){
        let mut p=Ports::<7>::new();
        p.bind_batch(&acl(),who(),&[8100,8101],1,20).unwrap();
        p.bind(&acl(),other(),9000,1,30).unwrap();
        assert_eq!(p.expired_owner_count(who(),1),0);
        assert_eq!(p.reusable_slot_count(1),4);
        assert_eq!(p.global_expiring_count(1,20),2);
        assert_eq!(p.global_lowest_port(1),Some(8100));
        assert_eq!(p.global_highest_port(1),Some(9000));
        assert_eq!(p.global_next_expiring_port(1),Some(8100));
        assert_eq!(p.owner_deadline_count(who(),20,1),2);
        let dummy=PortLease{owner:who(),port:0,deadline:0};
        let mut short:[PortLease;0]=[];
        assert_eq!(p.expiring_snapshot(1,20,&mut short),Err(Error::Full));
        let mut out=[dummy;7];
        assert_eq!(p.expiring_snapshot(1,20,&mut out),Ok(2));
        let mut ports=[0u16;7];
        assert_eq!(p.owner_deadline_ports(who(),20,1,&mut ports),Ok(2));
        assert_eq!(p.require_deadline_batch(who(),1,&[(8100,20),(8101,20)]),Ok(2));
        assert_eq!(p.replace_owner_port(&acl(),who(),8100,8200,2,20),Ok(()));
        assert_eq!(p.owner_port_deadline(who(),8200,2),Some(20));
        assert_eq!(p.swap_owned_ports(&acl(),who(),8200,8101,2,20,20),Ok(()));
        assert_eq!(p.owner_port_deadline(who(),8101,2),Some(20));
        assert_eq!(p.bind_from_unique_candidates(&acl(),who(),&[8200,8300],2,25),Ok(8300));
        assert_eq!(p.bind_above_floor(&acl(),who(),8400,2,2,25),Ok(8401));
        assert_eq!(p.bind_below_ceiling(&acl(),who(),8500,2,2,25),Ok(8499));
        assert_eq!(p.revoke_exact_batch(who(),2,&[(8101,20),(8200,20)]),Ok(2));
        assert_eq!(p.release_owner_deadline(who(),25),3);
        assert_eq!(p.count_owner(who(),2),0);
        assert_eq!(p.count_owner(other(),2),1);
        assert_eq!(p.release_owner_range(other(),9000,9000),1);
    }
    #[test]
    fn stale_owner_and_bad_range_leave_peer_leases_unmodified(){
        let mut p=Ports::<3>::new();
        p.bind(&acl(),who(),9000,1,4).unwrap();
        assert!(!p.release_if_expired(who(),9000,3));
        assert!(p.release_if_expired(who(),9000,4));
        assert_eq!(p.revoke_expired_owner(who(),5),0);
        assert_eq!(p.bind_from_unique_candidates(&acl(),who(),&[9001,9001],1,10),
            Err(Error::Invalid));
        assert_eq!(p.count_owner(who(),2),0);
    }
}


// R084 extension V: 20 lease-generation and expiry-aware resource operations.
// Numeric port ownership only: no device driver, TCP or TLS implementation.
impl<const N:usize> Ports<N>{
    /// Count existing port records for a specific PID incarnation, including
    /// expired records awaiting the kernel's scheduled reap.
    pub fn owner_record_count(&self,who:Identity)->usize{
        if who.pid==0||who.generation==0{return 0;}
        self.leases.iter().flatten().filter(|l|l.owner==who).count()
    }
    /// Count all stale occupied table entries no longer binding a live port.
    pub fn stale_slot_count(&self,now:u64)->usize{
        self.leases.iter().flatten().filter(|l|l.deadline<=now).count()
    }
    /// Determine whether a numeric port is currently held by any live owner.
    pub fn port_is_leased(&self,port:u16,now:u64)->bool{
        port>=1024&&self.leases.iter().flatten()
            .any(|l|l.port==port&&l.deadline>now)
    }
    /// Verify an owner currently controls the port with this exact deadline.
    pub fn owner_deadline_matches(&self,who:Identity,port:u16,now:u64,deadline:u64)->bool{
        who.pid!=0&&who.generation!=0&&
        self.owner_port_deadline(who,port,now)==Some(deadline)
    }
    /// Live port with the earliest observed deadline of one PID epoch.
    pub fn owner_soonest_port(&self,who:Identity,now:u64)->Option<u16>{
        self.owner_next_expiring_port(who,now)
    }
    /// Live port with furthest observed deadline for the same PID epoch.
    pub fn owner_latest_port(&self,who:Identity,now:u64)->Option<u16>{
        self.owner_last_expiring_port(who,now)
    }
    /// Find the owner's first active port above a numeric floor.
    pub fn owner_port_above(&self,who:Identity,low:u16,now:u64)->Option<u16>{
        if who.pid==0||who.generation==0{return None;}
        self.leases.iter().flatten().filter(|l|l.owner==who&&
            l.port>low&&l.deadline>now).map(|l|l.port).min()
    }
    /// Find the owner's highest active port below a numeric ceiling.
    pub fn owner_port_below(&self,who:Identity,high:u16,now:u64)->Option<u16>{
        if who.pid==0||who.generation==0{return None;}
        self.leases.iter().flatten().filter(|l|l.owner==who&&
            l.port<high&&l.deadline>now).map(|l|l.port).max()
    }
    /// Count live numeric leases with a given exact deadline.
    pub fn live_at_deadline(&self,deadline:u64,now:u64)->usize{
        if deadline<=now{return 0;}
        self.leases.iter().flatten().filter(|l|l.deadline==deadline).count()
    }
    /// Count unique PID/generation owners using one or more live ports.
    pub fn distinct_live_owners(&self,now:u64)->usize{
        let mut n=0;
        for i in 0..N{
            let Some(l)=self.leases[i]else{continue;};
            if l.deadline<=now{continue;}
            if !self.leases[..i].iter().flatten().any(|x|
                x.owner==l.owner&&x.deadline>now){n+=1;}
        }
        n
    }
    /// Return one owner's most recently expiring active lease without
    /// disclosing peer resources.
    pub fn owner_furthest_lease(&self,who:Identity,now:u64)->Option<PortLease>{
        if who.pid==0||who.generation==0{return None;}
        self.leases.iter().flatten().filter(|l|
            l.owner==who&&l.deadline>now).max_by_key(|l|l.deadline).copied()
    }
    /// Return one owner's earliest active lease for preemptive renewal.
    pub fn owner_nearest_lease(&self,who:Identity,now:u64)->Option<PortLease>{
        if who.pid==0||who.generation==0{return None;}
        self.leases.iter().flatten().filter(|l|
            l.owner==who&&l.deadline>now).min_by_key(|l|l.deadline).copied()
    }
    /// Release a lease only if PID epoch and expected deadline still match.
    pub fn release_live_exact(&mut self,who:Identity,port:u16,
        now:u64,expected:u64)->Result<(),Error>{
        if !self.owner_deadline_matches(who,port,now,expected){
            return Err(Error::Conflict);
        }
        self.release_if_deadline(who,port,expected)
    }
    /// Atomic release of two distinct live ports from one owner generation.
    pub fn release_owned_pair(&mut self,who:Identity,a:u16,b:u16,
        now:u64,exp_a:u64,exp_b:u64)->Result<(),Error>{
        if a==b{return Err(Error::Invalid);}
        self.require_deadline_batch(who,now,&[(a,exp_a),(b,exp_b)])?;
        self.release_deadline_batch(who,&[(a,exp_a),(b,exp_b)]).map(|_|())
    }
    /// Atomically acquire two distinct ports for one authorized service.
    pub fn bind_owned_pair<const G:usize>(&mut self,auth:&Access<G>,
        who:Identity,a:u16,b:u16,now:u64,deadline:u64)->Result<(),Error>{
        if a==b{return Err(Error::Invalid);}
        self.bind_batch(auth,who,&[a,b],now,deadline).map(|_|())
    }
    /// Set an exact active lease's new expiration to the maximum of current
    /// and requested time, with observed version to avoid stale writes.
    pub fn extend_at_least<const G:usize>(&mut self,auth:&Access<G>,
        who:Identity,port:u16,now:u64,observed:u64,minimum:u64)
        ->Result<(),Error>{
        if minimum<=observed{return Err(Error::Invalid);}
        self.renew_exact(auth,who,port,now,observed,minimum)
    }
    /// Set deadline earlier only after verifying capability and old version.
    pub fn expire_no_later_than<const G:usize>(&mut self,auth:&Access<G>,
        who:Identity,port:u16,now:u64,observed:u64,latest:u64)
        ->Result<(),Error>{
        self.shorten_exact(auth,who,port,now,observed,latest)
    }
    /// Atomic renewal of two distinct ports with identical old/new deadline.
    pub fn extend_pair<const G:usize>(&mut self,auth:&Access<G>,
        who:Identity,a:u16,b:u16,now:u64,old:u64,new:u64)->Result<(),Error>{
        if a==b{return Err(Error::Invalid);}
        self.renew_batch(auth,who,&[(a,old,new),(b,old,new)],now).map(|_|())
    }
    /// Release all currently expired leases for an exact owner generation.
    pub fn reclaim_owner_stale(&mut self,who:Identity,now:u64)->usize{
        self.reap_expired_owner(who,now)
    }
    /// Revoke one exact current numeric lease, without granting a new owner.
    pub fn revoke_owned_at_deadline(&mut self,who:Identity,port:u16,
        now:u64,deadline:u64)->Result<(),Error>{
        self.release_live_exact(who,port,now,deadline)
    }
}
#[cfg(test)]
mod r084_v5_port_tests{
    use super::*;
    fn who()->Identity{Identity{pid:440,generation:8}}
    fn peer()->Identity{Identity{pid:441,generation:8}}
    fn auth()->Access<2>{
        let mut x=Access::new();
        x.grant(who(),SOCKET,100).unwrap();x.grant(peer(),SOCKET,100).unwrap();x
    }
    #[test]
    fn real_lease_deadlines_do_not_cross_process_generation(){
        let mut p=Ports::<5>::new();
        assert_eq!(p.bind_owned_pair(&auth(),who(),8010,8011,1,20),Ok(()));
        p.bind(&auth(),peer(),8100,1,30).unwrap();
        assert_eq!(p.owner_record_count(who()),2);
        assert_eq!(p.stale_slot_count(1),0);
        assert!(p.port_is_leased(8010,1));
        assert!(p.owner_deadline_matches(who(),8010,1,20));
        assert_eq!(p.owner_soonest_port(who(),1),Some(8010));
        assert_eq!(p.owner_latest_port(who(),1),Some(8011));
        assert_eq!(p.owner_port_above(who(),8010,1),Some(8011));
        assert_eq!(p.owner_port_below(who(),8011,1),Some(8010));
        assert_eq!(p.live_at_deadline(20,1),2);
        assert_eq!(p.distinct_live_owners(1),2);
        assert_eq!(p.owner_furthest_lease(who(),1).unwrap().deadline,20);
        assert_eq!(p.owner_nearest_lease(who(),1).unwrap().deadline,20);
        assert_eq!(p.extend_pair(&auth(),who(),8010,8011,2,20,40),Ok(()));
        assert_eq!(p.release_live_exact(who(),8010,2,20),Err(Error::Conflict));
        assert_eq!(p.expire_no_later_than(&auth(),who(),8010,2,40,25),Ok(()));
        assert_eq!(p.extend_at_least(&auth(),who(),8010,2,25,50),Ok(()));
        assert_eq!(p.release_owned_pair(who(),8010,8011,2,50,40),Ok(()));
        assert_eq!(p.revoke_owned_at_deadline(peer(),8100,2,30),Ok(()));
        assert_eq!(p.reclaim_owner_stale(who(),45),0);
        assert_eq!(p.active_count(2),0);
    }
    #[test]
    fn failed_pair_reassignment_does_not_release_other_process_ports(){
        let mut p=Ports::<3>::new();
        p.bind(&auth(),peer(),9000,1,20).unwrap();
        assert_eq!(p.bind_owned_pair(&auth(),who(),9001,9000,1,30),Err(Error::Conflict));
        assert_eq!(p.owner_record_count(who()),0);
        assert_eq!(p.owner_record_count(peer()),1);
        assert_eq!(p.release_owned_pair(who(),9000,9001,1,20,30),Err(Error::Conflict));
        assert_eq!(p.owner_record_count(peer()),1);
    }
}


// R084 batch VI: 20 precise numeric port-lease maintenance operations.
// Owner/generation, SOCKET capability, and monotonic time remain mandatory.
// No TCP/IP transport or certificate verification is supplied by this table.
impl<const N:usize> Ports<N>{
    // 1. Count leases of one exact owner which are still active at now.
    pub fn live_owner_records(&self,owner:Identity,now:u64)->usize{
        self.count_owner(owner,now)
    }
    // 2. Count live leases held by an owner with deadline >= cutoff.
    pub fn owner_live_after_cutoff(&self,owner:Identity,now:u64,cutoff:u64)->usize{
        if owner.pid==0||owner.generation==0{return 0;}
        self.leases.iter().flatten().filter(|x|
            x.owner==owner&&x.deadline>now&&x.deadline>=cutoff).count()
    }
    // 3. Count leases in a half-open numeric port region for one owner.
    pub fn owner_port_range_count(&self,owner:Identity,lo:u16,hi:u16,now:u64)->usize{
        if owner.pid==0||owner.generation==0||hi<=lo{return 0;}
        self.leases.iter().flatten().filter(|x|
            x.owner==owner&&x.port>=lo&&x.port<hi&&x.deadline>now).count()
    }
    // 4. Oldest currently active lease on a bounded numeric port range.
    pub fn earliest_lease_in_range(&self,lo:u16,hi:u16,now:u64)->Option<PortLease>{
        if lo<1024||hi<lo{return None;}
        self.leases.iter().flatten().filter(|x|
            x.port>=lo&&x.port<=hi&&x.deadline>now)
            .min_by_key(|x|x.deadline).copied()
    }
    // 5. Last active lease to expire inside one range.
    pub fn latest_lease_in_range(&self,lo:u16,hi:u16,now:u64)->Option<PortLease>{
        if lo<1024||hi<lo{return None;}
        self.leases.iter().flatten().filter(|x|
            x.port>=lo&&x.port<=hi&&x.deadline>now)
            .max_by_key(|x|x.deadline).copied()
    }
    // 6. Exact port version for one active owner, without peer disclosure.
    pub fn active_owner_lease(&self,owner:Identity,port:u16,now:u64)
        ->Option<PortLease>{
        if owner.pid==0||owner.generation==0{return None;}
        self.leases.iter().flatten().find(|x|
            x.owner==owner&&x.port==port&&x.deadline>now).copied()
    }
    // 7. Count distinct owner generations in active range without heap use.
    pub fn distinct_owners_in_range(&self,lo:u16,hi:u16,now:u64)->usize{
        if lo<1024||hi<lo{return 0;}
        let mut count=0;
        for i in 0..N{
            let Some(x)=self.leases[i]else{continue;};
            if x.port<lo||x.port>hi||x.deadline<=now{continue;}
            if !self.leases[..i].iter().flatten().any(|y|
                y.owner==x.owner&&y.port>=lo&&y.port<=hi&&y.deadline>now){
                count+=1;
            }
        }
        count
    }
    // 8. Snapshot owner port leases from a selected inclusive deadline range.
    pub fn owner_deadline_range(&self,owner:Identity,now:u64,lo:u64,hi:u64,
        out:&mut[PortLease])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||hi<lo{return Err(Error::Invalid);}
        let needed=self.leases.iter().flatten().filter(|x|
            x.owner==owner&&x.deadline>now&&x.deadline>=lo&&x.deadline<=hi).count();
        if out.len()<needed{return Err(Error::Full);}
        let mut at=0;
        for x in self.leases.iter().flatten(){
            if x.owner==owner&&x.deadline>now&&x.deadline>=lo&&x.deadline<=hi{
                out[at]=*x;at+=1;
            }
        }
        Ok(at)
    }
    // 9. Snapshot only the owner's active port numbers in one closed range.
    pub fn owner_active_ports_in_range(&self,owner:Identity,lo:u16,hi:u16,
        now:u64,out:&mut[u16])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||lo<1024||hi<lo{return Err(Error::Invalid);}
        let needed=self.owner_ports_between(owner,lo,hi,now);
        if out.len()<needed{return Err(Error::Full);}
        let mut at=0;
        for x in self.leases.iter().flatten(){
            if x.owner==owner&&x.port>=lo&&x.port<=hi&&x.deadline>now{
                out[at]=x.port;at+=1;
            }
        }
        Ok(at)
    }
    // 10. Verify a complete active owner record with exact observed deadline.
    pub fn exact_active_lease(&self,owner:Identity,port:u16,deadline:u64,
        now:u64)->bool{
        self.active_owner_lease(owner,port,now).is_some_and(|x|x.deadline==deadline)
    }
    // 11. Renew a port only when the supplied observed deadline still matches.
    pub fn renew_owned_if_version<const G:usize>(&mut self,auth:&Access<G>,
        owner:Identity,port:u16,now:u64,observed:u64,next:u64)
        ->Result<(),Error>{
        self.renew_exact(auth,owner,port,now,observed,next)
    }
    // 12. Shorten a matching lease to expire sooner, with SOCKET grant.
    pub fn shorten_owned_if_version<const G:usize>(&mut self,auth:&Access<G>,
        owner:Identity,port:u16,now:u64,observed:u64,next:u64)
        ->Result<(),Error>{
        self.shorten_exact(auth,owner,port,now,observed,next)
    }
    // 13. Release exactly one live numeric lease after version validation.
    pub fn release_owner_port_version(&mut self,owner:Identity,port:u16,
        observed:u64,now:u64)->Result<(),Error>{
        self.release_live_exact(owner,port,now,observed)
    }
    // 14. Renew only a nonempty unique set of exact owner port versions.
    pub fn renew_distinct_ports<const G:usize>(&mut self,auth:&Access<G>,
        owner:Identity,now:u64,updates:&[(u16,u64,u64)])->Result<usize,Error>{
        self.renew_batch(auth,owner,updates,now)
    }
    // 15. Shorten the deadlines of a unique set, preserving peers on errors.
    pub fn shorten_distinct_ports<const G:usize>(&mut self,auth:&Access<G>,
        owner:Identity,now:u64,updates:&[(u16,u64,u64)])->Result<usize,Error>{
        self.shorten_batch(auth,owner,updates,now)
    }
    // 16. Validate an exact owner lease set's versions without side effects.
    pub fn match_owner_port_versions(&self,owner:Identity,now:u64,
        ports:&[(u16,u64)])->bool{
        self.require_deadline_batch(owner,now,ports).is_ok()
    }
    // 17. Atomic batch withdrawal only for current exact lease versions.
    pub fn withdraw_owner_port_versions(&mut self,owner:Identity,now:u64,
        ports:&[(u16,u64)])->Result<usize,Error>{
        self.revoke_exact_batch(owner,now,ports)
    }
    // 18. Release expired leases only for one owner inside port boundaries.
    pub fn reap_owner_expired_in_range(&mut self,owner:Identity,lo:u16,hi:u16,
        now:u64)->usize{
        if owner.pid==0||owner.generation==0||lo<1024||hi<lo{return 0;}
        let mut n=0;
        for item in &mut self.leases{
            if item.is_some_and(|x|x.owner==owner&&x.port>=lo&&x.port<=hi&&
                x.deadline<=now){
                *item=None;n+=1;
            }
        }
        n
    }
    // 19. Reclaim expired entries for all owners only in a bounded port region.
    pub fn reap_expired_port_window(&mut self,lo:u16,hi:u16,now:u64)->usize{
        if lo<1024||hi<lo{return 0;}
        let mut n=0;
        for item in &mut self.leases{
            if item.is_some_and(|x|x.port>=lo&&x.port<=hi&&x.deadline<=now){
                *item=None;n+=1;
            }
        }
        n
    }
    // 20. Validate table uniqueness for all *live* numeric ports.
    pub fn validate_live_port_table(&self,now:u64)->bool{
        for i in 0..N{
            let Some(x)=self.leases[i]else{continue;};
            if x.port<1024||x.owner.pid==0||x.owner.generation==0{return false;}
            if x.deadline<=now{continue;}
            if self.leases[..i].iter().flatten().any(|other|
                other.port==x.port&&other.deadline>now){return false;}
        }
        true
    }
}
#[cfg(test)]
mod r084_batch_vi_network_tests{
    use super::*;
    fn owner()->Identity{Identity{pid:554,generation:2}}
    fn peer()->Identity{Identity{pid:555,generation:2}}
    fn auth()->Access<2>{
        let mut x=Access::new();x.grant(owner(),SOCKET,90).unwrap();
        x.grant(peer(),SOCKET,90).unwrap();x
    }
    #[test]
    fn live_port_ranges_preserve_owner_epoch_and_atomic_versions(){
        let mut p=Ports::<6>::new();
        p.bind_batch(&auth(),owner(),&[8100,8101],1,20).unwrap();
        p.bind(&auth(),peer(),8200,1,35).unwrap();
        assert_eq!(p.live_owner_records(owner(),1),2);
        assert_eq!(p.owner_live_after_cutoff(owner(),1,20),2);
        assert_eq!(p.owner_port_range_count(owner(),8100,8102,1),2);
        assert_eq!(p.earliest_lease_in_range(8000,8300,1).unwrap().port,8100);
        assert_eq!(p.latest_lease_in_range(8000,8300,1).unwrap().port,8200);
        assert_eq!(p.active_owner_lease(owner(),8100,1).unwrap().deadline,20);
        assert_eq!(p.distinct_owners_in_range(8000,8300,1),2);
        let dummy=PortLease{owner:owner(),port:0,deadline:0};
        let mut tiny:[PortLease;0]=[];
        assert_eq!(p.owner_deadline_range(owner(),1,20,35,&mut tiny),Err(Error::Full));
        let mut leases=[dummy;6];
        assert_eq!(p.owner_deadline_range(owner(),1,20,35,&mut leases),Ok(2));
        let mut ports=[0u16;6];
        assert_eq!(p.owner_active_ports_in_range(owner(),8100,8101,1,&mut ports),Ok(2));
        assert!(p.exact_active_lease(owner(),8100,20,1));
        assert_eq!(p.renew_owned_if_version(&auth(),owner(),8100,2,20,30),Ok(()));
        assert_eq!(p.shorten_owned_if_version(&auth(),owner(),8101,2,20,15),Ok(()));
        assert_eq!(p.renew_distinct_ports(&auth(),owner(),2,&[(8100,30,40)]),Ok(1));
        assert_eq!(p.shorten_distinct_ports(&auth(),owner(),2,&[(8101,15,12)]),Ok(1));
        assert!(p.match_owner_port_versions(owner(),2,&[(8100,40),(8101,12)]));
        assert_eq!(p.release_owner_port_version(owner(),8101,12,2),Ok(()));
        assert_eq!(p.withdraw_owner_port_versions(owner(),2,&[(8100,40)]),Ok(1));
        assert_eq!(p.reap_owner_expired_in_range(peer(),8200,8200,35),1);
        assert_eq!(p.reap_expired_port_window(8000,8300,35),0);
        assert!(p.validate_live_port_table(35));
    }
}
