//! R037/25: defensive ACPI RSDP + SDT header decoding.
//! Data comes from a bootloader-validated mapping; no physical dereference here.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Rsdp{pub revision:u8,pub root_address:u64,pub xsdt:bool}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Table{pub signature:[u8;4],pub length:u32,pub revision:u8}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum AcpiError{Truncated,Signature,Checksum,Malformed}
pub fn checksum_ok(bytes:&[u8])->bool{
    bytes.iter().fold(0u8,|sum,&b|sum.wrapping_add(b))==0
}
pub fn parse_rsdp(bytes:&[u8])->Result<Rsdp,AcpiError>{
    if bytes.len()<20{return Err(AcpiError::Truncated);}
    if &bytes[..8]!=b"RSD PTR "{return Err(AcpiError::Signature);}
    if !checksum_ok(&bytes[..20]){return Err(AcpiError::Checksum);}
    let revision=bytes[15];
    // ACPI RSDP revision 1 is reserved; v1 is 0 and extended RSDP is >=2.
    if revision==1{return Err(AcpiError::Malformed);}
    if revision>0{
        if bytes.len()<36{return Err(AcpiError::Truncated);}
        let length=u32::from_le_bytes(bytes[20..24].try_into().map_err(|_|AcpiError::Truncated)?);
        if length<36||length>4096{return Err(AcpiError::Malformed);}
        if bytes.len()<length as usize{return Err(AcpiError::Truncated);}
        if !checksum_ok(&bytes[..length as usize]){return Err(AcpiError::Checksum);}
        // ACPI 2+ reserves the final three RSDP bytes. Reject a malformed
        // firmware handoff before ever interpreting a physical table address.
        if bytes[33..36].iter().any(|&byte|byte!=0){return Err(AcpiError::Malformed);}
        let xsdt=u64::from_le_bytes(bytes[24..32].try_into().map_err(|_|AcpiError::Truncated)?);
        if xsdt!=0{
            if xsdt&7!=0{return Err(AcpiError::Malformed);}
            return Ok(Rsdp{revision,root_address:xsdt,xsdt:true});
        }
    }
    let rsdt=u32::from_le_bytes(bytes[16..20].try_into().map_err(|_|AcpiError::Truncated)?);
    if rsdt==0||rsdt&3!=0{return Err(AcpiError::Malformed);}
    Ok(Rsdp{revision,root_address:rsdt as u64,xsdt:false})
}
pub fn parse_sdt(bytes:&[u8])->Result<Table,AcpiError>{
    if bytes.len()<36{return Err(AcpiError::Truncated);}
    let length=u32::from_le_bytes(bytes[4..8].try_into().map_err(|_|AcpiError::Truncated)?);
    if length<36||length>1024*1024{return Err(AcpiError::Malformed);}
    if bytes.len()<length as usize{return Err(AcpiError::Truncated);}
    if !checksum_ok(&bytes[..length as usize]){return Err(AcpiError::Checksum);}
    let signature:[u8;4]=bytes[..4].try_into().map_err(|_|AcpiError::Truncated)?;
    if !signature.iter().all(|c|c.is_ascii_uppercase()||c.is_ascii_digit()){
        return Err(AcpiError::Signature);
    }
    Ok(Table{signature,length,revision:bytes[8]})
}
#[cfg(test)]
mod tests{
    use super::*;
    fn rsdp()->[u8;20]{
        let mut b=[0u8;20];b[..8].copy_from_slice(b"RSD PTR ");
        b[16..20].copy_from_slice(&0x1000u32.to_le_bytes());
        let sum=b[..20].iter().fold(0u8,|v,x|v.wrapping_add(*x));
        b[8]=0u8.wrapping_sub(sum);b
    }
    #[test]fn detects_rsdt_and_bad_checksum(){
        let b=rsdp();
        assert_eq!(parse_rsdp(&b),Ok(Rsdp{revision:0,root_address:0x1000,xsdt:false}));
        let mut invalid=b;invalid[19]^=1;
        assert_eq!(parse_rsdp(&invalid),Err(AcpiError::Checksum));
    }
    #[test]fn rejects_untrusted_length_and_truncated_sdt(){
        let mut b=[0u8;36];
        b[..4].copy_from_slice(b"APIC");
        b[4..8].copy_from_slice(&100u32.to_le_bytes());
        assert_eq!(parse_sdt(&b),Err(AcpiError::Truncated));
        assert_eq!(parse_rsdp(&[0u8;5]),Err(AcpiError::Truncated));
    }
    #[test]fn g8_firmware_rsdp_v2_rejects_reserved_bytes_and_bad_root_alignment(){
        fn fix_crc(bytes:&mut[u8],index:usize){
            bytes[index]=0;
            let sum=bytes.iter().fold(0u8,|s,&byte|s.wrapping_add(byte));
            bytes[index]=0u8.wrapping_sub(sum);
        }
        let mut v2=[0u8;36];
        v2[..8].copy_from_slice(b"RSD PTR ");
        v2[15]=2;
        v2[16..20].copy_from_slice(&0x1000u32.to_le_bytes());
        v2[20..24].copy_from_slice(&36u32.to_le_bytes());
        v2[24..32].copy_from_slice(&0x2000u64.to_le_bytes());
        fix_crc(&mut v2[..20],8);
        fix_crc(&mut v2[..],32);
        assert_eq!(parse_rsdp(&v2),Ok(Rsdp{
            revision:2,root_address:0x2000,xsdt:true
        }));
        let mut bad_reserved=v2;bad_reserved[33]=1;
        fix_crc(&mut bad_reserved[..],32);
        assert_eq!(parse_rsdp(&bad_reserved),Err(AcpiError::Malformed));
        let mut unaligned=v2;
        unaligned[24..32].copy_from_slice(&0x2004u64.to_le_bytes());
        fix_crc(&mut unaligned[..],32);
        assert_eq!(parse_rsdp(&unaligned),Err(AcpiError::Malformed));
        let mut bad_rsdt=v2;
        bad_rsdt[24..32].fill(0);
        bad_rsdt[16..20].copy_from_slice(&0x1002u32.to_le_bytes());
        fix_crc(&mut bad_rsdt[..20],8);
        fix_crc(&mut bad_rsdt[..],32);
        assert_eq!(parse_rsdp(&bad_rsdt),Err(AcpiError::Malformed));
    }
    #[test]fn accepts_valid_sdt_header(){
        let mut b=[0u8;36];
        b[..4].copy_from_slice(b"APIC");
        b[4..8].copy_from_slice(&36u32.to_le_bytes());
        let sum=b.iter().fold(0u8,|v,x|v.wrapping_add(*x));
        b[9]=0u8.wrapping_sub(sum);
        assert_eq!(parse_sdt(&b),Ok(Table{signature:*b"APIC",length:36,revision:0}));
    }
}

#[cfg(test)]
mod m1_ws04_acpi_revision_tests {
 use super::*;
 #[test] fn reject_reserved_revision_without_interpreting_extended_length() {
  let mut rsdp=[0u8;20];rsdp[..8].copy_from_slice(b"RSD PTR ");
  rsdp[15]=1;rsdp[16..20].copy_from_slice(&0x1000u32.to_le_bytes());
  let sum=rsdp.iter().fold(0u8,|s,&b|s.wrapping_add(b));
  rsdp[8]=rsdp[8].wrapping_sub(sum);
  assert_eq!(parse_rsdp(&rsdp),Err(AcpiError::Malformed));
  rsdp[15]=0;let sum=rsdp.iter().fold(0u8,|s,&b|s.wrapping_add(b));
  rsdp[8]=rsdp[8].wrapping_sub(sum);
  assert_eq!(parse_rsdp(&rsdp),Ok(Rsdp{
   revision:0,root_address:0x1000,xsdt:false}));
 }
}

/// Decode RSDT/XSDT child addresses without touching physical firmware memory.
/// A trusted mapper must separately validate every referenced SDT aperture.
/// The entire caller buffer is preserved on invalid entries or capacity.
pub fn parse_root_addresses(bytes:&[u8],out:&mut[u64])->Result<usize,AcpiError>{
 let table=parse_sdt(bytes)?;
 let width=if table.signature==*b"XSDT"{8}
    else if table.signature==*b"RSDT"{4}
    else{return Err(AcpiError::Signature)};
 let len=table.length as usize;
 if len<=36||(len-36)%width!=0{return Err(AcpiError::Malformed);}
 let count=(len-36)/width;
 if count>out.len()||count>256{return Err(AcpiError::Malformed);}
 let read=|index:usize|->Result<u64,AcpiError>{
  let start=36+index*width;
  if width==8{Ok(u64::from_le_bytes(bytes[start..start+8]
      .try_into().map_err(|_|AcpiError::Truncated)?))}
  else{Ok(u64::from(u32::from_le_bytes(bytes[start..start+4]
      .try_into().map_err(|_|AcpiError::Truncated)?)))}
 };
 for i in 0..count{
  let addr=read(i)?;
  if addr==0||addr%(width as u64)!=0{return Err(AcpiError::Malformed);}
  for j in 0..i{if read(j)?==addr{return Err(AcpiError::Malformed);}}
 }
 for (i,slot) in out[..count].iter_mut().enumerate(){*slot=read(i)?;}
 Ok(count)
}
#[cfg(test)]
mod ws25_acpi_root_table_tests{
 use super::*;
 fn fixture()->[u8;52]{
  let mut t=[0u8;52];t[..4].copy_from_slice(b"XSDT");
  t[4..8].copy_from_slice(&(52u32).to_le_bytes());
  t[36..44].copy_from_slice(&0x1000u64.to_le_bytes());
  t[44..52].copy_from_slice(&0x2000u64.to_le_bytes());
  let sum=t.iter().fold(0u8,|sum,&b|sum.wrapping_add(b));
  t[9]=0u8.wrapping_sub(sum);t
 }
 fn fix(t:&mut[u8]){t[9]=0;let sum=t.iter().fold(0u8,|s,&b|s.wrapping_add(b));
  t[9]=0u8.wrapping_sub(sum);}
 #[test]fn rejects_ambiguous_and_invalid_root_entries_transactionally(){
  let t=fixture();let mut out=[7u64;2];
  assert_eq!(parse_root_addresses(&t,&mut out),Ok(2));
  assert_eq!(out,[0x1000,0x2000]);
  let mut short=[9u64;1];
  assert_eq!(parse_root_addresses(&t,&mut short),Err(AcpiError::Malformed));
  assert_eq!(short,[9]);
  let mut duplicate=t;duplicate[44..52].copy_from_slice(&0x1000u64.to_le_bytes());
  fix(&mut duplicate);let mut unchanged=[9u64;2];
  assert_eq!(parse_root_addresses(&duplicate,&mut unchanged),Err(AcpiError::Malformed));
  assert_eq!(unchanged,[9;2]);
  duplicate[44..52].copy_from_slice(&0x2001u64.to_le_bytes());fix(&mut duplicate);
  assert_eq!(parse_root_addresses(&duplicate,&mut unchanged),Err(AcpiError::Malformed));
  assert_eq!(unchanged,[9;2]);
 }
}


// PR #782 WS42-44: mapped ACPI roots and bounded firmware structure checks.
/// WS42: Transactionally copy root entries only when an external boot-memory
/// mapping authority verifies each full SDT header. No physical dereference.
pub fn parse_mapped_root_addresses<F>(bytes:&[u8],out:&mut[u64],
 mut mapped:F)->Result<usize,AcpiError> where F:FnMut(u64,u64)->bool{
 let mut staged=[0u64;256];
 let n=parse_root_addresses(bytes,&mut staged)?;
 if n>out.len(){return Err(AcpiError::Malformed);}
 for &address in &staged[..n]{
  if !mapped(address,36){return Err(AcpiError::Malformed);}
 }
 out[..n].copy_from_slice(&staged[..n]);Ok(n)
}
/// WS43: Validate the entire MADT variable-record chain without trusting
/// firmware-provided entry lengths or silently accepting zero-size records.
pub fn count_madt_records(bytes:&[u8])->Result<usize,AcpiError>{
 let table=parse_sdt(bytes)?;
 if table.signature!=*b"APIC"{return Err(AcpiError::Signature);}
 if table.length<44{return Err(AcpiError::Malformed);}
 let mut at=44usize;let end=table.length as usize;let mut count=0;
 while at<end{
  if end-at<2{return Err(AcpiError::Truncated);}
  let kind=bytes[at];let len=usize::from(bytes[at+1]);
  if len<2||len>end-at{return Err(AcpiError::Malformed);}
  let expected=match kind{
   0=>Some(8),1=>Some(12),2=>Some(10),9=>Some(16),_=>None
  };
  if expected.is_some_and(|n|n!=len){return Err(AcpiError::Malformed);}
  count+=1;
  if count>256{return Err(AcpiError::Malformed);}
  at+=len;
 }
 Ok(count)
}
/// WS44: Check the mapped SDT physical aperture against a supplied trusted
/// boot-memory allocation [start,start+length), without computing unchecked
/// virtual pointers or reading unmapped memory.
pub fn validate_sdt_aperture(bytes:&[u8],address:u64,
 mapped_start:u64,mapped_length:u64)->Result<Table,AcpiError>{
 let table=parse_sdt(bytes)?;
 if address==0||address&3!=0||mapped_length==0 {
  return Err(AcpiError::Malformed);
 }
 let mapped_end=mapped_start.checked_add(mapped_length).ok_or(AcpiError::Malformed)?;
 let table_end=address.checked_add(u64::from(table.length))
  .ok_or(AcpiError::Malformed)?;
 if address<mapped_start||table_end>mapped_end{return Err(AcpiError::Malformed);}
 Ok(table)
}
#[cfg(test)]
mod pr782_ws42_44_tests{
 use super::*;
 fn fix(t:&mut[u8]){
  t[9]=0;let sum=t.iter().fold(0u8,|s,&v|s.wrapping_add(v));
  t[9]=0u8.wrapping_sub(sum);
 }
 fn xsdt()->[u8;52]{
  let mut t=[0u8;52];t[..4].copy_from_slice(b"XSDT");
  t[4..8].copy_from_slice(&52u32.to_le_bytes());
  t[36..44].copy_from_slice(&0x1000u64.to_le_bytes());
  t[44..52].copy_from_slice(&0x2000u64.to_le_bytes());fix(&mut t);t
 }
 #[test]fn ws42_unowned_root_leaves_output_unchanged(){
  let t=xsdt();let mut out=[99u64;2];
  assert_eq!(parse_mapped_root_addresses(&t,&mut out,|addr,_|addr==0x1000),
   Err(AcpiError::Malformed));
  assert_eq!(out,[99;2]);
  assert_eq!(parse_mapped_root_addresses(&t,&mut out,|_,n|n==36),Ok(2));
  assert_eq!(out,[0x1000,0x2000]);
 }
 #[test]fn ws43_madt_records_have_bounded_nonzero_lengths(){
  let mut t=[0u8;52];t[..4].copy_from_slice(b"APIC");
  t[4..8].copy_from_slice(&52u32.to_le_bytes());
  t[44]=0;t[45]=8;fix(&mut t);
  assert_eq!(count_madt_records(&t),Ok(1));
  t[45]=0;fix(&mut t);
  assert_eq!(count_madt_records(&t),Err(AcpiError::Malformed));
  t[45]=9;fix(&mut t);
  assert_eq!(count_madt_records(&t),Err(AcpiError::Malformed));
 }
 #[test]fn ws44_table_span_must_fit_the_trusted_mapping(){
  let t=xsdt();
  assert_eq!(validate_sdt_aperture(&t,0x1000,0x1000,52),
   Ok(Table{signature:*b"XSDT",length:52,revision:0}));
  assert_eq!(validate_sdt_aperture(&t,0x1000,0x1000,51),
   Err(AcpiError::Malformed));
  assert_eq!(validate_sdt_aperture(&t,0x1000,u64::MAX-5,16),
   Err(AcpiError::Malformed));
 }
}
