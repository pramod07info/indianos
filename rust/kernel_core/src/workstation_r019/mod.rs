//! R019 developer workstation workflows: 20 bounded, no-allocator modules.
//! Real integration with the existing RAM filesystem, editor and trusted
//! package catalog. The kernel still lacks a full graphical IDE, native
//! compiler worker, real Git client, and production remote debugger.
//! Caller IDs must originate from a trusted scheduler/supervisor.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error { Invalid, Denied, Full, Missing, Stale, Conflict, Overflow, NotReady }
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
#[cfg(test)]
mod r019_integration_tests {
 use super::*;
 #[test]fn full_workstation_pipeline_reaches_release_without_bypassing_rights(){
  assert_eq!(agent_20::smoke(),Ok(()));
 }
 #[test]fn each_subsystem_is_linked_and_rejects_unauthorized_operations(){
  let mut debugger=agent_08::Session::<2>::new();
  assert_eq!(debugger.attach(1,2,false),Err(Error::Denied));
  let mut extensions=agent_15::Registry::<1>::new();
  assert!(!extensions.can_call(7,1,1));
  let mut preview=agent_16::Preview::new(7,1,8080).unwrap();
  assert_eq!(preview.start(8,1),Err(Error::Denied));
  let mut breaks=agent_07::Breakpoints::<1>::new();
  assert_eq!(breaks.hit(7,1,1),Err(Error::Missing));
 }
}
