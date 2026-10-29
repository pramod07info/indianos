//! R029/19: real guest file-backed workspace session save/load with integrity checks.
//! RAM-backed guest VFS is not power-loss persistent storage by itself.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
const MAGIC:&[u8;4]=b"IW29";
fn digest(bytes:&[u8])->u64{
 let mut h=0xcbf29ce484222325u64;
 for &b in bytes{h=(h^(b as u64)).wrapping_mul(0x100000001b);}h
}
fn path(who:Identity,out:&mut[u8;64])->usize{
 let prefix=b"/workspace/session-";
 out[..prefix.len()].copy_from_slice(prefix);
 let mut num=who.pid;let mut digits=[0u8;10];let mut n=0;
 loop{digits[n]=b'0'+(num%10)as u8;num/=10;n+=1;if num==0{break;}}
 for i in 0..n{out[prefix.len()+i]=digits[n-1-i];}
 prefix.len()+n
}
pub fn save<const F:usize>(fs:&mut RamFs<F>,who:Identity,expected:u64,
 body:&[u8])->Result<u64,Error>{
 if who.pid==0||who.generation==0||fs.version()!=expected{
  return Err(Error::Stale);
 }
 if body.len()>CONTENT_MAX-24{return Err(Error::Full);}
 let mut data=[0u8;CONTENT_MAX];data[..4].copy_from_slice(MAGIC);
 data[4..12].copy_from_slice(&who.generation.to_le_bytes());
 data[12..20].copy_from_slice(&digest(body).to_le_bytes());
 data[20..24].copy_from_slice(&(body.len() as u32).to_le_bytes());
 data[24..24+body.len()].copy_from_slice(body);
 let mut p=[0u8;64];let n=path(who,&mut p);
 fs.replace(&p[..n],&data[..24+body.len()]).map_err(|_|Error::Full)
}
pub fn load<const F:usize>(fs:&RamFs<F>,who:Identity,
 out:&mut[u8])->Result<usize,Error>{
 if who.pid==0||who.generation==0{return Err(Error::Denied);}
 let mut p=[0u8;64];let n=path(who,&mut p);
 let (_,size)=fs.metadata(&p[..n]).map_err(|_|Error::Missing)?;
 if size<24{return Err(Error::Invalid);}
 let mut data=[0u8;CONTENT_MAX];fs.read(&p[..n],0,&mut data[..size])
  .map_err(|_|Error::Missing)?;
 if &data[..4]!=MAGIC||data[4..12]!=who.generation.to_le_bytes(){
  return Err(Error::Denied);
 }
 let len=u32::from_le_bytes(data[20..24].try_into().map_err(|_|Error::Invalid)?) as usize;
 let check=u64::from_le_bytes(data[12..20].try_into().map_err(|_|Error::Invalid)?);
 if len!=size-24||check!=digest(&data[24..size]){return Err(Error::Conflict);}
 if len>out.len(){return Err(Error::Full);}
 out[..len].copy_from_slice(&data[24..size]);Ok(len)
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn actual_guest_workspace_layout_file_roundtrips(){
  let mut fs=RamFs::<1>::new();
  save(&mut fs,id(1),0,b"editor=2;terminal=1").unwrap();
  let mut out=[0;64];let n=load(&fs,id(1),&mut out).unwrap();
  assert_eq!(&out[..n],b"editor=2;terminal=1");
 }
 #[test]fn recycled_user_generation_cannot_load_older_workspace(){
  let mut fs=RamFs::<1>::new();save(&mut fs,id(1),0,b"private").unwrap();
  assert_eq!(load(&fs,id(2),&mut[0;32]),Err(Error::Denied));
 }
}