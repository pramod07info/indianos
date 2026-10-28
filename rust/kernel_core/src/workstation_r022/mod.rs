//! R022 developer workstation: 20 multi-function kernel-safe workstreams.
//!
//! Real RamFs path and byte operations are implemented in agents 01-10;
//! trusted build/process admission and product validation in agents 11-19;
//! agent 20 combines actual file editing, source inspection, build, review and
//! workspace restore into one guest smoke and host regression suite.
//! This is not a finished graphical IDE or native compiler executor.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Denied,Full,Missing,Stale,Conflict,Overflow,NotReady}
pub mod agent_01;
pub mod agent_02;
pub mod agent_03;
pub mod agent_04;
pub mod agent_05;
pub mod agent_06;
pub mod agent_07;
pub mod agent_08;
pub mod agent_09;
pub mod agent_10;
pub mod agent_11;
pub mod agent_12;
pub mod agent_13;
pub mod agent_14;
pub mod agent_15;
pub mod agent_16;
pub mod agent_17;
pub mod agent_18;
pub mod agent_19;
pub mod agent_20;
#[cfg(test)]mod integration_tests{
 use super::*;
 #[test]fn full_source_edit_test_build_and_release_smoke(){
  assert_eq!(agent_20::smoke(),Ok(()));
 }
 #[test]fn project_source_and_reviewer_gates_are_fail_closed(){
  use crate::batch7_ramfs::RamFs;
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/a",b"source").unwrap();
  let mut catalog=agent_09::Export::<1>::new();
  catalog.capture(&fs).unwrap();
  fs.replace(b"/a",b"tampered").unwrap();
  assert_eq!(catalog.verify(&fs),Err(Error::Stale));
  let mut gate=agent_15::Gateway::<1>::new();
  gate.request(agent_15::Request{id:1,owner:7,rights:3,
   command:agent_15::Command::Compile,artifact:1,expires:10,
   status:agent_15::Status::Requested}).unwrap();
  assert_eq!(gate.authorize(1,7,1,1),Err(Error::Denied));
 }
}
