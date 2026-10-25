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
    if revision>0{
        if bytes.len()<36{return Err(AcpiError::Truncated);}
        let length=u32::from_le_bytes(bytes[20..24].try_into().map_err(|_|AcpiError::Truncated)?);
        if length<36||length>4096{return Err(AcpiError::Malformed);}
        if bytes.len()<length as usize{return Err(AcpiError::Truncated);}
        if !checksum_ok(&bytes[..length as usize]){return Err(AcpiError::Checksum);}
        let xsdt=u64::from_le_bytes(bytes[24..32].try_into().map_err(|_|AcpiError::Truncated)?);
        if xsdt!=0{return Ok(Rsdp{revision,root_address:xsdt,xsdt:true});}
    }
    let rsdt=u32::from_le_bytes(bytes[16..20].try_into().map_err(|_|AcpiError::Truncated)?);
    if rsdt==0{return Err(AcpiError::Malformed);}
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
    #[test]fn accepts_valid_sdt_header(){
        let mut b=[0u8;36];
        b[..4].copy_from_slice(b"APIC");
        b[4..8].copy_from_slice(&36u32.to_le_bytes());
        let sum=b.iter().fold(0u8,|v,x|v.wrapping_add(*x));
        b[9]=0u8.wrapping_sub(sum);
        assert_eq!(parse_sdt(&b),Ok(Table{signature:*b"APIC",length:36,revision:0}));
    }
}
