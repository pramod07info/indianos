//! R038/37: APK/ZIP first-entry preflight; not an Android runtime.
#[derive(Debug,PartialEq,Eq)]pub enum Error{Malformed,Unsupported,Traversal}
pub fn first_name(bytes:&[u8])->Result<&[u8],Error>{
 if bytes.len()<30||&bytes[..4]!=b"PK\x03\x04"{return Err(Error::Malformed);}
 let flags=u16::from_le_bytes(bytes[6..8].try_into().map_err(|_|Error::Malformed)?);
 let method=u16::from_le_bytes(bytes[8..10].try_into().map_err(|_|Error::Malformed)?);
 if flags&9!=0||!(method==0||method==8){return Err(Error::Unsupported);}
 let name_len=u16::from_le_bytes(bytes[26..28].try_into().map_err(|_|Error::Malformed)?) as usize;
 let extra_len=u16::from_le_bytes(bytes[28..30].try_into().map_err(|_|Error::Malformed)?) as usize;
 if name_len==0||name_len>256||30usize.checked_add(name_len).and_then(|x|x.checked_add(extra_len))
  .is_none_or(|end|end>bytes.len()){return Err(Error::Malformed);}
 let name=&bytes[30..30+name_len];
 if name[0]==b'/'||name.contains(&b'\\')||name.contains(&0)||
  name.split(|&b|b==b'/').any(|p|p==b".."||p.is_empty()){return Err(Error::Traversal);}
 Ok(name)
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn traversal_denied(){
  let mut v=[0u8;64];v[..4].copy_from_slice(b"PK\x03\x04");
  v[26..28].copy_from_slice(&6u16.to_le_bytes());v[30..36].copy_from_slice(b"../bad");
  assert_eq!(first_name(&v),Err(Error::Traversal));
 }
 #[test]fn invalid_archive_rejected(){assert_eq!(first_name(&[]),Err(Error::Malformed));}
}