//! R034/08: Editor conflict-safe save decision.
use super::Error;
#[derive(Debug,PartialEq,Eq)]pub enum Decision{Save,Reload,Conflict}pub fn decide(disk_revision:u64,open_revision:u64,dirty:bool)->Decision{if disk_revision==open_revision{Decision::Save}else if !dirty{Decision::Reload}else{Decision::Conflict}}
#[cfg(test)] mod tests {use super::*;#[test]fn functional_and_boundary_cases(){assert_eq!(decide(2,1,true),Decision::Conflict);assert_eq!(decide(2,1,false),Decision::Reload);}}
