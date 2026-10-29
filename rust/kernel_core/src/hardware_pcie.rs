//! Issue #124 HD1: read-only PCI bridge topology and accelerator discovery.
//!
//! A bounded, allocator-free walk of real PCI-PCI bridges expands HD0's
//! legacy bus-0-only scan to their validated secondary buses. No device
//! configuration writes, BAR probing, DMA, privilege grants or driver loads.
//!
//! PCIe ECAM/MCFG discovery has a separate bounded parser; this native walk
//! currently uses x86 legacy configuration I/O for segment zero, buses
//! reachable from the root. Actual firmware ACPI MCFG mapping, other PCI
//! segments, USB, hot-plug and physical GPU/NPU drivers are future work.
use crate::hardware_inventory::{
 self as hw, Accelerator, CpuProbe, Device, Error as Hd0Error, Inventory,
 PciHeader, PciProbe, MAX_ACCELERATORS, PROFILE_ABI,
};

pub const TOPOLOGY_ABI:u16=2;
pub const MAX_DISCOVERED_BUSES:usize=32;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{
 Cpu(Hd0Error),Pci(Hd0Error),MalformedBridge,AmbiguousBus,
 TooManyBuses,TooManyAccelerators,OutputTooSmall,
}

pub trait BridgeProbe:PciProbe{
 /// Return the bridge's read-only PCI registers at offset 0x18:
 /// primary, secondary and subordinate bus numbers, respectively.
 /// No configuration writes or bus-number reprogramming are allowed.
 fn read_bridge_buses(&mut self,bus:u8,slot:u8,function:u8)
  ->Result<(u8,u8,u8),Hd0Error>;
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Topology{
 pub abi:u16,pub inventory:Inventory,
 pub scanned_buses:u8,pub discovered_bridges:u16,
 /// Firmware-provided PCIe ECAM ranges are NOT yet mapped or activated.
 pub pcie_ecam_active:bool,
}

pub fn scan<C:CpuProbe,P:BridgeProbe>(
 cpu:&C,provider:&mut P
)->Result<Topology,Error>{
 let cpu=hw::inspect_cpu(cpu).map_err(Error::Cpu)?;
 let mut state=Topology{
  abi:TOPOLOGY_ABI,
  inventory:Inventory{abi:PROFILE_ABI,cpu,
   accelerators:[None;MAX_ACCELERATORS],count:0,gpu_count:0,
   npu_count:0,physical_qpu_available:false,cpu_software_fallback:true},
  scanned_buses:0,discovered_bridges:0,pcie_ecam_active:false,
 };
 // Keep a one-time visit mark for EVERY possible PCI bus ID.
 // Only root bus 0 is scheduled initially; bridge secondary buses
 // are validated before enqueue. No blind scan of 256 buses.
 let mut scheduled=[false;256];
 let mut queue=[0u8;MAX_DISCOVERED_BUSES];
 // A queued bus inherits every ancestor's window transitively. Each
 // child's ceiling must fit its parent's ceiling; bus numbers increase.
 let mut ceilings=[255u8;MAX_DISCOVERED_BUSES];
 let mut parents=[0usize;MAX_DISCOVERED_BUSES];
 let mut tail=1usize;let mut head=0usize;
 scheduled[0]=true;
 while head<tail{
  let current=head;let bus=queue[current];head+=1;
  state.scanned_buses+=1;
  for slot in 0..32u8{
   let Some(raw0)=provider.read_header(bus,slot,0).map_err(Error::Pci)?
    else{continue};
   let Some(first)=hw::parse_pci(&raw0).map_err(Error::Pci)?
    else{continue};
   let total:u8=if first.header_type&0x80!=0{8}else{1};
   for function in 0..total{
    let info=if function==0{first}else{
     let Some(raw)=provider.read_header(bus,slot,function)
       .map_err(Error::Pci)? else{continue};
     let Some(found)=hw::parse_pci(&raw).map_err(Error::Pci)?
       else{continue};
     found
    };
    if let Some(kind)=hw::classify_pci(info){
     add(&mut state,bus,slot,function,info,kind)?;
    }
    // PCI-to-PCI bridges are header type 1, class 0x06 / subclass
    // 0x04. Read the primary/secondary/subordinate bus registers,
    // but NEVER reconfigure those registers.
    if info.class==0x06 && info.subclass==0x04{
     if info.header_type&0x7f!=1{return Err(Error::MalformedBridge);}
     let (primary,secondary,subordinate)=provider
       .read_bridge_buses(bus,slot,function).map_err(Error::Pci)?;
     // A PCI bridge can be physically present but unconfigured; bus 0
     // means no subordinate is reachable yet. Do not crash early boot.
     if secondary==0{continue;}
     if primary!=bus||secondary<=bus||subordinate<secondary
      ||subordinate>ceilings[current]{
      return Err(Error::MalformedBridge);
     }
     // Sibling bridges on this bus must route disjoint inclusive ranges.
     // Comparing only secondary IDs misses different starts with shared
     // descendants. Nested bridges are compared only with their siblings.
     for i in 1..tail{
      if parents[i]==current && secondary<=ceilings[i]
       && queue[i]<=subordinate{
       return Err(Error::AmbiguousBus);
      }
     }
     if scheduled[secondary as usize]{
      return Err(Error::AmbiguousBus);
     }
     if tail>=MAX_DISCOVERED_BUSES{return Err(Error::TooManyBuses);}
     scheduled[secondary as usize]=true;
     queue[tail]=secondary;ceilings[tail]=subordinate;
     parents[tail]=current;tail+=1;
     state.discovered_bridges+=1;
    }
   }
  }
 }
 Ok(state)
}

fn add(state:&mut Topology,bus:u8,slot:u8,function:u8,
 header:PciHeader,kind:Accelerator)->Result<(),Error>{
 if state.inventory.count as usize>=MAX_ACCELERATORS{
  return Err(Error::TooManyAccelerators);
 }
 let i=state.inventory.count as usize;
 state.inventory.accelerators[i]=Some(Device{
  bus,slot,function,header,kind
 });
 state.inventory.count+=1;
 match kind{
  Accelerator::Gpu=>state.inventory.gpu_count+=1,
  Accelerator::Npu=>state.inventory.npu_count+=1,
 }
 Ok(())
}

/// Caller-sized ASCII report for serial/installer display. The nested
/// HD0 report preserves existing smoke-test wording and explicitly
/// disclaims unsigned/inactive drivers. Never claims QPU hardware.
pub fn format_report(state:&Topology,out:&mut[u8])->Result<usize,Error>{
 let mut at=hw::format_report(&state.inventory,out)
   .map_err(|_|Error::OutputTooSmall)?;
 fn write(out:&mut[u8],at:&mut usize,data:&[u8])->Result<(),Error>{
  let end=at.checked_add(data.len()).ok_or(Error::OutputTooSmall)?;
  if end>=out.len(){return Err(Error::OutputTooSmall);}
  out[*at..end].copy_from_slice(data);*at=end;Ok(())
 }
 fn number(out:&mut[u8],at:&mut usize,mut x:u16)->Result<(),Error>{
  let mut tmp=[0u8;5];let mut len=0;
  loop{
   len+=1;tmp[5-len]=b'0'+(x%10) as u8;x/=10;
   if x==0{break;}
  }
  write(out,at,&tmp[5-len..])
 }
 write(out,&mut at,b"IndianOS HD1 PCI buses=")?;
 number(out,&mut at,state.scanned_buses as u16)?;
 write(out,&mut at,b" bridges=")?;
 number(out,&mut at,state.discovered_bridges)?;
 write(out,&mut at,b" secondary_GPU=")?;
 let behind=state.inventory.accelerators.iter().flatten()
  .filter(|dev|dev.bus!=0&&dev.kind==Accelerator::Gpu).count();
 number(out,&mut at,behind as u16)?;
 write(out,&mut at,b" read-only; ECAM=not mapped; GPU/NPU driver=not activated\n")?;
 out[at]=0;
 Ok(at)
}
/// Installer preflight is strictly read-only. Do not call it a full
/// installer until real graphical workflow/disk safety integration exists.
pub fn installer_preflight(state:&Topology)->hw::Preflight{
 hw::installer_preflight(&state.inventory)
}

#[cfg(all(target_arch="x86_64",target_os="none"))]
pub mod native{
 use super::*;
 use crate::hardware_inventory::native::{BootCpu,BootPci};
 impl BridgeProbe for BootPci{
  fn read_bridge_buses(&mut self,bus:u8,slot:u8,function:u8)
   ->Result<(u8,u8,u8),Hd0Error>{
   let register=unsafe{BootPci::read_config(bus,slot,function,0x18)};
   Ok((register as u8,(register>>8) as u8,(register>>16) as u8))
  }
 }
 pub fn detect_boot()->Result<Topology,Error>{
  scan(&BootCpu,&mut BootPci)
 }
}

#[cfg(test)]
mod tests{
 use super::*;
 use crate::hardware_inventory::{Registers,CPU_REQUIRED,CPU_AVX};
 struct Cpu;
 impl CpuProbe for Cpu{
  fn cpuid(&self,leaf:u32,_sub:u32)->Registers{
   match leaf{
    0=>Registers{eax:7,ebx:u32::from_le_bytes(*b"Auth"),
     edx:u32::from_le_bytes(*b"enti"),
     ecx:u32::from_le_bytes(*b"cAMD")},
    1=>Registers{eax:0x0087_0f11,ebx:0,ecx:1<<28,edx:1<<26},
    0x8000_0000=>Registers{eax:0x8000_0001,ebx:0,ecx:0,edx:0},
    0x8000_0001=>Registers{eax:0,ebx:0,ecx:0,
      edx:(1<<29)|(1<<20)},
    _=>Registers::zero()
   }
  }
 }
 #[derive(Clone)]
 struct Mock{
  devices:[[(Option<[u8;16]>,(u8,u8,u8));32];4],
  calls:usize,write_calls:usize,fail:bool,
 }
 impl Mock{
  fn new()->Self{
   Self{devices:[[(None,(0,0,0));32];4],
    calls:0,write_calls:0,fail:false}
  }
  fn add(&mut self,bus:usize,slot:usize,class:u8,
   subclass:u8,kind:u8,subordinate:(u8,u8,u8)){
   let mut raw=[0u8;16];
   raw[..2].copy_from_slice(&0x1af4u16.to_le_bytes());
   raw[2..4].copy_from_slice(&0x1050u16.to_le_bytes());
   raw[11]=class;raw[10]=subclass;raw[14]=kind;
   self.devices[bus][slot]=(Some(raw),subordinate);
  }
 }
 impl PciProbe for Mock{
  fn read_header(&mut self,bus:u8,slot:u8,function:u8)
   ->Result<Option<[u8;16]>,Hd0Error>{
   self.calls+=1;
   if self.fail{return Err(Hd0Error::ProbeFailed);}
   if function!=0||bus as usize>=self.devices.len(){return Ok(None);}
   Ok(self.devices[bus as usize][slot as usize].0)
  }
 }
 impl BridgeProbe for Mock{
  fn read_bridge_buses(&mut self,bus:u8,slot:u8,function:u8)
   ->Result<(u8,u8,u8),Hd0Error>{
   if function!=0||bus as usize>=self.devices.len(){
    return Err(Hd0Error::ProbeFailed);
   }
   Ok(self.devices[bus as usize][slot as usize].1)
  }
 }
 #[test]fn hd1_gpu_on_secondary_bus_and_npu_on_nested_bus(){
  let mut mock=Mock::new();
  mock.add(0,1,6,4,1,(0,1,2));
  mock.add(1,2,3,0,0,(0,0,0));
  mock.add(1,3,6,4,1,(1,2,2));
  mock.add(2,5,0x12,0,0,(0,0,0));
  let state=scan(&Cpu,&mut mock).unwrap();
  assert_eq!(state.scanned_buses,3);
  assert_eq!(state.discovered_bridges,2);
  assert_eq!(state.inventory.gpu_count,1);
  assert_eq!(state.inventory.npu_count,1);
  assert_eq!(state.inventory.accelerators[0].unwrap().bus,1);
  assert_eq!(state.inventory.accelerators[1].unwrap().bus,2);
  assert!(!state.inventory.physical_qpu_available);
  assert!(installer_preflight(&state).fallback);
  assert_eq!(mock.write_calls,0);
  let mut out=[0u8;320];
  let n=format_report(&state,&mut out).unwrap();
  let rendered=core::str::from_utf8(&out[..n]).unwrap();
  assert!(rendered.contains("GPU=1"));
  assert!(rendered.contains("HD1 PCI buses=3 bridges=2"));
  assert!(rendered.contains("secondary_GPU=1"));
  assert!(rendered.contains("driver=not activated"));
 }
 #[test]fn hd1_no_bridges_preserves_cpu_only_boot(){
  let mut mock=Mock::new();
  let scan=scan(&Cpu,&mut mock).unwrap();
  assert_eq!(scan.scanned_buses,1);
  assert_eq!(scan.discovered_bridges,0);
  assert_eq!(scan.inventory.count,0);
  assert!(scan.inventory.cpu_software_fallback);
  assert!(mock.calls>=32);
 }
 #[test]fn hd1_rejects_malformed_bridge_ranges_and_spoofed_primary(){
  for buses in [(0,2,1),(2,1,2),(0,1,0)]{
   let mut mock=Mock::new();
   mock.add(0,1,6,4,1,buses);
   assert_eq!(scan(&Cpu,&mut mock),Err(Error::MalformedBridge));
  }
 }
 #[test]fn hd1_unconfigured_bridge_is_safe_to_skip(){
  let mut mock=Mock::new();
  mock.add(0,1,6,4,1,(0,0,0));
  let result=scan(&Cpu,&mut mock).unwrap();
  assert_eq!(result.scanned_buses,1);
  assert_eq!(result.discovered_bridges,0);
  assert_eq!(result.inventory.gpu_count,0);
 }
 #[test]fn hd1_rejects_duplicate_secondary_bus_and_bridge_cycle(){
  let mut mock=Mock::new();
  mock.add(0,1,6,4,1,(0,1,2));
  mock.add(0,2,6,4,1,(0,1,2));
  assert_eq!(scan(&Cpu,&mut mock),Err(Error::AmbiguousBus));
  let mut cyclic=Mock::new();
  cyclic.add(0,1,6,4,1,(0,1,2));
  cyclic.add(1,1,6,4,1,(1,1,2));
  assert_eq!(scan(&Cpu,&mut cyclic),Err(Error::MalformedBridge));
 }
 #[test]fn hd1_failed_device_probe_never_claims_gpu_or_qpu(){
  let mut mock=Mock::new();mock.fail=true;
  assert_eq!(scan(&Cpu,&mut mock),Err(Error::Pci(Hd0Error::ProbeFailed)));
  let mut mock=Mock::new();
  mock.add(0,1,0x12,0,0,(0,0,0));
  let state=scan(&Cpu,&mut mock).unwrap();
  assert_eq!(state.inventory.npu_count,1);
  assert!(!state.inventory.physical_qpu_available);
 }
 #[test]fn hd1_reporting_bounds_and_installer_compatibility(){
  let mut p=Mock::new();p.add(0,2,3,0,0,(0,0,0));
  let result=scan(&Cpu,&mut p).unwrap();
  let mut small=[0u8;12];
  assert_eq!(format_report(&result,&mut small),Err(Error::OutputTooSmall));
  let preflight=installer_preflight(&result);
  assert!(preflight.boot_supported);
  assert_eq!(preflight.known_gpu_count,1);
  assert!(!preflight.qpu_available);
  let c=Cpu.cpuid(1,0);
  assert!(c.ecx&(1<<28)!=0);
  assert_ne!(CPU_AVX&result.inventory.cpu.features,0);
  assert_eq!(result.inventory.cpu.features&CPU_REQUIRED,CPU_REQUIRED);
 }
 // Sparse provider covers all bus IDs and functions without allocating a
 // 256-bus config image. Counts expose accidental reads beyond a window.
 struct RoutedMock{
  entries:[Option<(u8,u8,u8,[u8;16],(u8,u8,u8))>;40],
  reads:[usize;256],fail_bridge:bool,
 }
 impl RoutedMock{
  fn new()->Self{Self{entries:[None;40],reads:[0;256],fail_bridge:false}}
  fn device(&mut self,b:u8,s:u8,f:u8,class:u8,header:u8,buses:(u8,u8,u8)){
   let mut raw=[0u8;16];raw[0]=1;raw[2]=1;
   raw[11]=class;raw[10]=if class==6{4}else{0};raw[14]=header;
   *self.entries.iter_mut().find(|e|e.is_none()).unwrap()=
    Some((b,s,f,raw,buses));
  }
  fn bridge(&mut self,b:u8,s:u8,secondary:u8,end:u8){
   self.device(b,s,0,6,1,(b,secondary,end));
  }
 }
 impl PciProbe for RoutedMock{
  fn read_header(&mut self,b:u8,s:u8,f:u8)->Result<Option<[u8;16]>,Hd0Error>{
   self.reads[b as usize]+=1;
   Ok(self.entries.iter().flatten().find(|e|(e.0,e.1,e.2)==(b,s,f))
    .map(|e|e.3))
  }
 }
 impl BridgeProbe for RoutedMock{
  fn read_bridge_buses(&mut self,b:u8,s:u8,f:u8)->Result<(u8,u8,u8),Hd0Error>{
   if self.fail_bridge{return Err(Hd0Error::ProbeFailed);}
   self.entries.iter().flatten().find(|e|(e.0,e.1,e.2)==(b,s,f))
    .map(|e|e.4).ok_or(Hd0Error::ProbeFailed)
  }
 }
 #[test]fn bridge_windows_reject_ancestor_escape_before_probing_destination(){
  for (secondary,end) in [(4,4),(2,4)]{
   let mut p=RoutedMock::new();p.bridge(0,0,1,3);
   p.bridge(1,0,secondary,end);
   assert_eq!(scan(&Cpu,&mut p),Err(Error::MalformedBridge));
   assert_eq!(p.reads[secondary as usize],0);
  }
  // A grandchild may not regain a wider grandparent window.
  let mut p=RoutedMock::new();p.bridge(0,0,1,8);
  p.bridge(1,0,2,3);p.bridge(2,0,3,4);
  assert_eq!(scan(&Cpu,&mut p),Err(Error::MalformedBridge));
  assert_eq!(p.reads[3],0);assert_eq!(p.reads[4],0);
 }
 #[test]fn bridge_windows_reject_sibling_intersection_in_either_order(){
  for ranges in [[(1,5),(3,7)],[(3,7),(1,5)],[(1,3),(3,4)]]{
   let mut p=RoutedMock::new();
   p.bridge(0,0,ranges[0].0,ranges[0].1);
   p.bridge(0,1,ranges[1].0,ranges[1].1);
   assert_eq!(scan(&Cpu,&mut p),Err(Error::AmbiguousBus));
   assert!(p.reads[1..].iter().all(|n|*n==0));
  }
  let mut p=RoutedMock::new();p.bridge(0,0,1,8);
  p.bridge(1,0,2,5);p.bridge(1,1,4,6);
  assert_eq!(scan(&Cpu,&mut p),Err(Error::AmbiguousBus));
  assert_eq!(p.reads[2],0);assert_eq!(p.reads[4],0);
 }
 #[test]fn bridge_windows_require_increasing_secondary_even_without_cycle(){
  let mut p=RoutedMock::new();p.bridge(0,0,4,8);p.bridge(4,0,2,3);
  assert_eq!(scan(&Cpu,&mut p),Err(Error::MalformedBridge));
  assert_eq!(p.reads[2],0);
 }
 #[test]fn bridge_windows_accept_multilevel_multifunction_and_adjacent_siblings(){
  let mut p=RoutedMock::new();p.bridge(0,0,1,5);p.bridge(0,1,6,6);
  p.device(1,0,0,6,0x81,(1,2,3));
  p.device(1,0,1,6,1,(1,4,5));
  p.bridge(2,0,3,3);p.bridge(4,0,5,5);
  p.device(3,0,0,3,0x80,(0,0,0));
  p.device(3,0,7,0x12,0,(0,0,0));
  let result=scan(&Cpu,&mut p).unwrap();
  assert_eq!(result.scanned_buses,7);assert_eq!(result.discovered_bridges,6);
  assert_eq!(result.inventory.gpu_count,1);assert_eq!(result.inventory.npu_count,1);
  assert!(result.inventory.cpu_software_fallback);
  assert_eq!(p.reads[3],39);
  assert!(p.reads[7..].iter().all(|n|*n==0));
 }
 #[test]fn bridge_windows_preserve_bus_limit_and_bridge_read_failure(){
  let mut p=RoutedMock::new();
  for b in 0..32{p.bridge(b,0,b+1,32);}
  assert_eq!(scan(&Cpu,&mut p),Err(Error::TooManyBuses));
  assert_eq!(p.reads[32],0);
  let mut p=RoutedMock::new();p.bridge(0,0,1,1);p.fail_bridge=true;
  assert_eq!(scan(&Cpu,&mut p),Err(Error::Pci(Hd0Error::ProbeFailed)));
  assert_eq!(p.reads[1],0);
 }

}