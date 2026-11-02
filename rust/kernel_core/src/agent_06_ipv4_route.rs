//! In-memory IPv4 longest-prefix-match selection; no packet I/O.
pub const MAX_ROUTES: usize = 16;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route { pub network: u32, pub prefix: u8, pub next_hop: u32 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteError { TooMany, InvalidPrefix, UnalignedNetwork }
fn mask(prefix: u8) -> u32 {
    if prefix == 0 { 0 } else { u32::MAX << (32 - u32::from(prefix)) }
}
/// Match prevalidated routes. Equal prefix lengths select the first route.
pub fn lookup(routes: &[Route], destination: u32) -> Result<Option<Route>, RouteError> {
    if routes.len() > MAX_ROUTES { return Err(RouteError::TooMany); }
    let mut selected: Option<Route> = None;
    for &route in routes {
        if route.prefix > 32 { return Err(RouteError::InvalidPrefix); }
        let m = mask(route.prefix);
        if route.network & !m != 0 { return Err(RouteError::UnalignedNetwork); }
        if destination & m == route.network
            && selected.map_or(true, |old| route.prefix > old.prefix) {
            selected = Some(route);
        }
    }
    Ok(selected)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn specific_route_wins_over_default() {
        let routes = [Route { network: 0, prefix: 0, next_hop: 1 },
            Route { network: 0x0a00_0000, prefix: 8, next_hop: 2 },
            Route { network: 0x0a01_0203, prefix: 32, next_hop: 3 }];
        assert_eq!(lookup(&routes, 0x0a01_0203), Ok(Some(routes[2])));
        assert_eq!(lookup(&routes, 0x0a02_0001), Ok(Some(routes[1])));
        assert_eq!(lookup(&routes, 0x0808_0808), Ok(Some(routes[0])));
    }
    #[test]
    fn no_match_and_invalid_prefix() {
        assert_eq!(lookup(&[], 0), Ok(None));
        assert_eq!(lookup(&[Route { network: 0, prefix: 33, next_hop: 0 }], 0),
            Err(RouteError::InvalidPrefix));
    }
    #[test]
    fn reject_noncanonical_and_oversized_tables() {
        assert_eq!(lookup(&[Route { network: 1, prefix: 24, next_hop: 0 }], 0),
            Err(RouteError::UnalignedNetwork));
        let many = [Route { network: 0, prefix: 0, next_hop: 1 }; MAX_ROUTES + 1];
        assert_eq!(lookup(&many, 0), Err(RouteError::TooMany));
    }
}

/// Validate an entire route table before it can be used by a network service.
/// Duplicate prefix/network entries would otherwise depend on insertion order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteTableError { InvalidRoute(RouteError), Duplicate }
pub fn validate_table(routes: &[Route]) -> Result<(), RouteTableError> {
    lookup(routes, 0).map_err(RouteTableError::InvalidRoute)?;
    for (i, route) in routes.iter().enumerate() {
        for earlier in &routes[..i] {
            if earlier.network == route.network && earlier.prefix == route.prefix {
                return Err(RouteTableError::Duplicate);
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod table_tests {
    use super::*;
    #[test]
    fn accepts_overlapping_but_distinct_prefixes() {
        let routes = [Route { network: 0, prefix: 0, next_hop: 1 },
            Route { network: 0x0a00_0000, prefix: 8, next_hop: 2 }];
        assert_eq!(validate_table(&routes), Ok(()));
    }
    #[test]
    fn duplicate_route_is_rejected() {
        let route = Route { network: 0, prefix: 0, next_hop: 1 };
        assert_eq!(validate_table(&[route, route]), Err(RouteTableError::Duplicate));
    }
    #[test]
    fn malformed_entries_are_rejected_before_lookup() {
        assert_eq!(validate_table(&[Route { network: 2, prefix: 24, next_hop: 1 }]),
            Err(RouteTableError::InvalidRoute(RouteError::UnalignedNetwork)));
    }
}


// R007: bounded IPv4 routing table. No packets are sent by this policy layer.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ManagedRouteError{Invalid(RouteTableError),Capacity,Gateway,Missing}
pub struct RouteManager<const N:usize>{entries:[Route;N],len:usize}
impl<const N:usize> RouteManager<N>{
    pub const fn new()->Self{Self{entries:[Route{network:0,prefix:0,next_hop:0};N],len:0}}
    pub fn len(&self)->usize{self.len}
    pub fn replace(&mut self,routes:&[Route])->Result<usize,ManagedRouteError>{
        if routes.len()>N{return Err(ManagedRouteError::Capacity);}
        validate_table(routes).map_err(ManagedRouteError::Invalid)?;
        for r in routes{
            // The zero next hop designates an on-link route. Multicast,
            // loopback and limited broadcast cannot act as gateways.
            let hop=r.next_hop;
            if hop!=0&&(hop>>24==127||hop>=0xe000_0000){
                return Err(ManagedRouteError::Gateway);
            }
        }
        let mut staged=[Route{network:0,prefix:0,next_hop:0};N];
        staged[..routes.len()].copy_from_slice(routes);
        self.entries=staged;self.len=routes.len();
        Ok(self.len)
    }
    pub fn resolve(&self,destination:u32)->Result<Option<Route>,ManagedRouteError>{
        lookup(&self.entries[..self.len],destination)
            .map_err(|e|ManagedRouteError::Invalid(RouteTableError::InvalidRoute(e)))
    }
    pub fn remove(&mut self,network:u32,prefix:u8)->Result<(),ManagedRouteError>{
        let pos=self.entries[..self.len].iter().position(|r|
            r.network==network&&r.prefix==prefix).ok_or(ManagedRouteError::Missing)?;
        for i in pos..self.len-1{self.entries[i]=self.entries[i+1];}
        self.len-=1;self.entries[self.len]=Route{network:0,prefix:0,next_hop:0};
        Ok(())
    }
}
#[cfg(test)]mod r007_routes_tests{
    use super::*;
    fn r(net:u32,prefix:u8,hop:u32)->Route{
        Route{network:net,prefix,next_hop:hop}
    }
    #[test]fn selects_specific_route_and_removes_atomically(){
        let mut t=RouteManager::<3>::new();
        t.replace(&[r(0,0,0x0a00_0001),r(0x0a00_0000,8,0)]).unwrap();
        assert_eq!(t.resolve(0x0a04_0302),Ok(Some(r(0x0a00_0000,8,0))));
        assert_eq!(t.resolve(0x0808_0808),Ok(Some(r(0,0,0x0a00_0001))));
        t.remove(0x0a00_0000,8).unwrap();
        assert_eq!(t.resolve(0x0a04_0302),Ok(Some(r(0,0,0x0a00_0001))));
    }
    #[test]fn invalid_candidate_never_destroys_live_routes(){
        let mut t=RouteManager::<1>::new();
        t.replace(&[r(0,0,0x0a00_0001)]).unwrap();
        assert_eq!(t.replace(&[r(0,0,0xe000_0001)]),Err(ManagedRouteError::Gateway));
        assert_eq!(t.replace(&[r(0,0,1),r(0x0a00_0000,8,0)]),
            Err(ManagedRouteError::Capacity));
        assert_eq!(t.len(),1);
        assert_eq!(t.resolve(10),Ok(Some(r(0,0,0x0a00_0001))));
    }
    #[test]fn noncanonical_routes_and_unknown_deletion_reject(){
        let mut t=RouteManager::<2>::new();
        assert_eq!(t.replace(&[r(2,24,0)]),
            Err(ManagedRouteError::Invalid(RouteTableError::InvalidRoute(
                RouteError::UnalignedNetwork))));
        assert_eq!(t.remove(0,0),Err(ManagedRouteError::Missing));
    }
}


// R008: multiple candidate routes with link health and lease generations;
// longest-prefix and lowest-cost selection, no actual packet transmission.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct RouteCandidate{pub route:Route,pub metric:u16,pub link:u16,
 pub epoch:u64,pub healthy:bool}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum FailoverFault{Invalid,Full,Duplicate,Missing}
#[derive(Clone,Copy)]
pub struct FailoverRoutes<const N:usize>{entries:[Option<RouteCandidate>;N],leases:[u64;N]}
impl<const N:usize> FailoverRoutes<N>{
 pub const fn new()->Self{Self{entries:[None;N],leases:[0;N]}}
 pub fn len(&self)->usize{self.entries.iter().flatten().count()}
 pub fn add(&mut self,c:RouteCandidate)->Result<(),FailoverFault>{
  if c.link==0||c.epoch==0||validate_table(&[c.route]).is_err()||
   (c.route.next_hop!=0 &&
    (c.route.next_hop>>24==127||c.route.next_hop>=0xe000_0000)){
   return Err(FailoverFault::Invalid)
  }
  if self.entries.iter().flatten().any(|v|v.link==c.link&&
   v.route.network==c.route.network&&v.route.prefix==c.route.prefix){
   return Err(FailoverFault::Duplicate)
  }
  let index=self.entries.iter().position(Option::is_none).ok_or(FailoverFault::Full)?;
  self.entries[index]=Some(c);self.leases[index]=0;Ok(())
 }
 pub fn replace_batch(&mut self,items:&[RouteCandidate])->Result<usize,FailoverFault>{
  if items.is_empty()||items.len()>N{return Err(FailoverFault::Full)}
  let mut next=Self::new();
  for c in items{next.add(*c)?}
  *self=next;Ok(items.len())
 }
 pub fn set_health(&mut self,link:u16,epoch:u64,healthy:bool)
  ->Result<usize,FailoverFault>{
  if link==0||epoch==0{return Err(FailoverFault::Invalid)}
  let mut count=0;
  for item in &mut self.entries{
   if let Some(route)=item{
    if route.link==link{
     if route.epoch!=epoch{return Err(FailoverFault::Invalid)}
     count+=1;
    }
   }
  }
  if count==0{return Err(FailoverFault::Missing)}
  for item in &mut self.entries{
   if let Some(route)=item{if route.link==link{route.healthy=healthy}}
  }
  Ok(count)
 }
 pub fn resolve(&self,destination:u32)->Option<RouteCandidate>{
  let mut best:Option<RouteCandidate>=None;
  for c in self.entries.iter().flatten(){
   if !c.healthy{continue}
   let mask=if c.route.prefix==0{0}else{
    u32::MAX << (32-u32::from(c.route.prefix))
   };
   if destination&mask !=c.route.network{continue}
   if best.is_none_or(|old|c.route.prefix>old.route.prefix||
    (c.route.prefix==old.route.prefix&&c.metric<old.metric)){best=Some(*c)}
  }
  best
 }
 pub fn expire_link(&mut self,link:u16,epoch:u64)->usize{
  let mut removed=0;
  for slot in &mut self.entries{
   if slot.is_some_and(|v|v.link==link&&v.epoch==epoch){
    *slot=None;removed+=1
   }
  }
  removed
 }
}
#[cfg(test)]mod r008_network_failover_tests{
 use super::*;
 fn c(net:u32,prefix:u8,link:u16,metric:u16)->RouteCandidate{
  RouteCandidate{route:Route{network:net,prefix,next_hop:0},metric,link,
   epoch:4,healthy:true}
 }
 #[test]fn specific_route_overrides_fast_default(){
  let mut n=FailoverRoutes::<4>::new();
  n.add(c(0,0,1,1)).unwrap();n.add(c(0x0a00_0000,8,2,10)).unwrap();
  assert_eq!(n.resolve(0x0a01_0203).unwrap().link,2);
  assert_eq!(n.resolve(0x0808_0808).unwrap().link,1);
 }
 #[test]fn health_fallback_and_lease_expiry(){
  let mut n=FailoverRoutes::<3>::new();
  n.add(c(0,0,1,10)).unwrap();n.add(c(0,0,2,20)).unwrap();
  assert_eq!(n.resolve(123).unwrap().link,1);
  assert_eq!(n.set_health(1,3,false),Err(FailoverFault::Invalid));
  n.set_health(1,4,false).unwrap();
  assert_eq!(n.resolve(123).unwrap().link,2);
  assert_eq!(n.expire_link(2,4),1);
  assert_eq!(n.resolve(123),None);
 }
 #[test]fn invalid_batch_keeps_original_routes(){
  let mut n=FailoverRoutes::<2>::new();n.add(c(0,0,1,1)).unwrap();
  assert_eq!(n.replace_batch(&[c(0,0,2,1),c(0,0,2,2)]),
   Err(FailoverFault::Duplicate));
  assert_eq!(n.resolve(123).unwrap().link,1);
 }
}


// R114: updates to a bounded network policy snapshot are staged before
// publication; no hardware interface or guest packets are activated.
impl<const N:usize> RouteManager<N> {
    /// Replace a matching (network,prefix) route or append a new route.
    /// The previous table survives invalid gateways, duplicates and capacity.
    pub fn upsert(&mut self, route:Route)->Result<(),ManagedRouteError>{
        let mut staged=self.entries;
        let existing=staged[..self.len].iter().position(|r|
            r.network==route.network && r.prefix==route.prefix);
        let count=if let Some(i)=existing{
            staged[i]=route;
            self.len
        } else {
            if self.len==N{return Err(ManagedRouteError::Capacity);}
            staged[self.len]=route;
            self.len+1
        };
        self.replace(&staged[..count])?;
        Ok(())
    }
    /// Remove a whole set of exact route keys or none of them.
    pub fn remove_many(&mut self,keys:&[(u32,u8)])
        ->Result<usize,ManagedRouteError>
    {
        for (i,&(network,prefix)) in keys.iter().enumerate(){
            if keys[..i].contains(&(network,prefix)) {
                return Err(ManagedRouteError::Missing);
            }
            if !self.entries[..self.len].iter().any(|r|
                r.network==network&&r.prefix==prefix){
                return Err(ManagedRouteError::Missing);
            }
        }
        let mut staged=[Route{network:0,prefix:0,next_hop:0};N];
        let mut len=0;
        for &route in &self.entries[..self.len]{
            if keys.contains(&(route.network,route.prefix)){continue;}
            staged[len]=route;
            len+=1;
        }
        self.replace(&staged[..len])?;
        Ok(keys.len())
    }
    /// Copy the table only when the destination fits. No partially copied
    /// policy is returned after a capacity error.
    pub fn snapshot(&self,out:&mut[Route])->Result<usize,ManagedRouteError>{
        if out.len()<self.len{return Err(ManagedRouteError::Capacity);}
        out[..self.len].copy_from_slice(&self.entries[..self.len]);
        Ok(self.len)
    }
    pub fn exact(&self,network:u32,prefix:u8)->Option<Route>{
        self.entries[..self.len].iter().find(|r|
            r.network==network&&r.prefix==prefix).copied()
    }
}

#[cfg(test)]
mod r114_route_update_tests {
    use super::*;
    fn route(network:u32,prefix:u8,next_hop:u32)->Route{
        Route{network,prefix,next_hop}
    }
    #[test]
    fn update_keeps_old_route_on_invalid_gateway(){
        let mut t=RouteManager::<2>::new();
        t.upsert(route(0,0,0x0101_0101)).unwrap();
        assert_eq!(t.upsert(route(0,0,0xe000_0001)),Err(ManagedRouteError::Gateway));
        assert_eq!(t.exact(0,0),Some(route(0,0,0x0101_0101)));
        t.upsert(route(0,0,0x0202_0202)).unwrap();
        assert_eq!(t.resolve(1),Ok(Some(route(0,0,0x0202_0202))));
    }
    #[test]
    fn remove_many_is_atomic_on_unknown_and_duplicate_keys(){
        let mut t=RouteManager::<3>::new();
        t.replace(&[route(0,0,1),route(0x0a00_0000,8,0),
            route(0xc0a8_0000,16,0)]).unwrap();
        assert_eq!(t.remove_many(&[(0,0),(0x1100_0000,8)]),
            Err(ManagedRouteError::Missing));
        assert_eq!(t.len(),3);
        assert_eq!(t.remove_many(&[(0,0),(0,0)]),Err(ManagedRouteError::Missing));
        assert_eq!(t.len(),3);
        assert_eq!(t.remove_many(&[(0,0),(0x0a00_0000,8)]),Ok(2));
        assert_eq!(t.len(),1);
        assert_eq!(t.exact(0xc0a8_0000,16),Some(route(0xc0a8_0000,16,0)));
    }
    #[test]
    fn snapshot_never_partially_exposes_a_route_table(){
        let mut t=RouteManager::<2>::new();
        t.upsert(route(0,0,10)).unwrap();
        t.upsert(route(0x0a00_0000,8,0)).unwrap();
        let mut short=[route(7,32,7)];
        assert_eq!(t.snapshot(&mut short),Err(ManagedRouteError::Capacity));
        assert_eq!(short,[route(7,32,7)]);
        let mut all=[route(0,0,0);2];
        assert_eq!(t.snapshot(&mut all),Ok(2));
        assert_eq!(all,[route(0,0,10),route(0x0a00_0000,8,0)]);
    }
    #[test]
    fn zero_capacity_cannot_publish_route(){
        let mut t=RouteManager::<0>::new();
        assert_eq!(t.upsert(route(0,0,1)),Err(ManagedRouteError::Capacity));
        assert_eq!(t.remove_many(&[]),Ok(0));
        assert_eq!(t.exact(0,0),None);
    }
}


// R114 follow-up: process-generation scoped route policy. Ownership is issued
// by a privileged coordinator, not inferred from untrusted packet data.
// This is still a routing-policy layer: no NIC or user syscall is activated.
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum OwnerRouteError { Invalid, Denied, Route(ManagedRouteError) }
pub struct OwnerRoutes<const N:usize>{
    owner:Option<crate::batch5_session::Identity>,
    generation:u64,
    routes:RouteManager<N>,
}
impl<const N:usize> OwnerRoutes<N>{
    pub const fn new()->Self{
        Self{owner:None,generation:0,routes:RouteManager::new()}
    }
    /// Only a trusted kernel supervisor may issue an owner identity here.
    pub fn bind_kernel(&mut self,owner:crate::batch5_session::Identity,
        generation:u64)->Result<(),OwnerRouteError>{
        if owner.pid==0||owner.generation==0||generation==0 {
            return Err(OwnerRouteError::Invalid)
        }
        if self.owner.is_some(){return Err(OwnerRouteError::Denied)}
        self.owner=Some(owner);
        self.generation=generation;
        Ok(())
    }
    fn check(&self,owner:crate::batch5_session::Identity,
        generation:u64)->Result<(),OwnerRouteError>{
        if generation==0||self.owner!=Some(owner)||self.generation!=generation {
            return Err(OwnerRouteError::Denied)
        }
        Ok(())
    }
    pub fn publish(&mut self,owner:crate::batch5_session::Identity,
        generation:u64,routes:&[Route])->Result<usize,OwnerRouteError>{
        self.check(owner,generation)?;
        self.routes.replace(routes).map_err(OwnerRouteError::Route)
    }
    pub fn select(&self,owner:crate::batch5_session::Identity,
        generation:u64,destination:u32)->Result<Option<Route>,OwnerRouteError>{
        self.check(owner,generation)?;
        self.routes.resolve(destination).map_err(OwnerRouteError::Route)
    }
    pub fn retire(&mut self,owner:crate::batch5_session::Identity,
        generation:u64)->Result<(),OwnerRouteError>{
        self.check(owner,generation)?;
        self.owner=None;self.generation=0;
        self.routes=RouteManager::new();Ok(())
    }
}
/// A route choice is valid only while the exact link-health epoch remains
/// admitted. Rechecking prevents use of an old next-hop after link restart.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct RouteReceipt { pub selected:RouteCandidate }
impl<const N:usize> FailoverRoutes<N> {
    pub fn receipt(&self,destination:u32)->Option<RouteReceipt>{
        self.resolve(destination).map(|selected|RouteReceipt{selected})
    }
    pub fn receipt_is_current(&self,destination:u32,token:RouteReceipt)->bool{
        self.resolve(destination)==Some(token.selected)
    }
}
#[cfg(test)]
mod r114_authenticated_route_policy_tests {
    use super::*;
    use crate::batch5_session::Identity;
    fn id(epoch:u64)->Identity{Identity{pid:9,generation:epoch}}
    #[test]
    fn pid_reuse_cannot_inherit_routes_from_old_generation(){
        let mut policy=OwnerRoutes::<3>::new();
        let default=[Route{network:0,prefix:0,next_hop:1}];
        policy.bind_kernel(id(1),1).unwrap();
        assert_eq!(policy.publish(id(1),1,&default),Ok(1));
        assert_eq!(policy.select(id(2),1,123),Err(OwnerRouteError::Denied));
        assert_eq!(policy.select(id(1),2,123),Err(OwnerRouteError::Denied));
        assert_eq!(policy.retire(id(2),1),Err(OwnerRouteError::Denied));
        policy.retire(id(1),1).unwrap();
        policy.bind_kernel(id(2),2).unwrap();
        assert_eq!(policy.select(id(2),2,123),Ok(None));
        assert_eq!(policy.select(id(1),1,123),Err(OwnerRouteError::Denied));
    }
    #[test]
    fn invalid_route_update_preserves_previous_authorized_route(){
        let mut policy=OwnerRoutes::<1>::new();
        policy.bind_kernel(id(1),7).unwrap();
        policy.publish(id(1),7,&[Route{network:0,prefix:0,next_hop:1}]).unwrap();
        assert_eq!(policy.publish(id(1),7,&[Route{network:0,prefix:0,next_hop:0xe000_0001}]),
            Err(OwnerRouteError::Route(ManagedRouteError::Gateway)));
        assert_eq!(policy.select(id(1),7,123),
            Ok(Some(Route{network:0,prefix:0,next_hop:1})));
    }
    #[test]
    fn link_epoch_receipt_invalidates_after_failure_and_rebind(){
        let mut routes=FailoverRoutes::<2>::new();
        routes.add(RouteCandidate{route:Route{network:0,prefix:0,next_hop:1},
            metric:5,link:1,epoch:7,healthy:true}).unwrap();
        let receipt=routes.receipt(0x0808_0808).unwrap();
        assert!(routes.receipt_is_current(0x0808_0808,receipt));
        routes.set_health(1,7,false).unwrap();
        assert!(!routes.receipt_is_current(0x0808_0808,receipt));
        assert_eq!(routes.receipt(0x0808_0808),None);
        routes.set_health(1,7,true).unwrap();
        assert!(routes.receipt_is_current(0x0808_0808,receipt));
        routes.expire_link(1,7);
        assert!(!routes.receipt_is_current(0x0808_0808,receipt));
    }
    #[test]
    fn unauthorized_or_zero_identity_does_not_mutate_tables(){
        let mut p=OwnerRoutes::<0>::new();
        assert_eq!(p.bind_kernel(Identity{pid:0,generation:1},1),
            Err(OwnerRouteError::Invalid));
        p.bind_kernel(id(3),3).unwrap();
        assert_eq!(p.publish(id(3),3,&[Route{network:0,prefix:0,next_hop:1}]),
            Err(OwnerRouteError::Route(ManagedRouteError::Capacity)));
        assert_eq!(p.select(id(3),3,1),Ok(None));
    }
}


// R114: a zero deadline is a static route. Other routes have monotonic
// deadlines supplied by the trusted link manager, not by ring-3 packets.
impl<const N:usize> FailoverRoutes<N>{
 pub fn add_dhcp_lease(&mut self,route:RouteCandidate,now:u64,
                        expires_at:u64)->Result<(),FailoverFault>{
  if expires_at<=now||expires_at==0{return Err(FailoverFault::Invalid);}
  self.add(route)?;
  let index=self.entries.iter().position(|r|*r==Some(route))
    .ok_or(FailoverFault::Missing)?;
  self.leases[index]=expires_at;Ok(())
 }
 pub fn renew_dhcp_lease(&mut self,route:RouteCandidate,now:u64,
                          next:u64)->Result<(),FailoverFault>{
  let index=self.entries.iter().position(|r|*r==Some(route))
    .ok_or(FailoverFault::Missing)?;
  let prior=self.leases[index];
  if prior==0||now>=prior||next<=prior||next<=now{
   return Err(FailoverFault::Invalid)
  }
  self.leases[index]=next;Ok(())
 }
 pub fn expire_dhcp(&mut self,now:u64)->usize{
  let mut count=0;
  for i in 0..N{
   if self.entries[i].is_some()&&self.leases[i]!=0&&now>=self.leases[i]{
    self.entries[i]=None;self.leases[i]=0;count+=1;
   }
  }
  count
 }
 // Read-side lease check prevents using expired paths before next cleanup.
 pub fn resolve_at(&self,destination:u32,now:u64)->Option<RouteCandidate>{
  let mut staged=*self;
  staged.expire_dhcp(now);
  staged.resolve(destination)
 }
 // Atomic quarantine on network-interface reset; old generation is dead.
 pub fn begin_link_reset(&mut self,link:u16,old_epoch:u64,
                         next_epoch:u64)->Result<usize,FailoverFault>{
  if link==0||old_epoch==0||next_epoch<=old_epoch{
   return Err(FailoverFault::Invalid)
  }
  let mut n=0;
  for r in self.entries.iter().flatten(){
   if r.link==link{
    if r.epoch!=old_epoch{return Err(FailoverFault::Invalid)}
    n+=1;
   }
  }
  if n==0{return Err(FailoverFault::Missing)}
  for r in self.entries.iter_mut().flatten(){
   if r.link==link{r.epoch=next_epoch;r.healthy=false;}
  }
  Ok(n)
 }
 pub fn end_link_reset(&mut self,link:u16,epoch:u64)->Result<usize,FailoverFault>{
  self.set_health(link,epoch,true)
 }
}
#[cfg(test)]
mod r114_leased_routing_tests{
 use super::*;
 fn c(link:u16,epoch:u64,metric:u16)->RouteCandidate{
  RouteCandidate{route:Route{network:0,prefix:0,next_hop:0},
     metric,link,epoch,healthy:true}
 }
 #[test]
 fn expiry_falls_back_without_waiting_for_cleanup(){
  let mut routes=FailoverRoutes::<3>::new();
  routes.add(c(1,1,20)).unwrap();
  routes.add_dhcp_lease(c(2,2,1),100,120).unwrap();
  assert_eq!(routes.resolve_at(99,119).unwrap().link,2);
  assert_eq!(routes.resolve_at(99,120).unwrap().link,1);
  assert_eq!(routes.expire_dhcp(119),0);
  assert_eq!(routes.expire_dhcp(120),1);
  assert_eq!(routes.len(),1);
 }
 #[test]
 fn renewal_rejects_lease_replay_and_stale_link_epoch(){
  let mut routes=FailoverRoutes::<2>::new();let v=c(2,3,1);
  routes.add_dhcp_lease(v,10,20).unwrap();
  assert_eq!(routes.renew_dhcp_lease(v,20,30),Err(FailoverFault::Invalid));
  assert_eq!(routes.renew_dhcp_lease(v,12,20),Err(FailoverFault::Invalid));
  routes.renew_dhcp_lease(v,12,30).unwrap();
  assert!(routes.resolve_at(1,29).is_some());
  assert_eq!(routes.resolve_at(1,30),None);
  assert_eq!(routes.renew_dhcp_lease(c(2,4,1),13,40),Err(FailoverFault::Missing));
 }
 #[test]
 fn reset_quarantines_all_old_routes_and_keeps_peer(){
  let mut routes=FailoverRoutes::<3>::new();
  routes.add(c(1,1,1)).unwrap();routes.add(c(2,1,2)).unwrap();
  let old=routes.receipt(9).unwrap();
  assert_eq!(routes.begin_link_reset(1,2,3),Err(FailoverFault::Invalid));
  assert!(routes.receipt_is_current(9,old));
  assert_eq!(routes.begin_link_reset(1,1,2),Ok(1));
  assert_eq!(routes.resolve(9).unwrap().link,2);
  assert!(!routes.receipt_is_current(9,old));
  assert_eq!(routes.end_link_reset(1,1),Err(FailoverFault::Invalid));
  assert_eq!(routes.end_link_reset(1,2),Ok(1));
  assert_eq!(routes.resolve(9).unwrap().link,1);
  assert!(!routes.receipt_is_current(9,old));
 }
 #[test]
 fn denied_lease_and_zero_capacity_are_atomic(){
  let mut empty=FailoverRoutes::<0>::new();
  assert_eq!(empty.add_dhcp_lease(c(1,1,1),1,2),Err(FailoverFault::Full));
  let mut r=FailoverRoutes::<1>::new();
  assert_eq!(r.add_dhcp_lease(c(1,1,1),20,20),Err(FailoverFault::Invalid));
  assert_eq!(r.len(),0);
 }
}



/// R115: network transmit callers with a trusted monotonic clock must use
/// time-bounded receipts rather than the older epoch-only receipt. DHCP lease
/// expiry invalidates tokens even if no background sweep has run.
impl<const N: usize> FailoverRoutes<N> {
    pub fn receipt_at(&self, destination: u32, now: u64) -> Option<RouteReceipt> {
        self.resolve_at(destination, now)
            .map(|selected| RouteReceipt { selected })
    }

    pub fn receipt_is_current_at(
        &self, destination: u32, now: u64, token: RouteReceipt,
    ) -> bool {
        self.resolve_at(destination, now) == Some(token.selected)
    }
}

#[cfg(test)]
mod r115_lease_scoped_route_receipt_tests {
    use super::*;
    fn c(link: u16, metric: u16, epoch: u64) -> RouteCandidate {
        RouteCandidate { route: Route { network: 0, prefix: 0, next_hop: 0 },
            metric, link, epoch, healthy: true }
    }

    #[test]
    fn expiration_at_exact_deadline_invalidates_without_sweep() {
        let mut p = FailoverRoutes::<2>::new();
        p.add(c(1, 20, 1)).unwrap();
        p.add_dhcp_lease(c(2, 1, 1), 10, 20).unwrap();
        let before = p.receipt_at(0x08080808, 19).unwrap();
        assert_eq!(before.selected.link, 2);
        assert!(p.receipt_is_current_at(0x08080808, 19, before));
        assert!(!p.receipt_is_current_at(0x08080808, 20, before));
        assert_eq!(p.receipt_at(0x08080808, 20).unwrap().selected.link, 1);
        assert_eq!(p.len(), 2); // a read cannot mutate the table
    }

    #[test]
    fn interface_quarantine_changes_the_best_route_receipt() {
        let mut p = FailoverRoutes::<2>::new();
        p.add(c(1, 1, 2)).unwrap();
        p.add(c(2, 2, 3)).unwrap();
        let before = p.receipt_at(22, 10).unwrap();
        assert_eq!(before.selected.link, 1);
        p.begin_link_reset(1, 2, 3).unwrap();
        assert!(!p.receipt_is_current_at(22, 10, before));
        assert_eq!(p.receipt_at(22, 10).unwrap().selected.link, 2);
        p.end_link_reset(1, 3).unwrap();
        assert!(!p.receipt_is_current_at(22, 10, before));
        assert_eq!(p.receipt_at(22, 10).unwrap().selected.link, 1);
    }

    #[test]
    fn lease_renewal_retains_receipt_only_until_new_expiry() {
        let mut p = FailoverRoutes::<1>::new();
        let route = c(4, 1, 9);
        p.add_dhcp_lease(route, 10, 20).unwrap();
        let receipt = p.receipt_at(77, 15).unwrap();
        p.renew_dhcp_lease(route, 15, 30).unwrap();
        assert!(p.receipt_is_current_at(77, 20, receipt));
        assert!(p.receipt_is_current_at(77, 29, receipt));
        assert!(!p.receipt_is_current_at(77, 30, receipt));
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn absent_routes_zero_capacity_and_explicit_removal_fail_closed() {
        let empty = FailoverRoutes::<0>::new();
        assert_eq!(empty.receipt_at(1, 0), None);
        let mut p = FailoverRoutes::<1>::new();
        p.add(c(3, 1, 7)).unwrap();
        let receipt = p.receipt_at(1, 1).unwrap();
        assert_eq!(p.expire_link(3, 7), 1);
        assert!(!p.receipt_is_current_at(1, 1, receipt));
        assert_eq!(p.receipt_at(1, 1), None);
    }
}
