//! M2 Phase 5A: dedicated 64 KiB crash-consistent package BLOB storage.
//!
//! This is NOT a replacement for the existing 8-file, 2 KiB durablefs;
//! it requires its OWN validated BlockWindow on a reserved 258-sector
//! partition. Never mount it over R027/R031 durablefs or user data.
//!
//! Data are checksummed with streamed SHA-256, not digitally signed here.
//! A trusted launcher MUST separately verify M2P5/6 manifest Ed25519,
//! rights, publisher trust and execution approval before staging a process.
//! A successful storage write is never executable authorization.
use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};
use crate::m2_sha256::Stream;

pub const BANK_SECTORS:u64=128;
pub const CAPACITY:usize=BANK_SECTORS as usize*SECTOR_BYTES;
pub const REQUIRED_SECTORS:u64=2+2*BANK_SECTORS;
const MAGIC:&[u8;8]=b"M2BLOB1!";
const VERSION:u16=1;
const HEADER_HASH_END:usize=68;
const HEADER_END:usize=100;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Info{
 pub id:u32,pub version:u64,pub length:u32,pub digest:[u8;32],
 pub generation:u64,pub bank:u8,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{
 Device(BlockError),Capacity,Corrupt,NotFound,WrongApp,Downgrade,
 Invalid,HashMismatch,NoFallback,Source,CounterExhausted,
}
impl From<BlockError> for Error{
 fn from(error:BlockError)->Self{Self::Device(error)}
}
/// Implementation may fetch from a network, an already verified disk
/// stream or a process-owned pipe. Caller must authenticate package
/// MANIFEST separately: this API checks bytes, not publisher identity.
pub trait BundleSource{
 fn read_exact(&mut self,offset:u32,out:&mut[u8])->Result<(),()>;
}
pub struct BlobStore<D:BlockDevice>{
 disk:D,active:Option<Info>,backup:Option<Info>,
}
fn header(info:Info)->[u8;SECTOR_BYTES]{
 let mut b=[0u8;SECTOR_BYTES];
 b[..8].copy_from_slice(MAGIC);
 b[8..10].copy_from_slice(&VERSION.to_le_bytes());
 b[10]=info.bank;
 b[12..20].copy_from_slice(&info.generation.to_le_bytes());
 b[20..24].copy_from_slice(&info.id.to_le_bytes());
 b[24..32].copy_from_slice(&info.version.to_le_bytes());
 b[32..36].copy_from_slice(&info.length.to_le_bytes());
 b[36..68].copy_from_slice(&info.digest);
 let tag=crate::m2_sha256::digest(&b[..HEADER_HASH_END]);
 b[HEADER_HASH_END..HEADER_END].copy_from_slice(&tag);
 b
}
fn parse_header(slot:u8,b:&[u8;SECTOR_BYTES])->Option<Info>{
 if &b[..8]!=MAGIC||
   u16::from_le_bytes([b[8],b[9]])!=VERSION||
   b[10]!=slot||b[11]!=0||b[HEADER_END..].iter().any(|x|*x!=0){
  return None;
 }
 let info=Info{
  bank:slot,
  generation:u64::from_le_bytes(b[12..20].try_into().ok()?),
  id:u32::from_le_bytes(b[20..24].try_into().ok()?),
  version:u64::from_le_bytes(b[24..32].try_into().ok()?),
  length:u32::from_le_bytes(b[32..36].try_into().ok()?),
  digest:b[36..68].try_into().ok()?,
 };
 if info.id==0||info.version==0||info.generation==0||
  info.length==0||info.length as usize>CAPACITY||
  crate::m2_sha256::digest(&b[..HEADER_HASH_END])!=b[HEADER_HASH_END..HEADER_END]{
  return None;
 }
 Some(info)
}
fn lba(bank:u8,block:u64)->u64{2+bank as u64*BANK_SECTORS+block}
impl<D:BlockDevice> BlobStore<D>{
 /// No formatting of existing data. The caller must provide an isolated,
 /// specifically provisioned partition, never an arbitrary whole disk.
 pub fn mount(mut disk:D)->Result<Self,Error>{
  if disk.sector_count()<REQUIRED_SECTORS{return Err(Error::Capacity);}
  let mut entries:[Option<Info>;2]=[None,None];
  let mut blank=true;
  for slot in 0..2usize{
   let mut buffer=[0u8;SECTOR_BYTES];
   disk.read_sector(slot as u64,&mut buffer)?;
   if buffer.iter().any(|x|*x!=0){blank=false;}
   if let Some(info)=parse_header(slot as u8,&buffer){
    if Self::disk_hash(&mut disk,info)?==info.digest{entries[slot]=Some(info);}
   }
  }
  let (active,backup)=match(entries[0],entries[1]){
   (None,None) if blank=>(None,None),
   (None,None)=>return Err(Error::Corrupt),
   (Some(a),None)|(None,Some(a))=>(Some(a),None),
   (Some(a),Some(b))=>{
    if a.id!=b.id||a.generation==b.generation{
     return Err(Error::Corrupt);
    }
    if a.generation>b.generation{(Some(a),Some(b))}
    else{(Some(b),Some(a))}
   }
  };
  Ok(Self{disk,active,backup})
 }
 fn disk_hash(disk:&mut D,info:Info)->Result<[u8;32],Error>{
  let mut sha=Stream::new();
  let mut left=info.length as usize;
  let mut buf=[0u8;SECTOR_BYTES];
  let mut sector=0u64;
  while left>0{
   disk.read_sector(lba(info.bank,sector),&mut buf)?;
   let used=left.min(SECTOR_BYTES);
   sha.update(&buf[..used]).map_err(|_|Error::Invalid)?;
   left-=used;sector+=1;
  }
  Ok(sha.finish())
 }
 pub fn active(&self)->Option<Info>{self.active}
 pub fn backup(&self)->Option<Info>{self.backup}
 pub fn into_device(self)->D{self.disk}
 /// Stages one app to the inactive bank; the prior bank remains unmodified
 /// until a fully flushed, reread and hashed new manifest is committed.
 /// The caller-supplied expected hash is ONLY an integrity check.
 pub fn stage<R:BundleSource>(&mut self,app:u32,version:u64,
  length:usize,expected:[u8;32],source:&mut R)->Result<Info,Error>{
  if app==0||version==0||length==0{return Err(Error::Invalid);}
  if length>CAPACITY{return Err(Error::Capacity);}
  if let Some(active)=self.active{
   if app!=active.id{return Err(Error::WrongApp);}
   if version<=active.version{return Err(Error::Downgrade);}
  }
  let generation=match self.active{
   None=>1,Some(active)=>active.generation.checked_add(1)
      .ok_or(Error::CounterExhausted)?,
  };
  let slot=self.active.map_or(0,|a|1-a.bank);
  let mut left=length;
  let mut index=0usize;
  let mut hash=Stream::new();
  while left>0{
   let used=left.min(SECTOR_BYTES);
   let mut buf=[0u8;SECTOR_BYTES];
   source.read_exact(index as u32,&mut buf[..used])
    .map_err(|_|Error::Source)?;
   hash.update(&buf[..used]).map_err(|_|Error::Invalid)?;
   self.disk.write_sector(lba(slot,index as u64/SECTOR_BYTES as u64),
    &buf)?;
   left-=used;index+=used;
  }
  if hash.finish()!=expected{return Err(Error::HashMismatch);}
  let next=Info{id:app,version,length:length as u32,digest:expected,
    generation,bank:slot};
  // Read after write: no trust in a partial or dishonest sector writer.
  if Self::disk_hash(&mut self.disk,next)?!=expected{
   return Err(Error::HashMismatch);
  }
  let record=header(next);
  self.disk.write_sector(slot as u64,&record)?;
  let mut observed=[0u8;SECTOR_BYTES];
  self.disk.read_sector(slot as u64,&mut observed)?;
  if observed!=record{return Err(Error::Corrupt);}
  self.backup=self.active;
  self.active=Some(next);
  Ok(next)
 }
 /// Full streaming verification and selected-range copy occur in the
 /// SAME pass over the device. Returned bytes are valid only on Ok().
 /// On any error the entire requested output is zeroed.
 pub fn verified_range(&mut self,offset:usize,out:&mut[u8])->Result<(),Error>{
  let outcome=(||{
   let entry=self.active.ok_or(Error::NotFound)?;
   let end=offset.checked_add(out.len()).ok_or(Error::Invalid)?;
   if end>entry.length as usize{return Err(Error::Invalid);}
   let mut buf=[0u8;SECTOR_BYTES];
   let mut h=Stream::new();
   let mut pos=0usize;
   while pos<entry.length as usize{
    self.disk.read_sector(lba(entry.bank,pos as u64/SECTOR_BYTES as u64),
       &mut buf)?;
    let read=(entry.length as usize-pos).min(SECTOR_BYTES);
    h.update(&buf[..read]).map_err(|_|Error::Invalid)?;
    let start=offset.max(pos);
    let stop=end.min(pos+read);
    if start<stop{
     out[start-offset..stop-offset]
       .copy_from_slice(&buf[start-pos..stop-pos]);
    }
    pos+=read;
   }
   if h.finish()!=entry.digest{return Err(Error::HashMismatch);}
   Ok(())
  })();
  if outcome.is_err(){out.fill(0);}
  outcome
 }
 /// Roll back to the OTHER fully verified bank by giving it the NEXT
 /// generation. Does not rewrite executable bytes and is crash-safe.
 pub fn rollback(&mut self)->Result<Info,Error>{
  let active=self.active.ok_or(Error::NoFallback)?;
  let previous=self.backup.ok_or(Error::NoFallback)?;
  if active.id!=previous.id||active.bank==previous.bank{
   return Err(Error::Corrupt);
  }
  if Self::disk_hash(&mut self.disk,previous)?!=previous.digest{
   return Err(Error::HashMismatch);
  }
  let generation=active.generation.checked_add(1)
    .ok_or(Error::CounterExhausted)?;
  let restored=Info{generation,..previous};
  let bytes=header(restored);
  self.disk.write_sector(previous.bank as u64,&bytes)?;
  let mut observed=[0u8;SECTOR_BYTES];
  self.disk.read_sector(previous.bank as u64,&mut observed)?;
  if observed!=bytes{return Err(Error::Corrupt);}
  self.backup=Some(active);
  self.active=Some(restored);
  Ok(restored)
 }
}
#[cfg(test)]
mod m2_tests{
 use super::*;
 use crate::block::BlockWindow;
 #[derive(Clone)]
 struct MemoryDisk{
  sectors:std::vec::Vec<[u8;SECTOR_BYTES]>,
  writes:usize, fail_write:Option<usize>,flip_lba:Option<u64>,
 }
 impl MemoryDisk{
  fn new(sectors:usize)->Self{
   Self{sectors:std::vec![[0;SECTOR_BYTES];sectors],writes:0,
    fail_write:None,flip_lba:None}
  }
 }
 impl BlockDevice for MemoryDisk{
  fn sector_count(&self)->u64{self.sectors.len() as u64}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
    ->Result<(),BlockError>{
   let sector=self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
   out.copy_from_slice(sector);
   Ok(())
  }
  fn write_sector(&mut self,lba:u64,input:&[u8;SECTOR_BYTES])
    ->Result<(),BlockError>{
   if self.fail_write==Some(self.writes){return Err(BlockError::Io);}
   let sector=self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?;
   self.writes+=1;*sector=*input;
   if self.flip_lba==Some(lba){sector[4]^=1;}
   Ok(())
  }
 }
 struct Slice<'a>(&'a[u8]);
 impl BundleSource for Slice<'_>{
  fn read_exact(&mut self,offset:u32,out:&mut[u8])->Result<(),()>{
   let o=offset as usize;
   let data=self.0.get(o..o+out.len()).ok_or(())?;
   out.copy_from_slice(data);Ok(())
  }
 }
 fn package(len:usize,seed:u8)->std::vec::Vec<u8>{
  (0..len).map(|x|(x.wrapping_mul(113) as u8)^seed).collect()
 }
 fn install<D:BlockDevice>(
  store:&mut BlobStore<D>,version:u64,bytes:&[u8]
 )->Result<Info,Error>{
  store.stage(9,version,bytes.len(),crate::m2_sha256::digest(bytes),
   &mut Slice(bytes))
 }
 #[test]fn m2_blob_64k_package_streams_reboots_upgrades_and_rolls_back(){
  let disk=MemoryDisk::new(REQUIRED_SECTORS as usize);
  let mut store=BlobStore::mount(disk).unwrap();
  assert_eq!(store.active(),None);
  let first=package(51000,4);
  let later=package(CAPACITY,5);
  assert_eq!(install(&mut store,1,&first).unwrap().bank,0);
  let mut store=BlobStore::mount(store.into_device()).unwrap();
  let mut part=[0u8;1600];
  store.verified_range(999,&mut part).unwrap();
  assert_eq!(&part[..],&first[999..2599]);
  assert_eq!(install(&mut store,2,&later).unwrap().bank,1);
  let mut store=BlobStore::mount(store.into_device()).unwrap();
  assert_eq!(store.active().unwrap().version,2);
  assert_eq!(store.backup().unwrap().version,1);
  store.verified_range(CAPACITY-1600,&mut part).unwrap();
  assert_eq!(&part[..],&later[CAPACITY-1600..]);
  assert_eq!(store.rollback().unwrap().version,1);
  let mut store=BlobStore::mount(store.into_device()).unwrap();
  assert_eq!(store.active().unwrap().version,1);
  store.verified_range(999,&mut part).unwrap();
  assert_eq!(&part[..],&first[999..2599]);
 }
 #[test]fn m2_blob_corrupted_active_bank_falls_back_to_previous_hash_valid_bank(){
  let mut store=BlobStore::mount(MemoryDisk::new(REQUIRED_SECTORS as usize)).unwrap();
  let first=package(20000,1);
  let next=package(30000,2);
  install(&mut store,1,&first).unwrap();
  install(&mut store,2,&next).unwrap();
  let mut disk=store.into_device();
  disk.sectors[lba(1,8) as usize][16]^=9;
  let mut store=BlobStore::mount(disk).unwrap();
  assert_eq!(store.active().unwrap().version,1);
  assert_eq!(store.backup(),None);
  let mut check=[0u8;32];
  store.verified_range(200,&mut check).unwrap();
  assert_eq!(&check[..],&first[200..232]);
 }
 #[test]fn m2_blob_interrupted_overflow_write_never_publishes_new_package(){
  let first=package(17000,1);
  let second=package(54000,2);
  for offset in [0usize,1,19,65,106]{
   let mut store=BlobStore::mount(MemoryDisk::new(REQUIRED_SECTORS as usize)).unwrap();
   install(&mut store,1,&first).unwrap();
   let mut disk=store.into_device();
   disk.fail_write=Some(disk.writes+offset);
   let mut store=BlobStore::mount(disk).unwrap();
   assert!(install(&mut store,2,&second).is_err());
   let mut disk=store.into_device();disk.fail_write=None;
   let mut restored=BlobStore::mount(disk).unwrap();
   assert_eq!(restored.active().unwrap().version,1);
   let mut check=[0u8;64];restored.verified_range(20,&mut check).unwrap();
   assert_eq!(&check[..],&first[20..84]);
  }
 }
 #[test]fn m2_blob_rejects_short_sources_bad_hash_and_oversize(){
  let mut store=BlobStore::mount(MemoryDisk::new(REQUIRED_SECTORS as usize)).unwrap();
  let old=package(9000,1);
  assert_eq!(store.stage(9,1,old.len(),[0;32],&mut Slice(&old)),
    Err(Error::HashMismatch));
  assert_eq!(store.active(),None);
  assert_eq!(store.stage(9,1,CAPACITY+1,[0;32],&mut Slice(&old)),
    Err(Error::Capacity));
  assert_eq!(store.stage(9,1,10000,[0;32],&mut Slice(&old)),
    Err(Error::Source));
  assert_eq!(store.active(),None);
  install(&mut store,1,&old).unwrap();
  assert_eq!(install(&mut store,1,&old),Err(Error::Downgrade));
  assert_eq!(store.stage(10,2,old.len(),
    crate::m2_sha256::digest(&old),&mut Slice(&old)),Err(Error::WrongApp));
 }
 #[test]fn m2_blob_corrupt_metadata_denies_trust_and_no_range_leaks(){
  let first=package(15000,3);
  let mut store=BlobStore::mount(MemoryDisk::new(REQUIRED_SECTORS as usize)).unwrap();
  install(&mut store,1,&first).unwrap();
  let mut disk=store.into_device();
  disk.sectors[0][36]^=1;
  assert!(matches!(BlobStore::mount(disk),Err(Error::Corrupt)));
  let mut store=BlobStore::mount(MemoryDisk::new(REQUIRED_SECTORS as usize)).unwrap();
  install(&mut store,1,&first).unwrap();
  let mut disk=store.into_device();disk.sectors[lba(0,3) as usize][17]^=7;
  // Existing store may hold a cached manifest, but read-time tampering
  // must NOT return any bytes as usable verified output.
  let mut store=BlobStore{disk,active:Some(Info{id:9,version:1,
   length:first.len() as u32,digest:crate::m2_sha256::digest(&first),
   generation:1,bank:0}),backup:None};
  let mut output=[0x5a;80];
  assert_eq!(store.verified_range(17,&mut output),Err(Error::HashMismatch));
  assert_eq!(output,[0;80]);
 }
 #[test]fn m2_blob_partition_window_guards_neighbors(){
  let mut disk=MemoryDisk::new(REQUIRED_SECTORS as usize+2);
  disk.sectors[0].fill(0x21);
  let last=disk.sectors.len()-1;
  disk.sectors[last].fill(0x22);
  {
   let win=BlockWindow::new(&mut disk,1,REQUIRED_SECTORS).unwrap();
   let mut store=BlobStore::mount(win).unwrap();
   install(&mut store,1,&package(41000,2)).unwrap();
  }
  assert_eq!(disk.sectors[0],[0x21;SECTOR_BYTES]);
  assert_eq!(disk.sectors.last().unwrap(),&[0x22;SECTOR_BYTES]);
 }
}
