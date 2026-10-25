//! R038/36: checked Win64 PE/COFF header identification (not a Win32 runtime).
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub struct Header{pub machine:u16,pub sections:u16,pub optional_magic:u16}
#[derive(Debug,PartialEq,Eq)]pub enum Error{Malformed,Unsupported}
pub fn parse(bytes:&[u8])->Result<Header,Error>{
 if bytes.len()<64||&bytes[..2]!=b"MZ"{return Err(Error::Malformed);}
 let offset=u32::from_le_bytes(bytes[60..64].try_into().map_err(|_|Error::Malformed)?) as usize;
 let end=offset.checked_add(26).ok_or(Error::Malformed)?;
 if end>bytes.len()||&bytes[offset..offset+4]!=b"PE\0\0"{return Err(Error::Malformed);}
 let machine=u16::from_le_bytes(bytes[offset+4..offset+6].try_into().map_err(|_|Error::Malformed)?);
 let sections=u16::from_le_bytes(bytes[offset+6..offset+8].try_into().map_err(|_|Error::Malformed)?);
 let opt_bytes=u16::from_le_bytes(bytes[offset+20..offset+22].try_into().map_err(|_|Error::Malformed)?) as usize;
 if opt_bytes<2||offset.checked_add(24+opt_bytes).is_none_or(|v|v>bytes.len())||sections==0||sections>96{
  return Err(Error::Malformed);
 }
 let optional_magic=u16::from_le_bytes(bytes[offset+24..offset+26].try_into().map_err(|_|Error::Malformed)?);
 if machine!=0x8664||optional_magic!=0x20b{return Err(Error::Unsupported);}
 Ok(Header{machine,sections,optional_magic})
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn malformed_exe_rejected(){assert_eq!(parse(&[]),Err(Error::Malformed));}
 #[test]fn valid_pe32_plus_header(){
  let mut b=[0u8;100];b[..2].copy_from_slice(b"MZ");b[60..64].copy_from_slice(&64u32.to_le_bytes());
  b[64..68].copy_from_slice(b"PE\0\0");b[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
  b[70..72].copy_from_slice(&1u16.to_le_bytes());b[84..86].copy_from_slice(&2u16.to_le_bytes());
  b[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
  assert_eq!(parse(&b).unwrap().machine,0x8664);
 }
}