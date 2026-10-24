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
