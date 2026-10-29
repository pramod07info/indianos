//! R026/19: bounded CRC-protected workspace UI session checkpoint and recovery.
//! This is an in-memory serialization format; durable write to real disk is separate.
use super::Error;
use crate::batch5_session::Identity;
pub const CAP:usize=1024;
#[derive(Clone,Copy)]
pub struct Checkpoint{pub owner:Identity,pub version:u32,
 pub length:usize,pub checksum:u64,pub bytes:[u8;CAP]}
pub struct Recovery{current:Option<Checkpoint>}
fn checksum(bytes:&[u8])->u64{
 let mut h=0xcbf29ce484222325u64;
 for &b in bytes{h=(h^(b as u64)).wrapping_mul(0x100000001b);}h
}
impl Recovery{
 pub const fn new()->Self{Self{current:None}}
 pub fn save(&mut self,actor:Identity,version:u32,
  bytes:&[u8])->Result<(),Error>{
  if actor.pid==0||actor.generation==0||version==0||
   bytes.len()>CAP{return Err(Error::Invalid);}
  let mut item=Checkpoint{owner:actor,version,length:bytes.len(),
   checksum:checksum(bytes),bytes:[0;CAP]};
  item.bytes[..bytes.len()].copy_from_slice(bytes);
  self.current=Some(item);Ok(())
 }
 pub fn load(&self,actor:Identity,expected:u32,
  out:&mut[u8])->Result<usize,Error>{
  let item=self.current.ok_or(Error::Missing)?;
  if actor.pid==0||item.owner!=actor{return Err(Error::Denied);}
  if expected!=item.version{return Err(Error::Stale);}
  if item.length>CAP||checksum(&item.bytes[..item.length])!=item.checksum{
   return Err(Error::Conflict);
  }
  if out.len()<item.length{return Err(Error::Full);}
  out[..item.length].copy_from_slice(&item.bytes[..item.length]);Ok(item.length)
 }
 pub fn tamper(&mut self,supervisor:bool,index:usize,value:u8)
  ->Result<(),Error>{
  if !supervisor{return Err(Error::Denied);}
  let item=self.current.as_mut().ok_or(Error::Missing)?;
  if index>=item.length{return Err(Error::Invalid);}
  item.bytes[index]=value;Ok(())
 }
 pub fn clear(&mut self,actor:Identity)->Result<(),Error>{
  let item=self.current.ok_or(Error::Missing)?;
  if actor.pid==0||actor!=item.owner{return Err(Error::Denied);}
  self.current=None;Ok(())
 }
 pub fn version(&self)->Option<u32>{self.current.map(|x|x.version)}
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn restores_serialized_workspace_layout_and_tabs_bytes(){
  let mut r=Recovery::new();r.save(id(1),1,b"tabs:1,2;split:1").unwrap();
  let mut out=[0u8;32];let n=r.load(id(1),1,&mut out).unwrap();
  assert_eq!(&out[..n],b"tabs:1,2;split:1");
 }
 #[test]fn corrupted_snapshot_fails_integrity_check(){
  let mut r=Recovery::new();r.save(id(1),1,b"original").unwrap();
  r.tamper(true,0,b'X').unwrap();
  assert_eq!(r.load(id(1),1,&mut [0;32]),Err(Error::Conflict));
 }
 #[test]fn process_generation_prevents_restoring_foreign_ui_session(){
  let mut r=Recovery::new();r.save(id(1),1,b"tabs").unwrap();
  assert_eq!(r.load(id(2),1,&mut [0;32]),Err(Error::Denied));
 }
}
