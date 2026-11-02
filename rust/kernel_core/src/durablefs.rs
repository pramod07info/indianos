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
    // A broken/removable device must not verify payload sectors using
    // different geometries in the same checksummed generation.
    let capacity=disk.sector_count();
    for index in 0..left.div_ceil(SECTOR_BYTES){
        if disk.sector_count()!=capacity{return Err(FsError::Io);}
        let mut data=[0u8;SECTOR_BYTES];
        disk.read_sector(block_start(slot,e.bank)+index as u64,&mut data)?;
        if disk.sector_count()!=capacity{return Err(FsError::Io);}
        let useful=left.min(SECTOR_BYTES);
        hash=crc_state(hash,&data[..useful]);
        left-=useful;
    }
    if !hash!=e.checksum{return Err(FsError::InvalidVolume);}
    Ok(())
}
fn verify_metadata<D:BlockDevice>(disk:&mut D,meta:&Metadata)->Result<bool,FsError>{
    for (slot,entry) in meta.entries.iter().enumerate(){
        if !entry.used{continue;}
        match verified_payload(disk,slot,*entry){
            Ok(())=>{},
            // Checksum corruption is evidence that this checkpoint cannot
            // be used. A transport error is not: silently selecting an
            // older head on a transient read failure could lose committed
            // data when the recovery mount next publishes a new head.
            Err(FsError::InvalidVolume)=>return Ok(false),
            Err(error)=>return Err(error),
        }
    }
    Ok(true)
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

/// Read-only, fully checksum-verified redundancy information. A single
/// usable metadata bank is DEGRADDED, not proof that power-loss recovery is
/// complete. This report never edits or attempts to repair the disk.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Redundancy{Healthy,Degraded}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct VolumeHealth{
 pub redundancy:Redundancy,
 pub valid_copies:u8,
 pub current_generation:u64,
 pub previous_generation:Option<u64>,
}
pub struct DurableFs<D:BlockDevice>{
    // Geometry belongs to the validated mount. Reinterpreting cached heads
    // with a later device size could silently discard their bank ownership.
    disk:D,sectors:u64,meta:Metadata,active:u64,previous:Option<Metadata>,
    // Exact observed heads, including an invalid/degraded copy. A stale
    // mount must not choose a COW bank using obsolete extent ownership.
    heads:[[u8;SECTOR_BYTES];2],
}
impl<D:BlockDevice> DurableFs<D>{
    /// Formats only a fully blank bounded device; caller has opted into writing.
    /// Never auto-format from mount on corrupt or unexpected metadata.
    pub fn format_blank(mut disk:D)->Result<Self,FsError>{
        let sectors=disk.sector_count();
        if sectors<MIN_SECTORS || sectors>u32::MAX as u64 {
            return Err(FsError::InvalidVolume);
        }
        // Formatting must not erase a disk whose two metadata sectors
        // happen to be zero while another sector contains existing data.
        // The selected window must be EXPLICITLY empty in its entirety.
        for lba in 0..sectors{
            let mut sector=[0u8;SECTOR_BYTES];
            disk.read_sector(lba,&mut sector)?;
            if sector.iter().any(|&b|b!=0){return Err(FsError::InvalidVolume);}
        }
        if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
        let meta=Metadata::empty();
        let data=encode(&meta,sectors);
        disk.write_sector(0,&data)?;
        if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
        disk.write_sector(1,&data)?;
        // Formatting is explicitly opt-in, but it must still fail closed if
        // the selected device acknowledges a dropped or torn superblock.
        for lba in 0..2{
            let mut observed=[0u8;SECTOR_BYTES];
            disk.read_sector(lba,&mut observed)?;
            if observed!=data{return Err(FsError::InvalidVolume);}
        }
        if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
        Ok(Self{disk,sectors,meta,active:0,previous:Some(meta),heads:[data;2]})
    }
    pub fn mount(mut disk:D)->Result<Self,FsError>{
        let sectors=disk.sector_count();
        if sectors<MIN_SECTORS || sectors>u32::MAX as u64{
            return Err(FsError::InvalidVolume);
        }
        let mut candidates:[Option<Metadata>;2]=[None,None];
        let mut heads=[[0u8;SECTOR_BYTES];2];
        for (n,slot) in candidates.iter_mut().enumerate(){
            let mut bytes=[0u8;SECTOR_BYTES];
            disk.read_sector(n as u64,&mut bytes)?;
            heads[n]=bytes;
            if let Some(meta)=decode(&bytes,sectors){
                if verify_metadata(&mut disk,&meta)?{*slot=Some(meta);}
            }
        }
        if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
        // Two independently valid copies with the same generation must
        // describe exactly the same state. Picking one arbitrarily would
        // hide a split-brain/corrupted checkpoint and risk silent data loss.
        if let (Some(a),Some(b))=(candidates[0],candidates[1]){
            if a.generation==b.generation && encode(&a,sectors)!=encode(&b,sectors){
                return Err(FsError::InvalidVolume);
            }
        }
        if let (Some(a),Some(b))=(candidates[0],candidates[1]){
            // The two alternating metadata heads must be identical at the
            // initial checkpoint or exactly one generation apart. Larger
            // valid-but-divergent gaps indicate stale/mixed media, not an
            // ordinary interrupted write. Never silently pick a winner.
            if a.generation.abs_diff(b.generation)>1{
                return Err(FsError::InvalidVolume);
            }
        }
        let (active,meta,previous)=match (candidates[0],candidates[1]){
            (Some(a),Some(b)) if b.generation>a.generation=>(1,b,Some(a)),
            (Some(a),Some(b))=>(0,a,Some(b)),
            (Some(a),None)=>(0,a,None),
            (None,Some(b))=>(1,b,None),
            (None,None)=>return Err(FsError::InvalidVolume),
        };
        Ok(Self{disk,sectors,meta,active,previous,heads})
    }
    /// Verify both superblocks and all referenced COW payload checksums
    /// without modifying the device or choosing a destructive recovery path.
    /// Fail closed on two valid same-generation but divergent checkpoints.
    pub fn inspect_health(disk:&mut D)->Result<VolumeHealth,FsError>{
        let sectors=disk.sector_count();
        if sectors<MIN_SECTORS||sectors>u32::MAX as u64{
            return Err(FsError::InvalidVolume);
        }
        let mut copies=[None,None];
        for index in 0..2{
            let mut header=[0u8;SECTOR_BYTES];
            disk.read_sector(index as u64,&mut header)?;
            if let Some(meta)=decode(&header,sectors){
                if verify_metadata(disk,&meta)?{copies[index]=Some(meta);}
            }
        }
        if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
        let (latest,previous,valid_copies)=match (copies[0],copies[1]){
            (Some(a),Some(b))=>{
                if a.generation.abs_diff(b.generation)>1 ||
                   (a.generation==b.generation&&
                    encode(&a,sectors)!=encode(&b,sectors)){
                    return Err(FsError::InvalidVolume);
                }
                if a.generation>=b.generation{
                    (a,Some(b),2)
                }else{(b,Some(a),2)}
            },
            (Some(a),None)=>(a,None,1),
            (None,Some(b))=>(b,None,1),
            (None,None)=>return Err(FsError::InvalidVolume),
        };
        Ok(VolumeHealth{
            redundancy:if valid_copies==2{Redundancy::Healthy}
                       else{Redundancy::Degraded},
            valid_copies,
            current_generation:latest.generation,
            previous_generation:previous.map(|m|m.generation),
        })
    }
    /// Refuse a degraded or internally inconsistent mount for operations
    /// requiring two independently verified COW metadata copies. Read-only
    /// preflight still uses inspect_health to explain the specific condition.
    pub fn mount_healthy(mut disk:D)->Result<Self,FsError>{
        let health=Self::inspect_health(&mut disk)?;
        if health.redundancy!=Redundancy::Healthy{
            return Err(FsError::InvalidVolume);
        }
        Self::mount(disk)
    }
    /// Re-probe actual disk sectors, not cached metadata. Fail if a different
    /// process changed the current head while this filesystem was mounted.
    pub fn mounted_health(&mut self)->Result<VolumeHealth,FsError>{
        self.check_geometry()?;
        let report=Self::inspect_health(&mut self.disk)?;
        let mut active=[0u8;SECTOR_BYTES];
        self.disk.read_sector(self.active,&mut active)?;
        self.check_geometry()?;
        if active!=encode(&self.meta,self.sectors) ||
           report.current_generation!=self.meta.generation{
            return Err(FsError::InvalidVolume);
        }
        Ok(report)
    }
    /// Explicit, opt-in single-superblock redundancy repair. Only allowed
    /// when exactly ONE metadata head and every referenced payload verify.
    /// No file banks, directory entries or file contents are written.
    /// Do not use this as a replacement for fsync or media replacement.
    ///
    /// The previously committed head is read before writing the inactive
    /// metadata sector. A failed/partial write leaves the active head intact.
    /// The BlockDevice write contract must durably flush each sector.
    pub fn repair_redundancy(&mut self)->Result<bool,FsError>{
        self.check_heads()?;
        let health=self.mounted_health()?;
        if health.redundancy==Redundancy::Healthy{return Ok(false);}
        if health.valid_copies!=1{return Err(FsError::InvalidVolume);}
        let inactive=1-self.active;
        let bytes=encode(&self.meta,self.sectors);
        self.check_geometry()?;
        self.disk.write_sector(inactive,&bytes)?;
        let mut observed=[0u8;SECTOR_BYTES];
        self.disk.read_sector(inactive,&mut observed)?;
        if observed!=bytes{return Err(FsError::InvalidVolume);}
        // Validate BOTH newly published copies and all referenced file
        // payloads before acknowledging an admin-triggered repair.
        let after=Self::inspect_health(&mut self.disk)?;
        self.check_geometry()?;
        if after.valid_copies!=2 ||
           after.current_generation!=self.meta.generation{
            return Err(FsError::InvalidVolume);
        }
        self.previous=Some(self.meta);
        self.heads[inactive as usize]=bytes;
        Ok(true)
    }
    pub fn generation(&self)->u64{self.meta.generation}
    /// Explicit durable flush for an already mounted filesystem. A driver
    /// with no known flush contract fails closed instead of falsely
    /// acknowledging durability. Full checksum health (including payloads)
    /// is verified before and after the barrier: unchanged metadata heads
    /// alone are not evidence that previously committed file data is intact.
    /// A degraded volume must be explicitly repaired instead of receiving
    /// a misleading 'synced' response. No automatic repair or format.
    pub fn sync(&mut self)->Result<(),FsError>{
        self.check_heads()?;
        let before=self.mounted_health()?;
        if before.redundancy!=Redundancy::Healthy{
            return Err(FsError::InvalidVolume);
        }
        self.disk.sync()?;
        self.check_heads()?;
        let after=self.mounted_health()?;
        if after.redundancy!=Redundancy::Healthy{
            return Err(FsError::InvalidVolume);
        }
        Ok(())
    }
    /// Expose verified metadata names only, without mutable disk state.
    /// Used by the constrained R031 VFS for directory membership/listing.
    pub fn visit_names(&self,mut visit:impl FnMut(&str)){
        for entry in self.meta.entries.iter().filter(|e|e.used){
            let len=entry.name.iter().position(|&b|b==0).unwrap_or(NAME_BYTES);
            if let Ok(name)=core::str::from_utf8(&entry.name[..len]){visit(name);}
        }
    }
    /// Rename a single file by atomically publishing ONE metadata generation.
    /// Its data extents remain untouched, preserving both checkpoint copies.
    pub fn rename_path(&mut self,source:&str,target:&str)->Result<(),FsError>{
        let old=canonical_path(source)?;
        let new=canonical_path(target)?;
        let slot=self.find(old).ok_or(FsError::NotFound)?;
        if self.find(new).is_some(){return Err(FsError::Exists);}
        let mut next=self.meta;
        next.entries[slot].name=new;
        self.publish(next)
    }
    /// Move the full directory subtree in ONE checksummed metadata generation.
    /// No data banks are copied: R027 can recover the prior entire tree on a
    /// torn metadata sector, never a mixture of old and new directory names.
    pub fn rename_directory_tree(&mut self,from:&str,to:&str)->Result<(),FsError>{
        let old=canonical_path(from)?;
        let new=canonical_path(to)?;
        let old_len=old.iter().position(|&b|b==0).unwrap_or(NAME_BYTES);
        let new_len=new.iter().position(|&b|b==0).unwrap_or(NAME_BYTES);
        if old_len<=2||new_len<=2||old[0]!=b'~'||old[1]!=b'd'||
           new[0]!=b'~'||new[1]!=b'd'||old==new{
            return Err(FsError::InvalidName);
        }
        if self.find(old).is_none(){return Err(FsError::NotFound);}
        let old_stem=&old[2..old_len];
        let new_stem=&new[2..new_len];
        let mut next=self.meta;
        for (i,entry) in self.meta.entries.iter().enumerate(){
            if !entry.used{continue;}
            let len=entry.name.iter().position(|&b|b==0).unwrap_or(NAME_BYTES);
            if len<=2||entry.name[0]!=b'~'||
               !matches!(entry.name[1],b'd'|b'f'){continue;}
            let rest=&entry.name[2..len];
            if rest!=old_stem && !(rest.starts_with(old_stem) &&
                                   rest.get(old_stem.len())==Some(&b'~')){continue;}
            let suffix=&rest[old_stem.len()..];
            let new_size=2+new_stem.len()+suffix.len();
            if new_size>=NAME_BYTES{return Err(FsError::InvalidName);}
            let mut changed=[0u8;NAME_BYTES];
            changed[..2].copy_from_slice(&entry.name[..2]);
            changed[2..2+new_stem.len()].copy_from_slice(new_stem);
            changed[2+new_stem.len()..new_size].copy_from_slice(suffix);
            next.entries[i].name=changed;
        }
        // Check collisions before publishing, including orphan backing keys.
        for i in 0..ENTRIES{
            if !next.entries[i].used{continue;}
            for j in 0..i{
                if next.entries[j].used && next.entries[i].name==next.entries[j].name{
                    return Err(FsError::Exists);
                }
            }
        }
        self.publish(next)
    }
    /// Validate device-specific identity without releasing the mounted
    /// device and introducing another mount/read cycle. Kernel-internal
    /// callers must perform inspection only and discard the mount on error.
    pub(crate) fn validate_device<E>(&mut self,
        validate:impl FnOnce(&mut D)->Result<(),E>)->Result<(),E>{
        validate(&mut self.disk)
    }
    pub fn into_device(self)->D{self.disk}
    fn find(&self,name:[u8;NAME_BYTES])->Option<usize>{
        self.meta.entries.iter().position(|e|e.used && e.name==name)
    }
    fn available_bank(&self,slot:usize)->Option<u8>{
        let present=self.meta.entries[slot];
        let previous=self.previous.map(|m|m.entries[slot]).unwrap_or(Entry::EMPTY);
        // Payload corruption disqualifies a checkpoint for mounting, not
        // its bank ownership. Rewriting its bank could otherwise repair
        // that payload and revive a rejected namespace after a power cut,
        // before our replacement metadata is published. Keep references
        // from BOTH structurally valid observed heads until overwritten.
        // There are only two heads and three banks per slot, so this still
        // leaves a bank available even on a degraded recovery mount.
        let observed=self.heads.map(|head|decode(&head,self.sectors));
        (0..BANK_COUNT).find(|&bank|
            (!present.used || present.bank!=bank) &&
            (!previous.used || previous.bank!=bank) &&
            observed.iter().flatten().all(|meta|{
                let entry=meta.entries[slot];
                !entry.used || entry.bank!=bank
            }))
    }
    /// The volume owner must serialize whole operations across mounts.
    /// This detects stale handles between operations, not concurrent DMA
    /// writers or a substitute for a device-wide filesystem lock.
    fn check_heads(&mut self)->Result<(),FsError>{
        self.check_geometry()?;
        for index in 0..2 {
            let mut observed=[0u8;SECTOR_BYTES];
            self.disk.read_sector(index as u64,&mut observed)?;
            if observed!=self.heads[index]{return Err(FsError::InvalidVolume);}
        }
        self.check_geometry()
    }
    fn check_geometry(&self)->Result<(),FsError>{
        if self.disk.sector_count()!=self.sectors{return Err(FsError::InvalidVolume);}
        Ok(())
    }
    fn publish(&mut self,mut proposed:Metadata)->Result<(),FsError>{
        self.check_heads()?;
        proposed.generation=self.meta.generation.checked_add(1)
            .ok_or(FsError::InvalidVolume)?;
        let inactive=1-self.active;
        let bytes=encode(&proposed,self.sectors);
        self.check_geometry()?;
        self.disk.write_sector(inactive,&bytes)?;
        // Verify the readback BEFORE publishing the new in-memory head.
        // A block device that acknowledges but discards a metadata write
        // must never be treated as a committed filesystem transaction.
        let mut observed=[0u8;SECTOR_BYTES];
        self.disk.read_sector(inactive,&mut observed)?;
        self.check_geometry()?;
        if observed!=bytes{return Err(FsError::InvalidVolume);}
        self.previous=Some(self.meta);
        self.meta=proposed;
        self.active=inactive;
        self.heads[inactive as usize]=bytes;
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
            let lba=block_start(slot,bank)+i as u64;
            self.check_geometry()?;
            self.disk.write_sector(lba,&sector)?;
            // Detect devices that report successful writes but return stale
            // or corrupted data before making a new checksum head visible.
            let mut observed=[0u8;SECTOR_BYTES];
            self.disk.read_sector(lba,&mut observed)?;
            self.check_geometry()?;
            if observed!=sector{return Err(FsError::InvalidVolume);}
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
        // Check BEFORE writing data, not only when publishing metadata:
        // another mount may now reference our apparently-unused bank.
        self.check_heads()?;
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
        // never partially change the user's destination, even if a device
        // shrinks or expands while supplying checksum-valid payload bytes.
        self.check_geometry()?;
        let mut scratch=[0u8;MAX_FILE_BYTES];
        let count=len.div_ceil(SECTOR_BYTES);
        for i in 0..count{
            self.check_geometry()?;
            let mut sector=[0u8;SECTOR_BYTES];
            self.disk.read_sector(block_start(slot,entry.bank)+i as u64,&mut sector)?;
            self.check_geometry()?;
            let start=i*SECTOR_BYTES;
            let used=(len-start).min(SECTOR_BYTES);
            scratch[start..start+used].copy_from_slice(&sector[..used]);
        }
        if crc(&scratch[..len])!=entry.checksum{return Err(FsError::InvalidVolume);}
        self.check_geometry()?;
        out[..len].copy_from_slice(&scratch[..len]);
        Ok(len)
    }
    /// Read a bounded segment. Verify the ENTIRE stored payload/checksum before
    /// copying anything to caller memory, including a read across sectors.
    /// offset==EOF returns 0; offsets above the volume maximum are rejected.
    pub fn read_range_path(&mut self,path:&str,offset:u64,
                           out:&mut [u8])->Result<usize,FsError>{
        if offset>MAX_FILE_BYTES as u64{return Err(FsError::TooLarge);}
        let mut snapshot=[0u8;MAX_FILE_BYTES];
        let len=self.read_path(path,&mut snapshot)?;
        let start=offset as usize;
        if start>=len{return Ok(0);}
        let n=(len-start).min(out.len());
        out[..n].copy_from_slice(&snapshot[start..start+n]);
        Ok(n)
    }
    /// Transactional append/overwrite at an existing byte offset; no holes.
    /// Check bounds and verify the old file BEFORE any disk writes. A complete
    /// new image is staged in an unused COW bank and atomically published once.
    pub fn write_range_path(&mut self,path:&str,offset:u64,
                            bytes:&[u8])->Result<usize,FsError>{
        if bytes.is_empty()||offset>MAX_FILE_BYTES as u64{
            return Err(FsError::TooLarge);
        }
        let start=offset as usize;
        let end=start.checked_add(bytes.len()).ok_or(FsError::TooLarge)?;
        if end>MAX_FILE_BYTES{return Err(FsError::TooLarge);}
        let mut snapshot=[0u8;MAX_FILE_BYTES];
        let len=self.read_path(path,&mut snapshot)?;
        if start>len{return Err(FsError::TooLarge);}
        snapshot[start..end].copy_from_slice(bytes);
        self.overwrite_path(path,&snapshot[..len.max(end)])?;
        Ok(bytes.len())
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
    struct SharedDisk(std::rc::Rc<std::cell::RefCell<Disk>>);
    impl BlockDevice for SharedDisk {
        fn sector_count(&self)->u64{MIN_SECTORS}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
            self.0.borrow_mut().read_sector(lba,out)
        }
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
            self.0.borrow_mut().write_sector(lba,data)
        }
    }
    #[test]fn stale_mount_cannot_overwrite_newer_checkpoint_or_live_data_bank(){
        let shared=SharedDisk(std::rc::Rc::new(std::cell::RefCell::new(blank())));
        let mut writer=DurableFs::format_blank(shared.clone()).unwrap();
        writer.create_path("/data",b"initial").unwrap();
        let mut stale=DurableFs::mount(shared.clone()).unwrap();
        writer.overwrite_path("/data",b"second").unwrap();
        writer.overwrite_path("/data",b"newest").unwrap();
        // After two updates the stale mount's free bank is live again.
        let before=shared.0.borrow().sectors;
        let writes=shared.0.borrow().writes;
        assert_eq!(stale.overwrite_path("/data",b"corrupt"),Err(FsError::InvalidVolume));
        assert_eq!(stale.create_path("/new",b"bad"),Err(FsError::InvalidVolume));
        assert_eq!(stale.delete_path("/data"),Err(FsError::InvalidVolume));
        assert_eq!(stale.rename_path("/data","/moved"),Err(FsError::InvalidVolume));
        assert_eq!(stale.repair_redundancy(),Err(FsError::InvalidVolume));
        assert_eq!(shared.0.borrow().writes,writes);
        assert_eq!(shared.0.borrow().sectors,before);
        let mut fresh=DurableFs::mount(shared).unwrap();
        let mut out=[0u8;32];
        let n=fresh.read_path("/data",&mut out).unwrap();
        assert_eq!(&out[..n],b"newest");
        fresh.overwrite_path("/data",b"fresh mount succeeds").unwrap();
    }
    #[test]fn changed_inactive_head_and_failed_publish_require_remount(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/data",b"keep").unwrap();
        let inactive=1-fs.active as usize;
        fs.disk.sectors[inactive][0]^=1;
        let writes=fs.disk.writes;
        assert_eq!(fs.overwrite_path("/data",b"deny"),Err(FsError::InvalidVolume));
        assert_eq!(fs.disk.writes,writes);
        // Remounting explicitly observes the degraded copy and retains the
        // existing recovery behavior; no auto-repair is introduced.
        let mut fs=DurableFs::mount(fs.into_device()).unwrap();
        fs.overwrite_path("/data",b"after remount").unwrap();
        fs.disk.fail_at=Some(fs.disk.writes+4);
        assert_eq!(fs.overwrite_path("/data",b"torn"),Err(FsError::Io));
        fs.disk.fail_at=None;
        let writes=fs.disk.writes;
        assert_eq!(fs.overwrite_path("/data",b"retry"),Err(FsError::InvalidVolume));
        assert_eq!(fs.disk.writes,writes);
        let mut fs=DurableFs::mount(fs.into_device()).unwrap();
        assert_eq!(get(&mut fs,"/data"),b"after remount");
        fs.overwrite_path("/data",b"recovered").unwrap();
    }
    #[derive(Clone)]
    struct Disk{
        sectors:[[u8;SECTOR_BYTES];MIN_SECTORS as usize],
        fail_at:Option<usize>,
        ignore_at:Option<usize>,
        fail_read_lba:Option<u64>,
        writes:usize
    }
    impl BlockDevice for Disk{
        fn sector_count(&self)->u64{MIN_SECTORS}
        fn read_sector(&mut self,lba:u64,out:&mut [u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            if self.fail_read_lba==Some(lba){return Err(BlockError::Io);}
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
            if self.ignore_at==Some(self.writes){
                self.writes+=1; // Lying storage acknowledges a dropped write
                return Ok(());
            }
            *sector=*data;
            self.writes+=1;
            Ok(())
        }
    }
    fn blank()->Disk{
        Disk{sectors:[[0;SECTOR_BYTES];MIN_SECTORS as usize],fail_at:None,
            ignore_at:None,fail_read_lba:None,writes:0}
    }
    /// A crash between sector transfers does not tear a later metadata
    /// sector: it simply prevents that transfer and all subsequent writes.
    struct PowerCutDisk{disk:Disk,writes_left:usize}
    impl BlockDevice for PowerCutDisk{
        fn sector_count(&self)->u64{self.disk.sector_count()}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{self.disk.read_sector(lba,out)}
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            if self.writes_left==0{return Err(BlockError::Io);}
            self.disk.write_sector(lba,data)?;
            self.writes_left-=1;Ok(())
        }
    }
    struct ChangingGeometryDisk{
        disk:PowerCutDisk,sectors:std::rc::Rc<std::cell::Cell<u64>>,
        switch_on_read:Option<(u64,u64)>,
    }
    impl BlockDevice for ChangingGeometryDisk{
        fn sector_count(&self)->u64{self.sectors.get()}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            self.disk.read_sector(lba,out)?;
            if let Some((at,sectors))=self.switch_on_read{
                if lba==at{self.sectors.set(sectors);self.switch_on_read=None;}
            }
            Ok(())
        }
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{self.disk.write_sector(lba,data)}
    }
    fn payload_rejected_head(slot:usize,payload:&[u8],corrupt_byte:usize)->Disk{
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        for index in 0..slot{
            fs.create_path(&format!("/keep{index}"),b"unrelated committed data").unwrap();
        }
        fs.create_path("/rejected",payload).unwrap();
        let bank=fs.meta.entries[slot].bank;
        let mut disk=fs.into_device();
        disk.sectors[block_start(slot,bank) as usize+corrupt_byte/SECTOR_BYTES]
            [corrupt_byte%SECTOR_BYTES]^=1;
        disk
    }
    #[test]fn m1_recovery_powercut_cannot_revive_payload_rejected_namespace(){
        // Every slot, short/multisector/full-bank payload, first/last
        // payload byte, and each cut BEFORE the metadata publication.
        for slot in 0..ENTRIES{
            for len in [1usize,17,513,1100,MAX_FILE_BYTES]{
                let payload=vec![0x5a;len];
                for corrupt_byte in [0,len-1]{
                    let original=payload_rejected_head(slot,&payload,corrupt_byte);
                    let heads=[original.sectors[0],original.sectors[1]];
                    let recovery=DurableFs::mount(original.clone()).unwrap();
                    let generation=recovery.generation();
                    for cut in 0..=SECTORS_PER_BANK{
                        let disk=PowerCutDisk{disk:original.clone(),writes_left:cut};
                        let mut fs=DurableFs::mount(disk).unwrap();
                        assert_eq!(fs.create_path("/intended",&payload),Err(FsError::Io));
                        let disk=fs.into_device().disk;
                        assert_eq!([disk.sectors[0],disk.sectors[1]],heads);
                        assert_eq!(disk.writes,original.writes+cut);
                        let mut recovered=DurableFs::mount(disk).unwrap();
                        assert_eq!(recovered.generation(),generation,
                            "slot {slot}, len {len}, corrupt {corrupt_byte}, cut {cut}");
                        for name in ["/rejected","/intended"]{
                            assert_eq!(recovered.read_path(name,&mut [0;MAX_FILE_BYTES]),
                                Err(FsError::NotFound));
                        }
                        for index in 0..slot{
                            assert_eq!(get(&mut recovered,&format!("/keep{index}")),
                                b"unrelated committed data");
                        }
                        // Explicit remount/retry can still use the third
                        // bank and publish exactly the intended namespace.
                        recovered.create_path("/intended",&payload).unwrap();
                        let mut committed=DurableFs::mount_healthy(recovered.into_device()).unwrap();
                        assert_eq!(get(&mut committed,"/intended"),payload);
                        assert_eq!(committed.read_path("/rejected",&mut [0;MAX_FILE_BYTES]),
                            Err(FsError::NotFound));
                    }
                }
            }
        }
    }
    #[test]fn m1_recovery_quarantine_is_released_by_metadata_replacement(){
        // Fill every slot. Retaining the rejected bank must neither lose a
        // file slot nor permanently consume any of the three COW banks.
        let payload=[0x63;MAX_FILE_BYTES];
        let disk=payload_rejected_head(ENTRIES-1,&payload,MAX_FILE_BYTES-1);
        let mut recovered=DurableFs::mount(disk).unwrap();
        assert_eq!(recovered.available_bank(ENTRIES-1),Some(1));
        recovered.create_path("/intended",&payload).unwrap();
        assert_eq!(recovered.available_bank(ENTRIES-1),Some(0));
        for round in 0..12u8{
            let bytes=[round;MAX_FILE_BYTES];
            recovered.overwrite_path("/intended",&bytes).unwrap();
            recovered=DurableFs::mount_healthy(recovered.into_device()).unwrap();
            assert_eq!(get(&mut recovered,"/intended"),bytes);
        }
        for index in 0..ENTRIES-1{
            assert_eq!(get(&mut recovered,&format!("/keep{index}")),
                b"unrelated committed data");
        }
    }
    #[test]fn m1_recovery_surviving_and_rejected_heads_leave_third_bank_usable(){
        for len in [1usize,513,MAX_FILE_BYTES]{
            let before=vec![0x25;len];
            let rejected=vec![0x76;len];
            let mut fs=DurableFs::format_blank(blank()).unwrap();
            fs.create_path("/data",&before).unwrap();
            fs.overwrite_path("/data",&rejected).unwrap();
            assert_eq!(fs.meta.entries[0].bank,1);
            let mut original=fs.into_device();
            original.sectors[block_start(0,1) as usize+(len-1)/SECTOR_BYTES]
                [(len-1)%SECTOR_BYTES]^=1;
            for cut in 0..=SECTORS_PER_BANK{
                let disk=PowerCutDisk{disk:original.clone(),writes_left:cut};
                let mut fs=DurableFs::mount(disk).unwrap();
                assert_eq!(fs.available_bank(0),Some(2));
                assert_eq!(fs.overwrite_path("/data",&rejected),Err(FsError::Io));
                let mut recovered=DurableFs::mount(fs.into_device().disk).unwrap();
                assert_eq!(get(&mut recovered,"/data"),before);
                recovered.overwrite_path("/data",&rejected).unwrap();
                assert_eq!(recovered.meta.entries[0].bank,2);
                assert_eq!(recovered.available_bank(0),Some(1));
                let mut committed=DurableFs::mount_healthy(recovered.into_device()).unwrap();
                assert_eq!(get(&mut committed,"/data"),rejected);
            }
        }
    }
    #[test]fn m1_recovery_structurally_invalid_head_does_not_reserve_payload_bank(){
        let mut disk=payload_rejected_head(0,b"payload",0);
        disk.sectors[1][24]^=1; // bad metadata CRC cannot revive through data writes
        let mut recovered=DurableFs::mount(disk).unwrap();
        assert_eq!(recovered.available_bank(0),Some(0));
        recovered.create_path("/intended",b"payload").unwrap();
        let mut recovered=DurableFs::mount_healthy(recovered.into_device()).unwrap();
        assert_eq!(get(&mut recovered,"/intended"),b"payload");
    }

    #[test]fn m1_r088_mount_rejects_capacity_change_while_verifying_file_payload(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/data",b"persist").unwrap();
        let entry=fs.meta.entries[0];
        let payload_lba=block_start(0,entry.bank);
        let media=std::rc::Rc::new(std::cell::Cell::new(MIN_SECTORS));
        let disk=ChangingGeometryDisk{
            disk:PowerCutDisk{disk:fs.into_device(),writes_left:8},
            sectors:media,
            switch_on_read:Some((payload_lba,MIN_SECTORS+1)),
        };
        assert!(matches!(DurableFs::mount(disk),Err(FsError::Io)));
    }
    #[test]fn m1_r088_file_read_rejects_mid_sector_media_change_without_output(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/data",b"persist").unwrap();
        let entry=fs.meta.entries[0];
        let payload_lba=block_start(0,entry.bank);
        let media=std::rc::Rc::new(std::cell::Cell::new(MIN_SECTORS));
        let disk=ChangingGeometryDisk{
            disk:PowerCutDisk{disk:fs.into_device(),writes_left:8},
            sectors:media.clone(),switch_on_read:None,
        };
        let mut mounted=DurableFs::mount(disk).unwrap();
        mounted.disk.switch_on_read=Some((payload_lba,MIN_SECTORS-1));
        let mut out=[0xa5;24];
        assert_eq!(mounted.read_path("/data",&mut out),Err(FsError::InvalidVolume));
        assert_eq!(out,[0xa5;24]);
        assert_eq!(media.get(),MIN_SECTORS-1);
    }

    #[test]fn m1_r088_sync_reports_unsupported_driver_without_new_disk_writes(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/data",b"persist").unwrap();
        let writes=fs.disk.writes;
        let generation=fs.generation();
        assert_eq!(fs.sync(),Err(FsError::Io));
        assert_eq!(fs.disk.writes,writes);
        assert_eq!(fs.generation(),generation);
        assert_eq!(get(&mut fs,"/data"),b"persist");
    }
    #[test]fn m1_r088_sync_only_acknowledges_true_flush_and_preserves_content(){
        struct Flushable{disk:Disk,flushed:std::rc::Rc<std::cell::Cell<u32>>,fail:bool}
        impl BlockDevice for Flushable{
            fn sector_count(&self)->u64{self.disk.sector_count()}
            fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
              ->Result<(),BlockError>{self.disk.read_sector(lba,out)}
            fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])
              ->Result<(),BlockError>{self.disk.write_sector(lba,bytes)}
            fn sync(&mut self)->Result<(),BlockError>{
                if self.fail{return Err(BlockError::Io);}
                self.flushed.set(self.flushed.get()+1);Ok(())
            }
        }
        let flushed=std::rc::Rc::new(std::cell::Cell::new(0));
        let disk=Flushable{disk:blank(),flushed:flushed.clone(),fail:false};
        let mut fs=DurableFs::format_blank(disk).unwrap();
        fs.create_path("/data",b"durable").unwrap();
        let before=fs.generation();
        assert_eq!(fs.sync(),Ok(()));
        assert_eq!(flushed.get(),1);
        assert_eq!(fs.generation(),before);
        fs.disk.fail=true;
        assert_eq!(fs.sync(),Err(FsError::Io));
        assert_eq!(flushed.get(),1);
        fs.disk.fail=false;
        fs=DurableFs::mount(fs.into_device()).unwrap();
        let mut data=[0u8;32];
        let size=fs.read_path("/data",&mut data).unwrap();
        assert_eq!(&data[..size],b"durable");
        assert_eq!(fs.sync(),Ok(()));
        assert_eq!(flushed.get(),2);
    }

    #[test]fn m1_r089_fsync_refuses_corrupted_payload_before_touching_flush(){
        struct VerifiedDisk{
            disk:Disk,
            flushes:std::rc::Rc<std::cell::Cell<u32>>,
        }
        impl BlockDevice for VerifiedDisk{
            fn sector_count(&self)->u64{self.disk.sector_count()}
            fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
              ->Result<(),BlockError>{self.disk.read_sector(lba,out)}
            fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
              ->Result<(),BlockError>{self.disk.write_sector(lba,data)}
            fn sync(&mut self)->Result<(),BlockError>{
                self.flushes.set(self.flushes.get()+1);Ok(())
            }
        }
        let flushes=std::rc::Rc::new(std::cell::Cell::new(0));
        let mut fs=DurableFs::format_blank(VerifiedDisk{
            disk:blank(),flushes:flushes.clone()
        }).unwrap();
        fs.create_path("/data",b"important persisted content").unwrap();
        let slot=fs.find(canonical_path("/data").unwrap()).unwrap();
        let bank=fs.meta.entries[slot].bank;
        let lba=block_start(slot,bank) as usize;
        fs.disk.disk.sectors[lba][0]^=0x30; // metadata unchanged; payload CRC broken
        assert_eq!(fs.sync(),Err(FsError::InvalidVolume));
        assert_eq!(flushes.get(),0);
    }
    #[test]fn m1_r089_fsync_degraded_metadata_needs_explicit_repair(){
        struct VerifiedDisk{disk:Disk,flushes:u32}
        impl BlockDevice for VerifiedDisk{
            fn sector_count(&self)->u64{self.disk.sector_count()}
            fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
              ->Result<(),BlockError>{self.disk.read_sector(lba,out)}
            fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
              ->Result<(),BlockError>{self.disk.write_sector(lba,data)}
            fn sync(&mut self)->Result<(),BlockError>{self.flushes+=1;Ok(())}
        }
        let mut fs=DurableFs::format_blank(VerifiedDisk{
            disk:blank(),flushes:0
        }).unwrap();
        fs.create_path("/data",b"correct").unwrap();
        // Old checkpoint is corrupted, current payload still valid.
        let older=1-fs.active as usize;
        fs.disk.disk.sectors[older][24]^=0x80;
        assert_eq!(fs.sync(),Err(FsError::InvalidVolume));
        assert_eq!(fs.disk.flushes,0);
        assert_eq!(fs.disk.disk.writes>0,true);
        // A freshly mounted degraded volume may read data, but fsync must
        // not advertise fully healthy redundancy.
        let mut degraded=DurableFs::mount(fs.into_device()).unwrap();
        assert_eq!(degraded.sync(),Err(FsError::InvalidVolume));
        assert_eq!(degraded.disk.flushes,0);
    }
    #[test]fn m1_recovery_changed_geometry_cannot_drop_quarantine_or_mutate(){
        for changed in [0,MIN_SECTORS-1,MIN_SECTORS+1,u32::MAX as u64+1]{
            let original=payload_rejected_head(1,b"identical payload",0);
            let sectors=std::rc::Rc::new(std::cell::Cell::new(MIN_SECTORS));
            let disk=ChangingGeometryDisk{
                disk:PowerCutDisk{disk:original.clone(),writes_left:1},
                sectors:sectors.clone(),
                switch_on_read:None,
            };
            let mut fs=DurableFs::mount(disk).unwrap();
            sectors.set(changed);
            // The bank allocation must retain the originally decoded
            // rejected head even when live geometry becomes untrustworthy.
            assert_eq!(fs.available_bank(1),Some(1));
            assert_eq!(fs.create_path("/intended",b"identical payload"),
                Err(FsError::InvalidVolume));
            assert_eq!(fs.overwrite_path("/keep0",b"changed"),
                Err(FsError::InvalidVolume));
            assert_eq!(fs.delete_path("/keep0"),Err(FsError::InvalidVolume));
            assert_eq!(fs.rename_path("/keep0","/renamed"),Err(FsError::InvalidVolume));
            assert_eq!(fs.repair_redundancy(),Err(FsError::InvalidVolume));
            assert_eq!(fs.disk.disk.writes_left,1);
            assert_eq!(fs.disk.disk.disk.writes,original.writes);
            assert_eq!(fs.disk.disk.disk.sectors,original.sectors);
            sectors.set(MIN_SECTORS);
            let mut recovered=DurableFs::mount(fs.into_device().disk.disk).unwrap();
            assert_eq!(get(&mut recovered,"/keep0"),b"unrelated committed data");
            assert_eq!(recovered.read_path("/rejected",&mut [0;MAX_FILE_BYTES]),
                Err(FsError::NotFound));
        }
    }
    #[test]fn m1_recovery_geometry_must_stay_stable_through_mount_health_and_format(){
        fn switching(disk:Disk)->ChangingGeometryDisk{
            ChangingGeometryDisk{
                // Even an attempted write would return Io instead of the
                // expected geometry refusal, so these paths must not write.
                disk:PowerCutDisk{disk,writes_left:0},
                sectors:std::rc::Rc::new(std::cell::Cell::new(MIN_SECTORS)),
                switch_on_read:Some((1,MIN_SECTORS+1)),
            }
        }
        let fs=DurableFs::format_blank(blank()).unwrap();
        let disk=fs.into_device();
        assert!(matches!(DurableFs::mount(switching(disk.clone())),
            Err(FsError::InvalidVolume)));
        assert_eq!(DurableFs::inspect_health(&mut switching(disk)),
            Err(FsError::InvalidVolume));
        assert!(matches!(DurableFs::format_blank(switching(blank())),
            Err(FsError::InvalidVolume)));
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
    fn lying_storage_data_or_metadata_ack_cannot_commit_invalid_generation(){
        for dropped in [0usize,4usize]{
            let mut fs=DurableFs::format_blank(blank()).unwrap();
            fs.create_path("/data",b"previous committed").unwrap();
            let mut disk=fs.into_device();
            disk.ignore_at=Some(disk.writes+dropped);
            let mut fs=DurableFs::mount(disk).unwrap();
            assert_eq!(fs.overwrite_path("/data",b"new uncommitted"),
                Err(FsError::InvalidVolume));
            let mut disk=fs.into_device();
            disk.ignore_at=None;
            let mut remount=DurableFs::mount(disk).unwrap();
            assert_eq!(get(&mut remount,"/data"),b"previous committed");
        }
    }
    #[test]
    fn format_detects_acknowledged_but_missing_superblock(){
        for dropped in [0usize,1usize]{
            let mut disk=blank();disk.ignore_at=Some(dropped);
            assert!(matches!(DurableFs::format_blank(disk),
                Err(FsError::InvalidVolume)));
        }
    }
    #[test]
    fn equal_generation_split_brain_is_rejected_without_disk_mutation(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/data",b"committed").unwrap();
        let mut disk=fs.into_device();
        // Both copies pass CRC and data verification but disagree on
        // directory contents at the same generation.
        let active=1usize;
        let good=decode(&disk.sectors[active],MIN_SECTORS).unwrap();
        let mut divergent=good;
        divergent.entries[0]=Entry::EMPTY;
        disk.sectors[1-active]=encode(&divergent,MIN_SECTORS);
        let writes=disk.writes;
        assert!(matches!(DurableFs::mount(disk.clone()),
            Err(FsError::InvalidVolume)));
        assert_eq!(disk.writes,writes);
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
    #[test]
    fn m1_r045_clean_and_committed_volume_health_is_read_only(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        let original=fs.into_device();
        let mut d=original;
        let writes=d.writes;
        let h=DurableFs::inspect_health(&mut d).unwrap();
        assert_eq!(h.redundancy,Redundancy::Healthy);
        assert_eq!(h.valid_copies,2);
        assert_eq!(h.current_generation,1);
        assert_eq!(h.previous_generation,Some(1));
        assert_eq!(d.writes,writes);
        let mut fs=DurableFs::mount(d).unwrap();
        fs.create_path("/file",b"hello world").unwrap();
        let mut d=fs.into_device();
        let writes=d.writes;
        let h=DurableFs::inspect_health(&mut d).unwrap();
        assert_eq!(h.redundancy,Redundancy::Healthy);
        assert_eq!(h.valid_copies,2);
        assert_eq!(h.current_generation,2);
        assert_eq!(h.previous_generation,Some(1));
        assert_eq!(d.writes,writes);
    }
    #[test]
    fn m1_r045_single_metadata_copy_corruption_is_reported_as_degraded(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/file",b"stable").unwrap();
        let active=fs.active as usize;
        let mut d=fs.into_device();
        let before=d.writes;
        d.sectors[1-active][24]^=1;
        let h=DurableFs::inspect_health(&mut d).unwrap();
        assert_eq!(h.redundancy,Redundancy::Degraded);
        assert_eq!(h.valid_copies,1);
        assert_eq!(h.current_generation,2);
        assert_eq!(h.previous_generation,None);
        assert_eq!(d.writes,before);
        let mut mounted=DurableFs::mount(d).unwrap();
        assert_eq!(get(&mut mounted,"/file"),b"stable");
    }
    #[test]
    fn m1_r045_corrupt_latest_checkpoint_falls_back_to_previous_valid_bank(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/file",b"first").unwrap();
        fs.overwrite_path("/file",b"second").unwrap();
        let active=fs.active as usize;
        let mut d=fs.into_device();
        d.sectors[active][30]^=1;
        let h=DurableFs::inspect_health(&mut d).unwrap();
        assert_eq!(h.redundancy,Redundancy::Degraded);
        assert_eq!(h.current_generation,2);
        let mut fs=DurableFs::mount(d).unwrap();
        assert_eq!(get(&mut fs,"/file"),b"first");
    }
    #[test]
    fn m1_r045_two_invalid_copies_cannot_be_misreported_as_healthy(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/file",b"stable").unwrap();
        let mut d=fs.into_device();
        let before=d.writes;
        d.sectors[0][16]^=7;
        d.sectors[1][16]^=9;
        assert_eq!(DurableFs::inspect_health(&mut d),Err(FsError::InvalidVolume));
        assert_eq!(d.writes,before);
    }
    #[test]
    fn m1_r045_equal_generation_split_brain_health_rejected(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/file",b"data").unwrap();
        let mut d=fs.into_device();
        let same=decode(&d.sectors[1],MIN_SECTORS).unwrap();
        let mut different=same;
        different.entries[0]=Entry::EMPTY;
        d.sectors[0]=encode(&different,MIN_SECTORS);
        let writes=d.writes;
        assert_eq!(DurableFs::inspect_health(&mut d),Err(FsError::InvalidVolume));
        assert_eq!(d.writes,writes);
    }
    #[test]
    fn m1_r045_recovery_write_restores_redundancy_without_reformat(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/file",b"first").unwrap();
        let active=fs.active as usize;
        let mut d=fs.into_device();
        d.sectors[1-active][24]^=1;
        assert_eq!(DurableFs::inspect_health(&mut d).unwrap().valid_copies,1);
        let mut recovered=DurableFs::mount(d).unwrap();
        recovered.overwrite_path("/file",b"new commit").unwrap();
        let mut d=recovered.into_device();
        assert_eq!(DurableFs::inspect_health(&mut d).unwrap().valid_copies,2);
        let mut remounted=DurableFs::mount(d).unwrap();
        assert_eq!(get(&mut remounted,"/file"),b"new commit");
    }
    #[test]
    fn m1_r045_bad_data_checksum_disqualifies_only_referencing_generation(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/file",b"old").unwrap();
        fs.overwrite_path("/file",b"new").unwrap();
        let active=fs.active as usize;
        let bank=fs.meta.entries[0].bank;
        let mut d=fs.into_device();
        d.sectors[block_start(0,bank) as usize][0]^=0xff;
        let h=DurableFs::inspect_health(&mut d).unwrap();
        assert_eq!(h.valid_copies,1);
        assert_eq!(h.redundancy,Redundancy::Degraded);
        assert_eq!(h.current_generation,2);
        let mut mounted=DurableFs::mount(d).unwrap();
        assert_eq!(get(&mut mounted,"/file"),b"old");
        assert_ne!(active,2);
    }

    #[test]
    fn m1_payload_io_failure_never_masquerades_as_checkpoint_corruption(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/file",&[0x31;900]).unwrap();
        fs.overwrite_path("/file",&[0x72;900]).unwrap();
        let current=fs.meta.entries[0];
        let previous=fs.previous.unwrap().entries[0];
        let disk=fs.into_device();
        // Failure in either generation, including after one readable data
        // sector, is uncertainty about media rather than corruption proof.
        for entry in [current,previous]{
            for sector in [0,1]{
                let mut unreadable=disk.clone();
                unreadable.fail_read_lba=Some(block_start(0,entry.bank)+sector);
                let before=unreadable.sectors;
                let writes=unreadable.writes;
                assert_eq!(DurableFs::inspect_health(&mut unreadable),Err(FsError::Io));
                assert_eq!(unreadable.writes,writes);
                assert_eq!(unreadable.sectors,before);
                assert!(matches!(DurableFs::mount(unreadable.clone()),Err(FsError::Io)));
                assert!(matches!(DurableFs::mount_healthy(unreadable.clone()),Err(FsError::Io)));
                unreadable.fail_read_lba=None;
                let health=DurableFs::inspect_health(&mut unreadable).unwrap();
                assert_eq!(health.redundancy,Redundancy::Healthy);
                assert_eq!(health.current_generation,3);
                let mut remounted=DurableFs::mount(unreadable).unwrap();
                assert_eq!(get(&mut remounted,"/file"),[0x72;900]);
            }
        }
    }

    #[test]
    fn m1_repair_and_health_propagate_payload_io_without_writing(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/file",&[0x45;900]).unwrap();
        // Prepare a genuinely degraded metadata copy. A data read failure
        // must still block repair instead of producing a false diagnosis.
        let inactive=1-fs.active as usize;
        let mut disk=fs.into_device();
        disk.sectors[inactive][0]^=1;
        let mut fs=DurableFs::mount(disk).unwrap();
        let data_lba=block_start(0,fs.meta.entries[0].bank)+1;
        fs.disk.fail_read_lba=Some(data_lba);
        let before=fs.disk.sectors;
        let writes=fs.disk.writes;
        assert_eq!(fs.mounted_health(),Err(FsError::Io));
        assert_eq!(fs.repair_redundancy(),Err(FsError::Io));
        let mut output=[0xa5;900];
        assert_eq!(fs.read_path("/file",&mut output),Err(FsError::Io));
        assert_eq!(output,[0xa5;900]);
        assert_eq!(fs.disk.writes,writes);
        assert_eq!(fs.disk.sectors,before);
        fs.disk.fail_read_lba=None;
        assert_eq!(fs.repair_redundancy(),Ok(true));
        let mut remounted=DurableFs::mount_healthy(fs.into_device()).unwrap();
        assert_eq!(get(&mut remounted,"/file"),[0x45;900]);
    }

    #[test]
    fn m1_r046_strict_mount_rejects_degraded_but_recovery_mount_reads_file(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/note",b"keep").unwrap();
        let active=fs.active as usize;
        let mut disk=fs.into_device();
        disk.sectors[1-active][24]^=0x10;
        let initial=disk.writes;
        assert!(matches!(DurableFs::mount_healthy(disk.clone()),
            Err(FsError::InvalidVolume)));
        let mut recovered=DurableFs::mount(disk).unwrap();
        assert_eq!(get(&mut recovered,"/note"),b"keep");
        assert_eq!(recovered.mounted_health().unwrap().valid_copies,1);
        assert_eq!(recovered.disk.writes,initial);
    }
    #[test]
    fn m1_r046_explicit_repair_restores_both_superblocks_without_file_writes(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/note",b"committed data").unwrap();
        let active=fs.active as usize;
        let mut disk=fs.into_device();
        disk.sectors[1-active][24]^=0x41;
        let before=disk.sectors.clone();
        let before_writes=disk.writes;
        let mut fs=DurableFs::mount(disk).unwrap();
        assert_eq!(fs.mounted_health().unwrap().valid_copies,1);
        assert_eq!(fs.repair_redundancy(),Ok(true));
        assert_eq!(fs.disk.writes,before_writes+1);
        for i in 2..MIN_SECTORS as usize{
            assert_eq!(fs.disk.sectors[i],before[i],"payload sector {i} mutated");
        }
        let mut fs=DurableFs::mount_healthy(fs.into_device()).unwrap();
        assert_eq!(fs.mounted_health().unwrap().valid_copies,2);
        assert_eq!(get(&mut fs,"/note"),b"committed data");
    }
    #[test]
    fn m1_r046_noop_repair_never_erases_valid_rollback_generation(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/note",b"version one").unwrap();
        fs.overwrite_path("/note",b"version two").unwrap();
        let before=fs.disk.writes;
        assert_eq!(fs.repair_redundancy(),Ok(false));
        assert_eq!(fs.disk.writes,before);
        let health=fs.mounted_health().unwrap();
        assert_eq!(health.current_generation,3);
        assert_eq!(health.previous_generation,Some(2));
        assert_eq!(get(&mut fs,"/note"),b"version two");
    }
    #[test]
    fn m1_r046_failed_repair_write_preserves_previous_committed_payload(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/note",b"safe").unwrap();
        let active=fs.active as usize;
        let mut disk=fs.into_device();
        disk.sectors[1-active][24]^=1;
        disk.fail_at=Some(disk.writes);
        let mut fs=DurableFs::mount(disk).unwrap();
        assert_eq!(fs.repair_redundancy(),Err(FsError::Io));
        let mut disk=fs.into_device();
        disk.fail_at=None;
        let mut fs=DurableFs::mount(disk).unwrap();
        assert_eq!(get(&mut fs,"/note"),b"safe");
        assert_eq!(fs.mounted_health().unwrap().valid_copies,1);
        assert_eq!(fs.repair_redundancy(),Ok(true));
        let mut fs=DurableFs::mount_healthy(fs.into_device()).unwrap();
        assert_eq!(get(&mut fs,"/note"),b"safe");
    }
    #[test]
    fn m1_r046_acknowledged_but_missing_repair_write_never_signals_success(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/note",b"unchanged").unwrap();
        let active=fs.active as usize;
        let mut disk=fs.into_device();
        disk.sectors[1-active][24]^=1;
        disk.ignore_at=Some(disk.writes);
        let mut fs=DurableFs::mount(disk).unwrap();
        assert_eq!(fs.repair_redundancy(),Err(FsError::InvalidVolume));
        let mut disk=fs.into_device();
        disk.ignore_at=None;
        let mut fs=DurableFs::mount(disk).unwrap();
        assert_eq!(fs.mounted_health().unwrap().valid_copies,1);
        assert_eq!(get(&mut fs,"/note"),b"unchanged");
    }
    #[test]
    fn m1_r046_fully_healthy_repair_then_cow_write_and_remount(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/note",b"first").unwrap();
        let active=fs.active as usize;
        let mut disk=fs.into_device();
        disk.sectors[1-active][24]^=1;
        let mut fs=DurableFs::mount(disk).unwrap();
        fs.repair_redundancy().unwrap();
        fs.overwrite_path("/note",b"second").unwrap();
        let mut fs=DurableFs::mount_healthy(fs.into_device()).unwrap();
        assert_eq!(get(&mut fs,"/note"),b"second");
        assert_eq!(fs.mounted_health().unwrap().valid_copies,2);
    }
    #[test]
    fn m1_r046_reject_valid_checksums_with_nonadjacent_metadata_versions(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/note",b"first").unwrap();
        fs.overwrite_path("/note",b"second").unwrap();
        fs.overwrite_path("/note",b"third").unwrap();
        let older=(1-fs.active) as usize;
        let mut disk=fs.into_device();
        let mut decoded=decode(&disk.sectors[older],MIN_SECTORS).unwrap();
        decoded.generation=1; // still valid CRC/payload but impossible gap
        disk.sectors[older]=encode(&decoded,MIN_SECTORS);
        let before=disk.writes;
        assert_eq!(DurableFs::inspect_health(&mut disk),Err(FsError::InvalidVolume));
        assert!(matches!(DurableFs::mount(disk.clone()),Err(FsError::InvalidVolume)));
        assert_eq!(disk.writes,before);
    }
    #[test]
    fn m1_r046_repair_rejects_stale_live_superblock_without_writing(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/note",b"safe").unwrap();
        let active=fs.active as usize;
        fs.disk.sectors[1-active][24]^=1;
        fs.disk.sectors[active][24]^=1; // current cached head now invalid
        let writes=fs.disk.writes;
        assert_eq!(fs.mounted_health(),Err(FsError::InvalidVolume));
        assert_eq!(fs.repair_redundancy(),Err(FsError::InvalidVolume));
        assert_eq!(fs.disk.writes,writes);
    }
    #[test]
    fn m1_r046_torn_cow_update_at_each_sector_preserves_last_commit(){
        for stage in 0..5{
            let mut fs=DurableFs::format_blank(blank()).unwrap();
            fs.create_path("/note",&[0x55;1100]).unwrap();
            let mut disk=fs.into_device();
            disk.fail_at=Some(disk.writes+stage);
            let mut fs=DurableFs::mount(disk).unwrap();
            assert_eq!(fs.overwrite_path("/note",&[0xaa;1900]),Err(FsError::Io),
                "failed stage {stage}");
            let mut disk=fs.into_device();
            disk.fail_at=None;
            let mut remounted=DurableFs::mount(disk).unwrap();
            assert_eq!(get(&mut remounted,"/note").as_slice(),&[0x55;1100],
                "torn stage {stage}");
        }
    }
    #[test]
    fn m1_r046_silent_dropped_cow_write_at_each_stage_never_commits(){
        for stage in 0..5{
            let mut fs=DurableFs::format_blank(blank()).unwrap();
            fs.create_path("/note",&[0x55;1100]).unwrap();
            let mut disk=fs.into_device();
            disk.ignore_at=Some(disk.writes+stage);
            let mut fs=DurableFs::mount(disk).unwrap();
            assert_eq!(fs.overwrite_path("/note",&[0xaa;1900]),
                Err(FsError::InvalidVolume),"ignored stage {stage}");
            let mut disk=fs.into_device();
            disk.ignore_at=None;
            let mut remounted=DurableFs::mount(disk).unwrap();
            assert_eq!(get(&mut remounted,"/note").as_slice(),&[0x55;1100],
                "ignored stage {stage}");
        }
    }

}
