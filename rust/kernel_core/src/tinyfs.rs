//! R016: allocation-free, single-sector-per-file filesystem prototype.
//! Explicitly formatted volumes only. No automatic formatting of existing disks.
//! Sector 0 is a superblock, sector 1 a directory, sectors 2.. are file data.
//! This is NOT crash-safe; it has no journal, locking or multi-sector files.
use crate::block::{BlockDevice, BlockError, SECTOR_BYTES};
use crate::vfs_path::normalize_absolute;
const MAGIC: &[u8;8] = b"INDR016!";
const ENTRIES: usize = 16;
const ENTRY_BYTES: usize = 32;
const NAME_BYTES: usize = 24;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum FsError { Io, InvalidVolume, InvalidName, Exists, NotFound, Full, TooLarge }
impl From<BlockError> for FsError { fn from(_:BlockError)->Self{Self::Io} }
pub struct TinyFs<D:BlockDevice> { disk:D }
impl<D:BlockDevice> TinyFs<D> {
    /// Destructive: caller must select an empty, disposable device.
    pub fn format(mut disk:D)->Result<Self,FsError>{
        if disk.sector_count()<3 || disk.sector_count()>u32::MAX as u64 {return Err(FsError::InvalidVolume);}
        let mut header=[0u8;SECTOR_BYTES];
        header[..8].copy_from_slice(MAGIC);
        header[8..12].copy_from_slice(&(disk.sector_count() as u32).to_le_bytes());
        disk.write_sector(0,&header)?;
        disk.write_sector(1,&[0u8;SECTOR_BYTES])?;
        Ok(Self{disk})
    }
    pub fn mount(mut disk:D)->Result<Self,FsError>{
        let mut header=[0u8;SECTOR_BYTES];
        disk.read_sector(0,&mut header)?;
        if &header[..8]!=MAGIC || disk.sector_count()<3 ||
            u32::from_le_bytes(header[8..12].try_into().unwrap()) as u64 != disk.sector_count() ||
            header[12..].iter().any(|&v|v!=0) {return Err(FsError::InvalidVolume);}
        Ok(Self{disk})
    }
    fn name(name:&str)->Result<&[u8],FsError>{
        let bytes=name.as_bytes();
        if bytes.is_empty() || bytes.len()>=NAME_BYTES ||
           bytes.iter().any(|&b|b==0 || b==b'/' || b==b'\\' || b<0x20) {
            return Err(FsError::InvalidName);
        }
        Ok(bytes)
    }
    fn directory(&mut self)->Result<[u8;SECTOR_BYTES],FsError>{
        let mut dir=[0u8;SECTOR_BYTES];
        self.disk.read_sector(1,&mut dir)?;
        Ok(dir)
    }
    fn slot(dir:&[u8;SECTOR_BYTES],name:&[u8])->Option<usize>{
        (0..ENTRIES).find(|&i|{
            let e=&dir[i*ENTRY_BYTES..(i+1)*ENTRY_BYTES];
            e[0]!=0 && e[..NAME_BYTES].iter().position(|&b|b==0)==Some(name.len()) &&
            &e[..name.len()]==name
        })
    }
    pub fn create(&mut self,name:&str,data:&[u8])->Result<(),FsError>{
        let name=Self::name(name)?;
        if data.len()>SECTOR_BYTES{return Err(FsError::TooLarge);}
        let mut dir=self.directory()?;
        if Self::slot(&dir,name).is_some(){return Err(FsError::Exists);}
        let slot=(0..ENTRIES).find(|&i|dir[i*ENTRY_BYTES]==0 &&
            (i as u64+2)<self.disk.sector_count()).ok_or(FsError::Full)?;
        let mut sector=[0u8;SECTOR_BYTES];
        sector[..data.len()].copy_from_slice(data);
        self.disk.write_sector(slot as u64+2,&sector)?;
        let e=&mut dir[slot*ENTRY_BYTES..(slot+1)*ENTRY_BYTES];
        e.fill(0);
        e[..name.len()].copy_from_slice(name);
        e[NAME_BYTES..NAME_BYTES+4].copy_from_slice(&(data.len() as u32).to_le_bytes());
        self.disk.write_sector(1,&dir)?;
        Ok(())
    }
    pub fn read(&mut self,name:&str,out:&mut [u8])->Result<usize,FsError>{
        let name=Self::name(name)?;
        let dir=self.directory()?;
        let slot=Self::slot(&dir,name).ok_or(FsError::NotFound)?;
        let e=&dir[slot*ENTRY_BYTES..(slot+1)*ENTRY_BYTES];
        let len=u32::from_le_bytes(e[NAME_BYTES..NAME_BYTES+4].try_into().unwrap()) as usize;
        if len>SECTOR_BYTES{return Err(FsError::InvalidVolume);}
        if out.len()<len{return Err(FsError::TooLarge);}
        let mut sector=[0u8;SECTOR_BYTES];
        self.disk.read_sector(slot as u64+2,&mut sector)?;
        out[..len].copy_from_slice(&sector[..len]);
        Ok(len)
    }
    pub fn delete(&mut self,name:&str)->Result<(),FsError>{
        let name=Self::name(name)?;
        let mut dir=self.directory()?;
        let slot=Self::slot(&dir,name).ok_or(FsError::NotFound)?;
        // Zero data before removing metadata. No crash consistency guarantee.
        self.disk.write_sector(slot as u64+2,&[0u8;SECTOR_BYTES])?;
        dir[slot*ENTRY_BYTES..(slot+1)*ENTRY_BYTES].fill(0);
        self.disk.write_sector(1,&dir)?;
        Ok(())
    }
    /// Convert an absolute VFS pathname to the only layout this tiny
    /// filesystem currently supports: one filename directly under /.
    /// No nested directory is silently flattened or auto-created.
    fn root_name<'a>(path:&str, scratch:&'a mut [u8;NAME_BYTES]) -> Result<&'a str,FsError>{
        let n=normalize_absolute(path,scratch).map_err(|_|FsError::InvalidName)?;
        if n<=1 || scratch[1..n].contains(&b'/'){return Err(FsError::InvalidName);}
        core::str::from_utf8(&scratch[1..n]).map_err(|_|FsError::InvalidName)
    }
    /// Read/write APIs accept absolute VFS paths and reject nested directories.
    /// Validation is finished before touching any disk sectors.
    pub fn create_path(&mut self,path:&str,data:&[u8])->Result<(),FsError>{
        let mut normalized=[0u8;NAME_BYTES];
        let filename=Self::root_name(path,&mut normalized)?;
        self.create(filename,data)
    }
    pub fn read_path(&mut self,path:&str,out:&mut [u8])->Result<usize,FsError>{
        let mut normalized=[0u8;NAME_BYTES];
        let filename=Self::root_name(path,&mut normalized)?;
        self.read(filename,out)
    }
    pub fn delete_path(&mut self,path:&str)->Result<(),FsError>{
        let mut normalized=[0u8;NAME_BYTES];
        let filename=Self::root_name(path,&mut normalized)?;
        self.delete(filename)
    }
    pub fn into_device(self)->D{self.disk}
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Disk { sectors:[[u8;SECTOR_BYTES];5] }
    impl BlockDevice for Disk {
        fn sector_count(&self)->u64{5}
        fn read_sector(&mut self,lba:u64,out:&mut [u8;SECTOR_BYTES])->Result<(),BlockError>{
            let src=self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            *out=*src;Ok(())
        }
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
            let dst=self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?;
            *dst=*data;Ok(())
        }
    }
    fn disk()->Disk{Disk{sectors:[[0;SECTOR_BYTES];5]}}
    #[test] fn lifecycle_and_remount(){
        assert!(matches!(TinyFs::mount(disk()),Err(FsError::InvalidVolume)));
        let mut fs=TinyFs::format(disk()).unwrap();
        fs.create("hello",b"IndianOS").unwrap();
        assert_eq!(fs.create("hello",b"again"),Err(FsError::Exists));
        let mut out=[0;512];
        assert_eq!(fs.read("hello",&mut out),Ok(8));
        assert_eq!(&out[..8],b"IndianOS");
        let mut fs=TinyFs::mount(fs.into_device()).unwrap();
        assert_eq!(fs.read("hello",&mut out),Ok(8));
        fs.delete("hello").unwrap();
        assert_eq!(fs.read("hello",&mut out),Err(FsError::NotFound));
    }
    #[test] fn absolute_paths_round_trip_and_survive_remount(){
        let mut fs=TinyFs::format(disk()).unwrap();
        fs.create_path("//notes///./",b"persisted").unwrap();
        let mut out=[0;SECTOR_BYTES];
        assert_eq!(fs.read_path("/notes",&mut out),Ok(9));
        assert_eq!(&out[..9],b"persisted");
        assert_eq!(fs.create_path("/./notes",b"duplicate"),Err(FsError::Exists));
        let mut remounted=TinyFs::mount(fs.into_device()).unwrap();
        assert_eq!(remounted.read_path("///notes",&mut out),Ok(9));
        assert_eq!(&out[..9],b"persisted");
        remounted.delete_path("/./notes/").unwrap();
        assert_eq!(remounted.read_path("/notes",&mut out),Err(FsError::NotFound));
        // Slot is reclaimed and a different file can be stored.
        remounted.create_path("/again",b"new").unwrap();
        assert_eq!(remounted.read_path("/again",&mut out),Ok(3));
    }
    #[test] fn invalid_paths_fail_before_any_disk_mutation(){
        let mut fs=TinyFs::format(disk()).unwrap();
        fs.create_path("/existing",b"important").unwrap();
        let snapshot=fs.disk.sectors;
        let invalid=[
            "", "name", "/", "/./", "/a/b", "//a/./b",
            "/a/../existing", "/../existing", "/a\0b",
            "/abcdefghijklmnopqrstuvwx", // 24 bytes, capacity is 23
            "/back\\slash",
        ];
        let mut out=[0xaau8;SECTOR_BYTES];
        for path in invalid{
            assert_eq!(fs.create_path(path,b"oops"),Err(FsError::InvalidName),"{path:?}");
            assert_eq!(fs.read_path(path,&mut out),Err(FsError::InvalidName),"{path:?}");
            assert_eq!(fs.delete_path(path),Err(FsError::InvalidName),"{path:?}");
            assert_eq!(fs.disk.sectors,snapshot,"disk mutated for {path:?}");
        }
        assert!(out.iter().all(|b|*b==0xaa));
        assert_eq!(fs.read_path("/existing",&mut out),Ok(9));
        assert_eq!(&out[..9],b"important");
    }
    #[test] fn path_name_boundary_and_payload_capacity(){
        let mut fs=TinyFs::format(disk()).unwrap();
        let name=format!("/{}", "a".repeat(NAME_BYTES-1));
        fs.create_path(&name,&[7;SECTOR_BYTES]).unwrap();
        let mut data=[0u8;SECTOR_BYTES];
        assert_eq!(fs.read_path(&name,&mut data),Ok(SECTOR_BYTES));
        assert!(data.iter().all(|b|*b==7));
        assert_eq!(fs.create_path("/short",&[8;SECTOR_BYTES+1]),Err(FsError::TooLarge));
        assert_eq!(fs.read_path(&name,&mut [0u8;2]),Err(FsError::TooLarge));
        assert_eq!(fs.create_path(&name,b"x"),Err(FsError::Exists));
    }
    #[test] fn rejects_bad_inputs_and_capacity(){
        let mut fs=TinyFs::format(disk()).unwrap();
        assert_eq!(fs.create("../bad",b"x"),Err(FsError::InvalidName));
        assert_eq!(fs.create("big",&[0;513]),Err(FsError::TooLarge));
        for n in ["a","b","c"]{fs.create(n,b"ok").unwrap();}
        assert_eq!(fs.create("d",b"x"),Err(FsError::Full));
        assert_eq!(fs.read("missing",&mut [0;512]),Err(FsError::NotFound));
    }
}
