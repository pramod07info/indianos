//! M2 bounded static x86_64 ELF64 payload validation, no allocations.
//! Parsing only: this does NOT map segments, grant privileges, or execute apps.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Malformed,Unsupported,Unsafe}
const ELF_HEADER:usize=64;
const PH_SIZE:usize=56;
fn word(bytes:&[u8],offset:usize)->Result<u16,Error>{
 let part=bytes.get(offset..offset+2).ok_or(Error::Malformed)?;
 Ok(u16::from_le_bytes([part[0],part[1]]))
}
fn dword(bytes:&[u8],offset:usize)->Result<u32,Error>{
 let part=bytes.get(offset..offset+4).ok_or(Error::Malformed)?;
 Ok(u32::from_le_bytes(part.try_into().unwrap()))
}
fn qword(bytes:&[u8],offset:usize)->Result<u64,Error>{
 let part=bytes.get(offset..offset+8).ok_or(Error::Malformed)?;
 Ok(u64::from_le_bytes(part.try_into().unwrap()))
}
pub fn validate_static(image:&[u8])->Result<(),Error>{
 if image.len()<ELF_HEADER||&image[..4]!=b"\x7fELF"{
  return Err(Error::Malformed);
 }
 if image[4]!=2||image[5]!=1||image[6]!=1||word(image,16)?!=2||
    word(image,18)?!=62||dword(image,20)?!=1{
  return Err(Error::Unsupported);
 }
 if word(image,52)?!=64||word(image,54)?!=PH_SIZE as u16{
  return Err(Error::Malformed);
 }
 let count=word(image,56)? as usize;
 if count==0||count>16{return Err(Error::Unsafe);}
 let offset=usize::try_from(qword(image,32)?).map_err(|_|Error::Unsafe)?;
 if offset<ELF_HEADER||offset.checked_add(count*PH_SIZE).is_none_or(|n|n>image.len()){
  return Err(Error::Malformed);
 }
 let entry=qword(image,24)?;
 if !(0x400000..0x0000_8000_0000_0000).contains(&entry){
  return Err(Error::Unsafe);
 }
 let mut executable=false;
 let mut segments=0u8;
 for index in 0..count{
  let ph=offset+index*PH_SIZE;
  let kind=dword(image,ph)?;
  if kind==0 {continue;}
  // Reject interpreters, dynamically linked images, TLS and all unsupported
  // program headers. Linux ABI emulation is outside this milestone.
  if kind!=1{return Err(Error::Unsupported);}
  let flags=dword(image,ph+4)?;
  if flags&!7!=0||flags&4==0||flags&3==3{return Err(Error::Unsafe);}
  let file=usize::try_from(qword(image,ph+8)?).map_err(|_|Error::Unsafe)?;
  let address=qword(image,ph+16)?;
  let filesize=qword(image,ph+32)?;
  let memsize=qword(image,ph+40)?;
  let align=qword(image,ph+48)?;
  if memsize==0||filesize>memsize||
     file.checked_add(usize::try_from(filesize).map_err(|_|Error::Unsafe)?)
         .is_none_or(|n|n>image.len())||
     address<0x400000||
     address.checked_add(memsize).is_none_or(|end|end>0x0000_8000_0000_0000)||
     (align>1&&!align.is_power_of_two())||
     (align>1 && (address%align)!=(file as u64%align)){
    return Err(Error::Unsafe);
  }
  if flags&1!=0&& entry>=address&&entry<address+filesize{
   executable=true;
  }
  segments+=1;
 }
 if !executable||segments==0||
    crate::program::plan_elf64(image).is_none(){return Err(Error::Unsafe);}
 Ok(())
}
#[cfg(test)]pub(crate) mod tests{
 use super::*;
 pub(crate) fn sample()->[u8;124]{
  let mut elf=[0u8;124];
  elf[..4].copy_from_slice(b"\x7fELF");
  elf[4]=2;elf[5]=1;elf[6]=1;
  elf[16..18].copy_from_slice(&2u16.to_le_bytes());
  elf[18..20].copy_from_slice(&62u16.to_le_bytes());
  elf[20..24].copy_from_slice(&1u32.to_le_bytes());
  elf[24..32].copy_from_slice(&0x400078u64.to_le_bytes());
  elf[32..40].copy_from_slice(&64u64.to_le_bytes());
  elf[52..54].copy_from_slice(&64u16.to_le_bytes());
  elf[54..56].copy_from_slice(&56u16.to_le_bytes());
  elf[56..58].copy_from_slice(&1u16.to_le_bytes());
  elf[64..68].copy_from_slice(&1u32.to_le_bytes());
  elf[68..72].copy_from_slice(&5u32.to_le_bytes()); // RX, never WX
  elf[72..80].copy_from_slice(&0u64.to_le_bytes());
  elf[80..88].copy_from_slice(&0x400000u64.to_le_bytes());
  elf[96..104].copy_from_slice(&124u64.to_le_bytes());
  elf[104..112].copy_from_slice(&124u64.to_le_bytes());
  elf[112..120].copy_from_slice(&4096u64.to_le_bytes());
  elf[120..124].copy_from_slice(&[7,8,9,10]);
  elf
 }
 #[test]fn m2_static_elf_parser_refuses_dynamic_wx_invalid_and_short(){
  let image=sample();assert_eq!(validate_static(&image),Ok(()));
  assert!(crate::program::plan_elf64(&image).is_some());
  let mut wx=image;wx[68]=7;
  assert_eq!(validate_static(&wx),Err(Error::Unsafe));
  let mut dynamic=image;dynamic[16]=3;
  assert_eq!(validate_static(&dynamic),Err(Error::Unsupported));
  let mut malicious=image;malicious[96..104].copy_from_slice(&5000u64.to_le_bytes());
  assert_eq!(validate_static(&malicious),Err(Error::Unsafe));
  assert_eq!(validate_static(&image[..60]),Err(Error::Malformed));
  let mut unbacked=image;
  unbacked[24..32].copy_from_slice(&0x40007du64.to_le_bytes());
  assert_eq!(validate_static(&unbacked),Err(Error::Unsafe));
  let mut no_read=image;no_read[68]=1;
  assert_eq!(validate_static(&no_read),Err(Error::Unsafe));
  let mut overlap=image;overlap[72..80].copy_from_slice(&120u64.to_le_bytes());
  assert_eq!(validate_static(&overlap),Err(Error::Unsafe));
 }
}
