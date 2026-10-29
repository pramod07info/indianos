//! R030: twenty compiled native guest workstreams for developer-workstation functionality.
//! Pure algorithms and actual RamFs file access. Not an installed IDE or compiler;
//! RAM-backed edits are not power-loss durable without a persistent VFS.
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
 #[test]fn all_20_subsystems_join_in_guest_acceptance(){
  assert_eq!(agent_20::smoke(),Ok(()));
 }
 #[test]fn write_paths_reject_stale_editor_before_mutation(){
  let mut fs=crate::batch7_ramfs::RamFs::<1>::new();
  fs.replace(b"/p/main.rs",b"let token=1;").unwrap();
  assert_eq!(agent_05::replace_all(&mut fs,b"/p/main.rs",2,b"token",b"other"),
   Err(Error::Stale));
  assert_eq!(fs.version(),1);
 }
}
