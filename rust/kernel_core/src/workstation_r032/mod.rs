//! R032 Developer Workstation: bounded native guest project, editor, terminal, build and UI.
//! Models and RamFs operations require real integration into user-facing apps; the
//! test-only QEMU smoke is invoked with the existing test-r041-workspace feature.
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
 #[test]fn all_twenty_R032_workstreams_integrate_with_live_guest_files(){
  assert_eq!(agent_20::smoke(),Ok(()));
 }
 #[test]fn cross_project_mutation_and_stale_file_append_fail_closed(){
  use crate::batch7_ramfs::RamFs;
  use crate::batch5_session::Identity;
  use crate::workstation_r025::agent_02 as rights;
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/projects/p/a",b"old").unwrap();
  let who=Identity{pid:7,generation:1};
  let t=rights::grant(true,who,&fs,b"/projects/p",rights::WRITE,10).unwrap();
  assert_eq!(agent_13::append(&mut fs,who,t,1,b"/projects/p/a",0,b"new"),
   Err(Error::Stale));
  assert_eq!(agent_13::append(&mut fs,who,t,1,b"/private/a",1,b"new"),
   Err(Error::Denied));
  assert_eq!(fs.version(),1);
 }
}
