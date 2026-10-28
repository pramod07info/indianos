//! R023: 20 implementation tracks integrated as ONE guest-safe developer-workstation build.
//! Actual RAM VFS project source and read-only terminal commands run on existing
//! kernel-backed primitives; shell execution, native compiler binaries, secure
//! on-disk persistence and OS graphics are distinct unfinished integrations.
//! Each agent is a Git workstream, not an independent autonomous worker process.
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
 #[test]fn real_file_scaffold_terminal_build_test_release_and_ui_flow(){
  assert_eq!(agent_20::smoke(),Ok(()));
 }
 #[test]fn project_generation_rights_and_stale_file_contents_fail_closed(){
  use crate::batch5_session::Identity;
  use crate::batch7_ramfs::RamFs;
  let owner=Identity{pid:7,generation:1};
  let forged=Identity{pid:7,generation:2};
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/source",b"before").unwrap();
  let mut project=agent_06::History::<512,1>::new(owner);
  project.checkpoint(owner,&fs).unwrap();
  fs.replace(b"/source",b"after").unwrap();
  assert_eq!(project.undo(forged,&mut fs,2),Err(Error::Denied));
  let mut m=agent_18::Dispatcher::<1>::new(owner);
  assert_eq!(m.submit(forged,1,agent_18::Action::ReadFile,1,2),
   Err(Error::Denied));
 }
}
