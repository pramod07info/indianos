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


// PR782 WS106-110: bounded ACPI topology decoding. Physical MADT/HPET/FADT
// pointers remain untrusted until separately mapped by the boot memory owner.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct EnabledCpu{pub apic_id:u32,pub x2apic:bool}
/// WS106: Return only enabled, unique LAPIC/x2APIC identities from a
/// checksum-verified MADT. No AP startup, IPI, or IRQ topology is performed.
pub fn parse_enabled_apics(bytes:&[u8],out:&mut[EnabledCpu])
 ->Result<usize,AcpiError>{
 count_madt_records(bytes)?;
 let end=parse_sdt(bytes)?.length as usize;
 let mut staged=[EnabledCpu{apic_id:0,x2apic:false};256];
 let(mut at,mut count)=(44usize,0usize);
 while at<end{
  let kind=bytes[at];let len=bytes[at+1] as usize;
  let candidate=if kind==0{
   let flags=u32::from_le_bytes(bytes[at+4..at+8].try_into()
      .map_err(|_|AcpiError::Malformed)?);
   if flags&1!=0{Some(EnabledCpu{apic_id:bytes[at+3] as u32,x2apic:false})}
   else{None}
  }else if kind==9{
   let flags=u32::from_le_bytes(bytes[at+8..at+12].try_into()
      .map_err(|_|AcpiError::Malformed)?);
   if flags&1!=0{Some(EnabledCpu{apic_id:u32::from_le_bytes(
    bytes[at+4..at+8].try_into().map_err(|_|AcpiError::Malformed)?),x2apic:true})}
   else{None}
  }else{None};
  if let Some(cpu)=candidate{
   if staged[..count].iter().any(|previous|previous.apic_id==cpu.apic_id){
    return Err(AcpiError::Malformed);
   }
   staged[count]=cpu;count+=1;
  }
  at+=len;
 }
 if count>out.len(){return Err(AcpiError::Malformed);}
 out[..count].copy_from_slice(&staged[..count]);Ok(count)
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct IoApic{pub id:u8,pub address:u32,pub gsi_base:u32}
/// WS107: Collect unique mapped IOAPIC declarations. Firmware can still
/// lie: the supplied physical aperture must be checked before MMIO access.
pub fn parse_ioapics(bytes:&[u8],out:&mut[IoApic])->Result<usize,AcpiError>{
 count_madt_records(bytes)?;
 let end=parse_sdt(bytes)?.length as usize;
 let mut stage=[IoApic{id:0,address:0,gsi_base:0};256];
 let(mut at,mut count)=(44usize,0usize);
 while at<end{
  let kind=bytes[at];let len=bytes[at+1] as usize;
  if kind==1{
   let v=IoApic{id:bytes[at+2],address:u32::from_le_bytes(
    bytes[at+4..at+8].try_into().map_err(|_|AcpiError::Malformed)?),
    gsi_base:u32::from_le_bytes(bytes[at+8..at+12].try_into()
    .map_err(|_|AcpiError::Malformed)?)};
   if v.address==0||v.address&4095!=0||
    stage[..count].iter().any(|p|p.id==v.id||p.address==v.address||
      p.gsi_base==v.gsi_base){return Err(AcpiError::Malformed);}
   stage[count]=v;count+=1;
  }
  at+=len;
 }
 if count>out.len(){return Err(AcpiError::Malformed);}
 out[..count].copy_from_slice(&stage[..count]);Ok(count)
}
/// WS108: Validate the HPET timer's generic address structure without
/// mapping the register or enabling timer interrupts.
pub fn hpet_mmio_address(bytes:&[u8])->Result<u64,AcpiError>{
 let table=parse_sdt(bytes)?;
 if table.signature!=*b"HPET"{return Err(AcpiError::Signature);}
 if table.length!=56||bytes[40]!=0||bytes[41]!=64||
    bytes[42]!=0||!matches!(bytes[43],0|4){
  return Err(AcpiError::Malformed);
 }
 let address=u64::from_le_bytes(bytes[44..52].try_into()
  .map_err(|_|AcpiError::Malformed)?);
 let minimum_tick=u16::from_le_bytes(bytes[53..55].try_into()
  .map_err(|_|AcpiError::Malformed)?);
 if address==0||address&7!=0||minimum_tick==0{return Err(AcpiError::Malformed);}
 Ok(address)
}
/// WS109: Validate ACPI FADT reset GAS before exposing a numeric reset
/// register. The caller must validate privileges and perform the actual write.
pub fn fadt_reset_io(bytes:&[u8])->Result<(u64,u8),AcpiError>{
 let table=parse_sdt(bytes)?;
 if table.signature!=*b"FACP"{return Err(AcpiError::Signature);}
 if table.length<129||!matches!(bytes[116],0|1)||
    bytes[117]!=8||bytes[118]!=0||!matches!(bytes[119],0|1){
  return Err(AcpiError::Malformed);
 }
 let address=u64::from_le_bytes(bytes[120..128].try_into()
   .map_err(|_|AcpiError::Malformed)?);
 if address==0||bytes[128]==0{return Err(AcpiError::Malformed);}
 Ok((address,bytes[128]))
}
/// WS110: A MADT may carry one LAPIC address override, never two or an
/// unaligned/zero address. An absent override is reported as None.
pub fn madt_lapic_override(bytes:&[u8])->Result<Option<u64>,AcpiError>{
 count_madt_records(bytes)?;
 let end=parse_sdt(bytes)?.length as usize;
 let mut at=44usize;let mut result=None;
 while at<end{
  let kind=bytes[at];let len=bytes[at+1] as usize;
  if kind==5{
   if len!=12||result.is_some(){return Err(AcpiError::Malformed);}
   let addr=u64::from_le_bytes(bytes[at+4..at+12].try_into()
    .map_err(|_|AcpiError::Malformed)?);
   if addr==0||addr&4095!=0{return Err(AcpiError::Malformed);}
   result=Some(addr);
  }
  at+=len;
 }
 Ok(result)
}
#[cfg(test)]
mod pr782_ws106_110_tests{
 use super::*;
 use std::vec::Vec;
 fn fixed(mut t:Vec<u8>)->Vec<u8>{
  let len=t.len() as u32;t[4..8].copy_from_slice(&len.to_le_bytes());
  t[9]=0;let sum=t.iter().fold(0u8,|s,&b|s.wrapping_add(b));
  t[9]=0u8.wrapping_sub(sum);t
 }
 fn madt(records:&[u8])->Vec<u8>{
  let mut t=std::vec![0u8;44];t[..4].copy_from_slice(b"APIC");
  t.extend_from_slice(records);fixed(t)
 }
 #[test]fn ws106_cpu_apic_ids_are_unique_and_enabled(){
  let t=madt(&[0,8,0,2,1,0,0,0,0,8,0,3,0,0,0,0]);
  let mut out=[EnabledCpu{apic_id:99,x2apic:false};2];
  assert_eq!(parse_enabled_apics(&t,&mut out),Ok(1));
  assert_eq!(out[0].apic_id,2);
  let duplicate=madt(&[0,8,0,2,1,0,0,0,0,8,0,2,1,0,0,0]);
  assert_eq!(parse_enabled_apics(&duplicate,&mut out),Err(AcpiError::Malformed));
  assert_eq!(out[0].apic_id,2);
 }
 #[test]fn ws107_ioapic_geometry_and_duplicate_ownership(){
  let t=madt(&[1,12,3,0,0,0,0xc0,0xfe,0,0,0,0]);
  let mut out=[IoApic{id:0,address:0,gsi_base:0};1];
  assert_eq!(parse_ioapics(&t,&mut out),Ok(1));
  assert_eq!(out[0].address,0xfec0_0000);
  let bad=madt(&[1,12,3,0,1,0,0xc0,0xfe,0,0,0,0]);
  assert_eq!(parse_ioapics(&bad,&mut out),Err(AcpiError::Malformed));
 }
 #[test]fn ws108_hpet_unmapped_and_bad_gas_are_rejected(){
  let mut t=std::vec![0u8;56];t[..4].copy_from_slice(b"HPET");
  t[41]=64;t[43]=4;t[44..52].copy_from_slice(&0xfed0_0000u64.to_le_bytes());
  t[53..55].copy_from_slice(&1u16.to_le_bytes());let t=fixed(t);
  assert_eq!(hpet_mmio_address(&t),Ok(0xfed0_0000));
  let mut bad=t.clone();bad[40]=1;
  assert_eq!(hpet_mmio_address(&fixed(bad)),Err(AcpiError::Malformed));
 }
 #[test]fn ws109_fadt_reset_io_is_structurally_checked(){
  let mut t=std::vec![0u8;129];t[..4].copy_from_slice(b"FACP");
  t[116]=1;t[117]=8;t[119]=1;t[120..128].copy_from_slice(&0xcf9u64.to_le_bytes());
  t[128]=6;let t=fixed(t);
  assert_eq!(fadt_reset_io(&t),Ok((0xcf9,6)));
  let mut bad=t.clone();bad[128]=0;
  assert_eq!(fadt_reset_io(&fixed(bad)),Err(AcpiError::Malformed));
 }
 #[test]fn ws110_duplicate_or_invalid_lapic_override_is_rejected(){
  let single=madt(&[5,12,0,0,0,0,0xe0,0xfe,0,0,0,0]);
  assert_eq!(madt_lapic_override(&single),Ok(Some(0xfee0_0000)));
  let duplicate=madt(&[5,12,0,0,0,0,0xe0,0xfe,0,0,0,0,
    5,12,0,0,0,0,0xe0,0xfe,0,0,0,0]);
  assert_eq!(madt_lapic_override(&duplicate),Err(AcpiError::Malformed));
 }
}
