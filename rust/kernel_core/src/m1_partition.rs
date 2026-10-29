//! M1: defensive x86-64 512-byte-sector MBR/GPT partition discovery.
//! Caller must EXPLICITLY select the entry ordinal; never automatically
//! choose, mount or format an arbitrary physical disk. No writes during
//! discovery. GPT 1.0 with 128-byte entries (<=128) is supported; extended
//! MBR partitions, backup-header recovery and exotic sector sizes are not.
use crate::block::{BlockDevice,BlockError,BlockWindow,ReadOnlyDevice,SECTOR_BYTES};
use crate::hier_vfs::HierVfs;
use crate::durablefs::{DurableFs,FsError,VolumeHealth,MIN_SECTORS};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Scheme{Mbr,Gpt}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Partition{pub scheme:Scheme,pub index:u32,pub start:u64,pub sectors:u64}
/// Disk and per-partition GUIDs pin an explicit GPT selection across reboot.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct GptSelection{
 pub partition:Partition,pub disk_guid:[u8;16],
 pub unique_partition_guid:[u8;16],
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Io,Invalid,Unsupported,NotFound,Overlap,OutOfRange,TooSmall}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum MountError{Partition(Error),Filesystem(FsError)}
impl From<BlockError> for Error{fn from(_:BlockError)->Self{Self::Io}}
pub const MAX_GPT_ENTRIES:usize=128;
/// Complete validated, read-only partition catalog. Sparse on-disk
/// ordinals are preserved, and callers never infer that slot 0 is present.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct PartitionSet{
 scheme:Scheme,
 slots:[Option<Partition>;MAX_GPT_ENTRIES],
 gpt_guids:[[u8;16];MAX_GPT_ENTRIES],disk_guid:[u8;16],
 count:usize,
}
impl PartitionSet{
 fn empty(scheme:Scheme)->Self{
  Self{scheme,slots:[None;MAX_GPT_ENTRIES],
   gpt_guids:[[0;16];MAX_GPT_ENTRIES],disk_guid:[0;16],count:0}
 }
 pub fn scheme(&self)->Scheme{self.scheme}
 pub fn len(&self)->usize{self.count}
 pub fn is_empty(&self)->bool{self.count==0}
 pub fn get(&self,index:u32)->Option<Partition>{
  self.slots.get(index as usize).copied().flatten()
 }
 /// MBR has no unique partition GUID; no synthetic GPT identity is supplied.
 pub fn get_gpt_selection(&self,index:u32)->Option<GptSelection>{
  if self.scheme!=Scheme::Gpt{return None;}
  let partition=self.get(index)?;
  Some(GptSelection{partition,disk_guid:self.disk_guid,
   unique_partition_guid:*self.gpt_guids.get(index as usize)?})
 }
 /// Iterate only live entries with original indices, in stable disk order.
 pub fn iter(&self)->impl Iterator<Item=Partition>+'_{
  self.slots.iter().flatten().copied()
 }
 /// Never expose a partially populated output when caller capacity is short.
 pub fn copy_to(&self,output:&mut[Partition])->Result<usize,Error>{
  if output.len()<self.count{return Err(Error::TooSmall);}
  for (item,slot) in self.iter().zip(output.iter_mut()){*slot=item;}
  Ok(self.count)
 }
 fn record(&mut self,partition:Partition)->Result<(),Error>{
  let slot=self.slots.get_mut(partition.index as usize).ok_or(Error::Invalid)?;
  if slot.is_some(){return Err(Error::Invalid);}
  *slot=Some(partition);self.count+=1;Ok(())
 }
}
fn le32(src:&[u8])->u32{u32::from_le_bytes(src.try_into().unwrap())}
fn le64(src:&[u8])->u64{u64::from_le_bytes(src.try_into().unwrap())}
fn crc_update(mut state:u32,bytes:&[u8])->u32{
 for &b in bytes{
  state^=b as u32;
  for _ in 0..8{state=(state>>1)^if state&1!=0{0xedb8_8320}else{0};}
 }
 state
}
fn crc(bytes:&[u8])->u32{!crc_update(!0,bytes)}
fn region(start:u64,count:u64,capacity:u64)->Result<(u64,u64),Error>{
 if count==0||start==0{return Err(Error::Invalid);}
 let end=start.checked_add(count).ok_or(Error::OutOfRange)?;
 if end>capacity{return Err(Error::OutOfRange);}
 Ok((start,end))
}
fn read<D:BlockDevice>(disk:&mut D,sector:u64)->Result<[u8;SECTOR_BYTES],Error>{
 let mut buf=[0u8;SECTOR_BYTES];
 disk.read_sector(sector,&mut buf)?;
 Ok(buf)
}
fn mbr<D:BlockDevice>(disk:&mut D,first:&[u8;SECTOR_BYTES])
 ->Result<PartitionSet,Error>{
 let capacity=disk.sector_count();
 let mut seen=[None;4];
 let mut found=PartitionSet::empty(Scheme::Mbr);
 for i in 0..4{
  let off=446+i*16;
  let slot=&first[off..off+16];
  if !matches!(slot[0],0|0x80){return Err(Error::Invalid);}
  let kind=slot[4];let start=le32(&slot[8..12]) as u64;
  let count=le32(&slot[12..16]) as u64;
  if kind==0 {
   if start!=0||count!=0{return Err(Error::Invalid);}
   continue;
  }
  if matches!(kind,0x05|0x0f|0x85|0xee){return Err(Error::Unsupported);}
  let (begin,end)=region(start,count,capacity)?;
  for &(other_start,other_end) in seen.iter().flatten(){
   if begin<other_end&&other_start<end{return Err(Error::Overlap);}
  }
  seen[i]=Some((begin,end));
  found.record(Partition{scheme:Scheme::Mbr,index:i as u32,start,sectors:end-start})?;
 }
 Ok(found)
}
fn gpt<D:BlockDevice>(disk:&mut D)->Result<PartitionSet,Error>{
 let capacity=disk.sector_count();
 if capacity<68{return Err(Error::Invalid);}
 let hdr=read(disk,1)?;
 if &hdr[..8]!=b"EFI PART"||le32(&hdr[8..12])!=0x0001_0000{
  return Err(Error::Invalid);
 }
 let size=le32(&hdr[12..16]) as usize;
 if !(92..=SECTOR_BYTES).contains(&size)||hdr[20..24]!=[0;4]{
  return Err(Error::Invalid);
 }
 let expected=le32(&hdr[16..20]);
 let mut check=hdr;
 check[16..20].fill(0);
 if crc(&check[..size])!=expected{return Err(Error::Invalid);}
 let alternate=le64(&hdr[32..40]);
 let first=le64(&hdr[40..48]);
 let last=le64(&hdr[48..56]);
 let table=le64(&hdr[72..80]);
 let entries=le32(&hdr[80..84]) as usize;
 let entry_size=le32(&hdr[84..88]) as usize;
 let checksum=le32(&hdr[88..92]);
 if le64(&hdr[24..32])!=1||alternate!=capacity-1||
   first<3||last<first||last>=capacity||last>=alternate||
   table<2||table>=first||entries==0||entries>MAX_GPT_ENTRIES||
   entry_size!=128{return Err(Error::Unsupported);}
 let total=entries.checked_mul(entry_size).ok_or(Error::OutOfRange)?;
 let sectors=total.div_ceil(SECTOR_BYTES);
 let table_end=table.checked_add(sectors as u64).ok_or(Error::OutOfRange)?;
 if table_end>first{return Err(Error::Invalid);}
 // The last usable LBA may not overlap the secondary GPT entry array
 // immediately preceding the alternate header. No data may be mounted there.
 if alternate<=sectors as u64 || last>=alternate-sectors as u64 {
  return Err(Error::Invalid);
 }
 // Verify the complete entry array BEFORE interpreting any partition.
 let mut calculated=!0u32;
 for s in 0..sectors{
  let buf=read(disk,table+s as u64)?;
  let n=total.saturating_sub(s*SECTOR_BYTES).min(SECTOR_BYTES);
  calculated=crc_update(calculated,&buf[..n]);
 }
 if !calculated!=checksum{return Err(Error::Invalid);}
 // Verify the backup GPT header and its *separate* entry array before
 // mounting any selected volume. A valid primary CRC alone cannot prove
 // that the device's secondary metadata is consistent after interrupted
 // partition-table writes. Never attempt automatic repair here.
 let backup_table=alternate.checked_sub(sectors as u64)
   .ok_or(Error::OutOfRange)?;
 let backup=read(disk,alternate)?;
 if &backup[..8]!=b"EFI PART"||
    le32(&backup[8..12])!=0x0001_0000||
    le32(&backup[12..16]) as usize!=size||
    backup[20..24]!=[0;4]||
    le64(&backup[24..32])!=alternate||
    le64(&backup[32..40])!=1||
    le64(&backup[40..48])!=first||
    le64(&backup[48..56])!=last||
    backup[56..72]!=hdr[56..72]||
    le64(&backup[72..80])!=backup_table||
    le32(&backup[80..84]) as usize!=entries||
    le32(&backup[84..88]) as usize!=entry_size||
    le32(&backup[88..92])!=checksum{
  return Err(Error::Invalid);
 }
 let mut checked_backup=backup;
 checked_backup[16..20].fill(0);
 if crc(&checked_backup[..size])!=le32(&backup[16..20]){
  return Err(Error::Invalid);
 }
 let mut backup_crc=!0u32;
 for s in 0..sectors{
  let sector=read(disk,backup_table+s as u64)?;
  let n=total.saturating_sub(s*SECTOR_BYTES).min(SECTOR_BYTES);
  backup_crc=crc_update(backup_crc,&sector[..n]);
  // Equal CRC32 values alone cannot rule out a colliding backup GPT table.
  let primary=read(disk,table+s as u64)?;
  if sector[..n]!=primary[..n]{return Err(Error::Invalid);}
 }
 if !backup_crc!=checksum{return Err(Error::Invalid);}
 let mut found=PartitionSet::empty(Scheme::Gpt);
 found.disk_guid.copy_from_slice(&hdr[56..72]);
 // Store the identity from the same fully validated layout as its geometry.
 for i in 0..entries{
  let byte=i*entry_size;
  let lba=table+(byte/SECTOR_BYTES) as u64;
  let sector=read(disk,lba)?;
  let off=byte%SECTOR_BYTES;
  let entry=&sector[off..off+entry_size];
  if entry[..16].iter().all(|&v|v==0){
   // An unused type GUID must not carry stale bounds or a unique identifier.
   if entry.iter().any(|&v|v!=0){return Err(Error::Invalid);}
   // An absent selected slot must not mask later corrupt live entries.
   continue;
  }
  if entry[16..32].iter().all(|&v|v==0){return Err(Error::Invalid);}
  // An unused entry has a zero GUID, so checking earlier slots is safe.
  // Forbid duplicated live partition identifiers before selecting a mount.
  if found.gpt_guids[..i].iter().any(|guid| guid.as_slice()==&entry[16..32]){
   return Err(Error::Invalid);
  }
  let start=le64(&entry[32..40]);let end=le64(&entry[40..48]);
  if start<first||end>last||end<start{return Err(Error::OutOfRange);}
  let exclusive=end.checked_add(1).ok_or(Error::OutOfRange)?;
  for prior in found.iter(){
   let prior_end=prior.start+prior.sectors; // already validated below capacity
   if start<prior_end&&prior.start<exclusive{return Err(Error::Overlap);}
  }
  found.gpt_guids[i].copy_from_slice(&entry[16..32]);
  found.record(Partition{scheme:Scheme::Gpt,index:i as u32,start,sectors:exclusive-start})?;
 }
 Ok(found)
}
/// Validate the complete MBR/GPT table, both GPT copies and all live
/// partition bounds before returning any read-only discovery result.
/// No formatting, repair, mounting or writes occur here.
pub fn discover<D:BlockDevice>(disk:&mut D)->Result<PartitionSet,Error>{
 let mut read_only=ReadOnlyDevice::new(disk);
 discover_verified(&mut read_only)
}
fn discover_verified<D:BlockDevice>(disk:&mut D)->Result<PartitionSet,Error>{
 if disk.sector_count()<2{return Err(Error::Invalid);}
 let first=read(disk,0)?;
 if first[510]!=0x55||first[511]!=0xaa{return Err(Error::Invalid);}
 let protective=(0..4).any(|i|first[446+i*16+4]==0xee);
 if protective{
  if first[450]!=0xee||first[446]!=0||le32(&first[454..458])!=1{
   return Err(Error::Invalid);
  }
  // Reject ambiguous hybrid layouts and multiple protective entries.
  // This is a strict read-only discovery policy, never a repair/formatter.
  for slot in 1..4{
   if first[446+slot*16..446+(slot+1)*16].iter().any(|&b|b!=0){
    return Err(Error::Unsupported);
   }
  }
  let covered=le32(&first[458..462]) as u64;
  if covered<(disk.sector_count()-1).min(u32::MAX as u64){
   return Err(Error::Invalid);
  }
  gpt(disk)
 }else{mbr(disk,&first)}
}
/// Select one existing ordinal only after validating the entire layout.
pub fn inspect<D:BlockDevice>(disk:&mut D,index:u32)->Result<Partition,Error>{
 discover(disk)?.get(index).ok_or(Error::NotFound)
}
/// The disk and partition GUIDs come from a fully read-only GPT validation.
pub fn inspect_gpt_selection<D:BlockDevice>(disk:&mut D,index:u32)
 ->Result<GptSelection,Error>{
 let catalog=discover(disk)?;
 if catalog.scheme()!=Scheme::Gpt{return Err(Error::Unsupported);}
 catalog.get_gpt_selection(index).ok_or(Error::NotFound)
}
/// Read-only validated disk geometry and independent COW metadata health.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct PartitionHealth{pub partition:Partition,pub volume:VolumeHealth}
/// Probe a user-selected partition without formatting or mounting it.
/// The whole partition table is validated, then both superblocks/payload
/// checksums are read from an isolated BlockWindow. No write is performed.
pub fn probe_volume<D:BlockDevice>(disk:&mut D,index:u32)
 ->Result<PartitionHealth,MountError>{
 let partition=inspect(disk,index).map_err(MountError::Partition)?;
 if partition.sectors<MIN_SECTORS{
  return Err(MountError::Partition(Error::OutOfRange));
 }
 let mut window=BlockWindow::new(disk,partition.start,partition.sectors)
  .map_err(|_|MountError::Partition(Error::OutOfRange))?;
 let mut read_only=ReadOnlyDevice::new(&mut window);
 let volume=DurableFs::inspect_health(&mut read_only)
  .map_err(MountError::Filesystem)?;
 Ok(PartitionHealth{partition,volume})
}
/// Mount a previously chosen partition only when its full on-disk geometry
/// still matches a fresh complete read-only layout validation. This closes a
/// time-of-check/time-of-use gap before bounded VFS selection. No formatting.
pub fn mount_checked<'a,D:BlockDevice>(disk:&'a mut D,
 expected:Partition)->Result<HierVfs<BlockWindow<'a,D>>,MountError>{
 let actual=inspect(disk,expected.index).map_err(MountError::Partition)?;
 if actual!=expected{return Err(MountError::Partition(Error::Invalid));}
 if actual.sectors<MIN_SECTORS{
  return Err(MountError::Partition(Error::OutOfRange));
 }
 let bounded=BlockWindow::new(disk,actual.start,actual.sectors)
  .map_err(|_|MountError::Partition(Error::OutOfRange))?;
 HierVfs::mount(bounded).map_err(MountError::Filesystem)
}
/// Strict preflight-selected mount. Refuse a changed GPT/MBR geometry or
/// any degraded COW filesystem. No autoformat, recovery or disk writes.
pub fn mount_checked_healthy<'a,D:BlockDevice>(disk:&'a mut D,
 expected:Partition)->Result<HierVfs<BlockWindow<'a,D>>,MountError>{
 // Check the user's selected geometry BEFORE reading any filesystem
 // metadata, so a stale selection is rejected independently of health.
 let fresh=inspect(disk,expected.index).map_err(MountError::Partition)?;
 if fresh!=expected{return Err(MountError::Partition(Error::Invalid));}
 let health=probe_volume(disk,expected.index)?;
 if health.partition!=expected{return Err(MountError::Partition(Error::Invalid));}
 if health.volume.redundancy!=crate::durablefs::Redundancy::Healthy{
  return Err(MountError::Filesystem(FsError::InvalidVolume));
 }
 let bounded=BlockWindow::new(disk,expected.start,expected.sectors)
  .map_err(|_|MountError::Partition(Error::OutOfRange))?;
 HierVfs::mount_healthy(bounded).map_err(MountError::Filesystem)
}
/// Mount previously provisioned IndianOS volume inside an explicitly chosen
/// partition. The disk is borrowed; the view cannot access neighboring
/// partitions. Neither inspection nor mount formats any disk.
/// Reject a changed disk GUID or partition GUID even when LBAs are unchanged.
/// Caller must quiesce the block device between preflight and mounting.
pub fn mount_gpt_selected_healthy<'a,D:BlockDevice>(disk:&'a mut D,
 expected:GptSelection)->Result<HierVfs<BlockWindow<'a,D>>,MountError>{
 let actual=inspect_gpt_selection(disk,expected.partition.index)
  .map_err(MountError::Partition)?;
 if actual!=expected{return Err(MountError::Partition(Error::Invalid));}
 mount_checked_healthy(disk,actual.partition)
}
pub fn mount<'a,D:BlockDevice>(disk:&'a mut D,index:u32)
 ->Result<HierVfs<BlockWindow<'a,D>>,MountError>{
 let part=inspect(disk,index).map_err(MountError::Partition)?;
 if part.sectors<MIN_SECTORS{return Err(MountError::Partition(Error::OutOfRange));}
 let bounded=BlockWindow::new(disk,part.start,part.sectors)
  .map_err(|_|MountError::Partition(Error::OutOfRange))?;
 HierVfs::mount(bounded).map_err(MountError::Filesystem)
}
#[cfg(test)]
mod m1_partition_tests{
 use super::*;
 const COUNT:usize=256;
 struct Disk{data:[[u8;SECTOR_BYTES];COUNT],writes:usize,fail_at:Option<usize>,fail_read:Option<u64>}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{COUNT as u64}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   if self.fail_read==Some(lba){return Err(BlockError::Io);}
   *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,input:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   if self.fail_at==Some(self.writes){return Err(BlockError::Io);}
   *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*input;
   self.writes+=1;Ok(())
  }
 }
 fn new()->Disk{Disk{data:[[0;SECTOR_BYTES];COUNT],writes:0,fail_at:None,fail_read:None}}
 fn mbr_disk()->Disk{
  let mut d=new();d.data[0][510]=0x55;d.data[0][511]=0xaa;
  let entry=&mut d.data[0][446..462];entry[4]=0x83;
  entry[8..12].copy_from_slice(&10u32.to_le_bytes());
  entry[12..16].copy_from_slice(&100u32.to_le_bytes());d
 }
 fn gpt_disk()->Disk{
  let mut d=new();d.data[0][510]=0x55;d.data[0][511]=0xaa;
  d.data[0][450]=0xee;d.data[0][454..458].copy_from_slice(&1u32.to_le_bytes());
  d.data[0][458..462].copy_from_slice(&255u32.to_le_bytes());
  // Four 128-byte GPT entries in one sector, with a single live volume.
  let entry=&mut d.data[2][..128];entry[..16].copy_from_slice(&[1;16]);
  entry[16..32].copy_from_slice(&[2;16]);
  entry[32..40].copy_from_slice(&40u64.to_le_bytes());
  entry[40..48].copy_from_slice(&139u64.to_le_bytes());
  let table_crc=crc(&d.data[2]);
  let h=&mut d.data[1];
  h[..8].copy_from_slice(b"EFI PART");
  h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
  h[12..16].copy_from_slice(&92u32.to_le_bytes());
  h[24..32].copy_from_slice(&1u64.to_le_bytes());
  h[32..40].copy_from_slice(&255u64.to_le_bytes());
  h[40..48].copy_from_slice(&34u64.to_le_bytes());
  h[48..56].copy_from_slice(&222u64.to_le_bytes());
  h[72..80].copy_from_slice(&2u64.to_le_bytes());
  h[80..84].copy_from_slice(&4u32.to_le_bytes());
  h[84..88].copy_from_slice(&128u32.to_le_bytes());
  h[88..92].copy_from_slice(&table_crc.to_le_bytes());
  let hcrc=crc(&h[..92]);h[16..20].copy_from_slice(&hcrc.to_le_bytes());
  sync_backup(&mut d);
  d
 }
 // Keep the synthetic secondary table/header fully independent and valid,
 // including the secondary header's own CRC and reversed LBA pointers.
 fn sync_backup(d:&mut Disk){
  d.data[254]=d.data[2];
  let mut header=d.data[1];
  header[24..32].copy_from_slice(&255u64.to_le_bytes());
  header[32..40].copy_from_slice(&1u64.to_le_bytes());
  header[72..80].copy_from_slice(&254u64.to_le_bytes());
  header[16..20].fill(0);
  let checksum=crc(&header[..92]);
  header[16..20].copy_from_slice(&checksum.to_le_bytes());
  d.data[255]=header;
 }
 #[test]fn m1_mbr_partition_mount_and_reboot_guards(){
  let mut disk=mbr_disk();
  assert_eq!(inspect(&mut disk,0),Ok(Partition{scheme:Scheme::Mbr,index:0,start:10,sectors:100}));
  assert_eq!(disk.writes,0);
  assert_eq!(mount(&mut disk,0).err(),Some(MountError::Filesystem(FsError::InvalidVolume)));
  assert_eq!(disk.writes,0); // discovery/mount never format
  {
   let win=BlockWindow::new(&mut disk,10,100).unwrap();
   let mut fs=HierVfs::format_blank(win).unwrap();
   fs.mkdir("/home").unwrap();fs.create("/home/note",b"persist").unwrap();
  }
  {
   let mut fs=mount(&mut disk,0).unwrap();
   let mut out=[0;32];
   assert_eq!(fs.read("/home/note",&mut out),Ok(7));
   assert_eq!(&out[..7],b"persist");
  }
  assert_eq!(disk.data[0][510..512],[0x55,0xaa]);
  assert_eq!(disk.data[9],[0;SECTOR_BYTES]);
  assert_eq!(disk.data[110],[0;SECTOR_BYTES]);
 }
 #[test]fn m1_gpt_verified_header_table_and_bounded_remount(){
  let mut disk=gpt_disk();
  assert_eq!(inspect(&mut disk,0),Ok(Partition{scheme:Scheme::Gpt,index:0,start:40,sectors:100}));
  assert_eq!(disk.writes,0);
  {
   let win=BlockWindow::new(&mut disk,40,100).unwrap();
   let mut fs=HierVfs::format_blank(win).unwrap();
   fs.mkdir("/home").unwrap();fs.create("/home/a",b"gpt").unwrap();
  }
  let mut fs=mount(&mut disk,0).unwrap();
  let mut out=[0;12];
  assert_eq!(fs.read("/home/a",&mut out),Ok(3));
  assert_eq!(&out[..3],b"gpt");
 }
 #[test]fn m1_gpt_rejects_duplicate_unique_partition_guids_without_writes(){
  // Both partitions are in range and nonoverlapping. The only invalidity
  // is that they claim the same unique volume identifier.
  let mut disk=gpt_disk();
  {
   let second=&mut disk.data[2][128..256];
   second[..16].fill(3); // distinct partition type
   second[16..32].fill(4); // initially distinct partition identifier
   second[32..40].copy_from_slice(&150u64.to_le_bytes());
   second[40..48].copy_from_slice(&199u64.to_le_bytes());
  }
  let update_crcs=|disk:&mut Disk|{
   let table_checksum=crc(&disk.data[2]);
   disk.data[1][88..92].copy_from_slice(&table_checksum.to_le_bytes());
   disk.data[1][16..20].fill(0);
   let header_checksum=crc(&disk.data[1][..92]);
   disk.data[1][16..20].copy_from_slice(&header_checksum.to_le_bytes());
   sync_backup(disk);
  };
  update_crcs(&mut disk);
  assert_eq!(inspect(&mut disk,0),Ok(Partition{
   scheme:Scheme::Gpt,index:0,start:40,sectors:100
  }));
  disk.data[2][144..160].fill(2); // duplicate first entry's unique GUID
  update_crcs(&mut disk); // both GPT tables and headers are CRC-valid
  assert_eq!(inspect(&mut disk,0),Err(Error::Invalid));
  assert_eq!(disk.writes,0); // reject before any mount or write
 }
 #[test]fn m1_corrupted_headers_entries_and_overlap_rejected_without_writes(){
  let mut disk=gpt_disk();
  disk.data[1][40]^=0x1;
  assert_eq!(inspect(&mut disk,0),Err(Error::Invalid));
  let mut disk=gpt_disk();disk.data[2][0]^=0x1;
  assert_eq!(inspect(&mut disk,0),Err(Error::Invalid));
  let mut disk=mbr_disk();
  disk.data[0][462+4]=0x83;
  disk.data[0][462+8..462+12].copy_from_slice(&50u32.to_le_bytes());
  disk.data[0][462+12..462+16].copy_from_slice(&40u32.to_le_bytes());
  assert_eq!(inspect(&mut disk,0),Err(Error::Overlap));
  assert_eq!(disk.writes,0);
 }
 #[test]fn m1_gpt_absent_slot_cannot_hide_corrupt_later_partition(){
  let mut disk=gpt_disk();
  disk.data[2][..128].fill(0); // requested index zero is unused
  let entry=&mut disk.data[2][128..256];
  entry[..16].fill(7);entry[16..32].fill(9);
  entry[32..40].copy_from_slice(&40u64.to_le_bytes());
  entry[40..48].copy_from_slice(&230u64.to_le_bytes()); // past last usable 222
  let table_crc=crc(&disk.data[2]);
  disk.data[1][88..92].copy_from_slice(&table_crc.to_le_bytes());
  disk.data[1][16..20].fill(0);
  let header_crc=crc(&disk.data[1][..92]);
  disk.data[1][16..20].copy_from_slice(&header_crc.to_le_bytes());
  sync_backup(&mut disk); // backup agrees; the live entry is still invalid
  assert_eq!(inspect(&mut disk,0),Err(Error::OutOfRange));
  assert_eq!(disk.writes,0);
 }
 #[test]fn m1_gpt_rejects_hybrid_protective_mbr_and_truncated_coverage(){
  let mut disk=gpt_disk();
  disk.data[0][462+4]=0x83; // shadow partition beside GPT protective entry
  assert_eq!(inspect(&mut disk,0),Err(Error::Unsupported));
  assert_eq!(disk.writes,0);
  let mut disk=gpt_disk();
  disk.data[0][458..462].copy_from_slice(&10u32.to_le_bytes());
  assert_eq!(inspect(&mut disk,0),Err(Error::Invalid));
  assert_eq!(disk.writes,0);
 }
 #[test]fn m1_gpt_rejects_data_claiming_backup_metadata_sectors(){
  let mut disk=gpt_disk();
  disk.data[1][48..56].copy_from_slice(&254u64.to_le_bytes());
  disk.data[1][16..20].fill(0);
  let header_crc=crc(&disk.data[1][..92]);
  disk.data[1][16..20].copy_from_slice(&header_crc.to_le_bytes());
  assert_eq!(inspect(&mut disk,0),Err(Error::Invalid));
  assert_eq!(disk.writes,0);
 }
 #[test]fn m1_partition_failed_cow_commit_recovers_previous_file_after_reboot(){
  let mut disk=mbr_disk();
  {
   let window=BlockWindow::new(&mut disk,10,100).unwrap();
   let mut fs=HierVfs::format_blank(window).unwrap();
   fs.mkdir("/home").unwrap();
   fs.create("/home/note",b"committed").unwrap();
  }
  // The second write in the next transaction fails. Do not publish a
  // partially written new file or touch any sector outside this partition.
  disk.fail_at=Some(disk.writes+1);
  {
   let mut fs=mount(&mut disk,0).unwrap();
   assert!(fs.overwrite("/home/note",b"interrupted").is_err());
  }
  disk.fail_at=None;
  {
   let mut fs=mount(&mut disk,0).unwrap();
   let mut out=[0u8;32];
   assert_eq!(fs.read("/home/note",&mut out),Ok(9));
   assert_eq!(&out[..9],b"committed");
  }
  assert_eq!(disk.data[9],[0;SECTOR_BYTES]);
  assert_eq!(disk.data[110],[0;SECTOR_BYTES]);
 }
 #[test]fn m1_gpt_requires_backup_header_at_end_of_selected_device(){
  let mut disk=gpt_disk();
  disk.data[1][32..40].copy_from_slice(&200u64.to_le_bytes());
  disk.data[1][16..20].fill(0);
  let hcrc=crc(&disk.data[1][..92]);
  disk.data[1][16..20].copy_from_slice(&hcrc.to_le_bytes());
  assert_eq!(inspect(&mut disk,0),Err(Error::Unsupported));
  assert_eq!(disk.writes,0);
 }
 #[test]fn m1_partition_remount_keeps_last_committed_overwrite(){
  let mut disk=mbr_disk();
  {
   let window=BlockWindow::new(&mut disk,10,100).unwrap();
   let mut fs=HierVfs::format_blank(window).unwrap();
   fs.mkdir("/home").unwrap();
   fs.create("/home/note",b"old").unwrap();
   fs.overwrite("/home/note",b"new committed").unwrap();
  }
  let before=disk.writes;
  {
   let mut fs=mount(&mut disk,0).unwrap();
   let mut output=[0u8;32];
   assert_eq!(fs.read("/home/note",&mut output),Ok(13));
   assert_eq!(&output[..13],b"new committed");
  }
  assert_eq!(disk.writes,before); // inspection and remount are read-only
  assert_eq!(disk.data[9],[0;SECTOR_BYTES]);
  assert_eq!(disk.data[110],[0;SECTOR_BYTES]);
 }
 #[test]fn m1_unrecognized_media_and_bad_bounds_fail_closed(){
  let mut disk=new();assert_eq!(inspect(&mut disk,0),Err(Error::Invalid));
  let mut disk=mbr_disk();
  disk.data[0][446+8..446+12].copy_from_slice(&255u32.to_le_bytes());
  assert_eq!(inspect(&mut disk,0),Err(Error::OutOfRange));
  let mut disk=mbr_disk();assert_eq!(inspect(&mut disk,1),Err(Error::NotFound));
  let mut disk=gpt_disk();assert_eq!(inspect(&mut disk,4),Err(Error::NotFound));
 }
 #[test]fn m1_gpt_backup_header_and_secondary_table_must_match_primary(){
  let mut disk=gpt_disk();
  assert_eq!(inspect(&mut disk,0),Ok(Partition{
   scheme:Scheme::Gpt,index:0,start:40,sectors:100
  }));
  assert_eq!(disk.writes,0);
  // The primary GPT remains valid; corrupting only the backup must fail.
  let mut bad_header=gpt_disk();
  bad_header.data[255][16]^=1;
  assert_eq!(inspect(&mut bad_header,0),Err(Error::Invalid));
  assert_eq!(bad_header.writes,0);
  let mut bad_table=gpt_disk();
  bad_table.data[254][0]^=1;
  assert_eq!(inspect(&mut bad_table,0),Err(Error::Invalid));
  assert_eq!(bad_table.writes,0);
  let mut mismatched_pointer=gpt_disk();
  mismatched_pointer.data[255][32..40]
   .copy_from_slice(&2u64.to_le_bytes());
  mismatched_pointer.data[255][16..20].fill(0);
  let checksum=crc(&mismatched_pointer.data[255][..92]);
  mismatched_pointer.data[255][16..20].copy_from_slice(&checksum.to_le_bytes());
  assert_eq!(inspect(&mut mismatched_pointer,0),Err(Error::Invalid));
  assert_eq!(mismatched_pointer.writes,0);
 }

 fn rechecksum_gpt(d:&mut Disk){
  let table_crc=crc(&d.data[2]);
  d.data[1][88..92].copy_from_slice(&table_crc.to_le_bytes());
  d.data[1][16..20].fill(0);
  let header_crc=crc(&d.data[1][..92]);
  d.data[1][16..20].copy_from_slice(&header_crc.to_le_bytes());
  sync_backup(d);
 }
 #[test]fn m1_r045_mbr_inventory_preserves_sparse_ordinals_without_writes(){
  let mut d=mbr_disk();
  let slot=&mut d.data[0][446+2*16..446+3*16];
  slot[4]=0x83;slot[8..12].copy_from_slice(&120u32.to_le_bytes());
  slot[12..16].copy_from_slice(&100u32.to_le_bytes());
  let scanned=discover(&mut d).unwrap();
  assert_eq!(scanned.scheme(),Scheme::Mbr);
  assert_eq!(scanned.len(),2);
  assert_eq!(scanned.get(0).unwrap().start,10);
  assert_eq!(scanned.get(1),None);
  assert_eq!(scanned.get(2).unwrap().start,120);
  assert_eq!(scanned.get(4),None);
  assert_eq!(scanned.iter().map(|x|x.index).collect::<Vec<_>>(),vec![0,2]);
  assert_eq!(inspect(&mut d,1),Err(Error::NotFound));
  assert_eq!(d.writes,0);
 }
 #[test]fn m1_r045_gpt_inventory_preserves_sparse_entries_and_validates_all(){
  let mut d=gpt_disk();
  let entry=&mut d.data[2][2*128..3*128];
  entry[..16].fill(4);entry[16..32].fill(5);
  entry[32..40].copy_from_slice(&145u64.to_le_bytes());
  entry[40..48].copy_from_slice(&200u64.to_le_bytes());
  rechecksum_gpt(&mut d);
  let scanned=discover(&mut d).unwrap();
  assert_eq!(scanned.scheme(),Scheme::Gpt);
  assert_eq!(scanned.len(),2);
  assert_eq!(scanned.get(1),None);
  assert_eq!(scanned.get(2).unwrap().sectors,56);
  assert_eq!(scanned.iter().map(|p|p.index).collect::<Vec<_>>(),vec![0,2]);
  assert_eq!(d.writes,0);
  d.data[254][256]^=1;
  assert_eq!(discover(&mut d),Err(Error::Invalid));
  assert_eq!(d.writes,0);
 }
 #[test]fn m1_r045_inventory_short_destination_is_unmodified(){
  let mut d=mbr_disk();
  let slot=&mut d.data[0][462..478];
  slot[4]=0x83;slot[8..12].copy_from_slice(&120u32.to_le_bytes());
  slot[12..16].copy_from_slice(&90u32.to_le_bytes());
  let set=discover(&mut d).unwrap();
  let original=Partition{scheme:Scheme::Gpt,index:99,start:999,sectors:1};
  let mut short=[original;1];
  assert_eq!(set.copy_to(&mut short),Err(Error::TooSmall));
  assert_eq!(short,[original]);
  let mut wide=[original;3];
  assert_eq!(set.copy_to(&mut wide),Ok(2));
  assert_eq!(wide[0].index,0);
  assert_eq!(wide[1].index,1);
  assert_eq!(wide[2],original);
  assert_eq!(d.writes,0);
 }
 #[test]fn m1_r045_read_errors_and_corrupted_layout_fail_closed(){
  let mut d=gpt_disk();d.fail_read=Some(255);
  assert_eq!(discover(&mut d),Err(Error::Io));
  assert_eq!(d.writes,0);
  d.fail_read=None;d.data[0][462+4]=0x83;
  assert_eq!(discover(&mut d),Err(Error::Unsupported));
  assert_eq!(d.writes,0);
 }
 #[test]fn m1_r045_checked_mount_detects_changed_geometry_before_writing(){
  let mut d=mbr_disk();
  let original=inspect(&mut d,0).unwrap();
  let stale=Partition{start:original.start+1,..original};
  assert!(matches!(mount_checked(&mut d,stale),
   Err(MountError::Partition(Error::Invalid))));
  assert_eq!(d.writes,0);
  {
   let win=BlockWindow::new(&mut d,10,100).unwrap();
   let mut fs=HierVfs::format_blank(win).unwrap();
   fs.mkdir("/docs").unwrap();fs.create("/docs/file",b"stable").unwrap();
  }
  let before=d.writes;
  {
   let mut fs=mount_checked(&mut d,original).unwrap();
   let mut out=[0;32];
   assert_eq!(fs.read("/docs/file",&mut out),Ok(6));
   assert_eq!(&out[..6],b"stable");
  }
  assert_eq!(d.writes,before);
  assert_eq!(d.data[9],[0;SECTOR_BYTES]);
  assert_eq!(d.data[110],[0;SECTOR_BYTES]);
 }
 #[test]fn m1_r045_partition_probe_reports_verified_healthy_then_degraded(){
  let mut d=gpt_disk();
  let part=inspect(&mut d,0).unwrap();
  assert_eq!(probe_volume(&mut d,0),
   Err(MountError::Filesystem(FsError::InvalidVolume)));
  assert_eq!(d.writes,0);
  {
   let win=BlockWindow::new(&mut d,part.start,part.sectors).unwrap();
   let mut fs=HierVfs::format_blank(win).unwrap();
   fs.mkdir("/docs").unwrap();fs.create("/docs/a",b"new").unwrap();
  }
  let before=d.writes;
  let health=probe_volume(&mut d,0).unwrap();
  assert_eq!(health.partition,part);
  assert_eq!(health.volume.valid_copies,2);
  assert_eq!(health.volume.redundancy,crate::durablefs::Redundancy::Healthy);
  assert_eq!(d.writes,before);
  d.data[40][19]^=1; // corrupt only selected window's first superblock
  let degraded=probe_volume(&mut d,0).unwrap();
  assert_eq!(degraded.volume.valid_copies,1);
  assert_eq!(degraded.volume.redundancy,crate::durablefs::Redundancy::Degraded);
  assert_eq!(d.writes,before);
  assert_eq!(d.data[39],[0;SECTOR_BYTES]);
  assert_eq!(d.data[140],[0;SECTOR_BYTES]);
 }
 #[test]fn m1_r045_failed_selected_probe_cannot_format_any_device(){
  let mut d=mbr_disk();
  let before=d.writes;
  assert_eq!(probe_volume(&mut d,0),
   Err(MountError::Filesystem(FsError::InvalidVolume)));
  assert_eq!(probe_volume(&mut d,1),Err(MountError::Partition(Error::NotFound)));
  assert_eq!(d.writes,before);
  assert_eq!(d.data[10],[0;SECTOR_BYTES]);
 }
 #[test]fn m1_r045_backup_gpt_metadata_must_match_before_any_volume_probe(){
  let mut d=gpt_disk();
  d.data[255][16]^=0x2;
  assert_eq!(probe_volume(&mut d,0),Err(MountError::Partition(Error::Invalid)));
  assert_eq!(d.writes,0);
 }

 #[test]fn m1_r046_strict_partition_mount_refuses_degraded_until_explicit_repair(){
  let mut disk=mbr_disk();
  let selected=inspect(&mut disk,0).unwrap();
  {
   let window=BlockWindow::new(&mut disk,10,100).unwrap();
   let mut fs=HierVfs::format_blank(window).unwrap();
   fs.mkdir("/docs").unwrap();
   fs.create("/docs/note",b"preserved").unwrap();
  }
  let original=disk.writes;
  disk.data[11][24]^=0x11; // older superblock corrupt
  assert_eq!(probe_volume(&mut disk,0).unwrap().volume.valid_copies,1);
  assert!(matches!(mount_checked_healthy(&mut disk,selected),
    Err(MountError::Filesystem(FsError::InvalidVolume))));
  assert_eq!(disk.writes,original);
  {
   let mut fs=mount_checked(&mut disk,selected).unwrap();
   let mut readback=[0u8;32];
   assert_eq!(fs.read("/docs/note",&mut readback),Ok(9));
   assert_eq!(&readback[..9],b"preserved");
   assert_eq!(fs.repair_redundancy(),Ok(true));
  }
  {
   let mut fs=mount_checked_healthy(&mut disk,selected).unwrap();
   let mut readback=[0u8;32];
   assert_eq!(fs.read("/docs/note",&mut readback),Ok(9));
   assert_eq!(&readback[..9],b"preserved");
  }
  assert_eq!(disk.data[9],[0;SECTOR_BYTES]);
  assert_eq!(disk.data[110],[0;SECTOR_BYTES]);
 }
 #[test]fn m1_r046_strict_mount_rechecks_geometry_and_rejects_unformatted_disk(){
  let mut disk=gpt_disk();
  let selected=inspect(&mut disk,0).unwrap();
  let before=disk.writes;
  assert!(matches!(mount_checked_healthy(&mut disk,selected),
   Err(MountError::Filesystem(FsError::InvalidVolume))));
  assert_eq!(disk.writes,before);
  let stale=Partition{sectors:selected.sectors-1,..selected};
  assert_eq!(mount_checked_healthy(&mut disk,stale).err(),
   Some(MountError::Partition(Error::Invalid)));
  assert_eq!(disk.writes,before);
 }
 
 #[test]fn m1_r048_rejects_gpt_backup_array_with_equal_crc_but_different_bytes(){
  let mut d=gpt_disk();
  let original=crc(&d.data[2]);
  // The two perturbations cancel under CRC32 but not byte-for-byte.
  d.data[254][56]^=1;
  d.data[254][112..116].copy_from_slice(&[0xd7,0x13,0x95,0x1d]);
  assert_eq!(crc(&d.data[254]),original);
  assert_eq!(le32(&d.data[255][88..92]),original);
  assert_eq!(inspect(&mut d,0),Err(Error::Invalid));
  assert_eq!(d.writes,0);
 }
 #[test]fn m1_r048_rejects_reused_partition_geometry_with_different_guid(){
  let mut d=gpt_disk();
  let selected=inspect_gpt_selection(&mut d,0).unwrap();
  assert_eq!(selected.unique_partition_guid,[2;16]);
  d.data[2][16..32].fill(7);rechecksum_gpt(&mut d);
  assert_eq!(inspect(&mut d,0),Ok(selected.partition));
  assert_eq!(mount_gpt_selected_healthy(&mut d,selected).err(),
   Some(MountError::Partition(Error::Invalid)));
  assert_eq!(d.writes,0);
 }
 #[test]fn m1_r048_rejects_disk_guid_change_even_when_geometry_matches(){
  let mut d=gpt_disk();
  let selected=inspect_gpt_selection(&mut d,0).unwrap();
  d.data[1][56..72].fill(8);rechecksum_gpt(&mut d);
  assert_eq!(inspect(&mut d,0),Ok(selected.partition));
  assert_eq!(mount_gpt_selected_healthy(&mut d,selected).err(),
   Some(MountError::Partition(Error::Invalid)));
  assert_eq!(d.writes,0);
 }
 #[test]fn m1_r048_guid_pinned_mount_reads_existing_file_without_writes(){
  let mut d=gpt_disk();
  let selected=inspect_gpt_selection(&mut d,0).unwrap();
  {
   let win=BlockWindow::new(&mut d,40,100).unwrap();
   let mut fs=HierVfs::format_blank(win).unwrap();
   fs.mkdir("/docs").unwrap();fs.create("/docs/note",b"identity").unwrap();
  }
  let writes=d.writes;
  {
   let mut fs=mount_gpt_selected_healthy(&mut d,selected).unwrap();
   let mut out=[0u8;16];
   assert_eq!(fs.read("/docs/note",&mut out),Ok(8));
   assert_eq!(&out[..8],b"identity");
  }
  assert_eq!(d.writes,writes);
  assert_eq!(d.data[39],[0;SECTOR_BYTES]);
  assert_eq!(d.data[140],[0;SECTOR_BYTES]);
 }
 #[test]fn m1_r048_mbr_has_no_fabricated_guid_identity(){
  let mut d=mbr_disk();
  assert_eq!(inspect_gpt_selection(&mut d,0),Err(Error::Unsupported));
  assert_eq!(d.writes,0);
 }
 #[test]fn m1_r045_empty_valid_mbr_reports_no_partitions_without_format(){
  let mut d=new();d.data[0][510]=0x55;d.data[0][511]=0xaa;
  let inventory=discover(&mut d).unwrap();
  assert!(inventory.is_empty());
  assert_eq!(inventory.len(),0);
  assert_eq!(inventory.get(0),None);
  assert_eq!(inspect(&mut d,0),Err(Error::NotFound));
  assert_eq!(d.writes,0);
 }
 #[test]fn m1_r045_explicit_format_is_blocked_through_read_only_window(){
  let mut d=mbr_disk();
  let before=d.writes;
  {
   let mut window=BlockWindow::new(&mut d,10,100).unwrap();
   let readonly=ReadOnlyDevice::new(&mut window);
   // Even a mistaken explicit formatting call can never reach a disk
   // mutation through the read-only diagnostic capability.
   assert!(matches!(HierVfs::format_blank(readonly),Err(FsError::Io)));
  }
  assert_eq!(d.writes,before);
  assert_eq!(d.data[10],[0;SECTOR_BYTES]);
 }

}