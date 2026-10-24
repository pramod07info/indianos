//! R031.3 bounded hierarchical VFS over R027's durable COW metadata.
//! No on-disk format changes, allocation, recursive traversal, or symlinks.
//! Existing root /app and /note remain separate from reserved /~[df]* keys.
//! 8 total storage objects (directories count); max 21 bytes of logical path.
use crate::block::{BlockDevice,BlockWindow};
use crate::durablefs::{DurableFs,FsError,MIN_SECTORS};
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
 pub fn mount(disk:D)->Result<Self,FsError>{
  Ok(Self{fs:DurableFs::mount(disk)?})
 }
 pub fn format_blank(disk:D)->Result<Self,FsError>{
  Ok(Self{fs:DurableFs::format_blank(disk)?})
 }
 pub fn into_device(self)->D{self.fs.into_device()}
 pub fn generation(&self)->u64{self.fs.generation()}
 fn directory(&mut self,p:&str)->Result<(),FsError>{
  if p=="/"{return Ok(());}
  let k=checked(p,Kind::Directory)?;
  let mut buf=[0u8;8];
  if self.fs.read_path(k.name(),&mut buf)?!=8 ||&buf!=DIR_MARKER{
   return Err(FsError::InvalidVolume);
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
  self.fs.read_path(p.name(),out)
 }
 pub fn overwrite(&mut self,path:&str,data:&[u8])->Result<(),FsError>{
  let p=checked(path,Kind::File)?;
  self.parents(&p)?;
  self.fs.overwrite_path(p.name(),data)
 }
 pub fn unlink(&mut self,path:&str)->Result<(),FsError>{
  let p=checked(path,Kind::File)?;
  self.parents(&p)?;
  self.fs.delete_path(p.name())
 }
 pub fn rename_file(&mut self,source:&str,target:&str)->Result<(),FsError>{
  let from=checked(source,Kind::File)?;
  let to=checked(target,Kind::File)?;
  self.parents(&from)?;self.parents(&to)?;
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
  if path!="/"{self.directory(path)?;}
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
}
