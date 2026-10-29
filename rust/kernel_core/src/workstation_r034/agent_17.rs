//! R034/17: Debugger trace monotonic timestamp guard.
use super::Error;
pub struct Timepoint{last:u64,events:usize}impl Timepoint{pub fn new()->Self{Self{last:0,events:0}}pub fn observe(&mut self,trusted:bool,timestamp:u64)->Result<usize,Error>{if !trusted{return Err(Error::Denied);}if timestamp<=self.last{return Err(Error::Stale);}self.last=timestamp;self.events+=1;Ok(self.events)}}
#[cfg(test)] mod tests {use super::*;#[test]fn functional_and_boundary_cases(){let mut t=Timepoint::new();assert_eq!(t.observe(true,10),Ok(1));assert_eq!(t.observe(true,10),Err(Error::Stale));}}
