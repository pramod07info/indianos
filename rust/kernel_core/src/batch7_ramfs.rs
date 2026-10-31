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
 /// Apply a bounded collection of independent file replacements as one
 /// published in-memory transaction. A stale global revision, duplicate
 /// pathname, exhausted capacity, malformed path or oversize value changes
 /// NOTHING, including existing file versions.
 ///
 /// The caller must authenticate the process and enforce WRITE rights.
 /// This is RAM-only; an external storage layer must persist snapshots.
 pub fn replace_batch_if_revision(&mut self,expected:u64,
  requests:&[(&[u8],&[u8])])->Result<u64,Error>{
  if expected!=self.version{return Err(Error::Invalid);}
  if requests.is_empty()||requests.len()>16{return Err(Error::Invalid);}
  let mut staged=Self{files:self.files,version:self.version};
  for (i,&(path,data)) in requests.iter().enumerate(){
   if requests[..i].iter().any(|(earlier,_)|*earlier==path){
    return Err(Error::Invalid);
   }
   staged.replace(path,data)?;
  }
  *self=staged;
  Ok(self.version)
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
#[cfg(test)]
mod r058_atomic_batch_tests{
 use super::*;
 #[test]fn two_files_commit_together_with_distinct_versions(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/keep",b"old").unwrap();
  let rev=fs.version();
  let next=fs.replace_batch_if_revision(rev,
    &[(b"/keep",b"new"),(b"/added",b"payload")]).unwrap();
  assert_eq!(next,rev+2);
  assert_eq!(fs.count(),2);
  let mut data=[0u8;16];
  assert_eq!(fs.read(b"/keep",0,&mut data),Ok(3));
  assert_eq!(&data[..3],b"new");
  assert_eq!(fs.read(b"/added",0,&mut data),Ok(7));
  assert_eq!(&data[..7],b"payload");
  assert_eq!(fs.metadata(b"/keep").unwrap().0,rev+1);
  assert_eq!(fs.metadata(b"/added").unwrap().0,rev+2);
 }
 #[test]fn failed_second_write_and_stale_revision_roll_back_all_entries(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/one",b"safe").unwrap();
  let before=fs.version();
  assert_eq!(fs.replace_batch_if_revision(before,
    &[(b"/one",b"changed"),(b"/two",b"new")]),Err(Error::Full));
  assert_eq!(fs.version(),before);
  assert_eq!(fs.count(),1);
  assert_eq!(fs.metadata(b"/one"),Ok((before,4)));
  let mut data=[0;8];fs.read(b"/one",0,&mut data).unwrap();
  assert_eq!(&data[..4],b"safe");
  assert_eq!(fs.replace_batch_if_revision(0,&[(b"/one",b"wrong")]),
    Err(Error::Invalid));
  assert_eq!(fs.version(),before);
 }
 #[test]fn duplicate_paths_bad_request_and_oversize_fail_without_publish(){
  let mut fs=RamFs::<3>::new();
  let bad=[b'x';CONTENT_MAX+1];
  for batch in [
    [(b"/a".as_slice(),b"first".as_slice()),(b"/a",b"last")],
    [(b"/a",b"valid"),(b"/../escape",b"invalid")],
    [(b"/a",b"valid"),(b"/b",bad.as_slice())]
  ]{
   assert!(fs.replace_batch_if_revision(0,&batch).is_err());
   assert_eq!(fs.version(),0);assert_eq!(fs.count(),0);
  }
  assert_eq!(fs.replace_batch_if_revision(0,&[]),Err(Error::Invalid));
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
 /// Delete several individually versioned files as one RAM transaction.
 /// The global revision and each observed file revision must still match.
 /// Duplicate paths or an unavailable/changed file roll back the entire
 /// operation, including the global version counter. The kernel caller
 /// must enforce WRITE capability for every path before invoking this API.
 pub fn delete_batch_if_revision(&mut self,expected_global:u64,
  files:&[(&[u8],u64)])->Result<u64,Error>{
  if files.is_empty()||files.len()>N||expected_global!=self.version{
   return Err(Error::Invalid);
  }
  let mut staged=Self{files:self.files,version:self.version};
  for (i,&(path,expected_file)) in files.iter().enumerate(){
   if files[..i].iter().any(|&(prior,_)|prior==path){
    return Err(Error::Invalid);
   }
   staged.delete_if_version(path,expected_file)?;
  }
  *self=staged;
  Ok(self.version)
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
mod r059_batch_delete_tests{
 use super::*;
 #[test]fn r059_remove_two_versioned_files_and_preserve_unrelated_data(){
  let mut fs=RamFs::<4>::new();
  let a=fs.replace(b"/one",b"data").unwrap();
  let b=fs.replace(b"/two",b"secret").unwrap();
  fs.replace(b"/peer",b"untouched").unwrap();
  let current=fs.version();
  assert_eq!(fs.delete_batch_if_revision(current,&[(b"/one",a),(b"/two",b)]),
    Ok(current+2));
  assert_eq!(fs.count(),1);
  assert_eq!(fs.metadata(b"/one"),Err(Error::NotFound));
  let mut out=[0u8;16];assert_eq!(fs.read(b"/peer",0,&mut out),Ok(9));
  assert_eq!(&out[..9],b"untouched");
 }
 #[test]fn r059_stale_second_file_and_duplicate_request_rollback_all(){
  let mut fs=RamFs::<4>::new();
  let a=fs.replace(b"/one",b"original").unwrap();
  let b=fs.replace(b"/two",b"keep").unwrap();
  let generation=fs.version();
  assert_eq!(fs.delete_batch_if_revision(generation,
    &[(b"/one",a),(b"/two",b+1)]),Err(Error::Invalid));
  assert_eq!(fs.version(),generation);
  assert_eq!(fs.metadata(b"/one"),Ok((a,8)));
  assert_eq!(fs.delete_batch_if_revision(generation,
    &[(b"/one",a),(b"/one",a)]),Err(Error::Invalid));
  assert_eq!(fs.version(),generation);
  assert_eq!(fs.count(),2);
  assert_eq!(fs.delete_batch_if_revision(generation-1,&[(b"/one",a)]),
    Err(Error::Invalid));
 }
 #[test]fn r059_missing_and_malformed_paths_do_not_delete_valid_entry(){
  let mut fs=RamFs::<3>::new();
  let a=fs.replace(b"/one",b"safe").unwrap();
  assert_eq!(fs.delete_batch_if_revision(a,&[(b"/one",a),(b"/gone",7)]),
    Err(Error::NotFound));
  assert_eq!(fs.delete_batch_if_revision(a,&[(b"/one",a),(b"/../bad",1)]),
    Err(Error::Invalid));
  assert_eq!(fs.delete_batch_if_revision(a,&[]),Err(Error::Invalid));
  let mut out=[0u8;8];assert_eq!(fs.read(b"/one",0,&mut out),Ok(4));
  assert_eq!(&out[..4],b"safe");
  assert_eq!(fs.version(),a);
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


/// R006: deterministic, versioned export/import for an authorized storage
/// service. The caller must persist the exported bytes via a checksummed,
/// power-loss-safe block backend. This RAM-only implementation does NOT
/// establish reboot durability by itself.
pub const SNAPSHOT_HEADER:usize=14;
fn snapshot_checksum(bytes:&[u8])->u32{
 let mut h=0x811c9dc5u32;
 for &byte in bytes{h=(h^(byte as u32)).wrapping_mul(0x0100_0193);}
 h
}
impl<const N:usize> RamFs<N>{
 /// Encode an entire consistent RAM generation; a short caller buffer
 /// remains untouched, rather than receiving half a filesystem.
 pub fn export_snapshot(&self,out:&mut[u8])->Result<usize,Error>{
  if self.count()>u16::MAX as usize{return Err(Error::Invalid);}
  let mut required=SNAPSHOT_HEADER+4;
  for file in self.files.iter().flatten(){
   required=required.checked_add(1+2+8)
    .and_then(|n|n.checked_add(file.path_len))
    .and_then(|n|n.checked_add(file.data_len))
    .ok_or(Error::Exhausted)?;
  }
  if out.len()<required{return Err(Error::OutputTooSmall);}
  out[..4].copy_from_slice(b"IRFS");
  out[4..12].copy_from_slice(&self.version.to_le_bytes());
  out[12..14].copy_from_slice(&(self.count() as u16).to_le_bytes());
  let mut p=SNAPSHOT_HEADER;
  for file in self.files.iter().flatten(){
   out[p]=file.path_len as u8;p+=1;
   out[p..p+2].copy_from_slice(&(file.data_len as u16).to_le_bytes());p+=2;
   out[p..p+8].copy_from_slice(&file.version.to_le_bytes());p+=8;
   out[p..p+file.path_len].copy_from_slice(&file.path[..file.path_len]);
   p+=file.path_len;
   out[p..p+file.data_len].copy_from_slice(&file.data[..file.data_len]);
   p+=file.data_len;
  }
  let checksum=snapshot_checksum(&out[..p]);
  out[p..p+4].copy_from_slice(&checksum.to_le_bytes());
  Ok(required)
 }
 /// Validate the complete untrusted snapshot and publish only after
 /// checksum, path, generation, count and duplicate checks all pass.
 pub fn import_snapshot(&mut self,input:&[u8])->Result<(),Error>{
  if input.len()<SNAPSHOT_HEADER+4||&input[..4]!=b"IRFS"{
   return Err(Error::Invalid);
  }
  let data_len=input.len()-4;
  let got=u32::from_le_bytes(input[data_len..].try_into()
   .map_err(|_|Error::Invalid)?);
  if snapshot_checksum(&input[..data_len])!=got{return Err(Error::Invalid);}
  let generation=u64::from_le_bytes(input[4..12].try_into()
   .map_err(|_|Error::Invalid)?);
  let count=u16::from_le_bytes(input[12..14].try_into()
   .map_err(|_|Error::Invalid)?) as usize;
  if count>N||(count!=0&&generation==0){return Err(Error::Invalid);}
  let mut staged=Self::new();let mut p=SNAPSHOT_HEADER;
  for index in 0..count{
   if p.checked_add(11).is_none_or(|end|end>data_len){return Err(Error::Invalid);}
   let path_len=input[p] as usize;p+=1;
   let content_len=u16::from_le_bytes([input[p],input[p+1]]) as usize;p+=2;
   let version=u64::from_le_bytes(input[p..p+8].try_into()
    .map_err(|_|Error::Invalid)?);p+=8;
   let end=p.checked_add(path_len).and_then(|n|n.checked_add(content_len))
    .ok_or(Error::Invalid)?;
   if end>data_len||path_len>PATH_MAX||content_len>CONTENT_MAX||
      version==0||version>generation{return Err(Error::Invalid);}
   let path=&input[p..p+path_len];p+=path_len;
   if !Self::valid_path(path)||staged.lookup(path).is_some(){
    return Err(Error::Invalid);
   }
   let mut item=File{path:[0;PATH_MAX],path_len,
    data:[0;CONTENT_MAX],data_len:content_len,version};
   item.path[..path_len].copy_from_slice(path);
   item.data[..content_len].copy_from_slice(&input[p..p+content_len]);
   p+=content_len;staged.files[index]=Some(item);
  }
  if p!=data_len{return Err(Error::Invalid);}
  staged.version=generation;
  *self=staged;Ok(())
 }
}
#[cfg(test)]
mod r006_snapshot_tests{
 use super::*;
 #[test]fn snapshot_restores_entire_directory_and_versions(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/notes/one",b"first").unwrap();
  let v=fs.replace(b"/notes/two",b"second").unwrap();
  fs.delete_if_version(b"/notes/one",1).unwrap();
  let mut bytes=[0xccu8;512];let n=fs.export_snapshot(&mut bytes).unwrap();
  let mut restored=RamFs::<3>::new();
  restored.import_snapshot(&bytes[..n]).unwrap();
  assert_eq!(restored.count(),1);
  assert_eq!(restored.metadata(b"/notes/two"),Ok((v,6)));
  assert_eq!(restored.version(),3);
  let mut out=[0u8;6];
  assert_eq!(restored.read(b"/notes/two",0,&mut out),Ok(6));
  assert_eq!(&out,b"second");
 }
 #[test]fn tampering_and_truncation_cannot_clobber_existing_files(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/safe",b"original").unwrap();
  let mut raw=[0u8;128];let n=fs.export_snapshot(&mut raw).unwrap();
  raw[19]^=1;
  assert_eq!(fs.import_snapshot(&raw[..n]),Err(Error::Invalid));
  raw[19]^=1;
  assert_eq!(fs.import_snapshot(&raw[..n-1]),Err(Error::Invalid));
  let mut out=[0u8;8];fs.read(b"/safe",0,&mut out).unwrap();
  assert_eq!(&out,b"original");
 }
 #[test]fn short_encode_is_atomic_and_empty_snapshot_roundtrips(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"hello").unwrap();
  let mut out=[0xaau8;12];
  assert_eq!(fs.export_snapshot(&mut out),Err(Error::OutputTooSmall));
  assert_eq!(out,[0xaa;12]);
  let empty=RamFs::<1>::new();let mut image=[0u8;18];
  assert_eq!(empty.export_snapshot(&mut image),Ok(18));
  fs.import_snapshot(&image).unwrap();assert_eq!(fs.count(),0);
 }
}


/// R017 editor writes using a file-system revision precondition. A rejected
/// change leaves the old bytes and sequence intact. RAM is not durable disk.
impl<const N:usize> RamFs<N>{
 pub fn save_if_revision(&mut self,path:&[u8],data:&[u8],expected:u64)
  ->Result<u64,Error>{
  if self.version!=expected{return Err(Error::Invalid);}
  self.replace(path,data)
 }
 pub fn append_if_revision(&mut self,path:&[u8],suffix:&[u8],expected:u64)
  ->Result<u64,Error>{
  if self.version!=expected{return Err(Error::Invalid);}
  let i=self.lookup(path).ok_or(Error::NotFound)?;
  let entry=self.files[i].ok_or(Error::NotFound)?;
  let end=entry.data_len.checked_add(suffix.len()).ok_or(Error::Invalid)?;
  if end>CONTENT_MAX{return Err(Error::Invalid);}
  let mut tmp=[0u8;CONTENT_MAX];
  tmp[..entry.data_len].copy_from_slice(&entry.data[..entry.data_len]);
  tmp[entry.data_len..end].copy_from_slice(suffix);
  self.replace(path,&tmp[..end])
 }
 pub fn truncate_if_revision(&mut self,path:&[u8],len:usize,expected:u64)
  ->Result<u64,Error>{
  if self.version!=expected{return Err(Error::Invalid);}
  let i=self.lookup(path).ok_or(Error::NotFound)?;
  let entry=self.files[i].ok_or(Error::NotFound)?;
  if len>entry.data_len{return Err(Error::Invalid);}
  self.replace(path,&entry.data[..len])
 }
}
#[cfg(test)]mod r017_editor_save_tests{
 use super::*;
 #[test]fn save_append_truncate_same_file_preserves_bytes(){
  let mut f=RamFs::<1>::new();
  f.save_if_revision(b"/code",b"main",0).unwrap();
  f.append_if_revision(b"/code",b".rs",1).unwrap();
  f.truncate_if_revision(b"/code",4,2).unwrap();
  let mut out=[0;8];assert_eq!(f.read(b"/code",0,&mut out),Ok(4));
  assert_eq!(&out[..4],b"main");
 }
 #[test]fn stale_editor_save_does_not_overwrite_newer_revision(){
  let mut f=RamFs::<1>::new();f.replace(b"/code",b"new").unwrap();
  assert_eq!(f.save_if_revision(b"/code",b"old",0),Err(Error::Invalid));
  assert_eq!(f.append_if_revision(b"/code",b"bad",0),Err(Error::Invalid));
  assert_eq!(f.version(),1);
 }
 #[test]fn overcapacity_append_does_not_truncate_or_modify(){
  let mut f=RamFs::<1>::new();
  f.replace(b"/code",&[b'a';CONTENT_MAX]).unwrap();
  assert_eq!(f.append_if_revision(b"/code",b"x",1),Err(Error::Invalid));
  assert_eq!(f.truncate_if_revision(b"/code",CONTENT_MAX+1,1),
   Err(Error::Invalid));
  assert_eq!(f.version(),1);
 }
}


/// R060: one in-memory rename transaction can move several files, including
/// rename cycles (e.g. swapping two names), with optimistic concurrency.
/// All source contents and versions are read from the ORIGINAL directory.
/// Failed authentication at a higher layer, invalid paths or stale versions
/// must not cause partial changes. The caller must authorize every path.
impl<const N:usize> RamFs<N>{
 pub fn rename_batch_if_revision(&mut self,expected_global:u64,
   requests:&[(&[u8],&[u8],u64)])->Result<u64,Error>{
  if requests.is_empty()||requests.len()>N||requests.len()>16||
      self.version!=expected_global{return Err(Error::Invalid);}
  for (i,&(from,to,expected_file)) in requests.iter().enumerate(){
   if !Self::valid_path(from)||!Self::valid_path(to)||from==to||
       expected_file==0{return Err(Error::Invalid);}
   if requests[..i].iter().any(|&(old_from,old_to,_)|
        old_from==from||old_to==to){return Err(Error::Invalid);}
   let slot=self.lookup(from).ok_or(Error::NotFound)?;
   if self.files[slot].ok_or(Error::NotFound)?.version!=expected_file{
    return Err(Error::Invalid);
   }
   // An occupied destination is permitted only if it is also a source
   // being moved by this SAME transaction (a rename chain or cycle).
   if self.lookup(to).is_some()&&
      !requests.iter().any(|&(moving,_,_)|moving==to){
    return Err(Error::Invalid);
   }
  }
  let mut staged=Self{files:self.files,version:self.version};
  for &(from,to,_) in requests{
   let slot=self.lookup(from).ok_or(Error::NotFound)?;
   let mut file=self.files[slot].ok_or(Error::NotFound)?;
   let version=staged.version.checked_add(1).ok_or(Error::Exhausted)?;
   file.path=[0u8;PATH_MAX];
   file.path[..to.len()].copy_from_slice(to);
   file.path_len=to.len();
   file.version=version;
   staged.files[slot]=Some(file);
   staged.version=version;
  }
  *self=staged;
  Ok(self.version)
 }
}
#[cfg(test)]
mod r060_rename_batch_tests{
 use super::*;
 #[test]fn atomic_name_swap_retains_both_contents(){
  let mut fs=RamFs::<3>::new();
  let a=fs.replace(b"/left",b"alpha").unwrap();
  let b=fs.replace(b"/right",b"beta").unwrap();
  let generation=fs.version();
  assert_eq!(fs.rename_batch_if_revision(generation,
    &[(b"/left",b"/right",a),(b"/right",b"/left",b)]),
    Ok(generation+2));
  assert_eq!(fs.count(),2);
  let mut out=[0u8;8];
  assert_eq!(fs.read(b"/left",0,&mut out),Ok(4));
  assert_eq!(&out[..4],b"beta");
  assert_eq!(fs.read(b"/right",0,&mut out),Ok(5));
  assert_eq!(&out[..5],b"alpha");
 }
 #[test]fn collision_stale_second_source_and_duplicate_target_roll_back(){
  let mut fs=RamFs::<4>::new();
  let a=fs.replace(b"/a",b"safe").unwrap();
  let b=fs.replace(b"/b",b"keep").unwrap();
  fs.replace(b"/occupied",b"peer").unwrap();
  let current=fs.version();
  assert_eq!(fs.rename_batch_if_revision(current,
     &[(b"/a",b"/new",a),(b"/b",b"/occupied",b)]),
     Err(Error::Invalid));
  assert_eq!(fs.rename_batch_if_revision(current,
     &[(b"/a",b"/new",a),(b"/b",b"/other",b+1)]),
     Err(Error::Invalid));
  assert_eq!(fs.rename_batch_if_revision(current,
     &[(b"/a",b"/new",a),(b"/b",b"/new",b)]),
     Err(Error::Invalid));
  assert_eq!(fs.version(),current);
  assert_eq!(fs.metadata(b"/a"),Ok((a,4)));
  assert_eq!(fs.metadata(b"/b"),Ok((b,4)));
  assert_eq!(fs.count(),3);
 }
 #[test]fn malformed_source_chain_and_empty_batch_do_not_publish(){
  let mut fs=RamFs::<2>::new();
  let v=fs.replace(b"/a",b"data").unwrap();
  assert_eq!(fs.rename_batch_if_revision(v,&[]),Err(Error::Invalid));
  assert_eq!(fs.rename_batch_if_revision(v,
      &[(b"/a",b"/next",v),(b"/missing",b"/new",99)]),
      Err(Error::NotFound));
  assert_eq!(fs.rename_batch_if_revision(v,
      &[(b"/a",b"/../escape",v)]),Err(Error::Invalid));
  assert_eq!(fs.version(),v);
  let mut out=[0;8];fs.read(b"/a",0,&mut out).unwrap();
  assert_eq!(&out[..4],b"data");
 }
}


/// R061: trusted workstation transaction script for the RAM-backed Files
/// model. Operations are deliberately serialized on a PRIVATE filesystem
/// generation: any late full-table, stale file, collision, or invalid-path
/// error rolls back every preceding mutation. Never claim reboot durability.
#[derive(Clone,Copy)]
pub enum RamMutation<'a>{
 Put{path:&'a [u8],contents:&'a [u8]},
 Remove{path:&'a [u8],version:u64},
 Move{from:&'a [u8],to:&'a [u8],version:u64},
}
impl<const N:usize> RamFs<N>{
 /// Requires a kernel-authenticated caller with suitable read/write grants
 /// for EVERY source/destination before invoking. Not a raw userspace API.
 pub fn transact_if_revision(&mut self,expected_global:u64,
    operations:&[RamMutation<'_>])->Result<u64,Error>{
  if operations.is_empty()||operations.len()>16||
     expected_global!=self.version{return Err(Error::Invalid);}
  let mut staged=Self{files:self.files,version:self.version};
  for operation in operations{
   match *operation{
    RamMutation::Put{path,contents}=>{
     staged.replace(path,contents)?;
    },
    RamMutation::Remove{path,version}=>{
     staged.delete_if_version(path,version)?;
    },
    RamMutation::Move{from,to,version}=>{
     staged.rename_if_version(from,to,version)?;
    },
   }
  }
  *self=staged;
  Ok(self.version)
 }
}
#[cfg(test)]
mod r061_mixed_transaction_tests{
 use super::*;
 #[test]fn puts_removal_and_rename_publish_as_one_generation(){
  let mut fs=RamFs::<4>::new();
  let _keep=fs.replace(b"/keep",b"old").unwrap();
  let gone=fs.replace(b"/gone",b"remove").unwrap();
  let moved=fs.replace(b"/move",b"original").unwrap();
  let before=fs.version();
  assert_eq!(fs.transact_if_revision(before,&[
    RamMutation::Put{path:b"/keep",contents:b"updated"},
    RamMutation::Remove{path:b"/gone",version:gone},
    RamMutation::Move{from:b"/move",to:b"/renamed",version:moved},
    RamMutation::Put{path:b"/new",contents:b"new data"},
  ]),Ok(before+4));
  assert_eq!(fs.count(),3);
  assert_eq!(fs.metadata(b"/gone"),Err(Error::NotFound));
  assert_eq!(fs.metadata(b"/move"),Err(Error::NotFound));
  let mut out=[0u8;16];
  assert_eq!(fs.read(b"/keep",0,&mut out),Ok(7));
  assert_eq!(&out[..7],b"updated");
  assert_eq!(fs.read(b"/renamed",0,&mut out),Ok(8));
  assert_eq!(&out[..8],b"original");
  assert_eq!(fs.read(b"/new",0,&mut out),Ok(8));
 }
 #[test]fn late_version_conflict_and_name_collision_revert_earlier_updates(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/a",b"safe").unwrap();
  let b=fs.replace(b"/b",b"peer").unwrap();
  let before=fs.version();
  assert_eq!(fs.transact_if_revision(before,&[
    RamMutation::Put{path:b"/a",contents:b"changed"},
    RamMutation::Remove{path:b"/b",version:b+1},
  ]),Err(Error::Invalid));
  assert_eq!(fs.transact_if_revision(before,&[
    RamMutation::Put{path:b"/a",contents:b"changed"},
    RamMutation::Move{from:b"/b",to:b"/a",version:b},
  ]),Err(Error::Invalid));
  assert_eq!(fs.version(),before);
  let mut out=[0u8;8];
  assert_eq!(fs.read(b"/a",0,&mut out),Ok(4));
  assert_eq!(&out[..4],b"safe");
  assert_eq!(fs.read(b"/b",0,&mut out),Ok(4));
  assert_eq!(&out[..4],b"peer");
 }
 #[test]fn full_capacity_bad_path_and_stale_global_leave_all_bytes(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/keep",b"safe").unwrap();
  let before=fs.version();
  assert_eq!(fs.transact_if_revision(before,&[
    RamMutation::Put{path:b"/keep",contents:b"unsafe"},
    RamMutation::Put{path:b"/extra",contents:b"data"},
  ]),Err(Error::Full));
  assert_eq!(fs.transact_if_revision(before,&[
    RamMutation::Put{path:b"/keep",contents:b"unsafe"},
    RamMutation::Put{path:b"/../escape",contents:b"x"},
  ]),Err(Error::Invalid));
  assert_eq!(fs.transact_if_revision(0,&[
    RamMutation::Put{path:b"/keep",contents:b"unsafe"},
  ]),Err(Error::Invalid));
  assert_eq!(fs.transact_if_revision(before,&[]),Err(Error::Invalid));
  assert_eq!(fs.version(),before);
  let mut out=[0u8;8];fs.read(b"/keep",0,&mut out).unwrap();
  assert_eq!(&out[..4],b"safe");
 }
}


/// R062: Copy multiple authenticated source files into new RAM file names
/// in one optimistic transaction. A late capacity error, source revision
/// mismatch, duplicate destination or invalid path publishes NOTHING.
/// The calling service must check READ source and WRITE destination grants.
impl<const N:usize> RamFs<N>{
 pub fn copy_batch_if_revision(&mut self,expected_global:u64,
  requests:&[(&[u8],&[u8],u64)])->Result<u64,Error>{
  if requests.is_empty()||requests.len()>N||
      requests.len()>16||self.version!=expected_global{
   return Err(Error::Invalid);
  }
  let mut staged=Self{files:self.files,version:self.version};
  for (i,&(from,to,version)) in requests.iter().enumerate(){
   if requests[..i].iter().any(|&(_,previous,_)|previous==to){
    return Err(Error::Invalid);
   }
   if !Self::valid_path(from)||!Self::valid_path(to)||from==to{
    return Err(Error::Invalid);
   }
   let slot=staged.lookup(from).ok_or(Error::NotFound)?;
   let file=staged.files[slot].ok_or(Error::NotFound)?;
   if version==0||file.version!=version{return Err(Error::Invalid);}
   staged.copy_new(from,to)?;
  }
  *self=staged;
  Ok(self.version)
 }
}
#[cfg(test)]
mod r062_atomic_copy_tests{
 use super::*;
 #[test]fn distinct_sources_copy_data_and_versions_in_single_batch(){
  let mut fs=RamFs::<4>::new();
  let a=fs.replace(b"/a",b"alpha").unwrap();
  let b=fs.replace(b"/b",b"beta").unwrap();
  let start=fs.version();
  assert_eq!(fs.copy_batch_if_revision(start,&[
    (b"/a",b"/a-copy",a),(b"/b",b"/b-copy",b)
  ]),Ok(start+2));
  assert_eq!(fs.count(),4);
  let mut out=[0u8;16];
  assert_eq!(fs.read(b"/a-copy",0,&mut out),Ok(5));
  assert_eq!(&out[..5],b"alpha");
  assert_eq!(fs.read(b"/b-copy",0,&mut out),Ok(4));
  assert_eq!(&out[..4],b"beta");
  assert_eq!(fs.metadata(b"/a-copy").unwrap().0,start+1);
  assert_eq!(fs.metadata(b"/b-copy").unwrap().0,start+2);
 }
 #[test]fn late_collision_duplicate_target_and_stale_file_roll_back_first_copy(){
  let mut fs=RamFs::<4>::new();
  let a=fs.replace(b"/a",b"keep").unwrap();
  let b=fs.replace(b"/b",b"peer").unwrap();
  let rev=fs.version();
  for requests in [
   [(b"/a".as_slice(),b"/new".as_slice(),a),(b"/b",b"/a",b)],
   [(b"/a",b"/new",a),(b"/b",b"/new",b)],
   [(b"/a",b"/new",a),(b"/b",b"/other",b+1)],
  ]{
   assert_eq!(fs.copy_batch_if_revision(rev,&requests),Err(Error::Invalid));
   assert_eq!(fs.version(),rev);assert_eq!(fs.count(),2);
   assert_eq!(fs.metadata(b"/new"),Err(Error::NotFound));
  }
 }
 #[test]fn capacity_and_invalid_paths_do_not_publish_staged_copies(){
  let mut fs=RamFs::<3>::new();
  let a=fs.replace(b"/a",b"one").unwrap();
  let b=fs.replace(b"/b",b"two").unwrap();
  let rev=fs.version();
  assert_eq!(fs.copy_batch_if_revision(rev,&[
    (b"/a",b"/one",a),(b"/b",b"/two",b)
  ]),Err(Error::Full));
  assert_eq!(fs.copy_batch_if_revision(rev,&[
    (b"/a",b"/one",a),(b"/b",b"/../escape",b)
  ]),Err(Error::Invalid));
  assert_eq!(fs.copy_batch_if_revision(rev+1,&[
    (b"/a",b"/one",a)
  ]),Err(Error::Invalid));
  assert_eq!(fs.version(),rev);assert_eq!(fs.count(),2);
  let mut out=[0u8;8];fs.read(b"/a",0,&mut out).unwrap();
  assert_eq!(&out[..3],b"one");
 }
}


/// R063: optimistic Compare-And-Swap for an existing RAM-backed file.
/// A caller-observed generation AND exact old byte content must agree.
/// A mismatch never rewrites data or invalidates other GUI file views.
/// Kernel services must still authorize the file's WRITE capability.
impl<const N:usize> RamFs<N>{
 pub fn compare_exchange_contents(&mut self,path:&[u8],
    expected_version:u64,expected_bytes:&[u8],
    replacement:&[u8])->Result<u64,Error>{
  if !Self::valid_path(path)||expected_version==0||
      expected_bytes.len()>CONTENT_MAX||replacement.len()>CONTENT_MAX{
   return Err(Error::Invalid);
  }
  let slot=self.lookup(path).ok_or(Error::NotFound)?;
  let existing=self.files[slot].ok_or(Error::NotFound)?;
  if existing.version!=expected_version||
      existing.data_len!=expected_bytes.len()||
      &existing.data[..existing.data_len]!=expected_bytes{
   return Err(Error::Invalid);
  }
  if existing.data_len==replacement.len()&&
     &existing.data[..existing.data_len]==replacement{
   return Ok(self.version);
  }
  self.replace(path,replacement)
 }
}
#[cfg(test)]
mod r063_compare_exchange_tests{
 use super::*;
 #[test]fn exact_content_and_version_publish_one_atomic_update(){
  let mut fs=RamFs::<2>::new();
  let version=fs.replace(b"/project",b"v1").unwrap();
  let rev=fs.version();
  assert_eq!(fs.compare_exchange_contents(
   b"/project",version,b"v1",b"v2"),Ok(rev+1));
  let mut out=[0u8;8];
  assert_eq!(fs.read(b"/project",0,&mut out),Ok(2));
  assert_eq!(&out[..2],b"v2");
  assert_eq!(fs.metadata(b"/project"),Ok((rev+1,2)));
 }
 #[test]fn stale_version_or_wrong_old_bytes_never_overwrite_current_contents(){
  let mut fs=RamFs::<1>::new();
  let v=fs.replace(b"/project",b"safe").unwrap();
  let rev=fs.version();
  assert_eq!(fs.compare_exchange_contents(
   b"/project",v+1,b"safe",b"bad"),Err(Error::Invalid));
  assert_eq!(fs.compare_exchange_contents(
   b"/project",v,b"wrong",b"bad"),Err(Error::Invalid));
  assert_eq!(fs.compare_exchange_contents(
   b"/missing",v,b"safe",b"bad"),Err(Error::NotFound));
  assert_eq!(fs.version(),rev);
  let mut out=[0u8;8];
  assert_eq!(fs.read(b"/project",0,&mut out),Ok(4));
  assert_eq!(&out[..4],b"safe");
 }
 #[test]fn no_op_cas_and_invalid_paths_never_advance_file_generation(){
  let mut fs=RamFs::<1>::new();
  let v=fs.replace(b"/project",b"stable").unwrap();
  assert_eq!(fs.compare_exchange_contents(
   b"/project",v,b"stable",b"stable"),Ok(v));
  assert_eq!(fs.compare_exchange_contents(
   b"/../bad",v,b"stable",b"changed"),Err(Error::Invalid));
  assert_eq!(fs.compare_exchange_contents(
   b"/project",v,b"stable",&[b'x';CONTENT_MAX+1]),
   Err(Error::Invalid));
  assert_eq!(fs.version(),v);
  assert_eq!(fs.metadata(b"/project"),Ok((v,6)));
 }
}
