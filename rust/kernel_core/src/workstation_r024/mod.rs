//! R024 Developer Workstation - 20 scoped feature branches compiled in one integration.
//! Actual operations run against the kernel RamFs file table. This is NOT a
//! complete graphical IDE, compiled toolchain, general shell executor, or
//! physically durable filesystem. Trusted kernel/user ingress is still needed
//! for user authentication, syscall routing and external build execution.
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
 #[test]fn project_live_files_editor_terminal_and_build_metadata_workflow(){
  assert_eq!(agent_20::smoke(),Ok(()));
 }
 #[test]fn file_version_and_session_generation_isolate_user_changes(){
  use crate::batch5_session::Identity;
  use crate::batch7_ramfs::RamFs;
  let actor=Identity{pid:7,generation:1};
  let stranger=Identity{pid:7,generation:2};
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/src/a",b"one").unwrap();
  let mut locks=agent_16::Locks::<1>::new();
  let token=locks.acquire(actor,&fs,b"/src/a",1,100).unwrap();
  assert_eq!(locks.check(stranger,&fs,b"/src/a",token,2),Err(Error::Denied));
  fs.replace(b"/src/a",b"two").unwrap();
  assert_eq!(locks.check(actor,&fs,b"/src/a",token,2),Err(Error::Stale));
 }
}
