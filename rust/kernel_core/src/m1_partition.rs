//! M1: defensive x86-64 512-byte-sector MBR/GPT partition discovery.
//! Caller must EXPLICITLY select the entry ordinal; never automatically
//! choose, mount or format an arbitrary physical disk. No writes during
//! discovery. GPT 1.0 with 128-byte entries (<=128) is supported; extended
//! MBR partitions, backup-header recovery and exotic sector sizes are not.
use crate::block::{BlockDevice,BlockError,BlockWindow,SECTOR_BYTES};
use crate::hier_vfs::HierVfs;
use crate::durablefs::{FsError,MIN_SECTORS};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Scheme{Mbr,Gpt}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Partition{pub scheme:Scheme,pub index:u32,pub start:u64,pub sectors:u64}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Io,Invalid,Unsupported,NotFound,Overlap,OutOfRange}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum MountError{Partition(Error),Filesystem(FsError)}
impl From<BlockError> for Error{fn from(_:BlockError)->Self{Self::Io}}
const MAX_GPT_ENTRIES:usize=128;
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
fn mbr<D:BlockDevice>(disk:&mut D,index:u32,first:&[u8;SECTOR_BYTES])
 ->Result<Partition,Error>{
 if index>=4{return Err(Error::NotFound);}
 let capacity=disk.sector_count();
 let mut seen=[None;4];
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
 }
 let (start,end)=seen[index as usize].ok_or(Error::NotFound)?;
 Ok(Partition{scheme:Scheme::Mbr,index,start,sectors:end-start})
}
fn gpt<D:BlockDevice>(disk:&mut D,index:u32)->Result<Partition,Error>{
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
 if le64(&hdr[24..32])!=1||alternate<=1||alternate>=capacity||
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
 if index as usize>=entries{return Err(Error::NotFound);}
 let mut seen=[None;MAX_GPT_ENTRIES];
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
  let start=le64(&entry[32..40]);let end=le64(&entry[40..48]);
  if start<first||end>last||end<start{return Err(Error::OutOfRange);}
  let exclusive=end.checked_add(1).ok_or(Error::OutOfRange)?;
  for &(previous_start,previous_end) in seen.iter().flatten(){
   if start<previous_end&&previous_start<exclusive{return Err(Error::Overlap);}
  }
  seen[i]=Some((start,exclusive));
 }
 let (start,end)=seen[index as usize].ok_or(Error::NotFound)?;
 Ok(Partition{scheme:Scheme::Gpt,index,start,sectors:end-start})
}
pub fn inspect<D:BlockDevice>(disk:&mut D,index:u32)->Result<Partition,Error>{
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
  gpt(disk,index)
 }else{mbr(disk,index,&first)}
}
/// Mount previously provisioned IndianOS volume inside an explicitly chosen
/// partition. The disk is borrowed; the view cannot access neighboring
/// partitions. Neither inspection nor mount formats any disk.
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
 struct Disk{data:[[u8;SECTOR_BYTES];COUNT],writes:usize}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{COUNT as u64}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,input:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*input;
   self.writes+=1;Ok(())
  }
 }
 fn new()->Disk{Disk{data:[[0;SECTOR_BYTES];COUNT],writes:0}}
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
  d
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
}