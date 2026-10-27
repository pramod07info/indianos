//! R041: fixed-capacity, allocation-free guest RAM filesystem.
//! Implements the real read/replace contracts used by Batch 5/6.
//! Atomic in-process replacement; RAM IS NOT persistent after reboot.
//! The caller supplies validated canonical absolute paths and authenticates
//! the requesting PID. Durable block storage remains a separate backend.
use crate::batch5_vfs::GuestVfs;
use crate::batch6_storage::AtomicFileBackend;
pub const PATH_MAX:usize=96;
pub const CONTENT_MAX:usize=2048;
#[derive(Clone,Copy)]
struct File {
 path:[u8;PATH_MAX],
 path_len:usize,
 data:[u8;CONTENT_MAX],
 data_len:usize,
 version:u64,
}
pub struct RamFs<const N:usize>{
 files:[Option<File>;N],
 version:u64,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Full,NotFound,OutputTooSmall,Exhausted}
impl<const N:usize> RamFs<N>{
 pub const fn new()->Self{Self{files:[None;N],version:0}}
 fn valid_path(path:&[u8])->bool{
  path.len()>=2&&path.len()<=PATH_MAX&&path[0]==b'/'&&
  path[path.len()-1]!=b'/'&&!path.contains(&0)&&
  !path.contains(&b'\\')&&
  path.split(|b|*b==b'/').skip(1).all(|part|
   !part.is_empty()&&part!=b"."&&part!=b"..")
 }
 fn lookup(&self,path:&[u8])->Option<usize>{
  self.files.iter().position(|x|x.is_some_and(|f|
   f.path_len==path.len()&&&f.path[..f.path_len]==path))
 }
 pub fn version(&self)->u64{self.version}
 pub fn count(&self)->usize{self.files.iter().filter(|x|x.is_some()).count()}
 /// Copy all bytes into a local future generation BEFORE publishing it.
 pub fn replace(&mut self,path:&[u8],data:&[u8])->Result<u64,Error>{
  if !Self::valid_path(path)||data.len()>CONTENT_MAX{return Err(Error::Invalid);}
  let idx=match self.lookup(path){
   Some(i)=>i,
   None=>self.files.iter().position(Option::is_none).ok_or(Error::Full)?,
  };
  let version=self.version.checked_add(1).ok_or(Error::Exhausted)?;
  let mut next=File{path:[0;PATH_MAX],path_len:path.len(),
   data:[0;CONTENT_MAX],data_len:data.len(),version};
  next.path[..path.len()].copy_from_slice(path);
  next.data[..data.len()].copy_from_slice(data);
  self.files[idx]=Some(next);
  self.version=version;
  Ok(version)
 }
 pub fn read(&self,path:&[u8],offset:u64,out:&mut[u8])
  ->Result<usize,Error>{
  let file=self.lookup(path).and_then(|i|self.files[i]).ok_or(Error::NotFound)?;
  if offset>file.data_len as u64{return Ok(0);}
  let from=offset as usize;
  let n=out.len().min(file.data_len-from);
  out[..n].copy_from_slice(&file.data[from..from+n]);
  Ok(n)
 }
 /// Stable insertion-order index of a directory entry for "ls".
 pub fn entry(&self,index:usize,out:&mut[u8])
   ->Result<Option<(usize,usize,u64)>,Error>{
  let file=self.files.iter().flatten().nth(index);
  let Some(file)=file else{return Ok(None)};
  if out.len()<file.path_len{return Err(Error::OutputTooSmall);}
  out[..file.path_len].copy_from_slice(&file.path[..file.path_len]);
  Ok(Some((file.path_len,file.data_len,file.version)))
 }
}
impl<const N:usize> GuestVfs for RamFs<N>{
 fn read_at(&mut self,path:&[u8],offset:u64,out:&mut[u8])
  ->Result<usize,()>{self.read(path,offset,out).map_err(|_|())}
}
impl<const N:usize> AtomicFileBackend for RamFs<N>{
 fn write_atomic(&mut self,path:&[u8],contents:&[u8])->Result<(),()>{
  self.replace(path,contents).map(|_|()).map_err(|_|())
 }
 fn read_all(&mut self,path:&[u8],output:&mut[u8])->Result<usize,()>{
  let i=self.lookup(path).ok_or(())?;
  let file=self.files[i].ok_or(())?;
  if file.data_len>output.len(){return Err(());}
  output[..file.data_len].copy_from_slice(&file.data[..file.data_len]);
  Ok(file.data_len)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 use crate::batch5_vfs::FileTable;
 #[test]fn actual_write_read_replace_and_descriptor_access(){
  let mut disk=RamFs::<2>::new();
  assert_eq!(disk.replace(b"/docs/note",b"abc"),Ok(1));
  assert_eq!(disk.replace(b"/docs/note",b"longer"),Ok(2));
  assert_eq!(disk.count(),1);
  let handle=FileTable::<1>::new();
  let mut fd=handle;
  let h=fd.open(4,2,"/docs/note").unwrap();
  let mut out=[0u8;6];
  assert_eq!(fd.read(4,2,h,0,&mut out,&mut disk),Ok(6));
  assert_eq!(&out,b"longer");
  assert_eq!(fd.read(4,3,h,0,&mut out,&mut disk),
   Err(crate::batch5_vfs::FsError::Denied));
 }
 #[test]fn failed_mutation_keeps_original_and_version(){
  let mut disk=RamFs::<1>::new();disk.replace(b"/a",b"safe").unwrap();
  assert_eq!(disk.replace(b"/a/../x",b"bad"),Err(Error::Invalid));
  assert_eq!(disk.replace(b"/b",b"new"),Err(Error::Full));
  assert_eq!(disk.replace(b"/a",&[7;CONTENT_MAX+1]),Err(Error::Invalid));
  assert_eq!(disk.version(),1);
  let mut out=[0;4];assert_eq!(disk.read(b"/a",0,&mut out),Ok(4));
  assert_eq!(&out,b"safe");
 }
 #[test]fn backend_never_partially_writes_short_destination(){
  let mut disk=RamFs::<1>::new();disk.replace(b"/a",b"data").unwrap();
  let mut dst=[0xeeu8;2];
  assert_eq!(disk.read_all(b"/a",&mut dst),Err(()));
  assert_eq!(dst,[0xee;2]);
  assert_eq!(disk.replace(b"/../bad",b"x"),Err(Error::Invalid));
 }
}

/// Orbit R005: atomic RAM-file maintenance operations. These preserve the
/// in-memory directory invariant but do NOT supply durable reboot storage.
impl<const N:usize> RamFs<N>{
 /// Read one file's generation and length without copying its contents.
 pub fn metadata(&self,path:&[u8])->Result<(u64,usize),Error>{
  if !Self::valid_path(path){return Err(Error::Invalid);}
  let index=self.lookup(path).ok_or(Error::NotFound)?;
  let file=self.files[index].ok_or(Error::NotFound)?;
  Ok((file.version,file.data_len))
 }
 /// Delete only a file the caller has already authorized and observed.
 /// Version precondition prevents a stale UI action deleting new contents.
 pub fn delete_if_version(&mut self,path:&[u8],expected:u64)->Result<u64,Error>{
  if !Self::valid_path(path){return Err(Error::Invalid);}
  let index=self.lookup(path).ok_or(Error::NotFound)?;
  let file=self.files[index].ok_or(Error::NotFound)?;
  if expected==0||file.version!=expected{return Err(Error::Invalid);}
  let next=self.version.checked_add(1).ok_or(Error::Exhausted)?;
  self.files[index]=None;self.version=next;Ok(next)
 }
 /// Fail closed for duplicate destinations, stale file versions and bad
 /// paths. No source is destroyed until every precondition has passed.
 pub fn rename_if_version(&mut self,from:&[u8],to:&[u8],
  expected:u64)->Result<u64,Error>{
  if !Self::valid_path(from)||!Self::valid_path(to)||from==to{
   return Err(Error::Invalid);
  }
  let index=self.lookup(from).ok_or(Error::NotFound)?;
  let mut file=self.files[index].ok_or(Error::NotFound)?;
  if expected==0||file.version!=expected||self.lookup(to).is_some(){
   return Err(Error::Invalid);
  }
  let next=self.version.checked_add(1).ok_or(Error::Exhausted)?;
  file.path=[0;PATH_MAX];file.path[..to.len()].copy_from_slice(to);
  file.path_len=to.len();file.version=next;
  self.files[index]=Some(file);self.version=next;Ok(next)
 }
 /// Full-data copy with no partial target and no implicit overwrite.
 /// The trusted caller must enforce read and write grants on both paths.
 pub fn copy_new(&mut self,from:&[u8],to:&[u8])->Result<u64,Error>{
  if !Self::valid_path(from)||!Self::valid_path(to)||from==to{
   return Err(Error::Invalid);
  }
  let src=self.lookup(from).ok_or(Error::NotFound)?;
  if self.lookup(to).is_some(){return Err(Error::Invalid);}
  let dest=self.files.iter().position(Option::is_none).ok_or(Error::Full)?;
  let next=self.version.checked_add(1).ok_or(Error::Exhausted)?;
  let mut file=self.files[src].ok_or(Error::NotFound)?;
  file.path=[0;PATH_MAX];file.path[..to.len()].copy_from_slice(to);
  file.path_len=to.len();file.version=next;
  self.files[dest]=Some(file);self.version=next;Ok(next)
 }
}
#[cfg(test)]
mod orbit_r005_file_maintenance_tests{
 use super::*;
 #[test]fn copy_rename_delete_all_preserve_contents_and_generations(){
  let mut fs=RamFs::<3>::new();
  let first=fs.replace(b"/alpha",b"original").unwrap();
  assert_eq!(fs.metadata(b"/alpha"),Ok((first,8)));
  let copied=fs.copy_new(b"/alpha",b"/backup").unwrap();
  let renamed=fs.rename_if_version(b"/backup",b"/renamed",copied).unwrap();
  assert_eq!(fs.metadata(b"/backup"),Err(Error::NotFound));
  let mut out=[0u8;8];
  assert_eq!(fs.read(b"/renamed",0,&mut out),Ok(8));
  assert_eq!(&out,b"original");
  assert_eq!(fs.delete_if_version(b"/renamed",renamed),Ok(4));
  assert_eq!(fs.count(),1);
  assert_eq!(fs.metadata(b"/alpha"),Ok((first,8)));
 }
 #[test]fn collisions_and_stale_ui_versions_never_mutate_files(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"old").unwrap();
  fs.replace(b"/b",b"keep").unwrap();
  let v=fs.version();
  assert_eq!(fs.copy_new(b"/a",b"/b"),Err(Error::Invalid));
  assert_eq!(fs.rename_if_version(b"/a",b"/b",1),Err(Error::Invalid));
  assert_eq!(fs.delete_if_version(b"/a",99),Err(Error::Invalid));
  assert_eq!(fs.rename_if_version(b"/a",b"/../escape",1),Err(Error::Invalid));
  assert_eq!(fs.copy_new(b"/a",b"/c"),Err(Error::Full));
  assert_eq!(fs.version(),v);
  let mut out=[0u8;4];fs.read(b"/b",0,&mut out).unwrap();
  assert_eq!(&out,b"keep");
 }
 #[test]fn missing_files_and_zero_capacity_fail_without_publish(){
  let mut fs=RamFs::<0>::new();
  assert_eq!(fs.copy_new(b"/a",b"/b"),Err(Error::NotFound));
  assert_eq!(fs.rename_if_version(b"/a",b"/b",1),Err(Error::NotFound));
  assert_eq!(fs.delete_if_version(b"/a",1),Err(Error::NotFound));
  assert_eq!(fs.version(),0);
 }
}
