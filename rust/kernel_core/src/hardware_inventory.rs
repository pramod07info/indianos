//! Issue #124 / HD0: automatic read-only CPU and PCI capability inventory.
//!
//! Reusable by kernel early boot and future installer preflight. No heap, no
//! device configuration writes, no BAR mapping, no DMA, no driver binding,
//! no network connection, and NO automatic installation of vendor firmware.
//! Initial native PCI discovery covers x86-64 legacy configuration bus 0;
//! ACPI MCFG PCIe segments, USB, hotplug, and other CPU architectures need
//! additional supported backends and on-device validation.
//!
//! A detected graphics/AI device is not automatically a supported driver.
//! Physical quantum hardware is NEVER guessed from generic PCI classes;
//! the QPU state remains unavailable until an authenticated provider exists.

pub const PROFILE_ABI:u16=1;
pub const MAX_ACCELERATORS:usize=16;
pub const CPU_SSE2:u64=1<<0;
pub const CPU_NX:u64=1<<1;
pub const CPU_LONG_MODE:u64=1<<2;
pub const CPU_AVX:u64=1<<3;
pub const CPU_AVX2:u64=1<<4;
pub const CPU_HYPERVISOR:u64=1<<5;
pub const CPU_REQUIRED:u64=CPU_SSE2|CPU_NX|CPU_LONG_MODE;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Registers{pub eax:u32,pub ebx:u32,pub ecx:u32,pub edx:u32}
impl Registers{pub const fn zero()->Self{Self{eax:0,ebx:0,ecx:0,edx:0}}}

pub trait CpuProbe {
 /// Real implementation executes read-only CPUID on the boot CPU.
 fn cpuid(&self,leaf:u32,subleaf:u32)->Registers;
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Cpu{
 pub vendor:[u8;12],pub family:u16,pub model:u16,
 pub stepping:u8,pub features:u64,pub max_basic:u32,
 pub max_extended:u32,
}
impl Cpu{
 pub fn supported(&self)->bool{
  self.features&CPU_REQUIRED==CPU_REQUIRED
 }
 pub fn avx_usable(&self)->bool{
  // Having AVX instruction support in CPUID does NOT imply the OS has
  // saved/restored AVX state or enabled XCR0. This detector never does.
  false
 }
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{
 MissingCpuId,UnsupportedCpu,ProbeFailed,MalformedHeader,
 TooManyAccelerators,OutputTooSmall,
}
/// Strict x86-64 preflight. No hardware is touched in this pure decoder
/// besides the explicit caller-supplied read-only CPUID provider.
pub fn inspect_cpu<C:CpuProbe>(source:&C)->Result<Cpu,Error>{
 let leaf0=source.cpuid(0,0);
 if leaf0.eax<1{return Err(Error::MissingCpuId);}
 let mut vendor=[0u8;12];
 vendor[0..4].copy_from_slice(&leaf0.ebx.to_le_bytes());
 vendor[4..8].copy_from_slice(&leaf0.edx.to_le_bytes());
 vendor[8..12].copy_from_slice(&leaf0.ecx.to_le_bytes());
 let b=source.cpuid(1,0);
 let max_extended=source.cpuid(0x8000_0000,0).eax;
 let e=if max_extended>=0x8000_0001{
  source.cpuid(0x8000_0001,0)
 }else{Registers::zero()};
 let leaf7=if leaf0.eax>=7{source.cpuid(7,0)}else{Registers::zero()};
 let raw_family=(b.eax>>8)&0xf;
 let extended_family=(b.eax>>20)&0xff;
 let raw_model=(b.eax>>4)&0xf;
 let extended_model=(b.eax>>16)&0xf;
 let family=(raw_family+if raw_family==0xf{extended_family}else{0}) as u16;
 let model=(raw_model+if raw_family==6||raw_family==0xf{
  extended_model<<4
 }else{0}) as u16;
 let mut features=0u64;
 if b.edx&(1<<26)!=0{features|=CPU_SSE2;}
 if e.edx&(1<<20)!=0{features|=CPU_NX;}
 if e.edx&(1<<29)!=0{features|=CPU_LONG_MODE;}
 if b.ecx&(1<<28)!=0{features|=CPU_AVX;}
 if leaf7.ebx&(1<<5)!=0{features|=CPU_AVX2;}
 if b.ecx&(1<<31)!=0{features|=CPU_HYPERVISOR;}
 let cpu=Cpu{vendor,family,model,stepping:(b.eax&0xf) as u8,
  features,max_basic:leaf0.eax,max_extended};
 if !cpu.supported(){return Err(Error::UnsupportedCpu);}
 Ok(cpu)
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct PciHeader{
 pub vendor:u16,pub device:u16,pub class:u8,pub subclass:u8,
 pub prog_if:u8,pub header_type:u8,
}
pub fn parse_pci(raw:&[u8;16])->Result<Option<PciHeader>,Error>{
 let vendor=u16::from_le_bytes([raw[0],raw[1]]);
 let device=u16::from_le_bytes([raw[2],raw[3]]);
 if vendor==0xffff{return Ok(None);}
 if vendor==0||device==0xffff||raw[14]&0x7f>2{
  return Err(Error::MalformedHeader);
 }
 Ok(Some(PciHeader{vendor,device,class:raw[11],subclass:raw[10],
  prog_if:raw[9],header_type:raw[14]}))
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Accelerator{Gpu,Npu}
/// Deliberately conservative classification; PCI class is evidence of
/// function, NOT evidence that IndianOS can drive this accelerator.
pub fn classify_pci(header:PciHeader)->Option<Accelerator>{
 match (header.class,header.subclass){
  (0x03,_)=>Some(Accelerator::Gpu),
  (0x12,_)|(0x0b,0x40)=>Some(Accelerator::Npu),
  _=>None,
 }
}
pub trait PciProbe{
 /// Read a CONFIGURATION HEADER only. Never writes any PCI device config.
 fn read_header(&mut self,bus:u8,device:u8,function:u8)
  ->Result<Option<[u8;16]>,Error>;
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Device{
 pub bus:u8,pub slot:u8,pub function:u8,
 pub header:PciHeader,pub kind:Accelerator,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Inventory{
 pub abi:u16,pub cpu:Cpu,
 pub accelerators:[Option<Device>;MAX_ACCELERATORS],
 pub count:u8,pub gpu_count:u8,pub npu_count:u8,
 /// A QPU requires a separately authenticated real provider. Never infer
 /// quantum hardware from a generic CPU, coprocessor or PCI ID.
 pub physical_qpu_available:bool,
 pub cpu_software_fallback:bool,
}
/// Read-only, bounded x86 PCI bus-0 enumeration for the current early
/// QEMU/legacy-config stage. A real PCIe MCFG and bridge walk are pending.
/// The output is transactional: errors cannot expose a partly probed list.
pub fn discover<C:CpuProbe,P:PciProbe>(cpu:&C,pci:&mut P)->Result<Inventory,Error>{
 let cpu=inspect_cpu(cpu)?;
 let mut inv=Inventory{abi:PROFILE_ABI,cpu,
  accelerators:[None;MAX_ACCELERATORS],count:0,
  gpu_count:0,npu_count:0,physical_qpu_available:false,
  cpu_software_fallback:true};
 for slot in 0..32u8{
  let Some(raw)=pci.read_header(0,slot,0)?else{continue};
  let Some(info)=parse_pci(&raw)?else{continue};
  let total:u8=if info.header_type&0x80!=0{8}else{1};
  for function in 0..total{
   let header=if function==0{info}else{
    let Some(more)=pci.read_header(0,slot,function)?else{continue};
    let Some(header)=parse_pci(&more)?else{continue};
    header
   };
   if let Some(kind)=classify_pci(header){
    if inv.count as usize>=MAX_ACCELERATORS{
     return Err(Error::TooManyAccelerators);
    }
    let device=Device{bus:0,slot,function,header,kind};
    inv.accelerators[inv.count as usize]=Some(device);
    inv.count+=1;
    match kind{
     Accelerator::Gpu=>inv.gpu_count+=1,
     Accelerator::Npu=>inv.npu_count+=1,
    }
   }
  }
 }
 Ok(inv)
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Preflight{
 pub boot_supported:bool,pub known_gpu_count:u8,
 pub known_npu_count:u8,pub qpu_available:bool,
 pub fallback:bool,
}
/// Installer and kernel must use the SAME feature negotiation results.
/// This preflight never partitions media, downloads drivers or contacts
/// quantum services.
pub fn installer_preflight(info:&Inventory)->Preflight{
 Preflight{boot_supported:info.cpu.supported(),
  known_gpu_count:info.gpu_count,
  known_npu_count:info.npu_count,
  qpu_available:info.physical_qpu_available,
  fallback:info.cpu_software_fallback}
}
struct Writer<'a>{buf:&'a mut[u8],used:usize}
impl Writer<'_>{
 fn append(&mut self,bytes:&[u8])->Result<(),Error>{
  let end=self.used.checked_add(bytes.len()).ok_or(Error::OutputTooSmall)?;
  if end>=self.buf.len(){return Err(Error::OutputTooSmall);}
  self.buf[self.used..end].copy_from_slice(bytes);
  self.used=end;
  Ok(())
 }
 fn decimal(&mut self,num:u16)->Result<(),Error>{
  let mut digits=[0u8;5];let mut n=num;
  let mut count=0;
  loop{digits[4-count]=(n%10) as u8+b'0';count+=1;n/=10;if n==0{break;}}
  self.append(&digits[5-count..])
 }
}
/// Human-readable, null-terminated, no-allocation boot / installer report.
/// A serial print or UI can consume it without changing this data model.
pub fn format_report(info:&Inventory,out:&mut[u8])->Result<usize,Error>{
 out.fill(0);
 let mut w=Writer{buf:out,used:0};
 w.append(b"IndianOS HD0 CPU=")?;
 for c in info.cpu.vendor{
  w.append(&[if c.is_ascii_graphic(){c}else{b'?'}])?;
 }
 w.append(b" x86-64 family=")?;w.decimal(info.cpu.family)?;
 w.append(b" model=")?;w.decimal(info.cpu.model)?;
 w.append(b"; GPU=")?;w.decimal(info.gpu_count as u16)?;
 w.append(b" detected(no driver); NPU=")?;
 w.decimal(info.npu_count as u16)?;
 w.append(b" detected(no driver); QPU=unavailable; CPU fallback=enabled\n")?;
 let n=w.used;
 out[n]=0;
 Ok(n)
}
/// Real native providers compile ONLY in the freestanding x86-64 kernel.
/// They may be called once per boot after serial and boot metadata checks
/// on the bootstrap CPU, with interrupts disabled. NOT safe for concurrent
/// PCI bus scanners or unrestricted userland calls.
#[cfg(all(target_arch="x86_64",target_os="none"))]
pub mod native{
 use super::*;
 use core::arch::{asm,x86_64::__cpuid_count};
 pub struct BootCpu;
 impl CpuProbe for BootCpu{
  fn cpuid(&self,leaf:u32,subleaf:u32)->Registers{
   let r=unsafe{__cpuid_count(leaf,subleaf)};
   Registers{eax:r.eax,ebx:r.ebx,ecx:r.ecx,edx:r.edx}
  }
 }
 pub struct BootPci;
 impl BootPci{
  /// Only the PCI configuration-address selection register (0xCF8) is
  /// written; 0xCFC and all device registers are READ ONLY.
  /// No MMIO, BAR sizing writes, DMA setup or driver activation.
  unsafe fn read_config(bus:u8,slot:u8,function:u8,offset:u8)->u32{
   let previous:u32;
   unsafe{asm!("in eax, dx",in("dx")0xcf8u16,out("eax")previous,
    options(nomem,nostack,preserves_flags))};
   let address=0x8000_0000u32|
    ((bus as u32)<<16)|((slot as u32)<<11)|
    ((function as u32)<<8)|(offset as u32&0xfc);
   unsafe{asm!("out dx, eax",in("dx")0xcf8u16,in("eax")address,
    options(nomem,nostack,preserves_flags))};
   let value:u32;
   unsafe{asm!("in eax, dx",in("dx")0xcfcu16,out("eax")value,
    options(nomem,nostack,preserves_flags))};
   unsafe{asm!("out dx, eax",in("dx")0xcf8u16,in("eax")previous,
    options(nomem,nostack,preserves_flags))};
   value
  }
 }
 impl PciProbe for BootPci{
  fn read_header(&mut self,bus:u8,slot:u8,function:u8)
    ->Result<Option<[u8;16]>,Error>{
   if slot>=32||function>=8{return Err(Error::ProbeFailed);}
   let first=unsafe{Self::read_config(bus,slot,function,0)};
   if first&0xffff==0xffff{return Ok(None);}
   let mut raw=[0u8;16];
   raw[..4].copy_from_slice(&first.to_le_bytes());
   for off in [4u8,8,12]{
    let word=unsafe{Self::read_config(bus,slot,function,off)};
    raw[off as usize..off as usize+4].copy_from_slice(&word.to_le_bytes());
   }
   Ok(Some(raw))
  }
 }
 pub fn detect_boot()->Result<Inventory,Error>{
  discover(&BootCpu,&mut BootPci)
 }
}
#[cfg(test)]
mod tests{
 use super::*;
 struct FakeCpu{features:u64}
 impl CpuProbe for FakeCpu{
  fn cpuid(&self,leaf:u32,_subleaf:u32)->Registers{
   let mut r=Registers::zero();
   match leaf{
    0=>{r.eax=7;r.ebx=u32::from_le_bytes(*b"Genu");
        r.edx=u32::from_le_bytes(*b"ineI");
        r.ecx=u32::from_le_bytes(*b"ntel");},
    1=>{r.eax=0x0003_06a9;
        if self.features&CPU_SSE2!=0{r.edx|=1<<26;}
        if self.features&CPU_AVX!=0{r.ecx|=1<<28;}
        if self.features&CPU_HYPERVISOR!=0{r.ecx|=1<<31;}
       },
    7=>{if self.features&CPU_AVX2!=0{r.ebx|=1<<5;}},
    0x8000_0000=>r.eax=0x8000_0001,
    0x8000_0001=>{
     if self.features&CPU_NX!=0{r.edx|=1<<20;}
     if self.features&CPU_LONG_MODE!=0{r.edx|=1<<29;}
    },
    _=>{},
   }
   r
  }
 }
 #[derive(Clone)]
 struct FakePci{entries:[Option<[u8;16]>;32],seen:u32,fail:bool}
 impl FakePci{
  fn new()->Self{Self{entries:[None;32],seen:0,fail:false}}
  fn device(&mut self,slot:usize,class:u8,subclass:u8,vendor:u16,device:u16){
   let mut header=[0u8;16];
   header[..2].copy_from_slice(&vendor.to_le_bytes());
   header[2..4].copy_from_slice(&device.to_le_bytes());
   header[10]=subclass;header[11]=class;
   self.entries[slot]=Some(header);
  }
 }
 impl PciProbe for FakePci{
  fn read_header(&mut self,bus:u8,slot:u8,function:u8)
    ->Result<Option<[u8;16]>,Error>{
   self.seen+=1;
   if self.fail{return Err(Error::ProbeFailed);}
   if bus!=0||function!=0{return Ok(None);}
   Ok(self.entries[slot as usize])
  }
 }
 fn normal()->FakeCpu{FakeCpu{features:CPU_REQUIRED|CPU_AVX2|CPU_AVX}}
 #[test]fn hd0_cpu_vendor_feature_and_model_decode_is_read_only(){
  let cpu=inspect_cpu(&normal()).unwrap();
  assert_eq!(&cpu.vendor,b"GenuineIntel");
  assert_eq!(cpu.family,6);
  assert_eq!(cpu.model,58);
  assert!(cpu.features&CPU_AVX2!=0);
  assert!(cpu.supported());
  assert!(!cpu.avx_usable()); // no XSAVE/XCR0 enablement
 }
 #[test]fn hd0_missing_mandatory_nx_sse2_or_long_mode_fails_closed(){
  for missing in [CPU_NX,CPU_SSE2,CPU_LONG_MODE]{
   assert_eq!(inspect_cpu(&FakeCpu{features:CPU_REQUIRED&!missing}),
    Err(Error::UnsupportedCpu));
  }
 }
 #[test]fn hd0_gpu_and_npu_are_detected_without_claiming_working_drivers(){
  let mut p=FakePci::new();
  p.device(2,0x03,0x00,0x1234,0x1111);
  p.device(3,0x12,0,0x8086,0xabcd);
  p.device(4,0x02,0,0x1af4,0x1000); // NIC is not a GPU/QPU
  let inventory=discover(&normal(),&mut p).unwrap();
  assert_eq!(inventory.gpu_count,1);
  assert_eq!(inventory.npu_count,1);
  assert_eq!(inventory.count,2);
  assert_eq!(inventory.accelerators[0].unwrap().slot,2);
  assert!(!inventory.physical_qpu_available);
  let check=installer_preflight(&inventory);
  assert!(check.boot_supported);
  assert_eq!(check.known_gpu_count,1);
  assert!(check.fallback);
  let mut buffer=[0u8;180];
  let count=format_report(&inventory,&mut buffer).unwrap();
  let report=core::str::from_utf8(&buffer[..count]).unwrap();
  assert!(report.contains("GenuineIntel"));
  assert!(report.contains("GPU=1"));
  assert!(report.contains("NPU=1"));
  assert!(report.contains("QPU=unavailable"));
  assert_eq!(buffer[count],0);
 }
 #[test]fn hd0_cpu_only_is_first_class_and_never_invents_quantum_hardware(){
  let mut p=FakePci::new();
  let inv=discover(&normal(),&mut p).unwrap();
  assert_eq!(inv.count,0);
  assert_eq!(inv.gpu_count,0);
  assert_eq!(inv.npu_count,0);
  assert!(!inv.physical_qpu_available);
  assert!(inv.cpu_software_fallback);
  assert_eq!(p.seen,32);
 }
 #[test]fn hd0_pci_invalid_or_failed_snapshot_never_enables_accelerators(){
  let mut p=FakePci::new();
  p.device(1,3,0,0x8086,0x1000);
  p.device(2,0x12,0,0,4);
  assert_eq!(discover(&normal(),&mut p),Err(Error::MalformedHeader));
  let mut p=FakePci::new();p.fail=true;
  assert_eq!(discover(&normal(),&mut p),Err(Error::ProbeFailed));
  let mut out=[0u8;10];
  let mut p=FakePci::new();
  let inv=discover(&normal(),&mut p).unwrap();
  assert_eq!(format_report(&inv,&mut out),Err(Error::OutputTooSmall));
 }
 #[test]fn hd0_unknown_virtio_gpu_must_not_report_installed_driver(){
  let mut p=FakePci::new();
  p.device(2,0x03,0,0x1af4,0x1050);
  let inv=discover(&normal(),&mut p).unwrap();
  assert_eq!(inv.gpu_count,1);
  let mut out=[0u8;180];let n=format_report(&inv,&mut out).unwrap();
  assert!(core::str::from_utf8(&out[..n]).unwrap().contains("no driver"));
 }
}
