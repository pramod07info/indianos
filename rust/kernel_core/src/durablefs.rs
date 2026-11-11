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
use crate::fs_permissions::{Credentials,Permissions};

const MAGIC:&[u8;8]=b"INDR027!";
const FORMAT_VERSION:u32=1;
const PERMISSION_VERSION:u32=2;
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
    permissions:Permissions,
}
impl Entry{
    const EMPTY:Self=Self{name:[0;NAME_BYTES],len:0,checksum:0,bank:0,used:false,permissions:Permissions::LEGACY};
}
#[derive(Clone,Copy)]
struct Metadata{generation:u64,entries:[Entry;ENTRIES],format:u32}
impl Metadata{fn empty()->Self{Self{generation:1,entries:[Entry::EMPTY;ENTRIES],format:FORMAT_VERSION}}}

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
    output[8..12].copy_from_slice(&meta.format.to_le_bytes());
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
        if meta.format==PERMISSION_VERSION{
            output[offset+34..offset+36].copy_from_slice(&e.permissions.owner.to_le_bytes());
            output[offset+36..offset+38].copy_from_slice(&e.permissions.group.to_le_bytes());
            output[offset+38..offset+40].copy_from_slice(&e.permissions.mode.to_le_bytes());
        }
    }
    // CRC field is zero while computing checksum of the whole metadata sector.
    let sum=crc(&output);
    output[24..28].copy_from_slice(&sum.to_le_bytes());
    output
}
fn decode(sector:&[u8;SECTOR_BYTES],capacity:u64)->Option<Metadata>{
    let format=read_u32(&sector[8..12]);
    if &sector[..8]!=MAGIC || !matches!(format,FORMAT_VERSION|PERMISSION_VERSION) ||
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
    let mut result=Metadata{generation,entries:[Entry::EMPTY;ENTRIES],format};
    for index in 0..ENTRIES{
        let offset=HEADER_SIZE+index*ENTRY_SIZE;
        let entry=&sector[offset..offset+ENTRY_SIZE];
        if entry.iter().all(|&b|b==0){continue;}
        if entry[33]!=1 || (format==FORMAT_VERSION&&entry[34..].iter().any(|&b|b!=0)) ||
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
        let permissions=if format==FORMAT_VERSION{Permissions::LEGACY}else{
            Permissions::new(u16::from_le_bytes(entry[34..36].try_into().ok()?),
                u16::from_le_bytes(entry[36..38].try_into().ok()?),
                u16::from_le_bytes(entry[38..40].try_into().ok()?))?
        };
        result.entries[index]=Entry{
            name,len,checksum:read_u32(&entry[28..32]),
            bank:entry[32],used:true,permissions
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
/// Payload verification may perform many reads between the two headers.
/// Reject a mixed checkpoint snapshot before exposing admission results.
/// The volume owner still must serialize operations; this is not an atomic
/// snapshot against concurrent DMA writers or same-geometry media swaps.
fn recheck_heads<D:BlockDevice>(disk:&mut D,sectors:u64,
    expected:&[[u8;SECTOR_BYTES];2])->Result<(),FsError>{
    for (index,head) in expected.iter().enumerate(){
        if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
        let mut observed=[0u8;SECTOR_BYTES];
        disk.read_sector(index as u64,&mut observed)?;
        if disk.sector_count()!=sectors||observed!=*head{
            return Err(FsError::InvalidVolume);
        }
    }
    Ok(())
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
    // An uncertain physical write or flush permanently invalidates this
    // mounted session. Remount explicitly to reselect verified checkpoints.
    retired:core::cell::Cell<bool>,
}
/// Explicit recovery-only volume view that cannot write even when a
/// privileged caller accidentally invokes a mutating DurableFs API.
/// Unlike a software read-only flag, the underlying BlockDevice capability
/// refuses all sector writes and cache flushes. This does not auto-repair.
pub fn mount_recovery_readonly<'a,D:BlockDevice>(
 device:&'a mut D)->Result<DurableFs<crate::block::ReadOnlyDevice<'a,D>>,FsError>{
 DurableFs::mount(crate::block::ReadOnlyDevice::new(device))
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
            // Recheck around every read: a removable device can change
            // geometry partway through a long blankness scan.
            if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
            let mut sector=[0u8;SECTOR_BYTES];
            disk.read_sector(lba,&mut sector)?;
            if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
            if sector.iter().any(|&b|b!=0){return Err(FsError::InvalidVolume);}
        }
        if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
        let meta=Metadata::empty();
        let data=encode(&meta,sectors);
        disk.write_sector(0,&data)?;
        if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
        disk.write_sector(1,&data)?;
        if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
        // Formatting is explicitly opt-in, but it must still fail closed if
        // the selected device acknowledges a dropped or torn superblock.
        for lba in 0..2{
            if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
            let mut observed=[0u8;SECTOR_BYTES];
            disk.read_sector(lba,&mut observed)?;
            if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
            if observed!=data{return Err(FsError::InvalidVolume);}
        }
        if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
        Ok(Self{disk,sectors,meta,active:0,previous:Some(meta),heads:[data;2],retired:core::cell::Cell::new(false)})
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
            if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
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
        recheck_heads(&mut disk,sectors,&heads)?;
        Ok(Self{disk,sectors,meta,active,previous,heads,retired:core::cell::Cell::new(false)})
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
        let mut heads=[[0u8;SECTOR_BYTES];2];
        for index in 0..2{
            let mut header=[0u8;SECTOR_BYTES];
            disk.read_sector(index as u64,&mut header)?;
            if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
            heads[index]=header;
            if let Some(meta)=decode(&header,sectors){
                if verify_metadata(disk,&meta)?{copies[index]=Some(meta);}
            }
        }
        if disk.sector_count()!=sectors{return Err(FsError::InvalidVolume);}
        recheck_heads(disk,sectors,&heads)?;
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
    pub fn mount_healthy(disk:D)->Result<Self,FsError>{
        let mut mounted=Self::mount(disk)?;
        // Apply the strict policy to the mount actually returned. Checking
        // health before a separate mount can admit a newly degraded volume.
        let health=mounted.mounted_health()?;
        if health.redundancy!=Redundancy::Healthy{
            return Err(FsError::InvalidVolume);
        }
        Ok(mounted)
    }
    /// Re-probe actual disk sectors, not cached metadata. Fail if a different
    /// process changed either recorded head while this filesystem was mounted.
    pub fn mounted_health(&mut self)->Result<VolumeHealth,FsError>{
        self.check_heads()?;
        let report=Self::inspect_health(&mut self.disk)?;
        self.check_heads()?;
        if report.current_generation!=self.meta.generation{
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
        // Any uncertain repair write retires this session, forcing a new
        // health scan rather than silently reusing a possibly torn head.
        if let Err(error)=self.disk.write_sector(inactive,&bytes){
            self.retired.set(true);return Err(error.into());
        }
        let mut observed=[0u8;SECTOR_BYTES];
        if let Err(error)=self.disk.read_sector(inactive,&mut observed){
            self.retired.set(true);return Err(error.into());
        }
        if observed!=bytes{
            self.retired.set(true);return Err(FsError::InvalidVolume);
        }
        // Validate BOTH newly published copies and all referenced file
        // payloads before acknowledging an admin-triggered repair.
        let after=match Self::inspect_health(&mut self.disk){
            Ok(value)=>value,
            Err(error)=>{self.retired.set(true);return Err(error);}
        };
        self.check_geometry()?;
        if after.valid_copies!=2 ||
           after.current_generation!=self.meta.generation{
            self.retired.set(true);return Err(FsError::InvalidVolume);
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
        let result=self.sync_verified();
        if result.is_err(){self.retired.set(true);}
        result
    }
    fn sync_verified(&mut self)->Result<(),FsError>{
        self.check_heads()?;
        let before=self.mounted_health()?;
        if before.redundancy!=Redundancy::Healthy{
            return Err(FsError::InvalidVolume);
        }
        if let Err(error)=self.disk.sync(){
            self.retired.set(true);return Err(error.into());
        }
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
    /// Read checksummed discretionary metadata without exposing file bytes.
    pub fn permissions_path(&mut self,path:&str)->Result<Permissions,FsError>{
        self.check_heads()?;
        let name=canonical_path(path)?;
        let slot=self.find(name).ok_or(FsError::NotFound)?;
        Ok(self.meta.entries[slot].permissions)
    }
    /// Create data and its credentials in one COW checkpoint, never a window
    /// with an unrestricted newly created object followed by chmod.
    pub(crate) fn create_owned_path(&mut self,path:&str,data:&[u8],
        credentials:Credentials,mode:u16)->Result<(),FsError>{
        let permissions=Permissions::new(credentials.uid(),credentials.gid(),mode)
            .ok_or(FsError::InvalidName)?;
        self.put_permissions(path,data,false,Some(permissions))
    }
    pub(crate) fn chmod_path(&mut self,path:&str,credentials:Credentials,
        mode:u16)->Result<(),FsError>{
        self.check_heads()?;
        let name=canonical_path(path)?;
        let slot=self.find(name).ok_or(FsError::NotFound)?;
        let previous=self.meta.entries[slot].permissions;
        if credentials.uid()!=0&&credentials.uid()!=previous.owner{
            return Err(FsError::InvalidName);
        }
        let permissions=Permissions::new(previous.owner,previous.group,mode)
            .ok_or(FsError::InvalidName)?;
        if permissions==previous&&self.meta.format==PERMISSION_VERSION{return Ok(());}
        let mut next=self.meta;next.format=PERMISSION_VERSION;
        next.entries[slot].permissions=permissions;
        self.publish(next)
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
               !matches!(entry.name[1],b'd'|b'f'|b'l'){continue;}
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
        if self.retired.get(){return Err(FsError::InvalidVolume);}
        self.check_geometry()?;
        for index in 0..2 {
            let mut observed=[0u8;SECTOR_BYTES];
            if let Err(error)=self.disk.read_sector(index as u64,&mut observed){
                // Do not trust a failed transfer. Neither publish output
                // nor mutate the checkpoint. The next operation must
                // independently re-read BOTH expected on-disk heads.
                // Corruption/change, unlike a transient I/O error, retires.
                return Err(error.into());
            }
            if observed!=self.heads[index]{
                // Another mount or device replaced our checkpoint.
                // Retire this stale session instead of allowing later reads.
                self.retired.set(true);
                return Err(FsError::InvalidVolume);
            }
        }
        self.check_geometry()
    }
    fn check_geometry(&self)->Result<(),FsError>{
        if self.retired.get(){return Err(FsError::InvalidVolume);}
        if self.disk.sector_count()!=self.sectors{
            self.retired.set(true);
            return Err(FsError::InvalidVolume);
        }
        Ok(())
    }
    /// A checkpoint is usable only when every referenced file verifies.
    /// Metadata-only operations must not overwrite the rollback head while
    /// carrying forward payload corruption discovered since mount.
    fn check_payloads(&mut self,meta:Metadata)->Result<(),FsError>{
        self.check_geometry()?;
        let result=match verify_metadata(&mut self.disk,&meta){
            Ok(true)=>Ok(()),
            Ok(false)=>Err(FsError::InvalidVolume),
            Err(error)=>Err(error),
        };
        if result.is_err(){self.retired.set(true);}
        result?;
        self.check_geometry()
    }
    fn publish(&mut self,mut proposed:Metadata)->Result<(),FsError>{
        self.check_heads()?;
        proposed.generation=self.meta.generation.checked_add(1)
            .ok_or(FsError::InvalidVolume)?;
        self.check_payloads(proposed)?;
        self.check_heads()?;
        let inactive=1-self.active;
        let bytes=encode(&proposed,self.sectors);
        self.check_geometry()?;
        if let Err(error)=self.disk.write_sector(inactive,&bytes){
            self.retired.set(true);return Err(error.into());
        }
        // Verify the readback BEFORE publishing the new in-memory head.
        // A block device that acknowledges but discards a metadata write
        // must never be treated as a committed filesystem transaction.
        let mut observed=[0u8;SECTOR_BYTES];
        if let Err(error)=self.disk.read_sector(inactive,&mut observed){
            self.retired.set(true);return Err(error.into());
        }
        self.check_geometry()?;
        if observed!=bytes{
            self.retired.set(true);return Err(FsError::InvalidVolume);
        }
        // A successful metadata write can still damage a referenced bank.
        // Do not acknowledge a checkpoint whose payloads no longer verify.
        self.check_payloads(proposed)?;
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
            if let Err(error)=self.disk.write_sector(lba,&sector){
                self.retired.set(true);return Err(error.into());
            }
            // Detect devices that report successful writes but return stale
            // or corrupted data before making a new checksum head visible.
            let mut observed=[0u8;SECTOR_BYTES];
            if let Err(error)=self.disk.read_sector(lba,&mut observed){
                self.retired.set(true);return Err(error.into());
            }
            self.check_geometry()?;
            if observed!=sector{
                self.retired.set(true);return Err(FsError::InvalidVolume);
            }
        }
        Ok(())
    }
    fn put(&mut self,path:&str,data:&[u8],replace:bool)->Result<(),FsError>{
        self.put_permissions(path,data,replace,None)
    }
    fn put_permissions(&mut self,path:&str,data:&[u8],replace:bool,
        permissions:Option<Permissions>)->Result<(),FsError>{
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
        let permissions=permissions.unwrap_or(if replace{
            self.meta.entries[index].permissions
        }else{Permissions::LEGACY});
        if permissions!=Permissions::LEGACY{next.format=PERMISSION_VERSION;}
        next.entries[index]=Entry{
            name,len:data.len() as u32,checksum:crc(data),bank,used:true,permissions
        };
        self.publish(next)
    }
    /// Strict atomic COW publication: first flush the new data bank, then
    /// publish the metadata head, then flush the head. Unlike legacy
    /// put(), this explicitly fails on drivers without a known sync contract.
    /// Failure retires the mount; explicit revalidation/remount is required.
    fn put_durable(&mut self,path:&str,data:&[u8],replace:bool)
     ->Result<(),FsError>{
        let name=canonical_path(path)?;
        if data.len()>MAX_FILE_BYTES{return Err(FsError::TooLarge);}
        let index=if replace{
            self.find(name).ok_or(FsError::NotFound)?
        }else{
            if self.find(name).is_some(){return Err(FsError::Exists);}
            (0..ENTRIES).find(|&i|!self.meta.entries[i].used)
             .ok_or(FsError::Full)?
        };
        self.check_heads()?;
        let bank=self.available_bank(index).ok_or(FsError::Full)?;
        self.write_bank(index,bank,data)?;
        if let Err(e)=self.disk.sync(){
            self.retired.set(true);return Err(e.into());
        }
        self.check_heads()?;
        // An untrustworthy controller can return the freshly written bytes
        // before flushing, then discard them while claiming sync success.
        // Re-read every data sector AFTER the barrier, before publishing a
        // metadata checkpoint that would otherwise reference lost bytes.
        for i in 0..SECTORS_PER_BANK{
            let start=i*SECTOR_BYTES;
            let mut expected=[0u8;SECTOR_BYTES];
            if start<data.len(){
                let end=(start+SECTOR_BYTES).min(data.len());
                expected[..end-start].copy_from_slice(&data[start..end]);
            }
            let mut observed=[0u8;SECTOR_BYTES];
            self.check_geometry()?;
            if let Err(error)=self.disk.read_sector(block_start(index,bank)+i as u64,
                                                    &mut observed){
                self.retired.set(true);return Err(error.into());
            }
            self.check_geometry()?;
            if observed!=expected{
                self.retired.set(true);return Err(FsError::InvalidVolume);
            }
        }
        self.check_heads()?;
        let mut next=self.meta;
        next.entries[index]=Entry{name,len:data.len() as u32,
             checksum:crc(data),bank,used:true,permissions:if replace{
                 self.meta.entries[index].permissions
             }else{Permissions::LEGACY}};
        self.publish(next)?;
        if let Err(e)=self.disk.sync(){
            self.retired.set(true);return Err(e.into());
        }
        self.check_heads()?;
        // The final metadata barrier is another device operation: payload
        // integrity checked before it is not sufficient to acknowledge a
        // durable transaction. Keep uncertain sessions permanently retired.
        self.check_payloads(self.meta)?;
        self.check_heads()
    }
    /// Opt-in, flush-verified creation; a missing flush driver is an error.
    pub fn create_durable_path(&mut self,path:&str,data:&[u8])
     ->Result<(),FsError>{self.put_durable(path,data,false)}
    /// Opt-in durable overwrite with two ordered block-device barriers.
    pub fn overwrite_durable_path(&mut self,path:&str,data:&[u8])
     ->Result<(),FsError>{self.put_durable(path,data,true)}
    pub fn create_path(&mut self,path:&str,data:&[u8])->Result<(),FsError>{
        self.put(path,data,false)
    }
    /// Data sectors are flushed before atomically replacing a metadata copy.
    pub fn overwrite_path(&mut self,path:&str,data:&[u8])->Result<(),FsError>{
        self.put(path,data,true)
    }
    pub fn read_path(&mut self,path:&str,out:&mut [u8])->Result<usize,FsError>{
        // A stale mount can otherwise read an old data bank now reused by
        // another writer. Validate our recorded heads before every read.
        self.check_heads()?;
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
            if let Err(error)=self.disk.read_sector(
                block_start(slot,entry.bank)+i as u64,&mut sector){
                // A transient read failure is not an uncertain WRITE.
                // Keep the destination unchanged and require a new complete
                // metadata + payload verification on any later read.
                return Err(error.into());
            }
            self.check_geometry()?;
            let start=i*SECTOR_BYTES;
            let used=(len-start).min(SECTOR_BYTES);
            scratch[start..start+used].copy_from_slice(&sector[..used]);
        }
        if crc(&scratch[..len])!=entry.checksum{
            // The data bank changed after mount. Quarantine this mounted
            // session to prevent subsequent stale reads or bank reuse.
            self.retired.set(true);
            return Err(FsError::InvalidVolume);
        }
        // A different writer may publish while we read the payload.
        // Do not return even checksum-valid bytes from an invalidated mount.
        self.check_heads()?;
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

    /// Load one fully verified file into private fixed-size scratch. Never
    /// publish partial device output or trust only the metadata checksum.
    fn r092_snapshot(&mut self,path:&str)->Result<(usize,[u8;MAX_FILE_BYTES]),FsError>{
        let mut data=[0u8;MAX_FILE_BYTES];
        let len=self.read_path(path,&mut data)?;
        Ok((len,data))
    }
    /// Verified file length; catches stale geometry/corrupt payload before
    /// returning a value potentially used to allocate or edit file content.
    pub fn verified_len_path(&mut self,path:&str)->Result<usize,FsError>{
        let (len,_)=self.r092_snapshot(path)?;
        Ok(len)
    }
    /// CRC32 of the verified entire file, including empty files.
    pub fn verified_crc_path(&mut self,path:&str)->Result<u32,FsError>{
        let (len,data)=self.r092_snapshot(path)?;
        Ok(crc(&data[..len]))
    }
    /// Compare all bytes against a verified on-disk file without writing.
    pub fn equals_path(&mut self,path:&str,expected:&[u8])->Result<bool,FsError>{
        let (len,data)=self.r092_snapshot(path)?;
        Ok(len==expected.len() && &data[..len]==expected)
    }
    /// Atomic append to an existing file. No extension on an I/O failure.
    pub fn append_path(&mut self,path:&str,tail:&[u8])->Result<usize,FsError>{
        let (len,mut data)=self.r092_snapshot(path)?;
        let end=len.checked_add(tail.len()).ok_or(FsError::TooLarge)?;
        if end>MAX_FILE_BYTES{return Err(FsError::TooLarge);}
        if tail.is_empty(){return Ok(0);}
        data[len..end].copy_from_slice(tail);
        self.overwrite_path(path,&data[..end])?;
        Ok(tail.len())
    }
    /// Atomic truncate or zero-fill extension; never creates a sparse hole.
    pub fn resize_path(&mut self,path:&str,new_len:usize)->Result<(),FsError>{
        if new_len>MAX_FILE_BYTES{return Err(FsError::TooLarge);}
        let (len,mut data)=self.r092_snapshot(path)?;
        if new_len==len{return Ok(());}
        if new_len>len{data[len..new_len].fill(0);}
        self.overwrite_path(path,&data[..new_len])
    }
    /// Replace a bounded run with one byte without changing file size.
    pub fn fill_range_path(&mut self,path:&str,offset:usize,
                           count:usize,value:u8)->Result<usize,FsError>{
        let (len,mut data)=self.r092_snapshot(path)?;
        let end=offset.checked_add(count).ok_or(FsError::TooLarge)?;
        if end>len{return Err(FsError::TooLarge);}
        if count==0{return Ok(0);}
        data[offset..end].fill(value);
        self.overwrite_path(path,&data[..len])?;
        Ok(count)
    }
    /// Insert bytes at a file offset, shifting the tail. One COW publication.
    pub fn insert_range_path(&mut self,path:&str,offset:usize,
                             inserted:&[u8])->Result<usize,FsError>{
        let (len,mut data)=self.r092_snapshot(path)?;
        if offset>len{return Err(FsError::TooLarge);}
        let new_len=len.checked_add(inserted.len()).ok_or(FsError::TooLarge)?;
        if new_len>MAX_FILE_BYTES{return Err(FsError::TooLarge);}
        if inserted.is_empty(){return Ok(0);}
        data.copy_within(offset..len,offset+inserted.len());
        data[offset..offset+inserted.len()].copy_from_slice(inserted);
        self.overwrite_path(path,&data[..new_len])?;
        Ok(inserted.len())
    }
    /// Remove a range and close the gap, atomically replacing a file image.
    pub fn remove_range_path(&mut self,path:&str,offset:usize,
                             count:usize)->Result<usize,FsError>{
        let (len,mut data)=self.r092_snapshot(path)?;
        let end=offset.checked_add(count).ok_or(FsError::TooLarge)?;
        if end>len{return Err(FsError::TooLarge);}
        if count==0{return Ok(0);}
        data.copy_within(end..len,offset);
        self.overwrite_path(path,&data[..len-count])?;
        Ok(count)
    }
    /// Atomic variable-length splice (insert, replace, delete) at one offset.
    pub fn splice_path(&mut self,path:&str,offset:usize,remove:usize,
                       inserted:&[u8])->Result<usize,FsError>{
        let (len,data)=self.r092_snapshot(path)?;
        let end=offset.checked_add(remove).ok_or(FsError::TooLarge)?;
        if end>len{return Err(FsError::TooLarge);}
        let new_len=(len-remove).checked_add(inserted.len()).ok_or(FsError::TooLarge)?;
        if new_len>MAX_FILE_BYTES{return Err(FsError::TooLarge);}
        if remove==0 && inserted.is_empty(){return Ok(0);}
        let mut next=[0u8;MAX_FILE_BYTES];
        next[..offset].copy_from_slice(&data[..offset]);
        next[offset..offset+inserted.len()].copy_from_slice(inserted);
        next[offset+inserted.len()..new_len].copy_from_slice(&data[end..len]);
        self.overwrite_path(path,&next[..new_len])?;
        Ok(inserted.len())
    }
    /// Copy only checksum-verified source bytes into a new independent slot.
    /// Destination existence is checked before any physical writes.
    pub fn copy_path(&mut self,source:&str,target:&str)->Result<(),FsError>{
        let (len,data)=self.r092_snapshot(source)?;
        self.create_path(target,&data[..len])
    }
    /// Atomically exchange two directory names in ONE metadata generation.
    /// The existing payload banks and CRCs do not need rewriting.
    pub fn swap_paths(&mut self,left:&str,right:&str)->Result<(),FsError>{
        let a=canonical_path(left)?;
        let b=canonical_path(right)?;
        if a==b{return Err(FsError::InvalidName);}
        let ai=self.find(a).ok_or(FsError::NotFound)?;
        let bi=self.find(b).ok_or(FsError::NotFound)?;
        let mut next=self.meta;
        next.entries[ai].name=b;
        next.entries[bi].name=a;
        self.publish(next)
    }
    /// Atomically replace an existing destination path by renaming source.
    /// Both metadata edits publish together, without copying data extents.
    pub fn rename_overwrite_path(&mut self,source:&str,target:&str)
        ->Result<(),FsError>{
        let a=canonical_path(source)?;
        let b=canonical_path(target)?;
        if a==b{return Err(FsError::InvalidName);}
        let ai=self.find(a).ok_or(FsError::NotFound)?;
        let bi=self.find(b).ok_or(FsError::NotFound)?;
        let mut next=self.meta;
        next.entries[bi]=Entry::EMPTY;
        next.entries[ai].name=b;
        self.publish(next)
    }
    /// Compare-exchange file contents. A mismatch is a read-only operation.
    /// The mounted generation is checked before publishing any replacement.
    pub fn compare_exchange_path(&mut self,path:&str,
        expected:&[u8],next:&[u8])->Result<bool,FsError>{
        if next.len()>MAX_FILE_BYTES{return Err(FsError::TooLarge);}
        let (len,data)=self.r092_snapshot(path)?;
        if len!=expected.len() || &data[..len]!=expected{return Ok(false);}
        if expected==next{return Ok(true);}
        self.overwrite_path(path,next)?;
        Ok(true)
    }
    /// Conditional removal, checked against the full checksummed old file.
    pub fn delete_if_equal_path(&mut self,path:&str,
        expected:&[u8])->Result<bool,FsError>{
        let (len,data)=self.r092_snapshot(path)?;
        if len!=expected.len() || &data[..len]!=expected{return Ok(false);}
        self.delete_path(path)?;
        Ok(true)
    }
    /// Conditional namespace move without a data rewrite.
    pub fn rename_if_equal_path(&mut self,source:&str,target:&str,
        expected:&[u8])->Result<bool,FsError>{
        let (len,data)=self.r092_snapshot(source)?;
        if len!=expected.len() || &data[..len]!=expected{return Ok(false);}
        self.rename_path(source,target)?;
        Ok(true)
    }
    /// Verified source range into an existing destination. Source and target
    /// may be the same file; a private snapshot gives memmove semantics.
    pub fn copy_range_into_path(&mut self,source:&str,from:usize,
        count:usize,target:&str,to:usize)->Result<usize,FsError>{
        let (src_len,src)=self.r092_snapshot(source)?;
        let src_end=from.checked_add(count).ok_or(FsError::TooLarge)?;
        if src_end>src_len{return Err(FsError::TooLarge);}
        let (dst_len,mut dst)=self.r092_snapshot(target)?;
        let dst_end=to.checked_add(count).ok_or(FsError::TooLarge)?;
        if to>dst_len||dst_end>MAX_FILE_BYTES{return Err(FsError::TooLarge);}
        if count==0{return Ok(0);}
        dst[to..dst_end].copy_from_slice(&src[from..src_end]);
        self.overwrite_path(target,&dst[..dst_len.max(dst_end)])?;
        Ok(count)
    }
    /// Replace all disjoint literal byte occurrences in an existing file.
    /// Build and bound the complete image before a single COW commit.
    pub fn replace_all_path(&mut self,path:&str,needle:&[u8],
        replacement:&[u8])->Result<usize,FsError>{
        if needle.is_empty(){return Err(FsError::InvalidName);}
        let (len,old)=self.r092_snapshot(path)?;
        let mut next=[0u8;MAX_FILE_BYTES];
        let mut at=0usize;
        let mut write=0usize;
        let mut changed=0usize;
        while at<len{
            if needle.len()<=len-at && &old[at..at+needle.len()]==needle{
                let end=write.checked_add(replacement.len()).ok_or(FsError::TooLarge)?;
                if end>MAX_FILE_BYTES{return Err(FsError::TooLarge);}
                next[write..end].copy_from_slice(replacement);
                write=end;at+=needle.len();changed+=1;
            }else{
                if write==MAX_FILE_BYTES{return Err(FsError::TooLarge);}
                next[write]=old[at];at+=1;write+=1;
            }
        }
        if changed==0{return Ok(0);}
        self.overwrite_path(path,&next[..write])?;
        Ok(changed)
    }


    // R095: atomic verified file transformations.
    /// R095-01: prepend_path.
    pub fn prepend_path(&mut self,path:&str,head:&[u8])->Result<usize,FsError>{
 self.insert_range_path(path,0,head)
}

    /// R095-02: drop_prefix_path.
    pub fn drop_prefix_path(&mut self,path:&str,count:usize)->Result<usize,FsError>{
 self.remove_range_path(path,0,count)
}

    /// R095-03: drop_suffix_path.
    pub fn drop_suffix_path(&mut self,path:&str,count:usize)->Result<usize,FsError>{
 let (len,_)=self.r092_snapshot(path)?;
 if count>len{return Err(FsError::TooLarge);}
 self.remove_range_path(path,len-count,count)
}

    /// R095-04: replace_prefix_path.
    pub fn replace_prefix_path(&mut self,path:&str,expected:&[u8],replacement:&[u8])->Result<bool,FsError>{
 let (len,bytes)=self.r092_snapshot(path)?;
 if expected.len()>len||!bytes[..len].starts_with(expected){return Ok(false);}
 if expected==replacement{return Ok(true);}
 self.splice_path(path,0,expected.len(),replacement)?;
 Ok(true)
}

    /// R095-05: replace_suffix_path.
    pub fn replace_suffix_path(&mut self,path:&str,expected:&[u8],replacement:&[u8])->Result<bool,FsError>{
 let (len,bytes)=self.r092_snapshot(path)?;
 if expected.len()>len||!bytes[..len].ends_with(expected){return Ok(false);}
 if expected==replacement{return Ok(true);}
 self.splice_path(path,len-expected.len(),expected.len(),replacement)?;
 Ok(true)
}

    /// R095-06: reverse_range_path.
    pub fn reverse_range_path(&mut self,path:&str,start:usize,count:usize)->Result<usize,FsError>{
 let (len,mut data)=self.r092_snapshot(path)?;
 let end=start.checked_add(count).ok_or(FsError::TooLarge)?;
 if end>len{return Err(FsError::TooLarge);}
 if count<2{return Ok(0);}
 data[start..end].reverse();self.overwrite_path(path,&data[..len])?;Ok(count)
}

    /// R095-07: rotate_left_path.
    pub fn rotate_left_path(&mut self,path:&str,start:usize,count:usize,steps:usize)->Result<usize,FsError>{
 let (len,mut data)=self.r092_snapshot(path)?;
 let end=start.checked_add(count).ok_or(FsError::TooLarge)?;
 if end>len{return Err(FsError::TooLarge);}
 if count<2||steps%count==0{return Ok(0);}
 data[start..end].rotate_left(steps%count);
 self.overwrite_path(path,&data[..len])?;
 Ok(count)
}

    /// R095-08: rotate_right_path.
    pub fn rotate_right_path(&mut self,path:&str,start:usize,count:usize,steps:usize)->Result<usize,FsError>{
 let (len,mut data)=self.r092_snapshot(path)?;
 let end=start.checked_add(count).ok_or(FsError::TooLarge)?;
 if end>len{return Err(FsError::TooLarge);}
 if count<2||steps%count==0{return Ok(0);}
 data[start..end].rotate_right(steps%count);
 self.overwrite_path(path,&data[..len])?;
 Ok(count)
}

    /// R095-09: swap_ranges_path.
    pub fn swap_ranges_path(&mut self,path:&str,left:usize,right:usize,count:usize)->Result<usize,FsError>{
 let (len,mut data)=self.r092_snapshot(path)?;
 let a=left.checked_add(count).ok_or(FsError::TooLarge)?;
 let b=right.checked_add(count).ok_or(FsError::TooLarge)?;
 if a>len||b>len{return Err(FsError::TooLarge);}
 if count==0||left==right{return Ok(0);}
 if left<b && right<a{return Err(FsError::InvalidName);}
 for i in 0..count{data.swap(left+i,right+i);}
 self.overwrite_path(path,&data[..len])?;Ok(count)
}

    /// R095-10: move_range_path.
    pub fn move_range_path(&mut self,path:&str,from:usize,count:usize,to:usize)->Result<usize,FsError>{
 let (len,bytes)=self.r092_snapshot(path)?;
 let end=from.checked_add(count).ok_or(FsError::TooLarge)?;
 if end>len||to>len-count{return Err(FsError::TooLarge);}
 if count==0||from==to{return Ok(0);}
 let mut without=[0u8;MAX_FILE_BYTES];
 without[..from].copy_from_slice(&bytes[..from]);
 without[from..len-count].copy_from_slice(&bytes[end..len]);
 let mut next=[0u8;MAX_FILE_BYTES];
 next[..to].copy_from_slice(&without[..to]);
 next[to..to+count].copy_from_slice(&bytes[from..end]);
 next[to+count..len].copy_from_slice(&without[to..len-count]);
 self.overwrite_path(path,&next[..len])?;Ok(count)
}

    /// R095-11: xor_range_path.
    pub fn xor_range_path(&mut self,path:&str,start:usize,count:usize,mask:u8)->Result<usize,FsError>{
 let (len,mut data)=self.r092_snapshot(path)?;
 let end=start.checked_add(count).ok_or(FsError::TooLarge)?;
 if end>len{return Err(FsError::TooLarge);}
 if count==0||mask==0{return Ok(0);}
 for b in &mut data[start..end]{*b^=mask;}
 self.overwrite_path(path,&data[..len])?;Ok(count)
}

    /// R095-12: uppercase_ascii_range_path.
    pub fn uppercase_ascii_range_path(&mut self,path:&str,start:usize,count:usize)->Result<usize,FsError>{
 let (len,mut data)=self.r092_snapshot(path)?;
 let end=start.checked_add(count).ok_or(FsError::TooLarge)?;
 if end>len{return Err(FsError::TooLarge);}
 let mut changed=0usize;
 for b in &mut data[start..end]{let up=b.to_ascii_uppercase();if *b!=up{*b=up;changed+=1;}}
 if changed>0{self.overwrite_path(path,&data[..len])?;}
 Ok(changed)
}

    // R096: checksum-verified bounded inspection.
    /// R096-01: find_bytes_path.
    pub fn find_bytes_path(&mut self,path:&str,needle:&[u8])->Result<Option<usize>,FsError>{
 if needle.is_empty(){return Err(FsError::InvalidName);}
 let (len,data)=self.r092_snapshot(path)?;
 Ok(data[..len].windows(needle.len()).position(|w|w==needle))
}

    /// R096-02: rfind_bytes_path.
    pub fn rfind_bytes_path(&mut self,path:&str,needle:&[u8])->Result<Option<usize>,FsError>{
 if needle.is_empty(){return Err(FsError::InvalidName);}
 let (len,data)=self.r092_snapshot(path)?;
 Ok(data[..len].windows(needle.len()).rposition(|w|w==needle))
}

    /// R096-03: count_bytes_path.
    pub fn count_bytes_path(&mut self,path:&str,needle:&[u8])->Result<usize,FsError>{
 if needle.is_empty(){return Err(FsError::InvalidName);}
 let (len,data)=self.r092_snapshot(path)?;
 let mut at=0usize;let mut hits=0usize;
 while at+needle.len()<=len{
  if &data[at..at+needle.len()]==needle{hits+=1;at+=needle.len();}
  else{at+=1;}
 }
 Ok(hits)
}

    /// R096-04: has_prefix_path.
    pub fn has_prefix_path(&mut self,path:&str,prefix:&[u8])->Result<bool,FsError>{
 let (len,data)=self.r092_snapshot(path)?;Ok(data[..len].starts_with(prefix))
}

    /// R096-05: has_suffix_path.
    pub fn has_suffix_path(&mut self,path:&str,suffix:&[u8])->Result<bool,FsError>{
 let (len,data)=self.r092_snapshot(path)?;Ok(data[..len].ends_with(suffix))
}

    /// R096-06: read_byte_path.
    pub fn read_byte_path(&mut self,path:&str,offset:usize)->Result<Option<u8>,FsError>{
 if offset>=MAX_FILE_BYTES{return Err(FsError::TooLarge);}
 let (len,data)=self.r092_snapshot(path)?;
 Ok(data[..len].get(offset).copied())
}

    /// R096-07: read_head_path.
    pub fn read_head_path(&mut self,path:&str,out:&mut[u8])->Result<usize,FsError>{
 let (len,data)=self.r092_snapshot(path)?;
 let count=len.min(out.len());out[..count].copy_from_slice(&data[..count]);Ok(count)
}

    /// R096-08: read_tail_path.
    pub fn read_tail_path(&mut self,path:&str,out:&mut[u8])->Result<usize,FsError>{
 let (len,data)=self.r092_snapshot(path)?;
 let count=len.min(out.len());out[..count].copy_from_slice(&data[len-count..len]);Ok(count)
}

    /// R096-09: range_equals_path.
    pub fn range_equals_path(&mut self,path:&str,offset:usize,expected:&[u8])->Result<bool,FsError>{
 let (len,data)=self.r092_snapshot(path)?;
 let end=offset.checked_add(expected.len()).ok_or(FsError::TooLarge)?;
 if end>len{return Ok(false);}
 Ok(&data[offset..end]==expected)
}

    /// R096-10: verified_empty_path.
    pub fn verified_empty_path(&mut self,path:&str)->Result<bool,FsError>{
 let (len,_)=self.r092_snapshot(path)?;Ok(len==0)
}

    // R097: optimistic version-checked COW transactions.
    
 /// Re-read both metadata heads before accepting an optimistic generation.
 fn r097_expect_generation(&mut self,expected:u64)->Result<(),FsError>{
  if expected!=self.generation(){return Err(FsError::InvalidVolume);}
  self.check_heads()
 }

    /// R097-01: append_if_generation_path.
    pub fn append_if_generation_path(&mut self,path:&str,expected:u64,bytes:&[u8])->Result<usize,FsError>{
 self.r097_expect_generation(expected)?;self.append_path(path,bytes)
}

    /// R097-02: insert_if_generation_path.
    pub fn insert_if_generation_path(&mut self,path:&str,expected:u64,offset:usize,bytes:&[u8])->Result<usize,FsError>{
 self.r097_expect_generation(expected)?;self.insert_range_path(path,offset,bytes)
}

    /// R097-03: remove_if_generation_path.
    pub fn remove_if_generation_path(&mut self,path:&str,expected:u64,offset:usize,count:usize)->Result<usize,FsError>{
 self.r097_expect_generation(expected)?;self.remove_range_path(path,offset,count)
}

    /// R097-04: splice_if_generation_path.
    pub fn splice_if_generation_path(&mut self,path:&str,expected:u64,offset:usize,remove:usize,inserted:&[u8])->Result<usize,FsError>{
 self.r097_expect_generation(expected)?;self.splice_path(path,offset,remove,inserted)
}

    /// R097-05: fill_if_generation_path.
    pub fn fill_if_generation_path(&mut self,path:&str,expected:u64,offset:usize,count:usize,value:u8)->Result<usize,FsError>{
 self.r097_expect_generation(expected)?;self.fill_range_path(path,offset,count,value)
}

    /// R097-06: delete_if_generation_path.
    pub fn delete_if_generation_path(&mut self,path:&str,expected:u64)->Result<(),FsError>{
 self.r097_expect_generation(expected)?;self.delete_path(path)
}

    /// R097-07: swap_if_generation_path.
    pub fn swap_if_generation_path(&mut self,left:&str,right:&str,expected:u64)->Result<(),FsError>{
 self.r097_expect_generation(expected)?;self.swap_paths(left,right)
}

    /// R097-08: rename_if_generation_path.
    pub fn rename_if_generation_path(&mut self,from:&str,to:&str,expected:u64)->Result<(),FsError>{
 self.r097_expect_generation(expected)?;self.rename_path(from,to)
}

    /// R097-09: copy_if_generation_path.
    pub fn copy_if_generation_path(&mut self,from:&str,to:&str,expected:u64)->Result<(),FsError>{
 self.r097_expect_generation(expected)?;self.copy_path(from,to)
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
    #[test]fn r107_power_cut_in_first_or_last_data_sector_recovers_prior_file(){
        for failed_bank_sector in [0usize,SECTORS_PER_BANK-1]{
            let mut fs=DurableFs::format_blank(blank()).unwrap();
            let old=[b'A';MAX_FILE_BYTES];
            let new=[b'B';MAX_FILE_BYTES];
            fs.create_path("/important",&old).unwrap();
            let original_generation=fs.generation();
            fs.disk.fail_at=Some(fs.disk.writes+failed_bank_sector);
            assert_eq!(fs.overwrite_path("/important",&new),Err(FsError::Io));
            assert_eq!(fs.generation(),original_generation);
            assert_eq!(fs.read_path("/important",&mut[0;MAX_FILE_BYTES]),
                       Err(FsError::InvalidVolume));
            let mut media=fs.into_device();
            media.fail_at=None;
            let mut restored=DurableFs::mount(media).unwrap();
            assert_eq!(get(&mut restored,"/important"),old);
            assert_eq!(restored.generation(),original_generation);
        }
    }
    #[test]fn r107_power_cut_torn_either_metadata_head_restores_previous_tree(){
        for head_to_torn in [0u64,1u64]{
            let mut fs=DurableFs::format_blank(blank()).unwrap();
            fs.create_path("/important",b"original").unwrap();
            if 1-fs.active!=head_to_torn{
                fs.overwrite_path("/important",b"changed").unwrap();
            }
            assert_eq!(1-fs.active,head_to_torn);
            let expected=get(&mut fs,"/important");
            let old_generation=fs.generation();
            let writes=fs.disk.writes;
            fs.disk.fail_at=Some(writes+SECTORS_PER_BANK);
            assert_eq!(fs.overwrite_path("/important",b"candidate"),
                       Err(FsError::Io));
            assert_eq!(fs.generation(),old_generation);
            let mut media=fs.into_device();
            media.fail_at=None;
            let mut recovered=DurableFs::mount(media).unwrap();
            assert_eq!(get(&mut recovered,"/important"),expected);
            assert_eq!(recovered.generation(),old_generation);
        }
    }
    #[test]fn r107_metadata_read_error_requires_fresh_head_verification_before_retry(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/important",b"old").unwrap();
        let writes=fs.disk.writes;
        fs.disk.fail_read_lba=Some(0);
        assert_eq!(fs.read_path("/important",&mut[0;16]),Err(FsError::Io));
        fs.disk.fail_read_lba=None;
        // Read errors never authorize stale metadata. Changed heads still
        // retire the mount; unchanged verified heads permit retry.
        assert_eq!(get(&mut fs,"/important"),b"old");
        assert_eq!(fs.disk.writes,writes);
        let mut new_session=DurableFs::mount(fs.into_device()).unwrap();
        assert_eq!(get(&mut new_session,"/important"),b"old");
    }
    #[test]fn r107_failed_data_read_retires_session_without_exposing_output(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/important",b"old").unwrap();
        let data_lba=block_start(0,fs.meta.entries[0].bank);
        fs.disk.fail_read_lba=Some(data_lba);
        let mut out=[0x5a;16];
        assert_eq!(fs.read_path("/important",&mut out),Err(FsError::Io));
        assert_eq!(out,[0x5a;16]);
        fs.disk.fail_read_lba=None;
        assert_eq!(fs.read_path("/important",&mut out),Ok(3));
        assert_eq!(&out[..3],b"old");
        let mut new_session=DurableFs::mount(fs.into_device()).unwrap();
        assert_eq!(get(&mut new_session,"/important"),b"old");
    }
    #[test]fn r101_stale_mount_fails_closed_on_reads_and_stays_retired(){
        let shared=SharedDisk(std::rc::Rc::new(std::cell::RefCell::new(blank())));
        let mut owner=DurableFs::format_blank(shared.clone()).unwrap();
        owner.create_path("/note",b"old").unwrap();
        let mut stale=DurableFs::mount(shared.clone()).unwrap();
        owner.overwrite_path("/note",b"updated").unwrap();
        let mut out=[0xa5;MAX_FILE_BYTES];
        assert_eq!(stale.read_path("/note",&mut out),Err(FsError::InvalidVolume));
        assert_eq!(out,[0xa5;MAX_FILE_BYTES]);
        assert_eq!(stale.read_path("/note",&mut out),Err(FsError::InvalidVolume));
        assert_eq!(stale.overwrite_path("/note",b"bad"),Err(FsError::InvalidVolume));
        let mut fresh=DurableFs::mount(shared).unwrap();
        let mut recovered=[0u8;MAX_FILE_BYTES];
        let count=fresh.read_path("/note",&mut recovered).unwrap();
        assert_eq!(&recovered[..count],b"updated");
    }
    #[test]fn r101_read_current_mount_still_verifies_and_returns_payload(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/note",b"hello").unwrap();
        let mut out=[0xa5;10];
        assert_eq!(fs.read_path("/note",&mut out),Ok(5));
        assert_eq!(&out[..5],b"hello");
        assert_eq!(&out[5..],&[0xa5;5]);
    }
    #[test]fn r105_corrupt_live_bank_retires_mount_and_never_leaks_bytes(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/note",b"stable").unwrap();
        let lba=block_start(0,fs.meta.entries[0].bank) as usize;
        fs.disk.sectors[lba][0]^=0xff;
        let writes=fs.disk.writes;
        let mut out=[0x55u8;16];
        assert_eq!(fs.read_path("/note",&mut out),Err(FsError::InvalidVolume));
        assert_eq!(out,[0x55;16]);
        assert_eq!(fs.create_path("/next",b"deny"),Err(FsError::InvalidVolume));
        assert_eq!(fs.read_path("/note",&mut out),Err(FsError::InvalidVolume));
        assert_eq!(fs.disk.writes,writes);
    }
    #[test]fn r105_failed_repair_write_retires_session_until_explicit_remount(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/note",b"good").unwrap();
        let inactive=1-fs.active as usize;
        let mut disk=fs.into_device();
        disk.sectors[inactive][0]^=1;
        let mut fs=DurableFs::mount(disk).unwrap();
        let writes=fs.disk.writes;
        fs.disk.fail_at=Some(writes);
        assert_eq!(fs.repair_redundancy(),Err(FsError::Io));
        fs.disk.fail_at=None;
        assert_eq!(fs.repair_redundancy(),Err(FsError::InvalidVolume));
        assert_eq!(fs.create_path("/next",b"deny"),Err(FsError::InvalidVolume));
        let mut fresh=DurableFs::mount(fs.into_device()).unwrap();
        assert_eq!(fresh.repair_redundancy(),Ok(true));
        let mut out=[0u8;16];
        assert_eq!(fresh.read_path("/note",&mut out),Ok(4));
        assert_eq!(&out[..4],b"good");
    }
    #[test]fn mount_and_health_reject_headers_changed_during_payload_admission(){
        struct ChangingHeads{disk:SharedDisk,visits:usize,trigger:usize,fail:bool}
        impl BlockDevice for ChangingHeads{
            fn sector_count(&self)->u64{self.disk.sector_count()}
            fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
             ->Result<(),BlockError>{
                self.disk.read_sector(lba,out)?;
                if lba==1{
                    self.visits+=1;
                    if self.visits==self.trigger{
                        if self.fail{self.disk.0.borrow_mut().fail_read_lba=Some(0);}
                        else{self.disk.0.borrow_mut().sectors[0][0]^=1;}
                    }
                }
                Ok(())
            }
            fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])
             ->Result<(),BlockError>{panic!("admission attempted a write")}
        }
        for (operation,trigger) in [(0,1),(1,1),(2,2)]{
            for fail in [false,true]{
                let mut initial=DurableFs::format_blank(blank()).unwrap();
                initial.create_path("/note",b"committed").unwrap();
                let disk=SharedDisk(std::rc::Rc::new(std::cell::RefCell::new(initial.into_device())));
                let writes=disk.0.borrow().writes;
                let mut device=ChangingHeads{disk:disk.clone(),visits:0,trigger,fail};
                let result=match operation{
                    0=>DurableFs::mount(device).map(|_|()),
                    1=>DurableFs::inspect_health(&mut device).map(|_|()),
                    _=>DurableFs::mount_healthy(device).map(|_|()),
                };
                assert_eq!(result,Err(if fail{FsError::Io}else{FsError::InvalidVolume}));
                assert_eq!(disk.0.borrow().writes,writes);
                disk.0.borrow_mut().fail_read_lba=None;
                let mut recovery=DurableFs::mount(disk).unwrap();
                let mut out=[0u8;16];
                assert_eq!(recovery.read_path("/note",&mut out),Ok(9));
                assert_eq!(&out[..9],b"committed");
            }
        }
    }
    #[test]fn permission_metadata_survives_overwrite_and_rejects_invalid_modes_with_valid_crc(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        let owner=Credentials::kernel(1000,42);
        fs.create_owned_path("/owned",b"secret",owner,0o640).unwrap();
        assert_eq!(fs.meta.format,PERMISSION_VERSION);
        fs.overwrite_path("/owned",b"updated").unwrap();
        let mut fs=DurableFs::mount(fs.into_device()).unwrap();
        let permission=fs.permissions_path("/owned").unwrap();
        assert_eq!((permission.owner,permission.group,permission.mode),(1000,42,0o640));
        assert_eq!(fs.chmod_path("/owned",Credentials::kernel(2000,42),0o777),Err(FsError::InvalidName));
        assert_eq!(fs.chmod_path("/owned",owner,0o1000),Err(FsError::InvalidName));
        let mut disk=fs.into_device();
        for header in 0..2{
            let mut metadata=decode(&disk.sectors[header],disk.sector_count()).unwrap();
            metadata.entries[0].permissions.mode=0o1000;
            disk.sectors[header]=encode(&metadata,disk.sector_count());
        }
        assert!(matches!(DurableFs::mount(disk),Err(FsError::InvalidVolume)));
    }
    #[test]fn mounted_health_retires_changed_inactive_head_without_repair(){
        let disk=SharedDisk(std::rc::Rc::new(std::cell::RefCell::new(blank())));
        let mut fs=DurableFs::format_blank(disk.clone()).unwrap();
        fs.create_path("/note",b"committed").unwrap();
        let inactive=(1-fs.active) as usize;
        let original=disk.0.borrow().sectors[inactive];
        disk.0.borrow_mut().sectors[inactive][0]^=1;
        let writes=disk.0.borrow().writes;
        assert_eq!(fs.mounted_health(),Err(FsError::InvalidVolume));
        disk.0.borrow_mut().sectors[inactive]=original;
        assert_eq!(fs.mounted_health(),Err(FsError::InvalidVolume));
        assert_eq!(fs.create_path("/next",b"deny"),Err(FsError::InvalidVolume));
        assert_eq!(disk.0.borrow().writes,writes);
        let mut fresh=DurableFs::mount_healthy(disk).unwrap();
        let mut out=[0u8;16];
        assert_eq!(fresh.read_path("/note",&mut out),Ok(9));
        assert_eq!(&out[..9],b"committed");
    }
    #[test]fn checkpoint_publication_rejects_corrupt_unchanged_payload_before_writing(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/note",b"committed").unwrap();
        let head=fs.heads;
        let generation=fs.generation();
        let lba=block_start(0,fs.meta.entries[0].bank) as usize;
        fs.disk.sectors[lba][0]^=1;
        let writes=fs.disk.writes;
        assert_eq!(fs.rename_path("/note","/renamed"),Err(FsError::InvalidVolume));
        assert_eq!(fs.generation(),generation);
        assert_eq!(fs.disk.writes,writes);
        assert_eq!([fs.disk.sectors[0],fs.disk.sectors[1]],head);
        fs.disk.sectors[lba][0]^=1; // even restored bytes cannot revive the session
        assert_eq!(fs.rename_path("/note","/renamed"),Err(FsError::InvalidVolume));
        let mut fresh=DurableFs::mount(fs.into_device()).unwrap();
        fresh.rename_path("/note","/renamed").unwrap();
        assert_eq!(get(&mut fresh,"/renamed"),b"committed");
    }
    #[test]fn checkpoint_publication_refuses_payload_transport_failure_without_mutation(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/note",b"committed").unwrap();
        let lba=block_start(0,fs.meta.entries[0].bank);
        fs.disk.fail_read_lba=Some(lba);
        let writes=fs.disk.writes;
        assert_eq!(fs.rename_path("/note","/renamed"),Err(FsError::Io));
        assert_eq!(fs.disk.writes,writes);
        fs.disk.fail_read_lba=None;
        assert_eq!(fs.rename_path("/note","/renamed"),Err(FsError::InvalidVolume));
        let mut fresh=DurableFs::mount(fs.into_device()).unwrap();
        assert_eq!(get(&mut fresh,"/note"),b"committed");
    }
    #[test]fn publication_and_final_barrier_failures_never_acknowledge_bad_checkpoints(){
        struct FaultDisk{disk:Disk,target:Option<u64>,stage:u8,syncs:u8}
        impl BlockDevice for FaultDisk{
            fn sector_count(&self)->u64{self.disk.sector_count()}
            fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
             ->Result<(),BlockError>{self.disk.read_sector(lba,out)}
            fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
             ->Result<(),BlockError>{
                self.disk.write_sector(lba,data)?;
                if lba<2&&self.stage==0{
                    if let Some(target)=self.target{
                        self.disk.sectors[target as usize][0]^=1;
                    }
                }
                Ok(())
            }
            fn sync(&mut self)->Result<(),BlockError>{
                self.syncs+=1;
                if self.syncs==2{
                    if let Some(target)=self.target{
                        match self.stage{
                            1=>self.disk.sectors[target as usize][0]^=1,
                            2=>self.disk.fail_read_lba=Some(target),
                            3=>return Err(BlockError::Io),
                            _=>{},
                        }
                    }
                }
                Ok(())
            }
        }
        for stage in 0..4{
            let device=FaultDisk{disk:blank(),target:None,stage,syncs:0};
            let mut fs=DurableFs::format_blank(device).unwrap();
            fs.create_path("/note",b"previous").unwrap();
            let bank=fs.available_bank(0).unwrap();
            fs.disk.target=Some(block_start(0,bank));
            let expected=if stage<2{FsError::InvalidVolume}else{FsError::Io};
            assert_eq!(fs.overwrite_durable_path("/note",b"replacement"),Err(expected));
            let writes=fs.disk.disk.writes;
            let mut out=[0xa5;16];
            assert_eq!(fs.read_path("/note",&mut out),Err(FsError::InvalidVolume));
            assert_eq!(out,[0xa5;16]);
            assert_eq!(fs.overwrite_path("/note",b"retry"),Err(FsError::InvalidVolume));
            assert_eq!(fs.disk.disk.writes,writes);
            let mut disk=fs.into_device().disk;
            disk.fail_read_lba=None;
            let mut recovery=DurableFs::mount(disk).unwrap();
            let recovered=get(&mut recovery,"/note");
            if stage<2{assert_eq!(recovered,b"previous");}
            else{assert_eq!(recovered,b"replacement");}
        }
    }
    #[test]fn r103_strict_durable_overwrite_rejects_controller_lost_data_at_flush(){
        struct LieFlush{
          disk:Disk,armed:bool,flips:Option<u64>,sync_count:u32,
        }
        impl BlockDevice for LieFlush{
          fn sector_count(&self)->u64{self.disk.sector_count()}
          fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
           ->Result<(),BlockError>{self.disk.read_sector(lba,out)}
          fn write_sector(&mut self,lba:u64,payload:&[u8;SECTOR_BYTES])
           ->Result<(),BlockError>{
             self.disk.write_sector(lba,payload)?;
             if self.armed&&lba>=2{self.flips=Some(lba);}
             Ok(())
          }
          fn sync(&mut self)->Result<(),BlockError>{
             self.sync_count+=1;
             if self.armed {
               if let Some(lba)=self.flips.take(){
                 // Storage controller lied: data visible on write readback
                 // was lost at flush, but the flush still returns success.
                 self.disk.sectors[lba as usize][0]^=0x5a;
               }
               self.armed=false;
             }
             Ok(())
          }
        }
        let device=LieFlush{disk:blank(),armed:false,flips:None,sync_count:0};
        let mut fs=DurableFs::format_blank(device).unwrap();
        fs.create_path("/note",b"stable").unwrap();
        let gen=fs.generation();
        fs.disk.armed=true;
        assert_eq!(fs.overwrite_durable_path("/note",b"unstable"),
                   Err(FsError::InvalidVolume));
        assert_eq!(fs.generation(),gen);
        assert_eq!(fs.disk.sync_count,1); // never publish invalid checkpoint
        assert_eq!(fs.create_path("/other",b"x"),Err(FsError::InvalidVolume));
        let mut recovery=DurableFs::mount(fs.into_device()).unwrap();
        let mut out=[0u8;16];
        assert_eq!(recovery.read_path("/note",&mut out),Ok(6));
        assert_eq!(&out[..6],b"stable");
    }
    #[test]fn r102_strict_durable_create_and_overwrite_require_two_ordered_barriers(){
        use std::cell::Cell;
        use std::rc::Rc;
        struct FlushDisk{disk:Disk,barriers:Rc<Cell<u32>>}
        impl BlockDevice for FlushDisk{
            fn sector_count(&self)->u64{self.disk.sector_count()}
            fn read_sector(&mut self,l:u64,out:&mut[u8;SECTOR_BYTES])
             ->Result<(),BlockError>{self.disk.read_sector(l,out)}
            fn write_sector(&mut self,l:u64,buf:&[u8;SECTOR_BYTES])
             ->Result<(),BlockError>{self.disk.write_sector(l,buf)}
            fn sync(&mut self)->Result<(),BlockError>{
                self.barriers.set(self.barriers.get()+1);Ok(())
            }
        }
        let flushes=Rc::new(Cell::new(0));
        let d=FlushDisk{disk:blank(),barriers:flushes.clone()};
        let mut fs=DurableFs::format_blank(d).unwrap();
        fs.create_durable_path("/note",b"old").unwrap();
        assert_eq!(flushes.get(),2);
        fs.overwrite_durable_path("/note",b"new").unwrap();
        assert_eq!(flushes.get(),4);
        let mut fs=DurableFs::mount(fs.into_device()).unwrap();
        let mut out=[0u8;8];
        assert_eq!(fs.read_path("/note",&mut out),Ok(3));
        assert_eq!(&out[..3],b"new");
    }
    #[test]fn r102_durable_create_without_barrier_is_never_acknowledged(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        assert_eq!(fs.create_durable_path("/note",b"new"),Err(FsError::Io));
        assert_eq!(fs.create_path("/other",b"deny"),Err(FsError::InvalidVolume));
        let mut mounted=DurableFs::mount(fs.into_device()).unwrap();
        assert_eq!(mounted.read_path("/note",&mut[0;8]),Err(FsError::NotFound));
    }
    #[test]fn r102_explicit_readonly_recovery_cannot_write_sectors(){
        let mut formatted=DurableFs::format_blank(blank()).unwrap();
        formatted.create_path("/note",b"original").unwrap();
        let mut disk=formatted.into_device();
        let before=disk.writes;
        {
            let mut recovery=mount_recovery_readonly(&mut disk).unwrap();
            let mut buf=[0u8;20];
            assert_eq!(recovery.read_path("/note",&mut buf),Ok(8));
            assert_eq!(&buf[..8],b"original");
            assert_eq!(recovery.overwrite_path("/note",b"forbidden"),
                       Err(FsError::Io));
        }
        assert_eq!(disk.writes,before);
        let mut remounted=DurableFs::mount(disk).unwrap();
        assert_eq!(get(&mut remounted,"/note"),b"original");
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
        // A failed/unsupported flush is an uncertain durability boundary.
        // The session must be retired, even when the old payload reads fine.
        assert_eq!(fs.read_path("/data",&mut[0;MAX_FILE_BYTES]),
                   Err(FsError::InvalidVolume));
        // An explicit new mount verifies the checkpoint before readback.
        let mut remounted=DurableFs::mount(fs.into_device()).unwrap();
        assert_eq!(get(&mut remounted,"/data"),b"persist");
    }
    #[test]fn storage_retirement_sync_verification_failures_revoke_every_io_boundary(){
        struct FaultDisk{disk:Disk,reads:usize,fail_at:Option<usize>,flushes:usize}
        impl BlockDevice for FaultDisk{
            fn sector_count(&self)->u64{self.disk.sector_count()}
            fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
             ->Result<(),BlockError>{
                let index=self.reads;self.reads+=1;
                if self.fail_at==Some(index){return Err(BlockError::Io);}
                self.disk.read_sector(lba,out)
            }
            fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])
             ->Result<(),BlockError>{self.disk.write_sector(lba,bytes)}
            fn sync(&mut self)->Result<(),BlockError>{self.flushes+=1;Ok(())}
        }
        let mut initial=DurableFs::format_blank(blank()).unwrap();
        initial.create_path("/data",&[0x63;700]).unwrap();
        let image=initial.into_device();
        let make=||DurableFs::mount(FaultDisk{disk:image.clone(),reads:0,
            fail_at:None,flushes:0}).unwrap();
        let mut healthy=make();let start=healthy.disk.reads;
        assert_eq!(healthy.sync(),Ok(()));
        let boundaries=healthy.disk.reads-start;assert!(boundaries>4);
        for offset in 0..boundaries{
            let mut fs=make();let writes=fs.disk.disk.writes;
            fs.disk.fail_at=Some(fs.disk.reads+offset);
            assert_eq!(fs.sync(),Err(FsError::Io),"read boundary {offset}");
            fs.disk.fail_at=None;
            let reads=fs.disk.reads;let flushes=fs.disk.flushes;
            let mut out=[0xa5;MAX_FILE_BYTES];
            assert_eq!(fs.read_path("/data",&mut out),Err(FsError::InvalidVolume));
            assert_eq!(fs.overwrite_path("/data",b"must not write"),Err(FsError::InvalidVolume));
            assert_eq!(fs.sync(),Err(FsError::InvalidVolume));
            assert_eq!(out,[0xa5;MAX_FILE_BYTES]);
            assert_eq!((fs.disk.reads,fs.disk.disk.writes,fs.disk.flushes),
                (reads,writes,flushes));
            let mut remounted=DurableFs::mount(fs.into_device()).unwrap();
            assert_eq!(remounted.read_path("/data",&mut out),Ok(700));
            assert_eq!(&out[..700],&[0x63;700]);
            assert_eq!(remounted.sync(),Ok(()));
        }
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
    #[test]fn m1_r089_format_stops_after_geometry_changes_on_first_write(){
        use std::cell::Cell;
        use std::rc::Rc;
        struct ResizeAfterWrite{
            disk:Disk,
            geometry:Rc<Cell<u64>>,
            writes:Rc<Cell<usize>>,
        }
        impl BlockDevice for ResizeAfterWrite{
            fn sector_count(&self)->u64{self.geometry.get()}
            fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
                ->Result<(),BlockError>{self.disk.read_sector(lba,out)}
            fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])
                ->Result<(),BlockError>{
                self.disk.write_sector(lba,bytes)?;
                self.writes.set(self.writes.get()+1);
                self.geometry.set(MIN_SECTORS+1);
                Ok(())
            }
        }
        let geometry=Rc::new(Cell::new(MIN_SECTORS));
        let writes=Rc::new(Cell::new(0));
        let disk=ResizeAfterWrite{
            disk:blank(),geometry:geometry.clone(),writes:writes.clone()
        };
        assert!(matches!(DurableFs::format_blank(disk),
            Err(FsError::InvalidVolume)));
        assert_eq!(writes.get(),1);
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

    /// R090 F01: format blank creates two identical superblocks
    #[test]fn r090_f01_format_blank_creates_two_identical_superblocks(){
        let fs=DurableFs::format_blank(blank()).unwrap();let disk=fs.into_device();assert_eq!(disk.sectors[0],disk.sectors[1]);assert_eq!(&disk.sectors[0][..8],MAGIC);
    }
    /// R090 F02: mount rejects unformatted blank medium
    #[test]fn r090_f02_mount_rejects_unformatted_blank_medium(){
        assert!(matches!(DurableFs::mount(blank()),Err(FsError::InvalidVolume)));
    }
    /// R090 F03: format rejects data in last sector
    #[test]fn r090_f03_format_rejects_data_in_last_sector(){
        let mut disk=blank();disk.sectors[MIN_SECTORS as usize-1][511]=1;assert!(matches!(DurableFs::format_blank(disk),Err(FsError::InvalidVolume)));
    }
    /// R090 F04: create and remount preserves content
    #[test]fn r090_f04_create_and_remount_preserves_content(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/note",b"hello").unwrap();let mut fs=DurableFs::mount(fs.into_device()).unwrap();assert_eq!(get(&mut fs,"/note"),b"hello");
    }
    /// R090 F05: create empty file survives remount
    #[test]fn r090_f05_create_empty_file_survives_remount(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/empty",b"").unwrap();let mut fs=DurableFs::mount(fs.into_device()).unwrap();assert_eq!(get(&mut fs,"/empty"),b"");
    }
    /// R090 F06: maximum payload survives remount
    #[test]fn r090_f06_maximum_payload_survives_remount(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/max",&[0x5a;MAX_FILE_BYTES]).unwrap();let mut fs=DurableFs::mount(fs.into_device()).unwrap();assert_eq!(get(&mut fs,"/max"),[0x5a;MAX_FILE_BYTES]);
    }
    /// R090 F07: oversize payload is not published
    #[test]fn r090_f07_oversize_payload_is_not_published(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();assert_eq!(fs.create_path("/too-large",&[1;MAX_FILE_BYTES+1]),Err(FsError::TooLarge));assert_eq!(fs.read_path("/too-large",&mut[0;MAX_FILE_BYTES]),Err(FsError::NotFound));
    }
    /// R090 F08: duplicate create preserves original file
    #[test]fn r090_f08_duplicate_create_preserves_original_file(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/same",b"first").unwrap();assert_eq!(fs.create_path("/same",b"second"),Err(FsError::Exists));assert_eq!(get(&mut fs,"/same"),b"first");
    }
    /// R090 F09: overwrite existing file survives remount
    #[test]fn r090_f09_overwrite_existing_file_survives_remount(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/same",b"first").unwrap();fs.overwrite_path("/same",b"second").unwrap();let mut fs=DurableFs::mount(fs.into_device()).unwrap();assert_eq!(get(&mut fs,"/same"),b"second");
    }
    /// R090 F10: overwrite missing file fails without creating
    #[test]fn r090_f10_overwrite_missing_file_fails_without_creating(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();assert_eq!(fs.overwrite_path("/missing",b"new"),Err(FsError::NotFound));assert_eq!(fs.read_path("/missing",&mut[0;MAX_FILE_BYTES]),Err(FsError::NotFound));
    }
    /// R090 F11: read missing file is not found
    #[test]fn r090_f11_read_missing_file_is_not_found(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();let mut out=[0xa5;32];assert_eq!(fs.read_path("/missing",&mut out),Err(FsError::NotFound));assert_eq!(out,[0xa5;32]);
    }
    /// R090 F12: short read buffer is not partially published
    #[test]fn r090_f12_short_read_buffer_is_not_partially_published(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/note",b"abc").unwrap();let mut out=[0xa5;2];assert_eq!(fs.read_path("/note",&mut out),Err(FsError::TooLarge));assert_eq!(out,[0xa5;2]);
    }
    /// R090 F13: deletion survives fresh mount
    #[test]fn r090_f13_deletion_survives_fresh_mount(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/note",b"abc").unwrap();fs.delete_path("/note").unwrap();let mut fs=DurableFs::mount(fs.into_device()).unwrap();assert_eq!(fs.read_path("/note",&mut[0;10]),Err(FsError::NotFound));
    }
    /// R090 F14: delete missing does not create metadata
    #[test]fn r090_f14_delete_missing_does_not_create_metadata(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();let before=fs.generation();assert_eq!(fs.delete_path("/absent"),Err(FsError::NotFound));assert_eq!(fs.generation(),before);
    }
    /// R090 F15: rename file survives fresh mount
    #[test]fn r090_f15_rename_file_survives_fresh_mount(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/from",b"kept").unwrap();fs.rename_path("/from","/to").unwrap();let mut fs=DurableFs::mount(fs.into_device()).unwrap();assert_eq!(get(&mut fs,"/to"),b"kept");assert_eq!(fs.read_path("/from",&mut[0;16]),Err(FsError::NotFound));
    }
    /// R090 F16: create ignores no invalid parent escape
    #[test]fn r090_f16_create_ignores_no_invalid_parent_escape(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();assert_eq!(fs.create_path("/../unsafe",b"x"),Err(FsError::InvalidName));assert_eq!(fs.read_path("/unsafe",&mut[0;16]),Err(FsError::NotFound));
    }
    /// R090 F17: create two files preserves both after overwrite
    #[test]fn r090_f17_create_two_files_preserves_both_after_overwrite(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/one",b"first").unwrap();fs.create_path("/two",b"second").unwrap();fs.overwrite_path("/one",b"updated").unwrap();let mut fs=DurableFs::mount(fs.into_device()).unwrap();assert_eq!(get(&mut fs,"/one"),b"updated");assert_eq!(get(&mut fs,"/two"),b"second");
    }
    /// R090 F18: two corrupt superblocks refuse mount
    #[test]fn r090_f18_two_corrupt_superblocks_refuse_mount(){
        let fs=DurableFs::format_blank(blank()).unwrap();let mut disk=fs.into_device();disk.sectors[0][24]^=1;disk.sectors[1][24]^=1;assert!(matches!(DurableFs::mount(disk),Err(FsError::InvalidVolume)));
    }
    /// R090 F19: corrupt new payload cannot publish file on mount
    #[test]fn r090_f19_corrupt_new_payload_cannot_publish_file_on_mount(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/new",b"content").unwrap();let bank=fs.meta.entries[0].bank;let mut disk=fs.into_device();disk.sectors[block_start(0,bank) as usize][0]^=1;let mut fs=DurableFs::mount(disk).unwrap();assert_eq!(fs.read_path("/new",&mut[0;MAX_FILE_BYTES]),Err(FsError::NotFound));
    }
    /// R090 F20: failing first overwrite preserves prior checkpoint
    #[test]fn r090_f20_failing_first_overwrite_preserves_prior_checkpoint(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/note",b"old").unwrap();let mut disk=fs.into_device();disk.fail_at=Some(disk.writes);let mut fs=DurableFs::mount(disk).unwrap();assert_eq!(fs.overwrite_path("/note",b"new"),Err(FsError::Io));let mut disk=fs.into_device();disk.fail_at=None;let mut fs=DurableFs::mount(disk).unwrap();assert_eq!(get(&mut fs,"/note"),b"old");
    }

    #[test]fn r091_recovery_format_valid_disk_still_mounts(){
        let disk=DurableFs::format_blank(blank()).unwrap().into_device();
        assert!(DurableFs::mount(disk).is_ok());
    }
    #[test]fn r091_recovery_mount_rejects_media_removed_between_headers(){
        let original=DurableFs::format_blank(blank()).unwrap().into_device();
        let capacity=std::rc::Rc::new(std::cell::Cell::new(MIN_SECTORS));
        let change=ChangingGeometryDisk{
            disk:PowerCutDisk{disk:original,writes_left:0},
            sectors:capacity,
            switch_on_read:Some((0,MIN_SECTORS+1)),
        };
        assert!(matches!(DurableFs::mount(change),Err(FsError::InvalidVolume)));
    }
    #[test]fn r091_recovery_health_rejects_media_removed_between_headers(){
        let original=DurableFs::format_blank(blank()).unwrap().into_device();
        let mut change=ChangingGeometryDisk{
            disk:PowerCutDisk{disk:original,writes_left:0},
            sectors:std::rc::Rc::new(std::cell::Cell::new(MIN_SECTORS)),
            switch_on_read:Some((0,MIN_SECTORS+1)),
        };
        assert_eq!(DurableFs::inspect_health(&mut change),Err(FsError::InvalidVolume));
    }

    /// R092 filesystem operation regression 01
    #[test] fn r092_fs_01_verified_metadata_read(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abc").unwrap();assert_eq!(fs.verified_len_path("/x"),Ok(3));assert_eq!(fs.verified_crc_path("/x"),Ok(crc(b"abc")));assert_eq!(fs.equals_path("/x",b"abc"),Ok(true));assert_eq!(fs.equals_path("/x",b"ab"),Ok(false));
    }
    /// R092 filesystem operation regression 02
    #[test] fn r092_fs_02_append_survives_remount(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"one").unwrap();assert_eq!(fs.append_path("/x",b"two"),Ok(3));let mut fs=DurableFs::mount(fs.into_device()).unwrap();assert_eq!(get(&mut fs,"/x"),b"onetwo");
    }
    /// R092 filesystem operation regression 03
    #[test] fn r092_fs_03_append_at_maximum_rolls_back(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",&[7;MAX_FILE_BYTES]).unwrap();let generation=fs.generation();assert_eq!(fs.append_path("/x",b"x"),Err(FsError::TooLarge));assert_eq!(fs.generation(),generation);assert_eq!(get(&mut fs,"/x"),[7;MAX_FILE_BYTES]);
    }
    /// R092 filesystem operation regression 04
    #[test] fn r092_fs_04_resize_truncate_and_zero_extend(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abc").unwrap();fs.resize_path("/x",1).unwrap();assert_eq!(get(&mut fs,"/x"),b"a");fs.resize_path("/x",4).unwrap();let mut fs=DurableFs::mount(fs.into_device()).unwrap();assert_eq!(get(&mut fs,"/x"),b"a\0\0\0");
    }
    /// R092 filesystem operation regression 05
    #[test] fn r092_fs_05_fill_bounded_range(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcde").unwrap();assert_eq!(fs.fill_range_path("/x",1,3,b'x'),Ok(3));assert_eq!(get(&mut fs,"/x"),b"axxxe");
    }
    /// R092 filesystem operation regression 06
    #[test] fn r092_fs_06_fill_invalid_offset_has_no_writes(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abc").unwrap();let old=fs.disk.writes;assert_eq!(fs.fill_range_path("/x",2,2,0),Err(FsError::TooLarge));assert_eq!(fs.disk.writes,old);
    }
    /// R092 filesystem operation regression 07
    #[test] fn r092_fs_07_insert_middle_with_shift(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcd").unwrap();assert_eq!(fs.insert_range_path("/x",2,b"XY"),Ok(2));assert_eq!(get(&mut fs,"/x"),b"abXYcd");
    }
    /// R092 filesystem operation regression 08
    #[test] fn r092_fs_08_insert_overflow_no_mutation(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",&[1;MAX_FILE_BYTES]).unwrap();let old=fs.generation();assert_eq!(fs.insert_range_path("/x",0,b"x"),Err(FsError::TooLarge));assert_eq!(fs.generation(),old);
    }
    /// R092 filesystem operation regression 09
    #[test] fn r092_fs_09_remove_middle_compacts_file(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcdef").unwrap();assert_eq!(fs.remove_range_path("/x",1,3),Ok(3));assert_eq!(get(&mut fs,"/x"),b"aef");
    }
    /// R092 filesystem operation regression 10
    #[test] fn r092_fs_10_remove_all_yields_empty(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"hi").unwrap();assert_eq!(fs.remove_range_path("/x",0,2),Ok(2));assert_eq!(get(&mut fs,"/x"),b"");
    }
    /// R092 filesystem operation regression 11
    #[test] fn r092_fs_11_splice_growth_and_shrink(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"123456").unwrap();assert_eq!(fs.splice_path("/x",2,2,b"LONG"),Ok(4));assert_eq!(get(&mut fs,"/x"),b"12LONG56");fs.splice_path("/x",2,4,b"y").unwrap();assert_eq!(get(&mut fs,"/x"),b"12y56");
    }
    /// R092 filesystem operation regression 12
    #[test] fn r092_fs_12_clone_independent_file(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"first").unwrap();fs.copy_path("/x","/y").unwrap();fs.overwrite_path("/x",b"changed").unwrap();let mut fs=DurableFs::mount(fs.into_device()).unwrap();assert_eq!(get(&mut fs,"/x"),b"changed");assert_eq!(get(&mut fs,"/y"),b"first");
    }
    /// R092 filesystem operation regression 13
    #[test] fn r092_fs_13_swap_names_single_generation(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/a",b"AA").unwrap();fs.create_path("/b",b"BB").unwrap();let gen=fs.generation();fs.swap_paths("/a","/b").unwrap();assert_eq!(fs.generation(),gen+1);let mut fs=DurableFs::mount(fs.into_device()).unwrap();assert_eq!(get(&mut fs,"/a"),b"BB");assert_eq!(get(&mut fs,"/b"),b"AA");
    }
    /// R092 filesystem operation regression 14
    #[test] fn r092_fs_14_replace_destination_atomic_rename(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/a",b"new").unwrap();fs.create_path("/b",b"old").unwrap();let gen=fs.generation();fs.rename_overwrite_path("/a","/b").unwrap();assert_eq!(fs.generation(),gen+1);let mut fs=DurableFs::mount(fs.into_device()).unwrap();assert_eq!(fs.read_path("/a",&mut[0;16]),Err(FsError::NotFound));assert_eq!(get(&mut fs,"/b"),b"new");
    }
    /// R092 filesystem operation regression 15
    #[test] fn r092_fs_15_compare_exchange_mismatch_no_mutation(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"old").unwrap();let gen=fs.generation();let writes=fs.disk.writes;assert_eq!(fs.compare_exchange_path("/x",b"wrong",b"new"),Ok(false));assert_eq!(fs.generation(),gen);assert_eq!(fs.disk.writes,writes);
    }
    /// R092 filesystem operation regression 16
    #[test] fn r092_fs_16_compare_exchange_commit_survives_reboot(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"old").unwrap();assert_eq!(fs.compare_exchange_path("/x",b"old",b"new"),Ok(true));let mut fs=DurableFs::mount(fs.into_device()).unwrap();assert_eq!(get(&mut fs,"/x"),b"new");
    }
    /// R092 filesystem operation regression 17
    #[test] fn r092_fs_17_delete_if_equal_guards_data(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"saved").unwrap();assert_eq!(fs.delete_if_equal_path("/x",b"wrong"),Ok(false));assert_eq!(fs.delete_if_equal_path("/x",b"saved"),Ok(true));assert_eq!(fs.read_path("/x",&mut[0;16]),Err(FsError::NotFound));
    }
    /// R092 filesystem operation regression 18
    #[test] fn r092_fs_18_rename_if_equal_guarded(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"saved").unwrap();assert_eq!(fs.rename_if_equal_path("/x","/y",b"wrong"),Ok(false));assert_eq!(fs.rename_if_equal_path("/x","/y",b"saved"),Ok(true));assert_eq!(get(&mut fs,"/y"),b"saved");
    }
    /// R092 filesystem operation regression 19
    #[test] fn r092_fs_19_copy_between_file_ranges(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/a",b"abcdef").unwrap();fs.create_path("/b",b"XY").unwrap();assert_eq!(fs.copy_range_into_path("/a",2,3,"/b",1),Ok(3));assert_eq!(get(&mut fs,"/b"),b"Xcde");
    }
    /// R092 filesystem operation regression 20
    #[test] fn r092_fs_20_copy_overlap_uses_snapshot(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/a",b"abcdef").unwrap();assert_eq!(fs.copy_range_into_path("/a",0,3,"/a",2),Ok(3));assert_eq!(get(&mut fs,"/a"),b"ababcf");
    }
    /// R092 filesystem operation regression 21
    #[test] fn r092_fs_21_replace_all_expanding(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/a",b"cat dog cat").unwrap();assert_eq!(fs.replace_all_path("/a",b"cat",b"tiger"),Ok(2));let mut fs=DurableFs::mount(fs.into_device()).unwrap();assert_eq!(get(&mut fs,"/a"),b"tiger dog tiger");
    }
    /// R092 filesystem operation regression 22
    #[test] fn r092_fs_22_replace_all_shrinking(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/a",b"one one").unwrap();assert_eq!(fs.replace_all_path("/a",b"one",b"x"),Ok(2));assert_eq!(get(&mut fs,"/a"),b"x x");
    }
    /// R092 filesystem operation regression 23
    #[test] fn r092_fs_23_replace_all_no_match_does_not_publish(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/a",b"abc").unwrap();let gen=fs.generation();let writes=fs.disk.writes;assert_eq!(fs.replace_all_path("/a",b"absent",b"y"),Ok(0));assert_eq!(fs.generation(),gen);assert_eq!(fs.disk.writes,writes);
    }
    /// R092 filesystem operation regression 24
    #[test] fn r092_fs_24_copy_failed_source_corruption_never_mutates_target(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/a",b"old").unwrap();let b=fs.meta.entries[0].bank;fs.disk.sectors[block_start(0,b) as usize][0]^=1;let writes=fs.disk.writes;assert_eq!(fs.copy_path("/a","/b"),Err(FsError::InvalidVolume));assert_eq!(fs.disk.writes,writes);
    }
    /// R092 filesystem operation regression 25
    #[test] fn r092_fs_25_rename_swap_torn_metadata_falls_back_to_previous_head(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/a",b"AAA").unwrap();fs.create_path("/b",b"BBB").unwrap();fs.disk.fail_at=Some(fs.disk.writes);assert_eq!(fs.swap_paths("/a","/b"),Err(FsError::Io));let mut disk=fs.into_device();disk.fail_at=None;let mut fs=DurableFs::mount(disk).unwrap();assert_eq!(get(&mut fs,"/a"),b"AAA");assert_eq!(get(&mut fs,"/b"),b"BBB");
    }
    /// R092 filesystem operation regression 26
    #[test] fn r092_fs_26_insert_failed_bank_write_preserves_old_data(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/a",b"old").unwrap();fs.disk.fail_at=Some(fs.disk.writes);assert_eq!(fs.insert_range_path("/a",1,b"X"),Err(FsError::Io));let mut disk=fs.into_device();disk.fail_at=None;let mut fs=DurableFs::mount(disk).unwrap();assert_eq!(get(&mut fs,"/a"),b"old");
    }

    #[test] fn r095_097_01_prepend_and_drop_prefix(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"ab").unwrap();assert_eq!(fs.prepend_path("/x",b"hi"),Ok(2));assert_eq!(get(&mut fs,"/x"),b"hiab");assert_eq!(fs.drop_prefix_path("/x",2),Ok(2));assert_eq!(get(&mut fs,"/x"),b"ab");}
    #[test] fn r095_097_02_drop_suffix(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abc").unwrap();assert_eq!(fs.drop_suffix_path("/x",1),Ok(1));assert_eq!(get(&mut fs,"/x"),b"ab");}
    #[test] fn r095_097_03_replace_prefix(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abc").unwrap();assert_eq!(fs.replace_prefix_path("/x",b"bad",b"z"),Ok(false));assert_eq!(fs.replace_prefix_path("/x",b"ab",b"xy"),Ok(true));assert_eq!(get(&mut fs,"/x"),b"xyc");}
    #[test] fn r095_097_04_replace_suffix(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abc").unwrap();assert_eq!(fs.replace_suffix_path("/x",b"bc",b"Z"),Ok(true));assert_eq!(get(&mut fs,"/x"),b"aZ");}
    #[test] fn r095_097_05_reverse_range(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcdef").unwrap();assert_eq!(fs.reverse_range_path("/x",1,4),Ok(4));assert_eq!(get(&mut fs,"/x"),b"aedcbf");}
    #[test] fn r095_097_06_rotate_left(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcdef").unwrap();assert_eq!(fs.rotate_left_path("/x",1,4,1),Ok(4));assert_eq!(get(&mut fs,"/x"),b"acdebf");}
    #[test] fn r095_097_07_rotate_right(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcdef").unwrap();assert_eq!(fs.rotate_right_path("/x",1,4,1),Ok(4));assert_eq!(get(&mut fs,"/x"),b"aebcdf");}
    #[test] fn r095_097_08_swap_ranges(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcdef").unwrap();assert_eq!(fs.swap_ranges_path("/x",0,4,2),Ok(2));assert_eq!(get(&mut fs,"/x"),b"efcdab");}
    #[test] fn r095_097_09_move_range(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcdef").unwrap();assert_eq!(fs.move_range_path("/x",1,2,4),Ok(2));assert_eq!(get(&mut fs,"/x"),b"adefbc");}
    #[test] fn r095_097_10_xor_range(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",&[1,2,3]).unwrap();assert_eq!(fs.xor_range_path("/x",1,2,0xff),Ok(2));assert_eq!(get(&mut fs,"/x"),[1,253,252]);}
    #[test] fn r095_097_11_uppercase_range(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abC123").unwrap();assert_eq!(fs.uppercase_ascii_range_path("/x",0,4),Ok(2));assert_eq!(get(&mut fs,"/x"),b"ABC123");}
    #[test] fn r095_097_12_search_find(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcabc").unwrap();assert_eq!(fs.find_bytes_path("/x",b"bc"),Ok(Some(1)));assert_eq!(fs.find_bytes_path("/x",b"xx"),Ok(None));}
    #[test] fn r095_097_13_search_rfind(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcabc").unwrap();assert_eq!(fs.rfind_bytes_path("/x",b"bc"),Ok(Some(4)));}
    #[test] fn r095_097_14_search_count(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"aaaaa").unwrap();assert_eq!(fs.count_bytes_path("/x",b"aa"),Ok(2));}
    #[test] fn r095_097_15_search_prefix(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"alpha").unwrap();assert_eq!(fs.has_prefix_path("/x",b"al"),Ok(true));assert_eq!(fs.has_prefix_path("/x",b"la"),Ok(false));}
    #[test] fn r095_097_16_search_suffix(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"alpha").unwrap();assert_eq!(fs.has_suffix_path("/x",b"ha"),Ok(true));assert_eq!(fs.has_suffix_path("/x",b"al"),Ok(false));}
    #[test] fn r095_097_17_search_read_byte(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abc").unwrap();assert_eq!(fs.read_byte_path("/x",1),Ok(Some(b'b')));assert_eq!(fs.read_byte_path("/x",3),Ok(None));}
    #[test] fn r095_097_18_search_read_head(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcdef").unwrap();let mut out=[0xaa;3];assert_eq!(fs.read_head_path("/x",&mut out),Ok(3));assert_eq!(&out,b"abc");}
    #[test] fn r095_097_19_search_read_tail(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcdef").unwrap();let mut out=[0xaa;3];assert_eq!(fs.read_tail_path("/x",&mut out),Ok(3));assert_eq!(&out,b"def");}
    #[test] fn r095_097_20_search_range_equal(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcdef").unwrap();assert_eq!(fs.range_equals_path("/x",2,b"cde"),Ok(true));assert_eq!(fs.range_equals_path("/x",5,b"zzz"),Ok(false));}
    #[test] fn r095_097_21_search_empty_verified(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"").unwrap();assert_eq!(fs.verified_empty_path("/x"),Ok(true));fs.append_path("/x",b"x").unwrap();assert_eq!(fs.verified_empty_path("/x"),Ok(false));}
    #[test] fn r095_097_22_gen_append(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"one").unwrap();let g=fs.generation();assert_eq!(fs.append_if_generation_path("/x",g+1,b"x"),Err(FsError::InvalidVolume));assert_eq!(fs.append_if_generation_path("/x",g,b"x"),Ok(1));assert_eq!(get(&mut fs,"/x"),b"onex");}
    #[test] fn r095_097_23_gen_insert(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"ac").unwrap();let g=fs.generation();assert_eq!(fs.insert_if_generation_path("/x",g,1,b"b"),Ok(1));assert_eq!(get(&mut fs,"/x"),b"abc");}
    #[test] fn r095_097_24_gen_remove(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcd").unwrap();let g=fs.generation();assert_eq!(fs.remove_if_generation_path("/x",g,1,2),Ok(2));assert_eq!(get(&mut fs,"/x"),b"ad");}
    #[test] fn r095_097_25_gen_splice(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcd").unwrap();let g=fs.generation();assert_eq!(fs.splice_if_generation_path("/x",g,1,2,b"XX"),Ok(2));assert_eq!(get(&mut fs,"/x"),b"aXXd");}
    #[test] fn r095_097_26_gen_fill(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcd").unwrap();let g=fs.generation();assert_eq!(fs.fill_if_generation_path("/x",g,1,2,b'X'),Ok(2));assert_eq!(get(&mut fs,"/x"),b"aXXd");}
    #[test] fn r095_097_27_gen_delete(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/x",b"abcd").unwrap();let g=fs.generation();assert_eq!(fs.delete_if_generation_path("/x",g+1),Err(FsError::InvalidVolume));fs.delete_if_generation_path("/x",g).unwrap();assert_eq!(fs.read_path("/x",&mut[0;8]),Err(FsError::NotFound));}
    #[test] fn r095_097_28_gen_swap(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/a",b"left").unwrap();fs.create_path("/b",b"right").unwrap();let g=fs.generation();fs.swap_if_generation_path("/a","/b",g).unwrap();assert_eq!(get(&mut fs,"/a"),b"right");}
    #[test] fn r095_097_29_gen_rename(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/a",b"left").unwrap();let g=fs.generation();fs.rename_if_generation_path("/a","/b",g).unwrap();assert_eq!(get(&mut fs,"/b"),b"left");}
    #[test] fn r095_097_30_gen_copy(){let mut fs=DurableFs::format_blank(blank()).unwrap();fs.create_path("/a",b"left").unwrap();let g=fs.generation();fs.copy_if_generation_path("/a","/b",g).unwrap();assert_eq!(get(&mut fs,"/b"),b"left");}


    #[test]fn r100_cow_torn_data_bank_error_retires_session_until_remount(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/record",b"old").unwrap();
        fs.disk.fail_at=Some(fs.disk.writes);
        assert_eq!(fs.overwrite_path("/record",b"new"),Err(FsError::Io));
        fs.disk.fail_at=None;
        let writes=fs.disk.writes;
        assert_eq!(fs.overwrite_path("/record",b"retry"),
                   Err(FsError::InvalidVolume));
        assert_eq!(fs.disk.writes,writes);
        let mut recovered=DurableFs::mount(fs.into_device()).unwrap();
        assert_eq!(get(&mut recovered,"/record"),b"old");
        recovered.overwrite_path("/record",b"success").unwrap();
    }
    #[test]fn r100_cow_lied_payload_write_retires_after_readback(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/record",b"old").unwrap();
        fs.disk.ignore_at=Some(fs.disk.writes);
        assert_eq!(fs.overwrite_path("/record",b"new"),
                   Err(FsError::InvalidVolume));
        fs.disk.ignore_at=None;
        let writes=fs.disk.writes;
        assert_eq!(fs.overwrite_path("/record",b"retry"),
                   Err(FsError::InvalidVolume));
        assert_eq!(fs.disk.writes,writes);
    }
    #[test]fn r100_cow_metadata_publish_failure_retires_session(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/record",b"old").unwrap();
        fs.disk.fail_at=Some(fs.disk.writes+SECTORS_PER_BANK);
        assert_eq!(fs.overwrite_path("/record",b"new"),Err(FsError::Io));
        fs.disk.fail_at=None;
        let writes=fs.disk.writes;
        assert_eq!(fs.overwrite_path("/record",b"retry"),
                   Err(FsError::InvalidVolume));
        assert_eq!(fs.disk.writes,writes);
    }
    #[test]fn r100_cow_retired_session_rejects_reads_until_remounted(){
        let mut fs=DurableFs::format_blank(blank()).unwrap();
        fs.create_path("/record",b"old").unwrap();
        fs.disk.fail_at=Some(fs.disk.writes);
        assert_eq!(fs.overwrite_path("/record",b"new"),Err(FsError::Io));
        // Data read may otherwise be apparently valid; do not reuse uncertain mount.
        assert_eq!(fs.read_path("/record",&mut[0;MAX_FILE_BYTES]),
                   Err(FsError::InvalidVolume));
    }

}
