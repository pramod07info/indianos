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
