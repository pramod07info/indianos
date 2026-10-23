//! R025 verified per-process capability and in-memory filesystem service.
//!
//! Single-BSP, non-reentrant kernel-only object; NEVER used in interrupt
//! context. No physical disk is attached and ordinary boots do not enable it.
//! Caller authenticates the PID against the active PCB before every method.
use crate::block::{BlockDevice, BlockError, SECTOR_BYTES};
use crate::capability::{Registry,CapabilityError};
use crate::tinyfs::{TinyFs,FsError};
use crate::vfs_path::normalize_absolute;

pub const SEND_RIGHT:u32=1;
pub const READ_RIGHT:u32=2;
pub const CREATE_RIGHT:u32=4;
pub const DELETE_RIGHT:u32=8;
pub const FILE_RIGHTS:u32=READ_RIGHT|CREATE_RIGHT|DELETE_RIGHT;
pub const FILE_BASE:u64=0x10000;
pub const MAX_FILE_TRANSFER:usize=64;

pub struct MemoryDisk { sectors:[[u8;SECTOR_BYTES];6] }
impl MemoryDisk{
    fn blank()->Self{Self{sectors:[[0;SECTOR_BYTES];6]}}
}
impl BlockDevice for MemoryDisk{
    fn sector_count(&self)->u64{6}
    fn read_sector(&mut self,lba:u64,out:&mut [u8;SECTOR_BYTES])->Result<(),BlockError>{
        let sector=self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
        *out=*sector;Ok(())
    }
    fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
        let sector=self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?;
        *sector=*data;Ok(())
    }
}

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum ServiceError {
    Denied,
    InvalidPath,
    InvalidLength,
    Disk(FsError),
}
impl From<FsError> for ServiceError{
    fn from(error:FsError)->Self{Self::Disk(error)}
}

pub struct R025Services {
    registry:Registry<8>,
    tokens:[[u64;2];2],
    fs:TinyFs<MemoryDisk>,
}

impl R025Services {
    pub fn new()->Option<Self>{
        let mut registry=Registry::<8>::new();
        let tokens=[
            [registry.grant(1,2,SEND_RIGHT).ok()?,
             registry.grant(1,FILE_BASE+1,FILE_RIGHTS).ok()?],
            [registry.grant(2,1,SEND_RIGHT).ok()?,
             registry.grant(2,FILE_BASE+2,FILE_RIGHTS).ok()?],
        ];
        let fs=TinyFs::format(MemoryDisk::blank()).ok()?;
        Some(Self{registry,tokens,fs})
    }

    fn entry(&self,owner:u32,resource:u64)->Option<u64>{
        let which=usize::try_from(owner.checked_sub(1)?).ok()?;
        let row=*self.tokens.get(which)?;
        match resource {
            1|2 => {
                let other=if owner==1{2}else{1};
                (resource==other).then_some(row[0])
            }
            v if v==FILE_BASE+owner as u64=>Some(row[1]),
            _=>None,
        }
    }

    pub fn token(&self,owner:u32,resource:u64)->Result<u64,ServiceError>{
        let token=self.entry(owner,resource).ok_or(ServiceError::Denied)?;
        let rights=if resource<FILE_BASE{SEND_RIGHT}else{FILE_RIGHTS};
        self.registry.authorize(owner,token,resource,rights)
            .map_err(|_|ServiceError::Denied)?;
        Ok(token)
    }

    pub fn authorize_send(&self,owner:u32,token:u64,receiver:u32)
        ->Result<(),ServiceError>
    {
        if owner==receiver || !(1..=2).contains(&receiver){
            return Err(ServiceError::Denied);
        }
        self.registry.authorize(owner,token,receiver as u64,SEND_RIGHT)
            .map_err(|_|ServiceError::Denied)
    }

    fn file_resource(path:&str)->Result<(u64,[u8;24],usize),ServiceError>{
        let mut normalized=[0u8;24];
        let len=normalize_absolute(path,&mut normalized)
            .map_err(|_|ServiceError::InvalidPath)?;
        let canonical=core::str::from_utf8(&normalized[..len])
            .map_err(|_|ServiceError::InvalidPath)?;
        let owner=if canonical.starts_with("/p1-") && canonical.len()>4 {1}
        else if canonical.starts_with("/p2-") && canonical.len()>4 {2}
        else{return Err(ServiceError::InvalidPath)};
        // Only one flat-root file component is allowed by TinyFs.
        if canonical[1..].as_bytes().contains(&b'/'){
            return Err(ServiceError::InvalidPath);
        }
        Ok((FILE_BASE+owner,normalized,len))
    }

    fn checked_path<'a>(&self,owner:u32,token:u64,path:&str,rights:u32,
        bytes:&'a mut [u8;24])->Result<&'a str,ServiceError>
    {
        let (resource,normalized,len)=Self::file_resource(path)?;
        self.registry.authorize(owner,token,resource,rights)
            .map_err(|_|ServiceError::Denied)?;
        bytes[..len].copy_from_slice(&normalized[..len]);
        core::str::from_utf8(&bytes[..len]).map_err(|_|ServiceError::InvalidPath)
    }

    pub fn create(&mut self,owner:u32,token:u64,path:&str,data:&[u8])
        ->Result<(),ServiceError>
    {
        if data.is_empty()||data.len()>MAX_FILE_TRANSFER{
            return Err(ServiceError::InvalidLength);
        }
        let mut name=[0u8;24];
        let path=self.checked_path(owner,token,path,CREATE_RIGHT,&mut name)?;
        self.fs.create_path(path,data).map_err(Into::into)
    }
    pub fn read(&mut self,owner:u32,token:u64,path:&str,out:&mut [u8])
        ->Result<usize,ServiceError>
    {
        if out.is_empty()||out.len()>MAX_FILE_TRANSFER{
            return Err(ServiceError::InvalidLength);
        }
        let mut name=[0u8;24];
        let path=self.checked_path(owner,token,path,READ_RIGHT,&mut name)?;
        self.fs.read_path(path,out).map_err(Into::into)
    }
    pub fn delete(&mut self,owner:u32,token:u64,path:&str)
        ->Result<(),ServiceError>
    {
        let mut name=[0u8;24];
        let path=self.checked_path(owner,token,path,DELETE_RIGHT,&mut name)?;
        self.fs.delete_path(path).map_err(Into::into)
    }

    pub fn revoke(&mut self,owner:u32,token:u64)->Result<(),ServiceError>{
        self.registry.revoke(owner,token).map_err(|_|ServiceError::Denied)
    }
    pub fn terminate(&mut self,pid:u32)->usize{
        self.registry.revoke_all(pid)
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn grants_are_scoped_to_pid_resource_and_rights(){
        let mut s=R025Services::new().unwrap();
        let send1=s.token(1,2).unwrap();
        let file1=s.token(1,FILE_BASE+1).unwrap();
        let file2=s.token(2,FILE_BASE+2).unwrap();
        assert_ne!(send1,file1);
        assert!(s.authorize_send(1,send1,2).is_ok());
        assert_eq!(s.authorize_send(2,send1,2),Err(ServiceError::Denied));
        assert_eq!(s.authorize_send(1,file1,2),Err(ServiceError::Denied));
        assert_eq!(s.token(2,FILE_BASE+1),Err(ServiceError::Denied));
        assert_eq!(s.create(2,file1,"/p1-note",b"fake"),Err(ServiceError::Denied));
        assert_eq!(s.create(2,file2,"/p1-note",b"fake"),Err(ServiceError::Denied));
        assert_eq!(s.create(1,file1,"/p2-note",b"fake"),Err(ServiceError::Denied));
    }
    #[test]
    fn file_roundtrip_namespace_checks_and_teardown(){
        let mut s=R025Services::new().unwrap();
        let a=s.token(1,FILE_BASE+1).unwrap();
        let b=s.token(2,FILE_BASE+2).unwrap();
        s.create(1,a,"//p1-note/./",b"good").unwrap();
        s.create(2,b,"/p2-note",b"other").unwrap();
        let mut buffer=[0u8;64];
        assert_eq!(s.read(1,a,"/p1-note",&mut buffer),Ok(4));
        assert_eq!(&buffer[..4],b"good");
        assert_eq!(s.read(2,b,"/p1-note",&mut buffer),Err(ServiceError::Denied));
        assert_eq!(s.delete(2,b,"/p1-note"),Err(ServiceError::Denied));
        for p in ["p1-note","/p1-note/../p2-note","/p1-note/sub","/p1-"] {
            assert!(s.create(1,a,p,b"invalid").is_err());
        }
        assert_eq!(s.read(2,b,"/p2-note",&mut buffer),Ok(5));
        s.delete(1,a,"/p1-note").unwrap();
        assert_eq!(s.read(1,a,"/p1-note",&mut buffer),Err(ServiceError::Disk(FsError::NotFound)));
        assert_eq!(s.terminate(1),2);
        assert_eq!(s.token(1,2),Err(ServiceError::Denied));
        assert_eq!(s.create(1,a,"/p1-new",b"bad"),Err(ServiceError::Denied));
        assert_eq!(s.read(2,b,"/p2-note",&mut buffer),Ok(5));
    }
    #[test]
    fn revocation_and_capacity_never_expand_privileges(){
        let mut s=R025Services::new().unwrap();
        let a=s.token(1,FILE_BASE+1).unwrap();
        assert!(s.revoke(1,a).is_ok());
        assert_eq!(s.revoke(1,a),Err(ServiceError::Denied));
        assert_eq!(s.token(1,FILE_BASE+1),Err(ServiceError::Denied));
        assert_eq!(s.create(1,a,"/p1-test",b"x"),Err(ServiceError::Denied));
        assert_eq!(s.revoke(2,s.token(1,2).unwrap()),Err(ServiceError::Denied));
        let b=s.token(2,FILE_BASE+2).unwrap();
        assert_eq!(s.create(2,b,"/p2-test",&[42u8;65]),Err(ServiceError::InvalidLength));
        assert_eq!(s.create(2,b,"/p2-test",&[]),Err(ServiceError::InvalidLength));
    }
}
