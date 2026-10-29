//! R034/16: Test result report aggregation.
use super::Error;
#[derive(Debug,PartialEq,Eq)]pub struct Report{pub passed:usize,pub failed:usize,pub skipped:usize}pub fn aggregate(statuses:&[u8])->Result<Report,Error>{if statuses.is_empty(){return Err(Error::Missing);}let mut report=Report{passed:0,failed:0,skipped:0};for s in statuses{match s{0=>report.skipped+=1,1=>report.passed+=1,2=>report.failed+=1,_=>return Err(Error::Invalid)}}Ok(report)}
#[cfg(test)] mod tests {use super::*;#[test]fn functional_and_boundary_cases(){assert_eq!(aggregate(&[1,1,2,0]),Ok(Report{passed:2,failed:1,skipped:1}));assert_eq!(aggregate(&[3]),Err(Error::Invalid));}}
