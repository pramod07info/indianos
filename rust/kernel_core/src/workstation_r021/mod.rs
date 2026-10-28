//! R021: twenty tested developer workstation features in a no_std kernel crate.
//! Each agent label is a separately committed workstream, not a separate
//! autonomous coding process. Trusted caller ingress is required for all
//! claimed user IDs, permissions, signer results, and supervisor events.
//! No full graphical IDE, native build worker, remote Git client, or
//! physically persistent filesystem is delivered by these primitives.
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
 #[test]fn complete_r021_workspace_pipeline(){
  assert_eq!(agent_20::smoke(),Ok(()));
 }
 #[test]fn build_and_developer_permissions_fail_closed(){
  let mut pool=agent_06::Pool::<1>::new(64,1);
  pool.submit(1,7,48,1,100).unwrap();pool.start(7,1,1).unwrap();
  assert_eq!(pool.finish(8,1,true),Err(Error::Missing));
  let mut checks=agent_14::Gate::new(7,10,11,0b11).unwrap();
  assert_eq!(checks.approve(7,11),Err(Error::Denied));
  assert_eq!(checks.ready(10),Err(Error::NotReady));
 }
}
