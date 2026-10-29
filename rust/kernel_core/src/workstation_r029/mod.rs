//! IndianOS R029: twenty real guest-file and workstation user workflows.
//! RAM-backed files and workspace state are not reboot durable without disk VFS.
//! Trusted external build/test/debugger workers must supply execution evidence.
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
 #[test]fn entire_live_workstation_guest_feature_acceptance(){
  assert_eq!(agent_20::smoke(),Ok(()));
 }
 #[test]fn stale_editors_cannot_overwrite_new_file_revisions(){
  use crate::batch7_ramfs::RamFs;
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"base").unwrap();
  fs.replace(b"/p/a",b"new").unwrap();
  assert_eq!(agent_02::merge(&mut fs,b"/p/a",1,b"base",b"incoming"),
   Err(Error::Stale));
 }
}
