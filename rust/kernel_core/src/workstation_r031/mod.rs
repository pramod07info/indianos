//! IndianOS R031 Developer Workstation: 20 functional kernel-side workstreams.
//! Works on live in-guest RamFs and trusted policy evidence. This is not a
//! full graphical IDE, native debugger engine, installed compiler, nor durable disk VFS.
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
 #[test]fn workstation_20_track_guest_acceptance(){
  assert_eq!(agent_20::smoke(),Ok(()));
 }
 #[test]fn failed_multi_file_source_edit_never_overwrites_user_files(){
  use crate::batch7_ramfs::RamFs;
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/projects/p/a.rs",b"fn old(){}").unwrap();
  let edits=[agent_03::Target{path:b"/projects/p/a.rs",revision:99}];
  assert_eq!(agent_03::rename_project::<1,2048>(
   &mut fs,1,&edits,b"old",b"new"),Err(Error::Stale));
  assert_eq!(fs.version(),1);
 }
}
