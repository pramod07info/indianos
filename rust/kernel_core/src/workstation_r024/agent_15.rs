//! R024/15: edit project configuration as validated real RAM-backed UTF-8 key=value file.
//! No executable configuration directives, environment injection or external package fetch.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
fn is_key(key:&[u8])->bool{
 !key.is_empty()&&key.len()<=48&&key.iter().all(|b|
  b.is_ascii_alphanumeric()||*b==b'_'||*b==b'-')
}
pub fn find(data:&[u8],key:&[u8])->Option<&[u8]>{
 if !is_key(key){return None;}
 for line in data.split(|b|*b==b'\n'){
  let Some(pos)=line.iter().position(|b|*b==b'=')else{continue};
  if &line[..pos]==key{return Some(&line[pos+1..]);}
 }None
}
pub fn validate(data:&[u8])->Result<usize,Error>{
 let mut n=0usize;
 for line in data.split(|b|*b==b'\n'){
  if line.is_empty(){continue;}
  let Some(pos)=line.iter().position(|b|*b==b'=')else{
   return Err(Error::Invalid);
  };
  if !is_key(&line[..pos])||line[pos+1..].len()>120||
   line[pos+1..].iter().any(|b|*b==0||*b==b'\r'){
   return Err(Error::Invalid);
  }
  n+=1;
 }Ok(n)
}
pub fn set<const F:usize>(fs:&mut RamFs<F>,path:&[u8],
 expected:u64,key:&[u8],value:&[u8])->Result<u64,Error>{
 if !is_key(key)||value.len()>120||value.iter().any(|b|*b==0||*b==b'\n'){
  return Err(Error::Invalid);
 }
 let (revision,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if revision!=expected{return Err(Error::Stale);}
 let mut content=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut content[..size]).map_err(|_|Error::Missing)?;
 validate(&content[..size])?;
 let mut out=[0u8;CONTENT_MAX];let mut n=0usize;let mut replaced=false;
 for line in content[..size].split(|b|*b==b'\n'){
  if line.is_empty(){continue;}
  let pos=line.iter().position(|b|*b==b'=').ok_or(Error::Invalid)?;
  let oldkey=&line[..pos];
  let chosen=if oldkey==key{replaced=true;value}else{&line[pos+1..]};
  if n+oldkey.len()+1+chosen.len()+1>CONTENT_MAX{return Err(Error::Full);}
  out[n..n+oldkey.len()].copy_from_slice(oldkey);n+=oldkey.len();
  out[n]=b'=';n+=1;out[n..n+chosen.len()].copy_from_slice(chosen);
  n+=chosen.len();out[n]=b'\n';n+=1;
 }
 if !replaced{
  if n+key.len()+value.len()+2>CONTENT_MAX{return Err(Error::Full);}
  out[n..n+key.len()].copy_from_slice(key);n+=key.len();
  out[n]=b'=';n+=1;out[n..n+value.len()].copy_from_slice(value);
  n+=value.len();out[n]=b'\n';n+=1;
 }
 fs.replace(path,&out[..n]).map_err(|_|Error::Full)
}
pub fn get<const F:usize>(fs:&RamFs<F>,path:&[u8],key:&[u8],
 out:&mut[u8])->Result<usize,Error>{
 let (_,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut text=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut text[..size]).map_err(|_|Error::Missing)?;
 let value=find(&text[..size],key).ok_or(Error::Missing)?;
 if value.len()>out.len(){return Err(Error::Full);}
 out[..value.len()].copy_from_slice(value);Ok(value.len())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn project_settings_write_and_read_existing_config_file(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/p/config",b"theme=light\nbuild=debug\n").unwrap();
  set(&mut fs,b"/p/config",1,b"theme",b"dark").unwrap();
  let mut out=[0u8;16];let n=get(&fs,b"/p/config",b"theme",&mut out).unwrap();
  assert_eq!(&out[..n],b"dark");
  set(&mut fs,b"/p/config",2,b"target",b"x86_64").unwrap();
  let n=get(&fs,b"/p/config",b"target",&mut out).unwrap();
  assert_eq!(&out[..n],b"x86_64");
 }
 #[test]fn malformed_config_and_command_injection_values_are denied(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"bad line").unwrap();
  assert_eq!(set(&mut fs,b"/a",1,b"x",b"y"),Err(Error::Invalid));
  assert_eq!(set(&mut fs,b"/a",1,b"x",b"bad\ncommand"),Err(Error::Invalid));
 }
 #[test]fn stale_config_edit_does_not overwrite current_settings(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"x=one\n").unwrap();
  fs.replace(b"/a",b"x=two\n").unwrap();
  assert_eq!(set(&mut fs,b"/a",1,b"x",b"three"),Err(Error::Stale));
 }
}
