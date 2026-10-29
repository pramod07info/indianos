//! R027: 20 substantial kernel-backed workstation workstreams.
//! Real changes are to RamFs guest bytes, not persistent disk; build/debug are
//! stateful policy registries requiring real trusted external execution.
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
 #[test]fn real_project_editor_build_terminal_debug_and_ui_workflow(){
  assert_eq!(agent_20::smoke(),Ok(()));
 }
 #[test]fn source_patch_rejects_other_editors_newer_revision(){
  use crate::batch7_ramfs::RamFs;
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  fs.replace(b"/a",b"new").unwrap();
  let patch=agent_05::Patch{start:0,end:3,expected:b"old",replacement:b"x"};
  assert_eq!(agent_05::apply(&mut fs,b"/a",1,patch),Err(Error::Stale));
 }
}
