//! R027/19: owner-bound verified workspace snapshot export/import.
//! In-memory UI state only; durable filesystem checkpoints are a separate layer.
use super::Error;
use crate::batch5_session::Identity;
pub const BYTES:usize=512;
#[derive(Clone,Copy)]
pub struct Snapshot{pub owner:Identity,pub format:u32,
 pub len:usize,pub checksum:u64,pub body:[u8;BYTES]}
pub struct Store{snapshot:Option<Snapshot>}
fn digest(bytes:&[u8])->u64{
 let mut h=0xcbf29ce484222325u64;
 for &b in bytes{h=(h^(b as u64)).wrapping_mul(0x100000001b);}h
}
impl Store{
 pub const fn new()->Self{Self{snapshot:None}}
 pub fn save(&mut self,owner:Identity,format:u32,
 body:&[u8])->Result<(),Error>{
  if owner.pid==0||owner.generation==0||format==0||body.len()>BYTES{
   return Err(Error::Invalid);
  }
  let mut item=Snapshot{owner,format,len:body.len(),
   checksum:digest(body),body:[0;BYTES]};
  item.body[..body.len()].copy_from_slice(body);
  self.snapshot=Some(item);Ok(())
 }
 pub fn export(&self,owner:Identity,out:&mut[u8])
  ->Result<(u32,usize),Error>{
  let item=self.snapshot.ok_or(Error::Missing)?;
  if item.owner!=owner||owner.pid==0{return Err(Error::Denied);}
  if item.len>BYTES||digest(&item.body[..item.len])!=item.checksum{
   return Err(Error::Conflict);
  }
  if out.len()<item.len{return Err(Error::Full);}
  out[..item.len].copy_from_slice(&item.body[..item.len]);
  Ok((item.format,item.len))
 }
 pub fn import(&mut self,owner:Identity,format:u32,
 data:&[u8],expected_checksum:u64)->Result<(),Error>{
  if digest(data)!=expected_checksum{return Err(Error::Conflict);}
  self.save(owner,format,data)
 }
 pub fn forget(&mut self,owner:Identity)->Result<(),Error>{
  let x=self.snapshot.ok_or(Error::Missing)?;
  if owner.pid==0||x.owner!=owner{return Err(Error::Denied);}
  self.snapshot=None;Ok(())
 }
 pub fn checksum(&self,owner:Identity)->Result<u64,Error>{
  let x=self.snapshot.ok_or(Error::Missing)?;
  if owner.pid==0||x.owner!=owner{return Err(Error::Denied);}
  Ok(x.checksum)
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn export_and_reimport_exact_verified_workspace_layout(){
  let mut store=Store::new();store.save(id(1),1,b"tabs=2;split=1").unwrap();
  let mut bytes=[0u8;64];let (format,n)=store.export(id(1),&mut bytes).unwrap();
  let check=store.checksum(id(1)).unwrap();
  let mut second=Store::new();
  second.import(id(1),format,&bytes[..n],check).unwrap();
  assert_eq!(second.export(id(1),&mut bytes),Ok((1,n)));
 }
 #[test]fn corrupt_import_checksum_cannot_publish_new_workspace(){
  let mut store=Store::new();
  assert_eq!(store.import(id(1),1,b"tampered",123),Err(Error::Conflict));
  assert_eq!(store.export(id(1),&mut [0;32]),Err(Error::Missing));
 }
 #[test]fn another_process_generation_cannot_export_private_layout(){
  let mut store=Store::new();store.save(id(1),1,b"private").unwrap();
  assert_eq!(store.export(id(2),&mut [0;32]),Err(Error::Denied));
 }
}
