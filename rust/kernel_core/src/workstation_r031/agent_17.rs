//! R031/17: release permit requiring matching source generation + trusted build/test evidence.
use super::{Error,agent_16};
use crate::batch7_ramfs::RamFs;
use crate::workstation_r030::{agent_16 as diagnostics,agent_17 as tests};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Permit{pub generation:u64,pub source_hash:u64,pub release_id:u64}
pub fn authorize<const F:usize,const N:usize>(owner:u64,trusted:bool,
 fs:&RamFs<F>,root:&[u8],compiler:&[u8],cache:&agent_16::Cache,
 ledger:&tests::Ledger<N>,report:diagnostics::Summary,release_id:u64)
 ->Result<Permit,Error>{
 if owner==0||!trusted||release_id==0{return Err(Error::Denied);}
 if !diagnostics::release_ready(report)||!ledger.green(owner).map_err(|_|Error::Denied)?{
  return Err(Error::Conflict);
 }
 if cache.decision(owner,fs,root,compiler)!=Ok(agent_16::BuildDecision::Cached){
  return Err(Error::Stale);
 }
 let (hash,count)=crate::workstation_r030::agent_14::fingerprint(fs,root,compiler)
  .map_err(|_|Error::Invalid)?;
 if count==0{return Err(Error::Missing);}
 Ok(Permit{generation:fs.version(),source_hash:hash,release_id})
}
pub fn current<const F:usize>(permit:Permit,fs:&RamFs<F>)->bool{
 permit.generation==fs.version()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn release_permit_requires_verified_build_tests_and_correct_source_revision(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/main.rs",b"fn a(){}").unwrap();
  let mut cache=agent_16::Cache::new(7);
  cache.certify(7,true,true,&fs,b"/p",b"rustc").unwrap();
  let mut ledger=tests::Ledger::<1>::new(7);
  ledger.record(7,true,tests::Test{suite:1,sequence:1,passed:true}).unwrap();
  let summary=diagnostics::parse(true,b"warning: unused").unwrap();
  let permit=authorize(7,true,&fs,b"/p",b"rustc",&cache,&ledger,summary,42).unwrap();
  assert!(current(permit,&fs));fs.replace(b"/p/main.rs",b"fn b(){}").unwrap();
  assert!(!current(permit,&fs));
 }
 #[test]fn compile_error_or_missing_tests_blocks_release(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/main.rs",b"fn a(){}").unwrap();
  let mut cache=agent_16::Cache::new(7);
  cache.certify(7,true,true,&fs,b"/p",b"rustc").unwrap();
  let ledger=tests::Ledger::<1>::new(7);
  let clean=diagnostics::parse(true,b"").unwrap();
  assert_eq!(authorize(7,true,&fs,b"/p",b"rustc",&cache,&ledger,clean,1),
   Err(Error::Conflict));
 }
}