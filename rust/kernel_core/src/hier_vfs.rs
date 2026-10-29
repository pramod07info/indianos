//! R031.3 bounded hierarchical VFS over R027's durable COW metadata.
//! No on-disk format changes, allocation, recursive traversal, or symlinks.
//! Existing root /app and /note remain separate from reserved /~[df]* keys.
//! 8 total storage objects (directories count); max 21 bytes of logical path.
use crate::block::{BlockDevice,BlockWindow};
use crate::durablefs::{DurableFs,FsError,VolumeHealth,MIN_SECTORS,MAX_FILE_BYTES};
use crate::vfs_path::normalize_absolute;
use crate::ata_pio::AtaPio;
pub const MAX_TRANSFER:usize=256;
const DIR_MARKER:&[u8;8]=b"INDDIR31";
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Kind{Directory,File}
#[derive(Clone,Copy)]
struct Path{
 canonical:[u8;64],canon_len:usize,
 key:[u8;24],key_len:usize,
}
impl Path{
 fn name(&self)->&str{core::str::from_utf8(&self.key[..self.key_len]).unwrap()}
 fn path(&self)->&str{core::str::from_utf8(&self.canonical[..self.canon_len]).unwrap()}
 fn stem(&self)->&str{&self.name()[3..]}
}
fn checked(input:&str,kind:Kind)->Result<Path,FsError>{
 let mut canonical=[0u8;64];
 let count=normalize_absolute(input,&mut canonical)
     .map_err(|_|FsError::InvalidName)?;
 if count<2 {return Err(FsError::InvalidName);}
 // UTF-8 and arbitrary colons/tilde are unsupported until full VFS
 // catalog storage. This mapping is unambiguous and collision free.
 for &byte in &canonical[1..count]{
  if !(byte.is_ascii_alphanumeric() || matches!(byte,b'/'|b'.'|b'_'|b'-')){
   return Err(FsError::InvalidName);
  }
 }
 let mut key=[0u8;24];
 key[..3].copy_from_slice(match kind{
  Kind::Directory=>b"/~d",Kind::File=>b"/~f"
 });
 let key_len=count+2;
 if key_len>24{return Err(FsError::InvalidName);}
 for (index,&byte) in canonical[1..count].iter().enumerate(){
  key[index+3]=if byte==b'/'{b'~'}else{byte};
 }
 Ok(Path{canonical,canon_len:count,key,key_len})
}
pub struct HierVfs<D:BlockDevice>{fs:DurableFs<D>}
impl<D:BlockDevice> HierVfs<D>{
 /// Pure read-only preflight for Files/Terminal before a selected volume
 /// is mounted. Returns verified redundancy without attempting any repair.
 pub fn probe_device(disk:&mut D)->Result<VolumeHealth,FsError>{
  DurableFs::inspect_health(disk)
 }
 pub fn mount(disk:D)->Result<Self,FsError>{
  Ok(Self{fs:DurableFs::mount(disk)?})
 }
 /// Strict Desktop/installer mount: refuse volume media with only a single
 /// surviving verified metadata head; caller can inspect the health first.
 pub fn mount_healthy(disk:D)->Result<Self,FsError>{
  Ok(Self{fs:DurableFs::mount_healthy(disk)?})
 }
 /// Re-read the mounted device and validate all file checksums plus both
 /// metadata copies without causing a write or automatic repair.
 pub fn mounted_health(&mut self)->Result<VolumeHealth,FsError>{
  self.fs.mounted_health()
 }
 /// Opt-in metadata-only recovery, for an authorized kernel Desktop route.
 /// Unlike a destructive filesystem format, it never overwrites file banks.
 pub fn repair_redundancy(&mut self)->Result<bool,FsError>{
  self.fs.repair_redundancy()
 }
 pub fn format_blank(disk:D)->Result<Self,FsError>{
  Ok(Self{fs:DurableFs::format_blank(disk)?})
 }
 pub fn into_device(self)->D{self.fs.into_device()}
 pub fn generation(&self)->u64{self.fs.generation()}
 fn contradictory_alias(&self,p:&Path)->bool{
  let mut key=p.key;key[2]=if key[2]==b'd'{b'f'}else{b'd'};
  let mut own=false;let mut opposite=false;
  self.fs.visit_names(|n|{
   if n.as_bytes()==&p.key[1..p.key_len]{own=true;}
   if n.as_bytes()==&key[1..p.key_len]{opposite=true;}
  });
  own && opposite
 }
 // Validate the entire chain iteratively; a leaf marker alone is insufficient.
 fn directory(&mut self,p:&str)->Result<(),FsError>{
  let k=checked(p,Kind::Directory)?;let name=k.path();
  for end in 1..=name.len(){
   if end!=name.len() && name.as_bytes()[end]!=b'/'{continue;}
   let prefix=checked(&name[..end],Kind::Directory)?;
   if self.contradictory_alias(&prefix){return Err(FsError::InvalidVolume);}
   let mut buf=[0u8;8];
   match self.fs.read_path(prefix.name(),&mut buf){
    Ok(8) if &buf==DIR_MARKER=>{},
    Ok(_)|Err(FsError::TooLarge)=>return Err(FsError::InvalidVolume),
    Err(e)=>return Err(e),
   }
  }
  Ok(())
 }
 // Metadata is bounded by the durable format, so snapshot names without heap
 // allocation before reading directory payloads through the mutable device.
 fn listing_integrity(&mut self)->Result<(),FsError>{
  let mut keys=[[0u8;24];crate::durablefs::MAX_FILES];
  let mut lengths=[0usize;crate::durablefs::MAX_FILES];let mut count=0;
  self.fs.visit_names(|name|{
   // Only ~d/~f are reserved by this VFS. Other flat legacy keys belong
   // to separate callers and must not be diagnosed as a corrupt namespace.
   if name.starts_with("~d") || name.starts_with("~f"){
    keys[count][..name.len()].copy_from_slice(name.as_bytes());
    lengths[count]=name.len();count+=1;
   }
  });
  for i in 0..count{
   let key=&keys[i][..lengths[i]];
   if key.len()<3 || !matches!(key[1],b'd'|b'f'){return Err(FsError::InvalidVolume);}
   let mut logical=[0u8;24];logical[0]=b'/';
   for (j,&byte) in key[2..].iter().enumerate(){
    logical[j+1]=if byte==b'~'{b'/'}else{byte};
   }
   let kind=if key[1]==b'd'{Kind::Directory}else{Kind::File};
   let raw=core::str::from_utf8(&logical[..key.len()-1]).map_err(|_|FsError::InvalidVolume)?;
   let path=checked(raw,kind).map_err(|_|FsError::InvalidVolume)?;
   if &path.key[1..path.key_len]!=key || self.contradictory_alias(&path){return Err(FsError::InvalidVolume);}
   let result=if kind==Kind::Directory{self.directory(path.path())}else{self.parents(&path)};
   match result{Err(FsError::NotFound)=>return Err(FsError::InvalidVolume),other=>other?}
  }
  Ok(())
 }
 fn parents(&mut self,path:&Path)->Result<(),FsError>{
  // Canonical '/' is implicit; every intermediate component must exist.
  let name=path.path();
  for (offset,&byte) in name.as_bytes().iter().enumerate().skip(1){
   if byte==b'/'{self.directory(&name[..offset])?;}
  }
  Ok(())
 }
 fn conflicting(&mut self,p:&str,kind:Kind)->Result<bool,FsError>{
  let opposite=checked(p,match kind{
   Kind::Directory=>Kind::File,Kind::File=>Kind::Directory
  })?;
  let mut b=[0u8;MAX_TRANSFER];
  match self.fs.read_path(opposite.name(),&mut b){
   Ok(_)=>Ok(true),
   Err(FsError::NotFound)=>Ok(false),
   Err(e)=>Err(e)
  }
 }
 pub fn mkdir(&mut self,path:&str)->Result<(),FsError>{
  let p=checked(path,Kind::Directory)?;
  self.parents(&p)?;
  if self.conflicting(p.path(),Kind::Directory)?{return Err(FsError::Exists);}
  self.fs.create_path(p.name(),DIR_MARKER)
 }
 pub fn rmdir(&mut self,path:&str)->Result<(),FsError>{
  let p=checked(path,Kind::Directory)?;
  self.directory(p.path())?;
  let mut prefix=[0u8;24];
  let stem=p.stem().as_bytes();
  if stem.len()+1>prefix.len(){return Err(FsError::InvalidName);}
  prefix[..stem.len()].copy_from_slice(stem);
  prefix[stem.len()]=b'~';
  let pref=&prefix[..stem.len()+1];
  let mut has_child=false;
  self.fs.visit_names(|name|{
   if name.len()>=3 && name.as_bytes()[0]==b'~' &&
      matches!(name.as_bytes()[1],b'f'|b'd') &&
      name.as_bytes()[2..].starts_with(pref){has_child=true;}
  });
  if has_child{return Err(FsError::Full);}
  if self.contradictory_alias(&p){return Err(FsError::InvalidVolume);}
  self.fs.delete_path(p.name())
 }
 pub fn create(&mut self,path:&str,data:&[u8])->Result<(),FsError>{
  let p=checked(path,Kind::File)?;
  self.parents(&p)?;
  if self.conflicting(p.path(),Kind::File)?{return Err(FsError::Exists);}
  self.fs.create_path(p.name(),data)
 }
 pub fn read(&mut self,path:&str,out:&mut [u8])->Result<usize,FsError>{
  let p=checked(path,Kind::File)?;
  self.parents(&p)?;
  if self.contradictory_alias(&p){return Err(FsError::InvalidVolume);}
  self.fs.read_path(p.name(),out)
 }
 pub fn overwrite(&mut self,path:&str,data:&[u8])->Result<(),FsError>{
  let p=checked(path,Kind::File)?;
  self.parents(&p)?;
  if self.contradictory_alias(&p){return Err(FsError::InvalidVolume);}
  self.fs.overwrite_path(p.name(),data)
 }
 /// Offset-based read of existing files. A large on-disk file can be read
 /// through many small ring3 transfers without weakening CRC verification.
 pub fn read_at(&mut self,path:&str,offset:u64,
                out:&mut [u8])->Result<usize,FsError>{
  if out.len()>MAX_TRANSFER{return Err(FsError::TooLarge);}
  let p=checked(path,Kind::File)?;
  self.parents(&p)?;
  if self.contradictory_alias(&p){return Err(FsError::InvalidVolume);}
  self.fs.read_range_path(p.name(),offset,out)
 }
 /// Offset-based append or in-place overwrite. Do not allow sparse holes.
 pub fn write_at(&mut self,path:&str,offset:u64,
                 data:&[u8])->Result<usize,FsError>{
  if data.is_empty()||data.len()>MAX_TRANSFER{return Err(FsError::TooLarge);}
  let p=checked(path,Kind::File)?;
  self.parents(&p)?;
  if self.contradictory_alias(&p){return Err(FsError::InvalidVolume);}
  self.fs.write_range_path(p.name(),offset,data)
 }
 /// Durable destination creation from a complete, CRC-verified source.
 /// The current R027 on-disk bank still limits file size; unlike small
 /// Terminal copy buffers this never silently truncates the source.
 /// Both paths are validated before any destination metadata publication.
 pub fn copy_file(&mut self,source:&str,target:&str)->Result<(),FsError>{
  let from=checked(source,Kind::File)?;
  let to=checked(target,Kind::File)?;
  if from.path()==to.path(){return Err(FsError::Exists);}
  self.parents(&from)?;self.parents(&to)?;
  if self.contradictory_alias(&from){return Err(FsError::InvalidVolume);}
  if self.conflicting(to.path(),Kind::File)?{return Err(FsError::Exists);}
  let mut data=[0u8;MAX_FILE_BYTES];
  let count=self.fs.read_path(from.name(),&mut data)?;
  self.fs.create_path(to.name(),&data[..count])
 }
 /// Update an existing destination atomically with the verified full
 /// source file, preserving the previous destination metadata on torn writes.
 /// Cannot cross the current fixed-bank volume's per-file size limit.
 pub fn copy_replace(&mut self,source:&str,destination:&str)->Result<(),FsError>{
  let from=checked(source,Kind::File)?;
  let to=checked(destination,Kind::File)?;
  if from.path()==to.path(){return Err(FsError::Exists);}
  self.parents(&from)?;self.parents(&to)?;
  if self.contradictory_alias(&from){return Err(FsError::InvalidVolume);}
  if self.conflicting(to.path(),Kind::File)?{return Err(FsError::Exists);}
  // Validate both existing files completely before any write transaction.
  let mut buffer=[0u8;MAX_FILE_BYTES];
  let count=self.fs.read_path(from.name(),&mut buffer)?;
  let mut prior=[0u8;MAX_FILE_BYTES];
  self.fs.read_path(to.name(),&mut prior)?;
  self.fs.overwrite_path(to.name(),&buffer[..count])
 }
 pub fn unlink(&mut self,path:&str)->Result<(),FsError>{
  let p=checked(path,Kind::File)?;
  self.parents(&p)?;
  if self.contradictory_alias(&p){return Err(FsError::InvalidVolume);}
  self.fs.delete_path(p.name())
 }
 pub fn rename_file(&mut self,source:&str,target:&str)->Result<(),FsError>{
  let from=checked(source,Kind::File)?;
  let to=checked(target,Kind::File)?;
  self.parents(&from)?;self.parents(&to)?;
  if self.contradictory_alias(&from){return Err(FsError::InvalidVolume);}
  if self.conflicting(to.path(),Kind::File)?{return Err(FsError::Exists);}
  self.fs.rename_path(from.name(),to.name())
 }
 pub fn rename_dir(&mut self,source:&str,target:&str)->Result<(),FsError>{
  let from=checked(source,Kind::Directory)?;
  let to=checked(target,Kind::Directory)?;
  if from.path()==to.path(){return Err(FsError::Exists);}
  // Refuse self-parenting and moves under any current descendant.
  if let Some(rest)=to.path().strip_prefix(from.path()){
   if rest.starts_with('/'){return Err(FsError::InvalidName);}
  }
  self.directory(from.path())?;
  self.parents(&to)?;
  if self.conflicting(to.path(),Kind::Directory)?{return Err(FsError::Exists);}
  let mut marker=[0u8;8];
  match self.fs.read_path(to.name(),&mut marker){
   Ok(_)=>Err(FsError::Exists),
   Err(FsError::NotFound)=>self.fs.rename_directory_tree(from.name(),to.name()),
   Err(e)=>Err(e),
  }
 }
 pub fn list_dir(&mut self,path:&str,out:&mut [u8])->Result<usize,FsError>{
  let mut canonical=[0u8;64];
  let len=normalize_absolute(path,&mut canonical).map_err(|_|FsError::InvalidName)?;
  let path=core::str::from_utf8(&canonical[..len]).map_err(|_|FsError::InvalidName)?;
  if path!="/"{self.directory(path)?;}
  self.listing_integrity()?;
  let stem=if path=="/"{None}else{Some(checked(path,Kind::Directory)?)};
  let prefix=stem.as_ref().map(|p|p.stem().as_bytes());
  let mut scratch=[0u8;MAX_TRANSFER];let mut used=0usize;
  self.fs.visit_names(|name|{
   let bytes=name.as_bytes();
   if bytes.len()<3||bytes[0]!=b'~'||!matches!(bytes[1],b'd'|b'f'){
    return;
   }
   let base=&bytes[2..];
   let child=if let Some(pref)=prefix{
    if !base.starts_with(pref)||base.get(pref.len())!=Some(&b'~'){return;}
    &base[pref.len()+1..]
   }else{base};
   if child.is_empty()||child.contains(&b'~'){return;}
   let size=2+child.len()+1;  // d:name\n or f:name\n
   if used==usize::MAX{return;}
   if used+size>scratch.len(){used=usize::MAX;return;}
   scratch[used]=bytes[1];
   scratch[used+1]=b':';
   scratch[used+2..used+2+child.len()].copy_from_slice(child);
   scratch[used+2+child.len()]=b'\n';
   used+=size;
  });
  if used==usize::MAX{return Err(FsError::TooLarge);}
  if out.len()<used{return Err(FsError::TooLarge);}
  out[..used].copy_from_slice(&scratch[..used]);Ok(used)
 }
 pub fn dispatch(&mut self,action:u64,path:&str,second:&str,
                 data:&mut [u8],used:usize)->Result<usize,FsError>{
  if data.len()>MAX_TRANSFER||used>data.len(){return Err(FsError::TooLarge);}
  match action{
   1=>{self.mkdir(path)?;Ok(0)},
   2=>{self.rmdir(path)?;Ok(0)},
   3=>{self.create(path,&data[..used])?;Ok(0)},
   4=>self.read(path,data),
   5=>{self.overwrite(path,&data[..used])?;Ok(0)},
   6=>{self.unlink(path)?;Ok(0)},
   7=>{self.rename_file(path,second)?;Ok(0)},
   8=>self.list_dir(path,data),
   9=>{self.rename_dir(path,second)?;Ok(0)},
   // R009 privileged VFS batch: durable, full-file CRC-verified copy APIs.
   // The caller retains responsibility for syscall capability admission.
   10=>{self.copy_file(path,second)?;Ok(0)},
   11=>{self.copy_replace(path,second)?;Ok(0)},
   _=>Err(FsError::InvalidName)
  }
 }
}
pub fn operation(action:u64,path:&str,second:&str,
                 data:&mut [u8],used:usize)->Result<usize,FsError>{
 let mut disk=unsafe{AtaPio::probe()}.map_err(|_|FsError::Io)?;
 if disk.sector_count()!=128||crate::r030_ondevice::COUNT<MIN_SECTORS{
  return Err(FsError::InvalidVolume);
 }
 let window=BlockWindow::new(&mut disk,crate::r030_ondevice::START,
                           crate::r030_ondevice::COUNT)?;
 let mut vfs=HierVfs::mount(window)?;
 vfs.dispatch(action,path,second,data,used)
}
/// Separate read/write operation for range I/O while preserving the existing
/// pathname ABI for R031.3 and R031.4. Caller authorizes PID1 and copies buffers.
pub fn range_operation(action:u64,path:&str,offset:u64,
                       data:&mut [u8],used:usize)->Result<usize,FsError>{
 if data.len()>MAX_TRANSFER||used>data.len()||offset>MAX_FILE_BYTES as u64{
  return Err(FsError::TooLarge);
 }
 let mut disk=unsafe{AtaPio::probe()}.map_err(|_|FsError::Io)?;
 if disk.sector_count()!=128||crate::r030_ondevice::COUNT<MIN_SECTORS{
  return Err(FsError::InvalidVolume);
 }
 let window=BlockWindow::new(&mut disk,crate::r030_ondevice::START,
                            crate::r030_ondevice::COUNT)?;
 let mut vfs=HierVfs::mount(window)?;
 match action{
  10=>vfs.read_at(path,offset,data),
  11=>vfs.write_at(path,offset,&data[..used]),
  _=>Err(FsError::InvalidName)
 }
}
#[cfg(test)]
mod tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 const COUNT:usize=100;
 #[derive(Clone)]
 struct Disk{blocks:[[u8;SECTOR_BYTES];COUNT],writes:usize,tear:Option<usize>}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{COUNT as u64}
  fn read_sector(&mut self,lba:u64,out:&mut [u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.blocks.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,from:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   let to=self.blocks.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?;
   if self.tear==Some(self.writes){
    to[0]^=0xf0;return Err(BlockError::Io);
   }
   *to=*from;self.writes+=1;Ok(())
  }
 }
 fn fresh()->HierVfs<Disk>{
  HierVfs::format_blank(Disk{blocks:[[0;SECTOR_BYTES];COUNT],writes:0,tear:None}).unwrap()
 }
 #[test]fn valid_wrong_type_lookup_is_not_namespace_corruption(){
  let mut v=fresh();v.mkdir("/dir").unwrap();v.create("/file",b"ok").unwrap();
  let before=v.generation();let mut out=[0xcc;32];
  assert_eq!(v.read("/dir",&mut out),Err(FsError::NotFound));
  assert_eq!(v.read_at("/dir",0,&mut out),Err(FsError::NotFound));
  assert_eq!(v.list_dir("/file",&mut out),Err(FsError::NotFound));
  assert_eq!(v.mkdir("/file/child"),Err(FsError::NotFound));
  assert_eq!(out,[0xcc;32]);assert_eq!(v.generation(),before);
 }
 #[test]fn contradictory_file_alias_cannot_read_or_mutate(){
  let mut v=fresh();v.create("/a",b"original").unwrap();v.create("/target",b"safe").unwrap();
  v.fs.create_path("/~da",DIR_MARKER).unwrap();
  let disk=v.into_device();let before=disk.blocks;let writes=disk.writes;
  let mut v=HierVfs::mount(disk).unwrap();let generation=v.generation();
  let mut out=[0xcc;32];
  assert_eq!(v.read("/a",&mut out),Err(FsError::InvalidVolume));
  assert_eq!(v.read_at("/a",0,&mut out),Err(FsError::InvalidVolume));
  assert_eq!(v.overwrite("/a",b"bad"),Err(FsError::InvalidVolume));
  assert_eq!(v.write_at("/a",0,b"bad"),Err(FsError::InvalidVolume));
  assert_eq!(v.unlink("/a"),Err(FsError::InvalidVolume));
  assert_eq!(v.rename_file("/a","/new"),Err(FsError::InvalidVolume));
  assert_eq!(v.copy_file("/a","/new"),Err(FsError::InvalidVolume));
  assert_eq!(v.copy_replace("/a","/target"),Err(FsError::InvalidVolume));
  assert_eq!(v.create("/a",b"collision"),Err(FsError::Exists));
  assert_eq!(v.copy_file("/target","/a"),Err(FsError::Exists));
  assert_eq!(out,[0xcc;32]);assert_eq!(v.generation(),generation);
  let disk=v.into_device();assert_eq!(disk.blocks,before);assert_eq!(disk.writes,writes);
 }
 #[test]fn canonical_listing_aliases_survive_remount(){
  let mut v=fresh();v.mkdir("//a/./").unwrap();v.create("/a/f",b"ok").unwrap();
  let mut v=HierVfs::mount(v.into_device()).unwrap();
  for path in ["/","////","/././","//.///"]{
   let mut out=[0xcc;32];let n=v.list_dir(path,&mut out).unwrap();
   assert_eq!(&out[..n],b"d:a\n");
  }
  let mut out=[0;32];let n=v.list_dir("//a/./",&mut out).unwrap();
  assert_eq!(&out[..n],b"f:f\n");
 }
 #[test]fn legacy_keys_outside_reserved_namespace_remain_opaque(){
  let mut v=fresh();v.fs.create_path("/~x-old",b"opaque").unwrap();
  v.mkdir("/a").unwrap();v.create("/a/f",b"ok").unwrap();
  let mut v=HierVfs::mount(v.into_device()).unwrap();let generation=v.generation();
  let mut out=[0xcc;32];let n=v.list_dir("/",&mut out).unwrap();
  assert_eq!(&out[..n],b"d:a\n");assert_eq!(v.generation(),generation);
  let mut raw=crate::durablefs::DurableFs::mount(v.into_device()).unwrap();
  assert_eq!(raw.read_path("/~x-old",&mut out),Ok(6));
  assert_eq!(&out[..6],b"opaque");
 }
 #[test]fn orphan_bad_marker_and_alias_fail_without_output_or_writes(){
  for case in 0..5{
   let mut v=fresh();
   match case{
    0=>{v.fs.create_path("/~da~b",DIR_MARKER).unwrap();},
    1=>{v.fs.create_path("/~da",b"bad").unwrap();},
    2=>{v.mkdir("/a").unwrap();v.fs.create_path("/~fa",b"alias").unwrap();},
    3=>{v.fs.create_path("/~fa~leaf",b"orphan").unwrap();},
    _=>{v.fs.create_path("/~da~~b",DIR_MARKER).unwrap();},
   }
   let disk=v.into_device();let before=disk.blocks;let writes=disk.writes;
   let mut v=HierVfs::mount(disk).unwrap();let generation=v.generation();
   let mut out=[0xcc;MAX_TRANSFER];
   assert_eq!(v.list_dir("/",&mut out),Err(FsError::InvalidVolume));
   assert_eq!(out,[0xcc;MAX_TRANSFER]);assert_eq!(v.generation(),generation);
   if case==0{
    assert_eq!(v.list_dir("/a/b",&mut out),Err(FsError::NotFound));
    assert_eq!(v.mkdir("/a/b/c"),Err(FsError::NotFound));
    assert_eq!(v.read("/a/b/f",&mut out),Err(FsError::NotFound));
   }
   if case==2{
    assert_eq!(v.mkdir("/a/b"),Err(FsError::InvalidVolume));
    assert_eq!(v.read("/a",&mut out),Err(FsError::InvalidVolume));
   }
   assert_eq!(out,[0xcc;MAX_TRANSFER]);
   let disk=v.into_device();assert_eq!(disk.blocks,before);assert_eq!(disk.writes,writes);
  }
 }
 #[test]fn directory_payload_read_fault_keeps_listing_and_media_unchanged(){
  struct Fault<'a>{disk:Disk,fail:&'a core::cell::Cell<bool>}
  impl BlockDevice for Fault<'_>{
   fn sector_count(&self)->u64{self.disk.sector_count()}
   fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
    if self.fail.get() && lba>=2{return Err(BlockError::Io);}
    self.disk.read_sector(lba,out)
   }
   fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
    self.disk.write_sector(lba,data)
   }
  }
  let mut v=fresh();v.mkdir("/a").unwrap();v.mkdir("/a/b").unwrap();
  let disk=v.into_device();let before=disk.blocks;let writes=disk.writes;
  let fail=core::cell::Cell::new(false);
  let mut v=HierVfs::mount(Fault{disk,fail:&fail}).unwrap();fail.set(true);
  let mut out=[0xcc;MAX_TRANSFER];
  assert_eq!(v.list_dir("/",&mut out),Err(FsError::Io));
  assert_eq!(v.list_dir("/a/b",&mut out),Err(FsError::Io));
  assert_eq!(v.mkdir("/a/b/c"),Err(FsError::Io));
  assert_eq!(out,[0xcc;MAX_TRANSFER]);
  let disk=v.into_device().disk;assert_eq!(disk.blocks,before);assert_eq!(disk.writes,writes);
 }
 #[test]fn hierarchy_reboot_listing_parent_and_conflicts(){
  let mut v=fresh();
  assert_eq!(v.create("/home/a",b"bad"),Err(FsError::NotFound));
  v.mkdir("/home").unwrap();v.mkdir("/home/docs").unwrap();v.mkdir("/etc").unwrap();
  v.create("/etc/config",b"alpha").unwrap();
  v.create("/home/docs/note",b"hello").unwrap();
  assert_eq!(v.rmdir("/home"),Err(FsError::Full));
  assert_eq!(v.rmdir("/home/docs"),Err(FsError::Full));
  assert_eq!(v.mkdir("/home/docs"),Err(FsError::Exists));
  assert_eq!(v.create("/etc",b"bad"),Err(FsError::Exists));
  let mut v=HierVfs::mount(v.into_device()).unwrap();
  let mut output=[0xaa;MAX_TRANSFER];
  assert_eq!(v.read("/etc/config",&mut output),Ok(5));
  assert_eq!(&output[..5],b"alpha");
  assert_eq!(v.list_dir("/",&mut output),Ok(13));
  assert_eq!(&output[..13],b"d:home\nd:etc\n");
  let n=v.list_dir("/home",&mut output).unwrap();
  assert_eq!(&output[..n],b"d:docs\n");
  let n=v.list_dir("/home/docs",&mut output).unwrap();
  assert_eq!(&output[..n],b"f:note\n");
  v.rename_file("/home/docs/note","/etc/renamed").unwrap();
  assert_eq!(v.read("/home/docs/note",&mut output),Err(FsError::NotFound));
  assert_eq!(v.read("/etc/renamed",&mut output),Ok(5));
  v.unlink("/etc/renamed").unwrap();v.rmdir("/home/docs").unwrap();
  let mut v=HierVfs::mount(v.into_device()).unwrap();
  assert_eq!(v.list_dir("/home",&mut output),Ok(0));
  assert_eq!(v.read("/etc/config",&mut output),Ok(5));
 }
 #[test]fn directory_names_cannot_escape_or_alias_reserved_keys(){
  let mut v=fresh();
  for p in ["/","a","/etc/../secret","/../../boot","/~fhome",
             "/ok~bad","/a:b","/£","/01234567890123456789012345"]{
   assert_eq!(v.mkdir(p),Err(FsError::InvalidName));
  }
  v.mkdir("/etc").unwrap();
  assert_eq!(v.create("/etc/../../../root",b"bad"),Err(FsError::InvalidName));
  let mut tiny=[0xcc;1];
  assert_eq!(v.list_dir("/",&mut tiny),Err(FsError::TooLarge));
  assert_eq!(tiny,[0xcc]);
 }
 #[test]fn atomic_rename_rolls_back_after_torn_metadata(){
  let mut v=fresh();
  v.mkdir("/home").unwrap();v.create("/home/note",b"persist").unwrap();
  let mut disk=v.into_device();
  disk.writes=0;disk.tear=Some(0);
  let mut mounted=HierVfs::mount(disk).unwrap();
  assert_eq!(mounted.rename_file("/home/note","/home/after"),Err(FsError::Io));
  let mut disk=mounted.into_device();disk.tear=None;
  let mut recovered=HierVfs::mount(disk).unwrap();
  let mut data=[0;MAX_TRANSFER];
  assert_eq!(recovered.read("/home/note",&mut data),Ok(7));
  assert_eq!(&data[..7],b"persist");
  assert_eq!(recovered.read("/home/after",&mut data),Err(FsError::NotFound));
 }
 #[test]fn rename_directory_moves_nested_tree_and_survives_remount(){
  let mut v=fresh();
  v.mkdir("/a").unwrap();v.mkdir("/a/b").unwrap();v.mkdir("/dst").unwrap();
  v.create("/a/b/note",b"nested").unwrap();
  v.create("/a/leaf",b"direct").unwrap();
  let before=v.generation();
  v.rename_dir("/a","/dst/z").unwrap();
  assert_eq!(v.generation(),before+1);
  let mut output=[0u8;MAX_TRANSFER];
  assert_eq!(v.list_dir("/a",&mut output),Err(FsError::NotFound));
  assert_eq!(v.read("/dst/z/b/note",&mut output),Ok(6));
  assert_eq!(&output[..6],b"nested");
  assert_eq!(v.read("/dst/z/leaf",&mut output),Ok(6));
  assert_eq!(&output[..6],b"direct");
  let mut v=HierVfs::mount(v.into_device()).unwrap();
  let n=v.list_dir("/dst/z",&mut output).unwrap();
  assert_eq!(&output[..n],b"d:b\nf:leaf\n");
  let n=v.list_dir("/dst/z/b",&mut output).unwrap();
  assert_eq!(&output[..n],b"f:note\n");
 }
 #[test]fn rename_directory_rejects_cycles_collisions_and_long_descendants(){
  let mut v=fresh();
  v.mkdir("/a").unwrap();v.mkdir("/a/b").unwrap();
  v.mkdir("/dst").unwrap();v.mkdir("/dst/used").unwrap();
  v.create("/a/b/c",b"stay").unwrap();
  v.create("/dst/file",b"safe").unwrap();
  let before=v.generation();
  assert_eq!(v.rename_dir("/a","/a/b/c"),Err(FsError::InvalidName));
  assert_eq!(v.rename_dir("/a","/a"),Err(FsError::Exists));
  assert_eq!(v.rename_dir("/a","/dst/used"),Err(FsError::Exists));
  assert_eq!(v.rename_dir("/a","/dst/file"),Err(FsError::Exists));
  assert_eq!(v.rename_dir("/a","/missing/new"),Err(FsError::NotFound));
  assert_eq!(v.rename_dir("/a","/dst/longlonglonglong"),Err(FsError::InvalidName));
  assert_eq!(v.generation(),before);
  let mut output=[0u8;MAX_TRANSFER];
  assert_eq!(v.read("/a/b/c",&mut output),Ok(4));
  assert_eq!(&output[..4],b"stay");
  // Even an orphaned backing entry cannot be overwritten by tree rename.
  v.fs.create_path("/~fdst~z~b~c",b"orphan").unwrap();
  let before=v.generation();
  assert_eq!(v.rename_dir("/a","/dst/z"),Err(FsError::Exists));
  assert_eq!(v.generation(),before);
 }
 #[test]fn torn_rename_metadata_recovers_entire_original_directory_tree(){
  let mut v=fresh();
  v.mkdir("/a").unwrap();v.mkdir("/a/b").unwrap();
  v.create("/a/b/note",b"persistent").unwrap();
  let mut disk=v.into_device();disk.writes=0;disk.tear=Some(0);
  let mut v=HierVfs::mount(disk).unwrap();
  assert_eq!(v.rename_dir("/a","/moved"),Err(FsError::Io));
  let mut disk=v.into_device();disk.tear=None;
  let mut v=HierVfs::mount(disk).unwrap();
  let mut output=[0u8;MAX_TRANSFER];
  assert_eq!(v.read("/a/b/note",&mut output),Ok(10));
  assert_eq!(&output[..10],b"persistent");
  assert_eq!(v.list_dir("/moved",&mut output),Err(FsError::NotFound));
 }
 #[test]fn range_io_two_kib_multisector_reboot_and_legacy_bounds(){
  let mut v=fresh();
  v.mkdir("/etc").unwrap();v.create("/etc/big",b"x").unwrap();
  for chunk in 0..32usize{
   let mut bytes=[0u8;64];
   for (j,b) in bytes.iter_mut().enumerate(){
    *b=((chunk*64+j) as u8).wrapping_mul(17).wrapping_add(3);
   }
   assert_eq!(v.write_at("/etc/big",(chunk*64) as u64,&bytes),Ok(64));
  }
  let mut output=[0xabu8;256];
  assert_eq!(v.read("/etc/big",&mut output),Err(FsError::TooLarge));
  assert_eq!(v.read_at("/etc/big",500,&mut output[..64]),Ok(64));
  for i in 0..64{assert_eq!(output[i],((500+i) as u8).wrapping_mul(17).wrapping_add(3));}
  assert_eq!(v.read_at("/etc/big",2048,&mut output[..64]),Ok(0));
  assert_eq!(v.write_at("/etc/big",512,b"PCH!"),Ok(4));
  let mut v=HierVfs::mount(v.into_device()).unwrap();
  assert_eq!(v.read_at("/etc/big",510,&mut output[..10]),Ok(10));
  assert_eq!(&output[2..6],b"PCH!");
  assert_eq!(v.read_at("/etc/big",1984,&mut output[..64]),Ok(64));
  for i in 0..64{assert_eq!(output[i],((1984+i) as u8).wrapping_mul(17).wrapping_add(3));}
  assert_eq!(v.read_at("/etc/big",2049,&mut output[..4]),Err(FsError::TooLarge));
 }
 #[test]fn range_io_rejects_holes_overflow_and_keeps_original_file(){
  let mut v=fresh();
  v.mkdir("/etc").unwrap();v.create("/etc/note",b"abc").unwrap();
  let before=v.generation();
  assert_eq!(v.write_at("/etc/note",4,b"X"),Err(FsError::TooLarge));
  assert_eq!(v.write_at("/etc/note",2048,b"X"),Err(FsError::TooLarge));
  assert_eq!(v.write_at("/etc/note",u64::MAX,b"X"),Err(FsError::TooLarge));
  assert_eq!(v.write_at("/etc/note",0,b""),Err(FsError::TooLarge));
  assert_eq!(v.read_at("/etc/note",u64::MAX,&mut [0u8;8]),Err(FsError::TooLarge));
  assert_eq!(v.write_at("/etc/missing",0,b"X"),Err(FsError::NotFound));
  assert_eq!(v.generation(),before);
  let mut out=[0u8;8];
  assert_eq!(v.read_at("/etc/note",0,&mut out),Ok(3));
  assert_eq!(&out[..3],b"abc");
  assert_eq!(v.write_at("/etc/note",3,b"def"),Ok(3));
  assert_eq!(v.read_at("/etc/note",1,&mut out),Ok(5));
  assert_eq!(&out[..5],b"bcdef");
 }
 #[test]fn failed_range_write_recovers_whole_old_content_after_reboot(){
  let mut v=fresh();
  v.mkdir("/etc").unwrap();v.create("/etc/note",b"original").unwrap();
  let mut disk=v.into_device();disk.writes=0;disk.tear=Some(2);
  let mut v=HierVfs::mount(disk).unwrap();
  assert_eq!(v.write_at("/etc/note",2,b"damaged"),Err(FsError::Io));
  let mut disk=v.into_device();disk.tear=None;
  let mut v=HierVfs::mount(disk).unwrap();
  let mut out=[0u8;32];
  assert_eq!(v.read_at("/etc/note",0,&mut out),Ok(8));
  assert_eq!(&out[..8],b"original");
 }
 #[test]fn latest_metadata_corruption_falls_back_to_previous_cow_copy(){
  let mut v=fresh();v.mkdir("/etc").unwrap();
  v.create("/etc/config",b"safe").unwrap();
  v.overwrite("/etc/config",b"safe").unwrap();
  let mut disk=v.into_device();
  let a=u64::from_le_bytes(disk.blocks[0][16..24].try_into().unwrap());
  let b=u64::from_le_bytes(disk.blocks[1][16..24].try_into().unwrap());
  let active=if a>=b{0}else{1};
  disk.blocks[active][24]^=0xff;
  let mut mounted=HierVfs::mount(disk).unwrap();
  let mut data=[0;MAX_TRANSFER];
  assert_eq!(mounted.read("/etc/config",&mut data),Ok(4));
  assert_eq!(&data[..4],b"safe");
 }

 #[test]fn complete_multi_chunk_file_copy_survives_device_remount(){
  let mut fs=fresh();fs.mkdir("/home").unwrap();
  let mut content=[0u8;700];
  for (i,b) in content.iter_mut().enumerate(){*b=(i%251) as u8;}
  fs.create("/home/source",&content).unwrap();
  fs.copy_file("/home/source","/home/copy").unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let mut read=[0u8;700];
  assert_eq!(fs.read("/home/copy",&mut read),Ok(700));
  assert_eq!(read,content);
  assert_eq!(fs.copy_file("/home/source","/home/copy"),Err(FsError::Exists));
  fs.overwrite("/home/copy",b"independent").unwrap();
  assert_eq!(fs.read("/home/source",&mut read),Ok(700));
  assert_eq!(read,content);
 }
 #[test]fn missing_source_and_missing_parent_do_not_publish_copy_metadata(){
  let mut fs=fresh();
  fs.create("/original",b"stable").unwrap();
  let old=fs.generation();
  assert_eq!(fs.copy_file("/absent","/copy"),Err(FsError::NotFound));
  assert_eq!(fs.copy_file("/original","/absent/copy"),Err(FsError::NotFound));
  assert_eq!(fs.copy_file("/original","/original"),Err(FsError::Exists));
  assert_eq!(fs.generation(),old);
  let mut output=[0u8;16];
  assert_eq!(fs.read("/original",&mut output),Ok(6));
  assert_eq!(&output[..6],b"stable");
 }


 #[test]fn full_file_replace_survives_reboot_without_truncating_source(){
  let mut fs=fresh();fs.create("/dst",b"old").unwrap();
  let mut payload=[0u8;600];
  for (i,p) in payload.iter_mut().enumerate(){*p=(i%200) as u8;}
  fs.create("/src",&payload).unwrap();
  fs.copy_replace("/src","/dst").unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let mut output=[0u8;600];
  assert_eq!(fs.read("/dst",&mut output),Ok(600));
  assert_eq!(output,payload);
  assert_eq!(fs.read("/src",&mut output),Ok(600));
  assert_eq!(output,payload);
 }
 #[test]fn absent_copy_replacement_does_not_create_file_or_mutate_generation(){
  let mut fs=fresh();fs.create("/src",b"data").unwrap();
  let before=fs.generation();
  assert_eq!(fs.copy_replace("/src","/missing"),Err(FsError::NotFound));
  assert_eq!(fs.copy_replace("/src","/src"),Err(FsError::Exists));
  assert_eq!(fs.generation(),before);
 }

 #[test]fn r009_copy_dispatch_persists_and_replaces_complete_files(){
  let mut v=fresh();
  v.mkdir("/home").unwrap();
  v.create("/home/src",b"hello-data").unwrap();
  let mut output=[0u8;MAX_TRANSFER];
  assert_eq!(v.dispatch(10,"/home/src","/home/copy",&mut output,0),Ok(0));
  assert_eq!(v.read("/home/copy",&mut output),Ok(10));
  assert_eq!(&output[..10],b"hello-data");
  v.overwrite("/home/src",b"upgraded").unwrap();
  assert_eq!(v.dispatch(11,"/home/src","/home/copy",&mut output,0),Ok(0));
  let mut v=HierVfs::mount(v.into_device()).unwrap();
  assert_eq!(v.read("/home/copy",&mut output),Ok(8));
  assert_eq!(&output[..8],b"upgraded");
 }
 #[test]fn r009_exact_read_never_exposes_a_partial_result(){
  let mut v=fresh();v.mkdir("/home").unwrap();
  v.create("/home/note",b"abc").unwrap();
  let mut out=[0xaa;5];
  assert_eq!(v.read_exact_range("/home/note",0,&mut out),Err(FsError::TooLarge));
  assert_eq!(out,[0xaa;5]);
  assert_eq!(v.read_exact_range("/home/note",0,&mut out[..3]),Ok(()));
  assert_eq!(&out[..3],b"abc");
 }
 #[test]fn r009_generation_guard_blocks_stale_writer(){
  let mut v=fresh();v.mkdir("/home").unwrap();
  v.create("/home/note",b"hello").unwrap();
  let before=v.generation();
  assert_eq!(v.overwrite_chunk("/home/note",0,before-1,b"bad"),
    Err(FsError::InvalidVolume));
  assert_eq!(v.overwrite_chunk("/home/note",0,before,b"ok").unwrap().0,2);
  let mut out=[0u8;8];
  assert_eq!(v.read("/home/note",&mut out),Ok(5));
  assert_eq!(&out[..5],b"okllo");
 }
 #[test]fn r010_snapshot_restore_survives_remount(){
  let mut fs=fresh();fs.mkdir("/home").unwrap();
  fs.create("/home/data",b"before").unwrap();
  let first=fs.generation();
  let backup_gen=fs.snapshot_checked("/home/data","/home/backup",first).unwrap();
  assert!(backup_gen>first);
  fs.overwrite("/home/data",b"after").unwrap();
  let now=fs.generation();
  let restored=fs.restore_snapshot("/home/backup","/home/data",now).unwrap();
  assert!(restored>now);
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let mut output=[0u8;32];
  assert_eq!(fs.read("/home/data",&mut output),Ok(6));
  assert_eq!(&output[..6],b"before");
 }
 #[test]fn r010_stale_restore_fails_without_mutation(){
  let mut fs=fresh();fs.mkdir("/home").unwrap();
  fs.create("/home/data",b"original").unwrap();
  let before=fs.generation();
  assert_eq!(fs.snapshot_checked("/home/data","/home/old",before-1),
    Err(FsError::InvalidVolume));
  assert_eq!(fs.generation(),before);
  let mut output=[0u8;16];
  assert_eq!(fs.read("/home/old",&mut output),Err(FsError::NotFound));
 }
 #[test]fn r010_verified_copy_checks_complete_payload(){
  let mut fs=fresh();
  let mut data=[0u8;700];for (i,b) in data.iter_mut().enumerate(){*b=(i%251) as u8;}
  fs.create("/src",&data).unwrap();
  let next=fs.copy_verified("/src","/dst",fs.generation()).unwrap();
  assert_eq!(next,fs.generation());
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let mut output=[0u8;700];
  assert_eq!(fs.read("/dst",&mut output),Ok(700));
  assert_eq!(output,data);
 }

}

/// R009: bounded seek/read/persist abstraction for authenticated file
/// services. Exposes existing atomic COW range operations while ensuring
/// no successful prefix is falsely reported on a short verified read.
impl<D:BlockDevice> HierVfs<D>{
 pub fn read_exact_range(&mut self,path:&str,offset:u64,
   output:&mut[u8])->Result<(),FsError>{
  if output.is_empty()||output.len()>MAX_TRANSFER{return Err(FsError::TooLarge);}
  let mut staged=[0u8;MAX_TRANSFER];
  let size=self.read_at(path,offset,&mut staged[..output.len()])?;
  if size!=output.len(){return Err(FsError::TooLarge);}
  output.copy_from_slice(&staged[..size]);Ok(())
 }
 pub fn overwrite_chunk(&mut self,path:&str,offset:u64,
   expected_generation:u64,chunk:&[u8])->Result<(usize,u64),FsError>{
  if self.generation()!=expected_generation{return Err(FsError::InvalidVolume);}
  let n=self.write_at(path,offset,chunk)?;
  Ok((n,self.generation()))
 }

}

 
/// R010 immutable-consumer snapshot and crash-safe restore primitives.
/// Snapshot allocation and restore both use the existing CRC-verified
/// alternate-bank filesystem commit, not partial block writes.
impl<D:BlockDevice> HierVfs<D>{
 pub fn snapshot_checked(&mut self,source:&str,backup:&str,
  generation:u64)->Result<u64,FsError>{
  if self.generation()!=generation{return Err(FsError::InvalidVolume);}
  self.copy_file(source,backup)?;
  Ok(self.generation())
 }
 pub fn restore_snapshot(&mut self,backup:&str,destination:&str,
  generation:u64)->Result<u64,FsError>{
  if self.generation()!=generation{return Err(FsError::InvalidVolume);}
  self.copy_replace(backup,destination)?;
  Ok(self.generation())
 }
 pub fn copy_verified(&mut self,source:&str,target:&str,
  generation:u64)->Result<u64,FsError>{
  if self.generation()!=generation{return Err(FsError::InvalidVolume);}
  // Verify the source's CRC and complete length before reserving
  // destination metadata. The full-file API never truncates silently.
  let mut original=[0u8;MAX_FILE_BYTES];
  let n=self.read(source,&mut original)?;
  self.copy_file(source,target)?;
  let mut verify=[0u8;MAX_FILE_BYTES];
  let stored=self.read(target,&mut verify)?;
  if stored!=n||verify[..stored]!=original[..n]{
   return Err(FsError::InvalidVolume);
  }
  Ok(self.generation())
 }
}

 
/// R011 full-file bounded COW transaction: validate every chunk first,
/// publish exactly one durable bank after all staged mutations succeed.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct RangeChange<'a>{pub offset:u64,pub bytes:&'a[u8]}
impl<D:BlockDevice> HierVfs<D>{
 pub fn write_transaction(&mut self,path:&str,expected_generation:u64,
  changes:&[RangeChange<'_>])->Result<u64,FsError>{
  if changes.is_empty()||self.generation()!=expected_generation{
   return Err(FsError::InvalidVolume);
  }
  let mut staged=[0u8;MAX_FILE_BYTES];
  let existing=self.read(path,&mut staged)?;
  let mut size=existing;
  for change in changes{
   if change.bytes.is_empty()||change.bytes.len()>MAX_TRANSFER||
      change.offset>size as u64{return Err(FsError::TooLarge);}
   let from=usize::try_from(change.offset).map_err(|_|FsError::TooLarge)?;
   let end=from.checked_add(change.bytes.len()).ok_or(FsError::TooLarge)?;
   if end>MAX_FILE_BYTES{return Err(FsError::TooLarge);}
   staged[from..end].copy_from_slice(change.bytes);
   size=size.max(end);
  }
  // One COW metadata publication for the entire requested batch.
  self.overwrite(path,&staged[..size])?;
  Ok(self.generation())
 }
 pub fn append_transaction(&mut self,path:&str,expected_generation:u64,
  chunks:&[&[u8]])->Result<u64,FsError>{
  if chunks.is_empty()||self.generation()!=expected_generation{
   return Err(FsError::InvalidVolume);
  }
  let mut staged=[0u8;MAX_FILE_BYTES];
  let mut size=self.read(path,&mut staged)?;
  for &chunk in chunks{
   if chunk.is_empty()||chunk.len()>MAX_TRANSFER{return Err(FsError::TooLarge);}
   let end=size.checked_add(chunk.len()).ok_or(FsError::TooLarge)?;
   if end>MAX_FILE_BYTES{return Err(FsError::TooLarge);}
   staged[size..end].copy_from_slice(chunk);size=end;
  }
  self.overwrite(path,&staged[..size])?;
  Ok(self.generation())
 }
}
#[cfg(test)]
mod r011_transaction_tests{
 use super::*;
 use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};
 const COUNT:usize=100;
 #[derive(Clone)]
 struct Disk{sectors:[[u8;SECTOR_BYTES];COUNT]}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{COUNT as u64}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*data;Ok(())
  }
 }
 fn ready()->HierVfs<Disk>{
  let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];COUNT]}).unwrap();
  fs.create("/app",b"12345").unwrap();fs
 }
 #[test]fn two_updates_commit_once_and_survive_remount(){
  let mut fs=ready();let before=fs.generation();
  let after=fs.write_transaction("/app",before,&[
   RangeChange{offset:1,bytes:b"ab"},RangeChange{offset:5,bytes:b"Z"}]).unwrap();
  assert_eq!(after,before+1);
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let mut out=[0u8;16];assert_eq!(fs.read("/app",&mut out),Ok(6));
  assert_eq!(&out[..6],b"1ab45Z");
 }
 #[test]fn invalid_second_chunk_never_publishes_first_chunk(){
  let mut fs=ready();let before=fs.generation();
  assert_eq!(fs.write_transaction("/app",before,&[
    RangeChange{offset:0,bytes:b"AAA"},
    RangeChange{offset:99,bytes:b"bad"}]),Err(FsError::TooLarge));
  assert_eq!(fs.generation(),before);
  let mut out=[0u8;10];assert_eq!(fs.read("/app",&mut out),Ok(5));
  assert_eq!(&out[..5],b"12345");
 }
 #[test]fn atomic_append_maintains_generation_and_rejects_stale_writer(){
  let mut fs=ready();let before=fs.generation();
  fs.append_transaction("/app",before,&[b"678",b"9"]).unwrap();
  assert_eq!(fs.append_transaction("/app",before,&[b"0"]),Err(FsError::InvalidVolume));
  let mut out=[0u8;16];assert_eq!(fs.read("/app",&mut out),Ok(9));
  assert_eq!(&out[..9],b"123456789");
 }
}


/// R012: durable checkpoint/restore for the current single-bank-sized file.
/// The existing COW filesystem persists the snapshot and target separately;
/// this is not yet an atomic multi-file transaction on disk.
impl<D:BlockDevice> HierVfs<D>{
 pub fn checkpoint(&mut self,path:&str,backup:&str,
   expected:u64)->Result<u64,FsError>{
  if self.generation()!=expected{return Err(FsError::InvalidVolume);}
  self.copy_file(path,backup)?;
  Ok(self.generation())
 }
 pub fn recover_checkpoint(&mut self,path:&str,backup:&str,
   expected:u64)->Result<u64,FsError>{
  if self.generation()!=expected{return Err(FsError::InvalidVolume);}
  let mut verified=[0u8;MAX_FILE_BYTES];
  let count=self.read(backup,&mut verified)?;
  // Re-read the checkpoint and verify its exact CRC-covered byte extent
  // before replacing the destination with one COW metadata publication.
  let mut repeated=[0u8;MAX_FILE_BYTES];
  if self.read(backup,&mut repeated)?!=count||
     verified[..count]!=repeated[..count]{
   return Err(FsError::InvalidVolume);
  }
  self.copy_replace(backup,path)?;
  Ok(self.generation())
 }
 pub fn read_generation_checked(&mut self,path:&str,expected:u64,
  output:&mut[u8])->Result<usize,FsError>{
  if self.generation()!=expected{return Err(FsError::InvalidVolume);}
  let n=self.read(path,output)?;
  if self.generation()!=expected{return Err(FsError::InvalidVolume);}
  Ok(n)
 }
}
#[cfg(test)]
mod r012_checkpoint_tests{
 use super::*;use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};
 const COUNT:usize=100;
 #[derive(Clone)]
 struct Disk{sectors:[[u8;SECTOR_BYTES];COUNT]}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{COUNT as u64}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*data;Ok(())
  }
 }
 fn disk()->HierVfs<Disk>{
  let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];COUNT]}).unwrap();
  fs.create("/app",b"before").unwrap();fs
 }
 #[test]fn checkpoint_restores_persisted_file_after_reboot(){
  let mut fs=disk();let gen=fs.generation();
  fs.checkpoint("/app","/backup",gen).unwrap();
  fs.overwrite("/app",b"changed").unwrap();
  let gen=fs.generation();
  fs.recover_checkpoint("/app","/backup",gen).unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let mut out=[0u8;16];assert_eq!(fs.read("/app",&mut out),Ok(6));
  assert_eq!(&out[..6],b"before");
 }
 #[test]fn stale_generation_never_overwrites_valid_volume(){
  let mut fs=disk();let gen=fs.generation();let mut out=[0xee;8];
  assert_eq!(fs.checkpoint("/app","/backup",gen-1),Err(FsError::InvalidVolume));
  assert_eq!(fs.read_generation_checked("/app",gen-1,&mut out),
    Err(FsError::InvalidVolume));
  assert_eq!(out,[0xee;8]);
  assert_eq!(fs.generation(),gen);
 }
}

/// R013 checkpoint lifecycle: refresh an existing backup through the
/// CRC-verified COW writer. Each file is individually crash-safe; a full
/// multi-file metadata journal is a separate kernel milestone.
impl<D:BlockDevice> HierVfs<D>{
 pub fn refresh_checkpoint(&mut self,source:&str,backup:&str,
  expected:u64)->Result<u64,FsError>{
  if self.generation()!=expected{return Err(FsError::InvalidVolume);}
  if source==backup{return Err(FsError::Exists);}
  let mut buf=[0u8;MAX_FILE_BYTES];
  self.read(source,&mut buf)?;
  match self.read(backup,&mut buf){
   Ok(_)=>self.copy_replace(source,backup)?,
   Err(FsError::NotFound)=>self.copy_file(source,backup)?,
   Err(err)=>return Err(err),
  }
  Ok(self.generation())
 }
 pub fn drop_checkpoint(&mut self,path:&str,expected:u64)
   ->Result<u64,FsError>{
  if self.generation()!=expected{return Err(FsError::InvalidVolume);}
  self.unlink(path)?;
  Ok(self.generation())
 }
 pub fn recover_if_changed(&mut self,source:&str,backup:&str,
   expected:u64)->Result<(u64,bool),FsError>{
  if self.generation()!=expected{return Err(FsError::InvalidVolume);}
  let mut existing=[0u8;MAX_FILE_BYTES];
  let n=self.read(source,&mut existing)?;
  let mut saved=[0u8;MAX_FILE_BYTES];
  let m=self.read(backup,&mut saved)?;
  if n==m&&existing[..n]==saved[..m]{return Ok((expected,false));}
  self.copy_replace(backup,source)?;
  Ok((self.generation(),true))
 }
}
#[cfg(test)]
mod r013_checkpoint_lifecycle_tests{
 use super::*;use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};
 const COUNT:usize=100;
 #[derive(Clone)]struct Disk{blocks:[[u8;SECTOR_BYTES];COUNT]}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{COUNT as u64}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
    ->Result<(),BlockError>{
   *out=*self.blocks.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
    ->Result<(),BlockError>{
   *self.blocks.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*data;Ok(())
  }
 }
 fn disk()->HierVfs<Disk>{
  let mut fs=HierVfs::format_blank(Disk{blocks:[[0u8;SECTOR_BYTES];COUNT]}).unwrap();
  fs.create("/a",b"one").unwrap();fs
 }
 #[test]fn refreshed_checkpoint_recovers_after_disk_remount(){
  let mut fs=disk();let g=fs.generation();
  fs.refresh_checkpoint("/a","/bak",g).unwrap();
  fs.overwrite("/a",b"two").unwrap();
  let g=fs.generation();fs.refresh_checkpoint("/a","/bak",g).unwrap();
  fs.overwrite("/a",b"bad").unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let g=fs.generation();
  assert_eq!(fs.recover_if_changed("/a","/bak",g).unwrap().1,true);
  let mut out=[0;8];assert_eq!(fs.read("/a",&mut out),Ok(3));
  assert_eq!(&out[..3],b"two");
 }
 #[test]fn equal_snapshot_skips_write_and_preserves_generation(){
  let mut fs=disk();let g=fs.generation();
  fs.refresh_checkpoint("/a","/bak",g).unwrap();
  let now=fs.generation();
  assert_eq!(fs.recover_if_changed("/a","/bak",now),Ok((now,false)));
  assert_eq!(fs.generation(),now);
 }
 #[test]fn stale_generation_cannot_discard_snapshot(){
  let mut fs=disk();let before=fs.generation();
  fs.refresh_checkpoint("/a","/bak",before).unwrap();
  assert_eq!(fs.drop_checkpoint("/bak",before),Err(FsError::InvalidVolume));
  let g=fs.generation();fs.drop_checkpoint("/bak",g).unwrap();
  assert_eq!(fs.read("/bak",&mut[0;4]),Err(FsError::NotFound));
 }
}

/// R014 avoid generating unnecessary durable bank writes for unchanged
/// application state; use one generation-fenced COW commit for real changes.
impl<D:BlockDevice> HierVfs<D>{
 pub fn overwrite_if_changed(&mut self,path:&str,expected:u64,
   content:&[u8])->Result<(u64,bool),FsError>{
  if self.generation()!=expected{return Err(FsError::InvalidVolume);}
  if content.len()>MAX_FILE_BYTES{return Err(FsError::TooLarge);}
  let mut existing=[0u8;MAX_FILE_BYTES];
  let n=self.read(path,&mut existing)?;
  if n==content.len()&&existing[..n]==*content{return Ok((expected,false));}
  self.overwrite(path,content)?;
  Ok((self.generation(),true))
 }
 /// Verify a named recovery copy before changing any destination metadata.
 pub fn restore_verified_if_different(&mut self,destination:&str,
   backup:&str,expected:u64)->Result<(u64,bool),FsError>{
  if self.generation()!=expected||destination==backup{
   return Err(FsError::InvalidVolume);
  }
  let mut saved=[0u8;MAX_FILE_BYTES];
  let n=self.read(backup,&mut saved)?;
  let (gen,changed)=self.overwrite_if_changed(destination,expected,&saved[..n])?;
  Ok((gen,changed))
 }
 pub fn read_two_checked(&mut self,first:&str,second:&str,expected:u64,
   a:&mut[u8],b:&mut[u8])->Result<(usize,usize),FsError>{
  if self.generation()!=expected{return Err(FsError::InvalidVolume);}
  let n=self.read(first,a)?;
  let m=self.read(second,b)?;
  if self.generation()!=expected{return Err(FsError::InvalidVolume);}
  Ok((n,m))
 }
}
#[cfg(test)]
mod r014_durable_idempotency_tests{
 use super::*;use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};
 const COUNT:usize=100;
 #[derive(Clone)]struct Device{sectors:[[u8;SECTOR_BYTES];COUNT]}
 impl BlockDevice for Device{
  fn sector_count(&self)->u64{COUNT as u64}
  fn read_sector(&mut self,n:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.sectors.get(n as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,n:u64,buffer:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.sectors.get_mut(n as usize).ok_or(BlockError::OutOfRange)?=*buffer;Ok(())
  }
 }
 fn fresh()->HierVfs<Device>{
  let mut fs=HierVfs::format_blank(Device{sectors:[[0;SECTOR_BYTES];COUNT]}).unwrap();
  fs.create("/data",b"hello").unwrap();fs
 }
 #[test]fn repeated_identical_write_skips_flash_wear_and_generation(){
  let mut fs=fresh();let g=fs.generation();
  assert_eq!(fs.overwrite_if_changed("/data",g,b"hello"),Ok((g,false)));
  assert_eq!(fs.generation(),g);
  let (next,changed)=fs.overwrite_if_changed("/data",g,b"world").unwrap();
  assert!(changed);assert_eq!(next,g+1);
 }
 #[test]fn cold_remount_restores_backup_and_skips_second_restore(){
  let mut fs=fresh();fs.checkpoint("/data","/backup",fs.generation()).unwrap();
  fs.overwrite("/data",b"bad").unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let g=fs.generation();
  assert_eq!(fs.restore_verified_if_different("/data","/backup",g),
    Ok((g+1,true)));
  let g=fs.generation();assert_eq!(fs.restore_verified_if_different(
    "/data","/backup",g),Ok((g,false)));
 }
 #[test]fn stale_writer_cannot_overwrite_or_read_as_current_generation(){
  let mut fs=fresh();let stale=fs.generation();
  fs.overwrite("/data",b"new").unwrap();
  assert_eq!(fs.overwrite_if_changed("/data",stale,b"old"),
    Err(FsError::InvalidVolume));
  let mut a=[0;8];let mut b=[0;8];
  assert_eq!(fs.read_two_checked("/data","/data",stale,&mut a,&mut b),
    Err(FsError::InvalidVolume));
 }
}


/// R016: pair reads must be atomic to the caller even if the second
/// path is absent, the generation is stale, or the output is too short.
impl<D:BlockDevice> HierVfs<D>{
 pub fn read_pair_atomic(&mut self,first:&str,second:&str,expected:u64,
  a_out:&mut[u8],b_out:&mut[u8])->Result<(usize,usize),FsError>{
  if self.generation()!=expected{return Err(FsError::InvalidVolume);}
  let mut a=[0u8;MAX_FILE_BYTES];let mut b=[0u8;MAX_FILE_BYTES];
  let n=self.read(first,&mut a)?;let m=self.read(second,&mut b)?;
  if self.generation()!=expected||n>a_out.len()||m>b_out.len(){
   return Err(FsError::InvalidVolume);
  }
  a_out[..n].copy_from_slice(&a[..n]);
  b_out[..m].copy_from_slice(&b[..m]);Ok((n,m))
 }
 pub fn verify_recovery_copy(&mut self,source:&str,backup:&str,
  expected:u64)->Result<bool,FsError>{
  if source==backup||self.generation()!=expected{
   return Err(FsError::InvalidVolume);
  }
  let mut a=[0;MAX_FILE_BYTES];let mut b=[0;MAX_FILE_BYTES];
  let (n,m)=self.read_pair_atomic(source,backup,expected,&mut a,&mut b)?;
  Ok(n==m&&a[..n]==b[..m])
 }
}
#[cfg(test)]mod r016_vfs_atomic_pair_tests{
 use super::*;use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};
 #[derive(Clone)]struct Disk([[u8;SECTOR_BYTES];100]);
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
  ->Result<(),BlockError>{
   *out=*self.0.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
  ->Result<(),BlockError>{
   *self.0.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*data;Ok(())
  }
 }
 fn setup()->HierVfs<Disk>{
  let mut f=HierVfs::format_blank(Disk([[0;SECTOR_BYTES];100])).unwrap();
  f.create("/a",b"alpha").unwrap();f.create("/b",b"beta").unwrap();f
 }
 #[test]fn valid_pair_returns_both_files(){
  let mut f=setup();let g=f.generation();let mut a=[0;8];let mut b=[0;8];
  assert_eq!(f.read_pair_atomic("/a","/b",g,
    &mut a,&mut b),Ok((5,4)));
  assert_eq!(&a[..5],b"alpha");assert_eq!(&b[..4],b"beta");
 }
 #[test]fn failed_second_read_does_not_expose_first(){
  let mut f=setup();let g=f.generation();let mut a=[9;8];let mut b=[8;8];
  assert!(f.read_pair_atomic("/a","/missing",g,
    &mut a,&mut b).is_err());
  assert_eq!(a,[9;8]);assert_eq!(b,[8;8]);
 }
 #[test]fn stale_and_short_output_fail_before_copy(){
  let mut f=setup();let g=f.generation();let mut short=[7u8;1];let mut b=[8;8];
  assert!(f.read_pair_atomic("/a","/b",g,
    &mut short,&mut b).is_err());
  assert_eq!(short,[7]);assert_eq!(b,[8;8]);
  assert!(f.read_pair_atomic("/a","/b",g+1,
    &mut short,&mut b).is_err());
 }
}


/// R017 bounded project-file content verification. The caller can compare
/// two durable files without exposing either into a partially filled result.
impl<D:BlockDevice> HierVfs<D>{
 pub fn checksum_file_at(&mut self,path:&str,expected:u64)
  ->Result<(usize,u32),FsError>{
  if self.generation()!=expected{return Err(FsError::InvalidVolume);}
  let mut data=[0u8;MAX_FILE_BYTES];
  let len=self.read(path,&mut data)?;
  if self.generation()!=expected{return Err(FsError::InvalidVolume);}
  let mut h=0x811c9dc5u32;
  for &byte in &data[..len]{h=(h^(byte as u32)).wrapping_mul(0x01000193);}
  Ok((len,h))
 }
 pub fn project_files_equal(&mut self,a:&str,b:&str,expected:u64)
  ->Result<bool,FsError>{
  if a==b{return Err(FsError::InvalidVolume);}
  let mut left=[0u8;MAX_FILE_BYTES];let mut right=[0u8;MAX_FILE_BYTES];
  let (an,bn)=self.read_pair_atomic(a,b,expected,&mut left,&mut right)?;
  Ok(an==bn&&left[..an]==right[..bn])
 }
}
#[cfg(test)]mod r017_project_verification_tests{
 use super::*;use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};
 struct Disk([[u8;SECTOR_BYTES];100]);
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,l:u64,b:&mut[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{*b=*self.0.get(l as usize).ok_or(BlockError::OutOfRange)?;Ok(())}
  fn write_sector(&mut self,l:u64,b:&[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{*self.0.get_mut(l as usize).ok_or(BlockError::OutOfRange)?=*b;Ok(())}
 }
 fn fs()->HierVfs<Disk>{
  let mut f=HierVfs::format_blank(Disk([[0;SECTOR_BYTES];100])).unwrap();
  f.create("/one",b"code").unwrap();f.create("/two",b"code").unwrap();f
 }
 #[test]fn checksums_and_equal_copies_are_consistent(){
  let mut f=fs();let g=f.generation();
  assert_eq!(f.project_files_equal("/one","/two",g),Ok(true));
  assert_eq!(f.checksum_file_at("/one",g).unwrap().0,4);
 }
 #[test]fn differing_files_do_not_compare_equal(){
  let mut f=fs();f.overwrite("/two",b"new").unwrap();
  let g=f.generation();
  assert_eq!(f.project_files_equal("/one","/two",g),Ok(false));
 }
 #[test]fn revision_mismatch_refuses_snapshot(){
  let mut f=fs();
  let g=f.generation();
  assert!(f.checksum_file_at("/one",g+1).is_err());
  assert!(f.project_files_equal("/one","/one",g).is_err());
 }
}

#[cfg(test)]
mod m1_r046_vfs_redundancy_integration_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 #[derive(Clone)]
 struct Disk{data:[[u8;SECTOR_BYTES];100],writes:usize}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,from:&[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*from;
   self.writes+=1;Ok(())
  }
 }
 fn fresh()->Disk{Disk{data:[[0u8;SECTOR_BYTES];100],writes:0}}
 fn committed()->Disk{
  let mut fs=HierVfs::format_blank(fresh()).unwrap();
  fs.mkdir("/home").unwrap();
  fs.create("/home/note",b"verified").unwrap();
  fs.into_device()
 }
 #[test]fn m1_r046_vfs_strict_boot_refuses_degraded_but_mount_can_inspect(){
  let mut disk=committed();
  disk.data[1][24]^=1;
  let before=disk.writes;
  assert_eq!(HierVfs::<Disk>::probe_device(&mut disk).unwrap().valid_copies,1);
  assert!(matches!(HierVfs::mount_healthy(disk.clone()),Err(FsError::InvalidVolume)));
  let mut fs=HierVfs::mount(disk).unwrap();
  let mut out=[0u8;32];
  assert_eq!(fs.read("/home/note",&mut out),Ok(8));
  assert_eq!(&out[..8],b"verified");
  assert_eq!(fs.mounted_health().unwrap().valid_copies,1);
  assert_eq!(fs.into_device().writes,before);
 }
 #[test]fn m1_r046_vfs_repair_restores_user_directory_after_remount(){
  let mut disk=committed();
  disk.data[1][24]^=1;
  let previous=disk.data;
  let mut fs=HierVfs::mount(disk).unwrap();
  assert_eq!(fs.repair_redundancy(),Ok(true));
  let disk=fs.into_device();
  assert_eq!(&disk.data[2..],&previous[2..]);
  let mut fs=HierVfs::mount_healthy(disk).unwrap();
  let mut names=[0u8;256];
  let n=fs.list_dir("/home",&mut names).unwrap();
  assert_eq!(&names[..n],b"f:note\n");
  let mut out=[0u8;16];
  assert_eq!(fs.read("/home/note",&mut out),Ok(8));
  assert_eq!(&out[..8],b"verified");
 }
 #[test]fn m1_r046_vfs_repair_does_not_touch_healthy_generations(){
  let mut disk=committed();
  let before=disk.writes;
  let snapshot=disk.data;
  let mut fs=HierVfs::mount_healthy(disk).unwrap();
  assert_eq!(fs.repair_redundancy(),Ok(false));
  let disk=fs.into_device();
  assert_eq!(disk.writes,before);
  assert_eq!(disk.data,snapshot);
 }
 #[test]fn m1_r046_vfs_corrupt_both_metadata_heads_refuses_repair_or_mount(){
  let mut disk=committed();
  disk.data[0][24]^=1;
  disk.data[1][24]^=1;
  let before=disk.writes;
  assert_eq!(HierVfs::<Disk>::probe_device(&mut disk),
   Err(FsError::InvalidVolume));
  assert!(matches!(HierVfs::mount(disk.clone()),Err(FsError::InvalidVolume)));
  assert!(matches!(HierVfs::mount_healthy(disk.clone()),Err(FsError::InvalidVolume)));
  assert_eq!(disk.writes,before);
 }
}
