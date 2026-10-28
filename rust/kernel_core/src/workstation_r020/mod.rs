//! R020: bounded native developer-workstation primitives and integration.
//!
//! This module is no_std-compatible and does not install a full GUI IDE, execute
//! external compilers, provide a real Git client, or persist RAM snapshots to disk.
//! Exact PID generations, trusted supervisors and per-resource authorization
//! remain mandatory ingress constraints in kernel integration.
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
 #[test]fn all_workstreams_wire_into_live_vfs_and_desktop(){
  assert_eq!(agent_20::smoke(),Ok(()));
 }
 #[test]fn stale_files_and_foreign_identity_never_update_live_project(){
  use crate::batch5_session::Identity;
  use crate::batch7_ramfs::RamFs;
  let owner=Identity{pid:1,generation:1};
  let bad=Identity{pid:1,generation:2};
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/safe",b"original").unwrap();
  let mut checkpoint=agent_01::Checkpoint::<256>::new();
  checkpoint.save(owner,&fs).unwrap();
  fs.replace(b"/safe",b"new").unwrap();
  assert_eq!(checkpoint.restore(bad,&mut fs,2),Err(Error::Denied));
  assert_eq!(checkpoint.restore(owner,&mut fs,1),Err(Error::Stale));
 }
}
