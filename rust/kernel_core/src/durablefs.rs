//! R027 allocation-free copy-on-write filesystem for isolated QEMU block devices.
//!
//! Format is intentionally distinct from R016 TinyFs. Two independently
//! checksummed metadata sectors describe fixed extents. Every file has THREE
//! independent four-sector banks, so the next update never overwrites data
//! referenced by either the current or last committed metadata copy.
//! There is no allocation, journal replay, writable shared cache, or SMP.
//! BlockDevice::write_sector MUST durably flush data before returning:
//! QEMU ATA PIO's implementation sends FLUSH CACHE after each sector.
//! Normal boot must never format arbitrary, existing host disks.
use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};
use crate::vfs_path::normalize_absolute;

const MAGIC:&[u8;8]=b"INDR027!";
const FORMAT_VERSION:u32=1;
const ENTRIES:usize=8;
const ENTRY_SIZE:usize=40;
const HEADER_SIZE:usize=32;
const NAME_BYTES:usize=24;
const BANK_COUNT:u8=3;
const SECTORS_PER_BANK:usize=4;
pub const MAX_FILE_BYTES:usize=SECTOR_BYTES*SECTORS_PER_BANK;
pub const MIN_SECTORS:u64=2+(ENTRIES*BANK_COUNT as usize*SECTORS_PER_BANK) as u64;
pub const MAX_FILES:usize=ENTRIES;

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum FsError{Io,InvalidVolume,InvalidName,Exists,NotFound,Full,TooLarge}
impl From<BlockError> for FsError{fn from(_:BlockError)->Self{Self::Io}}

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
struct Entry{
    name:[u8;NAME_BYTES],
    len:u32,
    checksum:u32,
    bank:u8,
    used:bool,
}
impl Entry{
    const EMPTY:Self=Self{name:[0;NAME_BYTES],len:0,checksum:0,bank:0,used:false};
}
#[derive(Clone,Copy)]
struct Metadata{generation:u64,entries:[Entry;ENTRIES]}
impl Metadata{fn empty()->Self{Self{generation:1,entries:[Entry::EMPTY;ENTRIES]}}}

fn crc_state(mut crc:u32,data:&[u8])->u32{
    for &byte in data{
        crc^=byte as u32;
        for _ in 0..8{
            crc=(crc>>1) ^ (if crc&1!=0{0xedb8_8320}else{0});
        }
    }
    crc
}
fn crc(data:&[u8])->u32{!crc_state(!0u32,data)}
fn read_u32(s:&[u8])->u32{u32::from_le_bytes(s.try_into().unwrap())}
fn read_u64(s:&[u8])->u64{u64::from_le_bytes(s.try_into().unwrap())}

fn encode(meta:&Metadata,sectors:u64)->[u8;SECTOR_BYTES]{
    let mut output=[0u8;SECTOR_BYTES];
    output[..8].copy_from_slice(MAGIC);
    output[8..12].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    output[12..16].copy_from_slice(&(sectors as u32).to_le_bytes());
    output[16..24].copy_from_slice(&meta.generation.to_le_bytes());
    for (i,e) in meta.entries.iter().enumerate(){
        if !e.used{continue;}
        let offset=HEADER_SIZE+i*ENTRY_SIZE;
        output[offset..offset+NAME_BYTES].copy_from_slice(&e.name);
        output[offset+24..offset+28].copy_from_slice(&e.len.to_le_bytes());
        output[offset+28..offset+32].copy_from_slice(&e.checksum.to_le_bytes());
        output[offset+32]=e.bank;
        output[offset+33]=1;
    }
    // CRC field is zero while computing checksum of the whole metadata sector.
    let sum=crc(&output);
    output[24..28].copy_from_slice(&sum.to_le_bytes());
    output
}
fn decode(sector:&[u8;SECTOR_BYTES],capacity:u64)->Option<Metadata>{
    if &sector[..8]!=MAGIC || read_u32(&sector[8..12])!=FORMAT_VERSION ||
       read_u32(&sector[12..16]) as u64!=capacity ||
       sector[28..32]!=[0;4] || sector[HEADER_SIZE+ENTRIES*ENTRY_SIZE..].iter().any(|&b|b!=0){
       return None;
    }
    let generation=read_u64(&sector[16..24]);
    if generation==0{return None;}
    let saved=read_u32(&sector[24..28]);
    let mut copy=*sector;
    copy[24..28].fill(0);
    if crc(&copy)!=saved{return None;}
    let mut result=Metadata{generation,entries:[Entry::EMPTY;ENTRIES]};
    for index in 0..ENTRIES{
        let offset=HEADER_SIZE+index*ENTRY_SIZE;
        let entry=&sector[offset..offset+ENTRY_SIZE];
        if entry.iter().all(|&b|b==0){continue;}
        if entry[33]!=1 || entry[34..].iter().any(|&b|b!=0) ||
           entry[32]>=BANK_COUNT{return None;}
        let name_len=entry[..NAME_BYTES].iter().position(|&b|b==0)?;
        if name_len==0 || entry[name_len..NAME_BYTES].iter().any(|&b|b!=0){return None;}
        let bytes=&entry[..name_len];
        if bytes.iter().any(|&b|b<0x20||b==b'/'||b==b'\\') ||
           core::str::from_utf8(bytes).is_err(){return None;}
        if (0..index).any(|i|result.entries[i].used &&
            result.entries[i].name[..name_len]==*bytes &&
            result.entries[i].name[name_len]==0){return None;}
        let len=read_u32(&entry[24..28]);
        if len as usize>MAX_FILE_BYTES{return None;}
        let mut name=[0u8;NAME_BYTES];
        name.copy_from_slice(&entry[..NAME_BYTES]);
        result.entries[index]=Entry{
            name,len,checksum:read_u32(&entry[28..32]),
            bank:entry[32],used:true
        };
    }
    Some(result)
}

fn block_start(slot:usize,bank:u8)->u64{
    2+((slot*BANK_COUNT as usize+bank as usize)*SECTORS_PER_BANK) as u64
}
fn verified_payload<D:BlockDevice>(disk:&mut D,slot:usize,e:Entry)
    ->Result<(),FsError>
{
    let mut hash=!0u32;
    let mut left=e.len as usize;
    for index in 0..left.div_ceil(SECTOR_BYTES){
        let mut data=[0u8;SECTOR_BYTES];
        disk.read_sector(block_start(slot,e.bank)+index as u64,&mut data)?;
        let useful=left.min(SECTOR_BYTES);
        hash=crc_state(hash,&data[..useful]);
        left-=useful;
    }
    if !hash!=e.checksum{return Err(FsError::InvalidVolume);}
    Ok(())
}
fn verify_metadata<D:BlockDevice>(disk:&mut D,meta:&Metadata)->bool{
    for (slot,entry) in meta.entries.iter().enumerate(){
        if entry.used && verified_payload(disk,slot,*entry).is_err(){return false;}
    }
    true
}
fn canonical_path(path:&str)->Result<[u8;NAME_BYTES],FsError>{
    let mut normalized=[0u8;NAME_BYTES];
    let size=normalize_absolute(path,&mut normalized).map_err(|_|FsError::InvalidName)?;
    if size<=1 || normalized[1..size].contains(&b'/'){return Err(FsError::InvalidName);}
    let name=&normalized[1..size];
    if name.is_empty() || name.len()>=NAME_BYTES ||
       name.iter().any(|&b|b<0x20||b==b'\\'){return Err(FsError::InvalidName);}
    let mut output=[0u8;NAME_BYTES];output[..name.len()].copy_from_slice(name);Ok(output)
}

pub struct DurableFs<D:BlockDevice>{
    disk:D,meta:Metadata,active:u64,previous:Option<Metadata>,
}
impl<D:BlockDevice> DurableFs<D>{
    /// Formats only a fully blank bounded device; caller has opted into writing.
    /// Never auto-format from mount on corrupt or unexpected metadata.
    pub fn format_blank(mut disk:D)->Result<Self,FsError>{
        if disk.sector_count()<MIN_SECTORS || disk.sector_count()>u32::MAX as u64 {
            return Err(FsError::InvalidVolume);
        }
        // Formatting must not erase a disk whose two metadata sectors
        // happen to be zero while another sector contains existing data.
        // The selected window must be EXPLICITLY empty in its entirety.
        for lba in 0..disk.sector_count(){
            let mut sector=[0u8;SECTOR_BYTES];
            disk.read_sector(lba,&mut sector)?;
            if sector.iter().any(|&b|b!=0){return Err(FsError::InvalidVolume);}
        }
        let meta=Metadata::empty();
        let data=encode(&meta,disk.sector_count());
        disk.write_sector(0,&data)?;
        disk.write_sector(1,&data)?;
        Ok(Self{disk,meta,active:0,previous:Some(meta)})
    }
    pub fn mount(mut disk:D)->Result<Self,FsError>{
        if disk.sector_count()<MIN_SECTORS || disk.sector_count()>u32::MAX as u64{
            return Err(FsError::InvalidVolume);
        }
        let mut candidates:[Option<Metadata>;2]=[None,None];
        for (n,slot) in candidates.iter_mut().enumerate(){
            let mut bytes=[0u8;SECTOR_BYTES];
            disk.read_sector(n as u64,&mut bytes)?;
            if let Some(meta)=decode(&bytes,disk.sector_count()){
                if verify_metadata(&mut disk,&meta){*slot=Some(meta);}
            }
        }
        let (active,meta,previous)=match (candidates[0],candidates[1]){
            (Some(a),Some(b)) if b.generation>a.generation=>(1,b,Some(a)),
            (Some(a),Some(b))=>(0,a,Some(b)),
            (Some(a),None)=>(0,a,None),
            (None,Some(b))=>(1,b,None),
            (None,None)=>return Err(FsError::InvalidVolume),
        };
        Ok(Self{disk,meta,active,previous})
    }
    pub fn generation(&self)->u64{self.meta.generation}
    pub fn into_device(self)->D{self.disk}
    fn find(&self,name:[u8;NAME_BYTES])->Option<usize>{
        self.meta.entries.iter().position(|e|e.used && e.name==name)
    }
    fn available_bank(&self,slot:usize)->Option<u8>{
        let present=self.meta.entries[slot];
        let previous=self.previous.map(|m|m.entries[slot]).unwrap_or(Entry::EMPTY);
        (0..BANK_COUNT).find(|&bank|
            (!present.used || present.bank!=bank) &&
            (!previous.used || previous.bank!=bank))
    }
    fn publish(&mut self,mut proposed:Metadata)->Result<(),FsError>{
        proposed.generation=self.meta.generation.checked_add(1)
            .ok_or(FsError::InvalidVolume)?;
        let inactive=1-self.active;
        let bytes=encode(&proposed,self.disk.sector_count());
        self.disk.write_sector(inactive,&bytes)?;
        self.previous=Some(self.meta);
        self.meta=proposed;
        self.active=inactive;
        Ok(())
    }
    fn write_bank(&mut self,slot:usize,bank:u8,data:&[u8])->Result<(),FsError>{
        for i in 0..SECTORS_PER_BANK{
            let mut sector=[0u8;SECTOR_BYTES];
            let start=i*SECTOR_BYTES;
            if start<data.len(){
                let end=(start+SECTOR_BYTES).min(data.len());
                sector[..end-start].copy_from_slice(&data[start..end]);
            }
            self.disk.write_sector(block_start(slot,bank)+i as u64,&sector)?;
        }
        Ok(())
    }
    fn put(&mut self,path:&str,data:&[u8],replace:bool)->Result<(),FsError>{
        let name=canonical_path(path)?;
        if data.len()>MAX_FILE_BYTES{return Err(FsError::TooLarge);}
        let index=if replace{
            self.find(name).ok_or(FsError::NotFound)?
        }else{
            if self.find(name).is_some(){return Err(FsError::Exists);}
            (0..ENTRIES).find(|&i|!self.meta.entries[i].used)
                .ok_or(FsError::Full)?
        };
        let bank=self.available_bank(index).ok_or(FsError::Full)?;
        self.write_bank(index,bank,data)?;
        let mut next=self.meta;
        next.entries[index]=Entry{
            name,len:data.len() as u32,checksum:crc(data),bank,used:true
        };
        self.publish(next)
    }
    pub fn create_path(&mut self,path:&str,data:&[u8])->Result<(),FsError>{
        self.put(path,data,false)
    }
    /// Data sectors are flushed before atomically replacing a metadata copy.
    pub fn overwrite_path(&mut self,path:&str,data:&[u8])->Result<(),FsError>{
        self.put(path,data,true)
    }
    pub fn read_path(&mut self,path:&str,out:&mut [u8])->Result<usize,FsError>{
        let name=canonical_path(path)?;
        let slot=self.find(name).ok_or(FsError::NotFound)?;
        let entry=self.meta.entries[slot];
        let len=entry.len as usize;
        if out.len()<len{return Err(FsError::TooLarge);}
        // Stage into private fixed scratch. On corruption/I/O failure
        // never partially change the user's destination.
        let mut scratch=[0u8;MAX_FILE_BYTES];
        let count=len.div_ceil(SECTOR_BYTES);
        for i in 0..count{
            let mut sector=[0u8;SECTOR_BYTES];
            self.disk.read_sector(block_start(slot,entry.bank)+i as u64,&mut sector)?;
            let start=i*SECTOR_BYTES;
            let used=(len-start).min(SECTOR_BYTES);
            scratch[start..start+used].copy_from_slice(&sector[..used]);
        }
        if crc(&scratch[..len])!=entry.checksum{return Err(FsError::InvalidVolume);}
        out[..len].copy_from_slice(&scratch[..len]);
        Ok(len)
    }
    pub fn delete_path(&mut self,path:&str)->Result<(),FsError>{
        let name=canonical_path(path)?;
        let slot=self.find(name).ok_or(FsError::NotFound)?;
        // Only metadata changes; preserving both generations' data banks
        // means an interrupted delete is recoverable.
        let mut next=self.meta;
        next.entries[slot]=Entry::EMPTY;
        self.publish(next)
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[derive(Clone)]
    struct Disk{
        sectors:[[u8;SECTOR_BYTES];MIN_SECTORS as usize],
        fail_at:Option<usize>,
        writes:usize
    }
    impl BlockDevice for Disk{
        fn sector_count(&self)->u64{MIN_SECTORS}
        fn read_sector(&mut self,lba:u64,out:&mut [u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            let value=self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            *out=*value;Ok(())
        }
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            let sector=self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?;
            if self.fail_at==Some(self.writes){
                // Simulate a power loss that tears one sector mid-write.
                sector[0]=0x7f;
                return Err(BlockError::Io);
            }
            *sector=*data;
            self.writes+=1;
            Ok(())
        }
    }
    fn blank()->Disk{
        Disk{sectors:[[0;SECTOR_BYTES];MIN_SECTORS as usize],fail_at:None,writes:0}
    }
    fn get(fs:&mut DurableFs<Disk>,path:&str)->Vec<u8>{
        let mut out=[0u8;MAX_FILE_BYTES];
        let n=fs.read_path(path,&mut out).unwrap();
        out[..n].to_vec()
    }
    #[test]
    fn reboot_multisector_overwrite_delete_and_maintain_other_files(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        let file1=(0..1701).map(|n|(n%251) as u8).collect::<Vec<_>>();
        let file2=(0..2048).map(|n|((n*7)%253) as u8).collect::<Vec<_>>();
        fs.create_path("//large/./",&file1).unwrap();
        fs.create_path("/untouched",b"stay").unwrap();
        assert_eq!(fs.create_path("/large",b"duplicate"),Err(FsError::Exists));
        let mut fs=DurableFs::mount(fs.into_device()).unwrap();
        assert_eq!(get(&mut fs,"/large"),file1);
        fs.overwrite_path("/large",&file2).unwrap();
        let mut fs=DurableFs::mount(fs.into_device()).unwrap();
        assert_eq!(get(&mut fs,"/large"),file2);
        assert_eq!(get(&mut fs,"/untouched"),b"stay");
        fs.delete_path("/large").unwrap();
        assert_eq!(fs.read_path("/large",&mut [0u8;MAX_FILE_BYTES]),Err(FsError::NotFound));
        fs.create_path("/new",b"reclaimed").unwrap();
        let mut fs=DurableFs::mount(fs.into_device()).unwrap();
        assert_eq!(get(&mut fs,"/new"),b"reclaimed");
        assert_eq!(get(&mut fs,"/untouched"),b"stay");
    }
    #[test]
    fn torn_new_metadata_recovers_previous_committed_version(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/data",&[4u8;900]).unwrap();
        fs.overwrite_path("/data",&[5u8;1025]).unwrap();
        let active=fs.active as usize;
        let mut disk=fs.into_device();
        disk.sectors[active][19]^=0x5a;
        let mut recovered=DurableFs::mount(disk).unwrap();
        assert_eq!(get(&mut recovered,"/data"),[4u8;900]);
        // A committed generation can be advanced after rollback.
        recovered.overwrite_path("/data",&[9u8;1949]).unwrap();
        let mut next=DurableFs::mount(recovered.into_device()).unwrap();
        assert_eq!(get(&mut next,"/data"),[9u8;1949]);
    }
    #[test]
    fn interrupted_writes_never_publish_half_written_files(){
        for fail_at in [0usize,2,4]{
            let mut fs=DurableFs::format_blank(blank()).unwrap();
            fs.create_path("/data",&[0x55;900]).unwrap();
            let mut disk=fs.into_device();
            disk.writes=0;disk.fail_at=Some(fail_at);
            let mut fs=DurableFs::mount(disk).unwrap();
            assert_eq!(fs.overwrite_path("/data",&[0x88;1400]),Err(FsError::Io));
            let mut disk=fs.into_device();
            disk.fail_at=None;
            let mut mounted=DurableFs::mount(disk).unwrap();
            assert_eq!(get(&mut mounted,"/data"),[0x55;900]);
        }
    }
    #[test]
    fn no_autoformat_corrupt_superblocks_and_safe_names(){
        let mut invalid=blank();
        invalid.sectors[0][0]=1;
        assert!(matches!(DurableFs::format_blank(invalid),Err(FsError::InvalidVolume)));
        let mut hidden_data=blank();
        hidden_data.sectors[MIN_SECTORS as usize-1][123]=0x77;
        assert!(matches!(DurableFs::format_blank(hidden_data),Err(FsError::InvalidVolume)));
        assert!(matches!(DurableFs::mount(blank()),Err(FsError::InvalidVolume)));
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        for p in ["","a","/","/../escape","/a/../b","/a/b","/back\\slash"]{
            assert_eq!(fs.create_path(p,b"x"),Err(FsError::InvalidName));
        }
        assert_eq!(fs.create_path("/oversized",&[1;MAX_FILE_BYTES+1]),Err(FsError::TooLarge));
        assert_eq!(fs.overwrite_path("/absent",b"missing"),Err(FsError::NotFound));
        fs.create_path("/short",b"ok").unwrap();
        let mut out=[0xaau8;1];
        assert_eq!(fs.read_path("/short",&mut out),Err(FsError::TooLarge));
        assert_eq!(out,[0xaa]);
    }
    #[test]
    fn one_bad_copy_and_corrupt_data_are_detected(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/first",&[3;1400]).unwrap();
        let active=fs.active as usize;
        let mut disk=fs.into_device();
        disk.sectors[1-active][24]^=0x08; // corrupt old metadata checksum
        let fs=DurableFs::mount(disk).unwrap();
        let mut disk=fs.into_device();
        let current_bank=if active==0{0}else{0}; // initial create chooses bank0
        disk.sectors[block_start(0,current_bank) as usize][0]^=0x01;
        assert!(matches!(DurableFs::mount(disk),Err(FsError::InvalidVolume)));
    }
}
