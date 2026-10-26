//! R031.6-8: allocation-free, per-process durable file handle table.
//! Supports four opaque handles and basic runtime rights, not POSIX ACLs.
//! Shared files are read-only for PID2; PID1 alone may create or modify files.
//! The table is transient; all data persists through the existing R027 COW VFS.
use crate::block::{BlockDevice,BlockWindow};
use crate::durablefs::{FsError,MAX_FILE_BYTES,MIN_SECTORS};
use crate::hier_vfs::HierVfs;
use crate::ata_pio::AtaPio;
use crate::fs_write_guard::{WriteGuard,Statistics};

pub const READ:u64=1;
pub const WRITE:u64=2;
const SLOTS:usize=4;
#[derive(Clone,Copy)]
struct Handle {
    token:u64, pid:u32, mode:u64, offset:u64,
    path:[u8;24], len:usize,
}
impl Handle{
    const EMPTY:Self=Self{token:0,pid:0,mode:0,offset:0,path:[0;24],len:0};
    fn pathname(&self)->&str{
        core::str::from_utf8(&self.path[..self.len]).unwrap()
    }
}
pub struct FileHandles{
    entries:[Handle;SLOTS],next:u64,
    // Kernel-owned service-wide budget: closing/opening handles must NOT
    // reset it. Actual per-user/per-app epochs remain S1 work.
    write_guard:WriteGuard,
}
impl FileHandles{
    pub const fn new()->Self{
        Self{entries:[Handle::EMPTY;SLOTS],next:1,
            write_guard:WriteGuard::new()}
    }
    pub fn security_statistics(&self)->Statistics{
        self.write_guard.statistics()
    }
    fn index(&self,pid:u32,token:u64)->Result<usize,FsError>{
        if token==0{return Err(FsError::InvalidName);}
        self.entries.iter().position(|e|e.token==token && e.pid==pid)
            .ok_or(FsError::InvalidName)
    }
    fn policy(pid:u32,path:&str,mode:u64)->bool{
        match pid{
            // PID1 is trusted init/service owner. Rights requested by a
            // particular handle can still only be reduced, not escalated.
            1=>(1..=3).contains(&mode),
            // PID2 reads *only* explicitly shared subtree files.
            2=>mode==READ && path.starts_with("/shared/"),
            _=>false,
        }
    }
    pub fn open<D:BlockDevice>(&mut self,pid:u32,path:&str,mode:u64,
                               fs:&mut HierVfs<D>)->Result<u64,FsError>{
        if !Self::policy(pid,path,mode)||path.len()>23||
           !path.starts_with('/')||path.as_bytes().contains(&0){
            return Err(FsError::InvalidName);
        }
        // S0: even trusted PID1's ordinary application handle cannot
        // write protected OS/boot/recovery paths. Existing read-only
        // access remains available for recovery and inspection.
        if mode&WRITE!=0&&WriteGuard::allow_path(path).is_err(){
            self.write_guard.deny();
            return Err(FsError::InvalidName);
        }
        let slot=self.entries.iter().position(|e|e.token==0)
            .ok_or(FsError::Full)?;
        // A zero-byte range read validates path, parent directories and the
        // complete data checksum before creating a usable handle.
        fs.read_at(path,0,&mut [])?;
        let token=self.next;
        self.next=self.next.checked_add(1).ok_or(FsError::Full)?;
        let mut bytes=[0u8;24];
        bytes[..path.len()].copy_from_slice(path.as_bytes());
        self.entries[slot]=Handle{token,pid,mode,offset:0,
                                  path:bytes,len:path.len()};
        Ok(token)
    }
    pub fn read<D:BlockDevice>(&mut self,pid:u32,token:u64,
                              out:&mut [u8],fs:&mut HierVfs<D>)
                              ->Result<usize,FsError>{
        if out.is_empty()||out.len()>72{return Err(FsError::TooLarge);}
        let slot=self.index(pid,token)?;
        let h=self.entries[slot];
        if h.mode&READ==0{return Err(FsError::InvalidName);}
        let n=fs.read_at(h.pathname(),h.offset,out)?;
        self.entries[slot].offset=h.offset.checked_add(n as u64)
            .ok_or(FsError::TooLarge)?;
        Ok(n)
    }
    pub fn write<D:BlockDevice>(&mut self,pid:u32,token:u64,
                               input:&[u8],fs:&mut HierVfs<D>)
                               ->Result<usize,FsError>{
        if input.is_empty()||input.len()>256{return Err(FsError::TooLarge);}
        let slot=self.index(pid,token)?;
        let h=self.entries[slot];
        if h.mode&WRITE==0{return Err(FsError::InvalidName);}
        // Authorize immediately before mutation, not only on open.
        // The budget is shared by all handles and survives close/reopen.
        if self.write_guard.preflight(h.pathname(),input.len()).is_err(){
            return Err(FsError::InvalidName);
        }
        // Charge *only* after a successful bounded, durable write.
        let n=fs.write_at(h.pathname(),h.offset,input)?;
        self.write_guard.completed(n);
        self.entries[slot].offset=h.offset.checked_add(n as u64)
            .ok_or(FsError::TooLarge)?;
        Ok(n)
    }
    pub fn seek(&mut self,pid:u32,token:u64,offset:u64)->Result<u64,FsError>{
        if offset>MAX_FILE_BYTES as u64{return Err(FsError::TooLarge);}
        let slot=self.index(pid,token)?;
        self.entries[slot].offset=offset;
        Ok(offset)
    }
    pub fn close(&mut self,pid:u32,token:u64)->Result<(),FsError>{
        let slot=self.index(pid,token)?;
        self.entries[slot]=Handle::EMPTY;
        Ok(())
    }
    pub fn terminate(&mut self,pid:u32){
        for h in &mut self.entries{
            if h.pid==pid{*h=Handle::EMPTY;}
        }
    }
    pub fn dispatch<D:BlockDevice>(&mut self,action:u64,pid:u32,
                                   token:u64,argument:u64,path:&str,
                                   buffer:&mut [u8],fs:&mut HierVfs<D>)
                                   ->Result<u64,FsError>{
        match action{
            1=>self.open(pid,path,argument,fs),
            2=>self.read(pid,token,buffer,fs).map(|n|n as u64),
            3=>self.write(pid,token,buffer,fs).map(|n|n as u64),
            4=>self.seek(pid,token,argument),
            5=>{self.close(pid,token)?;Ok(0)},
            _=>Err(FsError::InvalidName)
        }
    }
}
pub fn operation(table:&mut FileHandles,action:u64,pid:u32,
                 token:u64,argument:u64,path:&str,buffer:&mut [u8])
                 ->Result<u64,FsError>{
    let mut disk=unsafe{AtaPio::probe()}.map_err(|_|FsError::Io)?;
    if disk.sector_count()!=128||crate::r030_ondevice::COUNT<MIN_SECTORS{
        return Err(FsError::InvalidVolume);
    }
    let window=BlockWindow::new(&mut disk,crate::r030_ondevice::START,
                                crate::r030_ondevice::COUNT)?;
    let mut fs=HierVfs::mount(window)?;
    table.dispatch(action,pid,token,argument,path,buffer,&mut fs)
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::block::{BlockError,SECTOR_BYTES};
    const COUNT:usize=100;
    struct Disk{sectors:[[u8;SECTOR_BYTES];COUNT]}
    impl BlockDevice for Disk{
        fn sector_count(&self)->u64{COUNT as u64}
        fn read_sector(&mut self,lba:u64,out:&mut [u8;SECTOR_BYTES])->Result<(),BlockError>{
            *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*data;
            Ok(())
        }
    }
    fn fresh()->HierVfs<Disk>{
        HierVfs::format_blank(Disk{sectors:[[0u8;SECTOR_BYTES];COUNT]}).unwrap()
    }
    #[test] fn handle_seek_write_read_close_and_reuse(){
        let mut fs=fresh();
        fs.mkdir("/etc").unwrap();fs.create("/etc/note",b"hello").unwrap();
        let mut table=FileHandles::new();
        let fd=table.open(1,"/etc/note",READ|WRITE,&mut fs).unwrap();
        let mut buf=[0u8;72];
        assert_eq!(table.read(1,fd,&mut buf[..2],&mut fs),Ok(2));
        assert_eq!(&buf[..2],b"he");
        assert_eq!(table.seek(1,fd,2),Ok(2));
        assert_eq!(table.write(1,fd,b"LLO",&mut fs),Ok(3));
        assert_eq!(table.seek(1,fd,0),Ok(0));
        assert_eq!(table.read(1,fd,&mut buf[..8],&mut fs),Ok(5));
        assert_eq!(&buf[..5],b"heLLO");
        assert_eq!(table.read(1,fd,&mut buf[..8],&mut fs),Ok(0));
        table.close(1,fd).unwrap();
        assert_eq!(table.read(1,fd,&mut buf[..8],&mut fs),Err(FsError::InvalidName));
        let other=table.open(1,"/etc/note",READ,&mut fs).unwrap();
        assert_ne!(fd,other);
        assert_eq!(table.write(1,other,b"XX",&mut fs),Err(FsError::InvalidName));
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(fs.read_at("/etc/note",0,&mut buf),Ok(5));
        assert_eq!(&buf[..5],b"heLLO");
    }
    #[test] fn rights_owner_public_only_slots_and_teardown(){
        let mut fs=fresh();fs.mkdir("/etc").unwrap();fs.mkdir("/shared").unwrap();
        fs.create("/etc/private",b"secret").unwrap();
        fs.create("/shared/guide",b"public").unwrap();
        let mut table=FileHandles::new();let mut buf=[0u8;72];
        assert_eq!(table.open(2,"/etc/private",READ,&mut fs),Err(FsError::InvalidName));
        assert_eq!(table.open(2,"/shared/guide",WRITE,&mut fs),Err(FsError::InvalidName));
        assert_eq!(table.open(3,"/shared/guide",READ,&mut fs),Err(FsError::InvalidName));
        let shared=table.open(2,"/shared/guide",READ,&mut fs).unwrap();
        assert_eq!(table.read(1,shared,&mut buf,&mut fs),Err(FsError::InvalidName));
        assert_eq!(table.write(2,shared,b"x",&mut fs),Err(FsError::InvalidName));
        assert_eq!(table.read(2,shared,&mut buf,&mut fs),Ok(6));
        let owner=table.open(1,"/etc/private",READ,&mut fs).unwrap();
        let third=table.open(1,"/shared/guide",READ,&mut fs).unwrap();
        let fourth=table.open(1,"/etc/private",READ,&mut fs).unwrap();
        assert_eq!(table.open(1,"/etc/private",READ,&mut fs),Err(FsError::Full));
        assert_eq!(table.seek(2,owner,5),Err(FsError::InvalidName));
        assert_eq!(table.seek(2,shared,2049),Err(FsError::TooLarge));
        table.terminate(2);
        assert_eq!(table.read(2,shared,&mut buf,&mut fs),Err(FsError::InvalidName));
        assert!(table.open(1,"/etc/private",READ,&mut fs).is_ok());
        assert!(third>0&&fourth>0);
    }
    #[test] fn read_only_and_invalid_offsets_never_mutate_data(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"stored").unwrap();
        let mut handles=FileHandles::new();
        let fd=handles.open(1,"/shared/note",READ,&mut fs).unwrap();
        assert_eq!(handles.write(1,fd,b"x",&mut fs),Err(FsError::InvalidName));
        assert_eq!(handles.seek(1,fd,u64::MAX),Err(FsError::TooLarge));
        assert_eq!(handles.read(1,fd,&mut [0u8;73],&mut fs),Err(FsError::TooLarge));
        handles.close(1,fd).unwrap();
        let fd=handles.open(1,"/shared/note",WRITE,&mut fs).unwrap();
        assert_eq!(handles.read(1,fd,&mut [0u8;6],&mut fs),Err(FsError::InvalidName));
        assert_eq!(handles.seek(1,fd,8),Ok(8));
        assert_eq!(handles.write(1,fd,b"X",&mut fs),Err(FsError::TooLarge));
        let mut data=[0u8;16];
        assert_eq!(fs.read_at("/shared/note",0,&mut data),Ok(6));
        assert_eq!(&data[..6],b"stored");
    }
    #[test] fn security_protected_boot_recovery_and_system_files_never_change(){
        let mut fs=fresh();
        fs.mkdir("/boot").unwrap();fs.mkdir("/system").unwrap();
        fs.mkdir("/recovery").unwrap();
        fs.create("/boot/init",b"trusted").unwrap();
        fs.create("/system/config",b"kernel").unwrap();
        fs.create("/recovery/state",b"backup").unwrap();
        let mut handles=FileHandles::new();
        for path in ["/boot/init","/system/config","/recovery/state",
                     "//boot/./init"]{
            assert_eq!(handles.open(1,path,WRITE,&mut fs),
                Err(FsError::InvalidName),"{path}");
            assert_eq!(handles.open(1,path,READ|WRITE,&mut fs),
                Err(FsError::InvalidName),"{path}");
        }
        let read=handles.open(1,"/boot/init",READ,&mut fs).unwrap();
        let mut data=[0u8;24];
        assert_eq!(handles.read(1,read,&mut data,&mut fs),Ok(7));
        assert_eq!(&data[..7],b"trusted");
        handles.close(1,read).unwrap();
        for (path,expected) in [("/boot/init",b"trusted".as_slice()),
            ("/system/config",b"kernel".as_slice()),
            ("/recovery/state",b"backup".as_slice())]{
            let mut output=[0u8;24];
            let n=fs.read_at(path,0,&mut output).unwrap();
            assert_eq!(&output[..n],expected);
        }
        assert_eq!(handles.security_statistics().denials,8);
    }
    #[test] fn security_burst_write_limit_applies_across_handle_reopen_and_pid(){
        let mut fs=fresh();
        fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"original").unwrap();
        let mut handles=FileHandles::new();
        let mut h=handles.open(1,"/shared/note",WRITE,&mut fs).unwrap();
        for index in 0..crate::fs_write_guard::MAX_WRITE_OPERATIONS{
            if index%7==0&&index!=0{
                handles.close(1,h).unwrap();
                h=handles.open(1,"/shared/note",WRITE,&mut fs).unwrap();
            }
            handles.seek(1,h,0).unwrap();
            assert_eq!(handles.write(1,h,b"X",&mut fs),Ok(1));
        }
        handles.close(1,h).unwrap();
        let another=handles.open(1,"/shared/note",WRITE,&mut fs).unwrap();
        assert_eq!(handles.write(1,another,b"Y",&mut fs),
            Err(FsError::InvalidName));
        assert_eq!(handles.security_statistics().completed_writes,
            crate::fs_write_guard::MAX_WRITE_OPERATIONS);
        assert_eq!(handles.security_statistics().denials,1);
        let mut output=[0u8;24];
        let n=fs.read_at("/shared/note",0,&mut output).unwrap();
        assert_eq!(&output[..n],b"Xriginal");
        assert_eq!(handles.write(2,another,b"Z",&mut fs),
            Err(FsError::InvalidName));
    }
    #[test] fn security_failed_mutation_does_not_consume_write_budget(){
        let mut fs=fresh();
        fs.mkdir("/etc").unwrap();fs.create("/etc/a",b"one").unwrap();
        let mut handles=FileHandles::new();
        let fd=handles.open(1,"/etc/a",WRITE,&mut fs).unwrap();
        handles.seek(1,fd,99).unwrap();
        assert_eq!(handles.write(1,fd,b"X",&mut fs),Err(FsError::TooLarge));
        assert_eq!(handles.security_statistics().completed_writes,0);
        assert_eq!(handles.security_statistics().changed_bytes,0);
        handles.seek(1,fd,0).unwrap();
        assert_eq!(handles.write(1,fd,b"T",&mut fs),Ok(1));
        assert_eq!(handles.security_statistics().completed_writes,1);
        assert_eq!(handles.security_statistics().changed_bytes,1);
    }

}
