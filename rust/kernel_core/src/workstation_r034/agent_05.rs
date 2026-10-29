//! R034/05: Bounded editor find-next with wrap.
use super::Error;
pub fn next(haystack:&[u8],needle:&[u8],cursor:usize,wrap:bool)->Result<Option<usize>,Error>{if needle.is_empty()||cursor>haystack.len(){return Err(Error::Invalid);}let found=haystack[cursor..].windows(needle.len()).position(|x|x==needle).map(|p|p+cursor);Ok(found.or_else(||if wrap{haystack[..cursor].windows(needle.len()).position(|x|x==needle)}else{None}))}
#[cfg(test)] mod tests {use super::*;#[test]fn functional_and_boundary_cases(){assert_eq!(next(b"ab-ab",b"ab",3,false),Ok(Some(3)));assert_eq!(next(b"ab-ab",b"ab",5,true),Ok(Some(0)));}}
