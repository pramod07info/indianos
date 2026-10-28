//! Bounded numeric PCI BAR aperture check. Never maps or accesses MMIO.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aperture { pub base: u64, pub length: u64 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApertureError { Alignment, Empty, Overflow, OutsideAllowed }

/// The upper bound is exclusive, as is the aperture endpoint.
pub fn validate(aperture: Aperture, lower: u64, upper: u64, align: u64)
    -> Result<(), ApertureError>
{
    if align == 0 || !align.is_power_of_two() { return Err(ApertureError::Alignment); }
    if aperture.length == 0 { return Err(ApertureError::Empty); }
    if aperture.base % align != 0 || aperture.length % align != 0 {
        return Err(ApertureError::Alignment);
    }
    let end = aperture.base.checked_add(aperture.length).ok_or(ApertureError::Overflow)?;
    if lower >= upper || aperture.base < lower || end > upper {
        return Err(ApertureError::OutsideAllowed);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allows_page_aligned_aperture_in_reserved_range() {
        assert_eq!(validate(Aperture { base: 0x2000, length: 0x1000 },
            0x1000, 0x4000, 0x1000), Ok(()));
    }
    #[test]
    fn rejects_invalid_alignment_and_zero_length() {
        assert_eq!(validate(Aperture { base: 0x2001, length: 0x1000 },
            0x1000, 0x4000, 0x1000), Err(ApertureError::Alignment));
        assert_eq!(validate(Aperture { base: 0x2000, length: 0 },
            0x1000, 0x4000, 0x1000), Err(ApertureError::Empty));
        assert_eq!(validate(Aperture { base: 0x2000, length: 0x1000 },
            0x1000, 0x4000, 3), Err(ApertureError::Alignment));
    }
    #[test]
    fn handles_overflow_and_exclusive_boundary() {
        assert_eq!(validate(Aperture { base: u64::MAX - 0x0fff, length: 0x2000 },
            0, u64::MAX, 0x1000), Err(ApertureError::Overflow));
        assert_eq!(validate(Aperture { base: 0x3000, length: 0x2000 },
            0x1000, 0x4000, 0x1000), Err(ApertureError::OutsideAllowed));
    }
}

/// Preflight a complete MMIO aperture set before mapping anything. This
/// numeric-only check rejects cross-device overlapping BAR ranges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetError { Invalid(ApertureError), Overlap, TooMany }
pub const MAX_APERTURES: usize = 32;
pub fn validate_set(regions: &[Aperture], lower: u64, upper: u64, align: u64)
    -> Result<(), SetError>
{
    if regions.len() > MAX_APERTURES { return Err(SetError::TooMany); }
    for (i, region) in regions.iter().enumerate() {
        validate(*region, lower, upper, align).map_err(SetError::Invalid)?;
        let end = region.base + region.length; // validated non-overflow above
        for earlier in &regions[..i] {
            let earlier_end = earlier.base + earlier.length;
            if region.base < earlier_end && earlier.base < end {
                return Err(SetError::Overlap);
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod set_tests {
    use super::*;
    #[test]
    fn adjacent_bar_regions_are_allowed() {
        let items = [Aperture { base: 0x1000, length: 0x1000 },
            Aperture { base: 0x2000, length: 0x1000 }];
        assert_eq!(validate_set(&items, 0x1000, 0x4000, 0x1000), Ok(()));
    }
    #[test]
    fn overlapping_bars_cannot_be_mapped() {
        let items = [Aperture { base: 0x1000, length: 0x2000 },
            Aperture { base: 0x2000, length: 0x1000 }];
        assert_eq!(validate_set(&items, 0x1000, 0x5000, 0x1000), Err(SetError::Overlap));
    }
    #[test]
    fn invalid_or_oversized_sets_fail_closed() {
        let many = [Aperture { base: 0x1000, length: 0x1000 }; MAX_APERTURES + 1];
        assert_eq!(validate_set(&many, 0, 0x10000, 0x1000), Err(SetError::TooMany));
        assert_eq!(validate_set(&[Aperture { base: 0, length: 0 }], 0, 10, 1),
            Err(SetError::Invalid(ApertureError::Empty)));
    }
}


/// Decode read-only PCI configuration bytes supplied by a trusted enumerator.
/// This routine never performs IN/OUT instructions, maps BARs or enables DMA.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub vendor: u16, pub device: u16, pub class: u8,
    pub subclass: u8, pub programming_interface: u8, pub header_type: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError { TooShort, Absent, InvalidHeader, NotStorage }

pub fn decode_config(header: &[u8]) -> Result<DeviceInfo, ConfigError> {
    if header.len() < 16 { return Err(ConfigError::TooShort); }
    let vendor = u16::from_le_bytes([header[0], header[1]]);
    let device = u16::from_le_bytes([header[2], header[3]]);
    if vendor == 0xffff || vendor == 0 || device == 0xffff { return Err(ConfigError::Absent); }
    let header_type = header[14] & 0x7f;
    if header_type > 1 { return Err(ConfigError::InvalidHeader); }
    Ok(DeviceInfo {
        vendor, device, programming_interface: header[9],
        subclass: header[10], class: header[11], header_type,
    })
}

/// Restrict experimental disk drivers to recognized controller classes.
/// AHCI/NVMe require separate audited drivers; reporting is not activation.
pub fn storage_controller(header: &[u8]) -> Result<DeviceInfo, ConfigError> {
    let info = decode_config(header)?;
    if info.class != 0x01 || !matches!(info.subclass, 0x01 | 0x06 | 0x08) {
        return Err(ConfigError::NotStorage);
    }
    Ok(info)
}
#[cfg(test)]
mod pci_discovery_tests {
    use super::*;
    fn header(class: u8, subclass: u8) -> [u8; 16] {
        let mut h = [0; 16];
        h[0..2].copy_from_slice(&0x8086u16.to_le_bytes());
        h[2..4].copy_from_slice(&0x2922u16.to_le_bytes());
        h[11] = class; h[10] = subclass; h
    }
    #[test]
    fn recognizes_storage_class_without_enabling_hardware() {
        let ide = storage_controller(&header(1, 1)).unwrap();
        assert_eq!((ide.vendor, ide.class, ide.subclass), (0x8086, 1, 1));
        assert_eq!(storage_controller(&header(1, 6)).unwrap().subclass, 6);
        assert_eq!(storage_controller(&header(1, 8)).unwrap().subclass, 8);
    }
    #[test]
    fn rejects_network_devices_missing_and_truncated_headers() {
        assert_eq!(storage_controller(&header(2, 0)), Err(ConfigError::NotStorage));
        assert_eq!(decode_config(&[0; 4]), Err(ConfigError::TooShort));
        assert_eq!(decode_config(&[0xff; 16]), Err(ConfigError::Absent));
    }
    #[test]
    fn rejects_invalid_header_and_no_device() {
        let mut h = header(1, 1);
        h[14] = 0x82; // multifunction bit is legal; type 2 is not
        assert_eq!(decode_config(&h), Err(ConfigError::InvalidHeader));
        h[14] = 0x80;
        assert_eq!(decode_config(&h).unwrap().header_type, 0);
        h[0..2].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(decode_config(&h), Err(ConfigError::Absent));
    }
}


/// Batch 1: consume immutable config-space header snapshots from a trusted
/// PCI enumerator. No bus I/O, BAR mapping or storage-controller activation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveryError { TooMany, InsufficientOutput, Config(ConfigError) }
pub const MAX_PCI_HEADERS: usize = 256;
pub fn discover_storage_headers(headers: &[[u8;16]],
    output: &mut [Option<DeviceInfo>]) -> Result<usize,DiscoveryError> {
    if headers.len()>MAX_PCI_HEADERS { return Err(DiscoveryError::TooMany); }
    let mut count=0;
    // Preflight the entire input before writing the first output slot.
    for h in headers {
        match storage_controller(h) {
            Ok(_) => count+=1,
            Err(ConfigError::Absent | ConfigError::NotStorage) => {},
            Err(e) => return Err(DiscoveryError::Config(e)),
        }
    }
    if count > output.len(){return Err(DiscoveryError::InsufficientOutput);}
    let mut n=0;
    for h in headers {
        if let Ok(device)=storage_controller(h){
            output[n]=Some(device);
            n+=1;
        }
    }
    Ok(n)
}
#[cfg(test)]
mod batch1_pci_snapshot_tests {
    use super::*;
    fn header(class:u8,subclass:u8)->[u8;16]{
        let mut h=[0u8;16];
        h[..2].copy_from_slice(&0x8086u16.to_le_bytes());
        h[2..4].copy_from_slice(&0x2922u16.to_le_bytes());
        h[11]=class;h[10]=subclass;h
    }
    #[test]
    fn filters_storage_classes_from_trusted_config_snapshots() {
        let headers=[header(2,0),header(1,6),header(1,8),[0xff;16]];
        let mut output=[None;2];
        assert_eq!(discover_storage_headers(&headers,&mut output),Ok(2));
        assert_eq!(output[0].unwrap().subclass,6);
        assert_eq!(output[1].unwrap().subclass,8);
    }
    #[test]
    fn invalid_header_and_capacity_fail_before_writing() {
        let mut bad=header(1,6);bad[14]=2;
        let sentinel=Some(storage_controller(&header(1,1)).unwrap());
        let mut output=[sentinel;1];
        assert_eq!(discover_storage_headers(&[header(1,6),bad],&mut output),
            Err(DiscoveryError::Config(ConfigError::InvalidHeader)));
        assert_eq!(output,[sentinel]);
        assert_eq!(discover_storage_headers(&[header(1,6),header(1,8)],&mut output),
            Err(DiscoveryError::InsufficientOutput));
        assert_eq!(output,[sentinel]);
    }
}


// R007: classify trusted read-only PCI config snapshots for driver discovery.
// Never enables bus mastering, maps MMIO or executes device DMA.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum PciKind{Storage,Network,Display}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ClassifiedDevice{pub slot:u16,pub info:DeviceInfo,pub kind:PciKind}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum PciPlanError{Capacity,Config(ConfigError),Aperture(SetError)}
pub fn classify_headers(headers:&[[u8;16]],out:&mut[ClassifiedDevice])
    ->Result<usize,PciPlanError>{
    if headers.len()>MAX_PCI_HEADERS{return Err(PciPlanError::Capacity);}
    let mut needed=0usize;
    for h in headers{
        match decode_config(h){
            Ok(d) if matches!(d.class,1|2|3)=>needed+=1,
            Ok(_)|Err(ConfigError::Absent)=>{},
            Err(e)=>return Err(PciPlanError::Config(e)),
        }
    }
    if needed>out.len(){return Err(PciPlanError::Capacity);}
    let mut count=0;
    for (slot,h) in headers.iter().enumerate(){
        if let Ok(info)=decode_config(h){
            let kind=match info.class{
                1=>Some(PciKind::Storage),
                2=>Some(PciKind::Network),
                3=>Some(PciKind::Display),
                _=>None,
            };
            if let Some(kind)=kind{
                out[count]=ClassifiedDevice{slot:slot as u16,info,kind};count+=1;
            }
        }
    }
    Ok(count)
}
pub fn dma_window_preflight(regions:&[Aperture],reserved:&[Aperture],
    lower:u64,upper:u64,alignment:u64)->Result<(),PciPlanError>{
    // Every candidate must be valid even if it does not conflict.
    validate_set(regions,lower,upper,alignment).map_err(PciPlanError::Aperture)?;
    validate_set(reserved,lower,upper,alignment).map_err(PciPlanError::Aperture)?;
    for region in regions{
        let end=region.base+region.length;
        for r in reserved{
            if region.base<r.base+r.length&&r.base<end{
                return Err(PciPlanError::Aperture(SetError::Overlap));
            }
        }
    }
    Ok(())
}
#[cfg(test)]mod r007_device_tests{
    use super::*;
    fn header(class:u8)->[u8;16]{
        let mut h=[0u8;16];
        h[..2].copy_from_slice(&0x8086u16.to_le_bytes());
        h[2..4].copy_from_slice(&0x100eu16.to_le_bytes());
        h[11]=class;h
    }
    #[test]fn classifies_storage_network_and_display_in_stable_order(){
        let headers=[header(2),[0xff;16],header(3),header(1)];
        let sentinel=ClassifiedDevice{slot:99,info:decode_config(&header(1)).unwrap(),
            kind:PciKind::Storage};
        let mut out=[sentinel;3];
        assert_eq!(classify_headers(&headers,&mut out),Ok(3));
        assert_eq!(out[0].kind,PciKind::Network);
        assert_eq!(out[1].slot,2);
        assert_eq!(out[2].kind,PciKind::Storage);
    }
    #[test]fn short_output_does_not_publish_partial_device_ids(){
        let mut out=[ClassifiedDevice{slot:9,info:decode_config(&header(1)).unwrap(),
            kind:PciKind::Storage};1];
        let before=out;
        assert_eq!(classify_headers(&[header(1),header(3)],&mut out),
            Err(PciPlanError::Capacity));
        assert_eq!(out,before);
    }
    #[test]fn overlapping_or_misaligned_dma_windows_are_denied(){
        let allowed=[Aperture{base:0x2000,length:0x1000}];
        let reserved=[Aperture{base:0x2000,length:0x2000}];
        assert_eq!(dma_window_preflight(&allowed,&reserved,0x1000,0x10000,0x1000),
            Err(PciPlanError::Aperture(SetError::Overlap)));
        assert_eq!(dma_window_preflight(&allowed,&[],0x1000,0x10000,0x1000),Ok(()));
    }
}
