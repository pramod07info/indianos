//! M2 App Store view model backed by a trusted catalog and real persistent
//! inventory records. No framebuffer/window rendering, package download,
//! native process launch or cryptographic implementation lives here.
use crate::block::BlockDevice;
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,VIEW};
use crate::m2_sdk::Catalog;
use crate::m2_inventory::{Inventory,Error as InventoryError};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{NotInstalled,Installed,UpdateAvailable}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct AppCard{pub id:u32,pub available:u64,pub installed:u64,
 pub rights:u32,pub state:State}
#[derive(Debug,PartialEq,Eq)]
pub enum Error{Denied,Unknown,Disk(InventoryError)}
pub fn card<D:BlockDevice,const APPS:usize,const G:usize>(
 catalog:&Catalog<APPS>,inventory:&mut Inventory<D>,
 auth:&Access<G>,viewer:Identity,now:u64,id:u32)->Result<AppCard,Error>{
 auth.require(viewer,VIEW,now).map_err(|_|Error::Denied)?;
 let entry=catalog.get(id).ok_or(Error::Unknown)?;
 let local=inventory.installed(id).map_err(Error::Disk)?;
 let installed=local.map_or(0,|p|p.version);
 Ok(AppCard{id,available:entry.version,installed,rights:entry.rights,
  state:if installed==0{State::NotInstalled}
  else if installed<entry.version{State::UpdateAvailable}
  else{State::Installed}})
}
#[cfg(test)]mod m2_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 use crate::m2_sdk::{Package,ABI};
 const CAP:usize=100;
 struct Disk{data:[[u8;SECTOR_BYTES];CAP]}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{CAP as u64}
  fn read_sector(&mut self,i:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.data.get(i as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,i:u64,value:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.data.get_mut(i as usize).ok_or(BlockError::OutOfRange)?=*value;Ok(())
  }
 }
 #[test]fn m2_catalog_reads_live_installed_inventory_without_writes(){
  let mut catalog=Catalog::<1>::new();
  catalog.add(Package{id:9,version:3,abi:ABI,rights:1,
   deps:[0;4],payload_digest:[5;32]}).unwrap();
  let disk=Disk{data:[[0;SECTOR_BYTES];CAP]};
  let mut i=Inventory::from_mounted(crate::hier_vfs::HierVfs::format_blank(disk).unwrap());
  i.init_directory().unwrap();
  let viewer=Identity{pid:5,generation:1};
  let mut acl=Access::<1>::new();acl.grant(viewer,VIEW,30).unwrap();
  assert_eq!(card(&catalog,&mut i,&acl,viewer,1,9).unwrap(),
   AppCard{id:9,available:3,installed:0,rights:1,state:State::NotInstalled});
  assert!(matches!(card(&catalog,&mut i,&acl,
   Identity{generation:2,..viewer},1,9),Err(Error::Denied)));
  assert_eq!(card(&catalog,&mut i,&acl,viewer,31,9),Err(Error::Denied));
 }
}
