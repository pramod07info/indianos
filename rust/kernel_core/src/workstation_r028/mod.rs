//! IndianOS R028: 20 independently scoped major Developer Workstation Rust modules.
//! Real byte manipulation operates on guest RamFs; builds/debugger require trusted workers.
//! RAM snapshots/preferences do not imply physical-disk persistence or complete GUI rendering.
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
 #[test]fn source_edit_terminal_build_debug_and_workstation_guest_flow(){
  assert_eq!(agent_20::smoke(),Ok(()));
 }
 #[test]fn false_source_revisions_are_refused_before_atomic_file_changes(){
  use crate::batch7_ramfs::RamFs;
  let mut fs=RamFs::<1>::new();fs.replace(b"/project/a",b"old").unwrap();
  let patch=crate::workstation_r027::agent_05::Patch{
   start:0,end:3,expected:b"old",replacement:b"new"};
  assert_eq!(crate::workstation_r027::agent_05::apply(&mut fs,b"/project/a",0,patch),
   Err(crate::workstation_r027::Error::Stale));
 }
}
