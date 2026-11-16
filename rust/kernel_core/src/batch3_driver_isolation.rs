//! R037/21: quarantine-able driver grants for trusted, kernel-enumerated devices.
//! No IOMMU or MMIO access is performed. Actual DMA isolation requires hardware.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Grant { pub driver:u32,pub generation:u64,pub region_start:u64,
    pub region_length:u64,pub irq:u16,pub active:bool }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum DriverError { Invalid,Overlap,Duplicate,Full,Stale,Quarantined }
pub struct Drivers<const N:usize>{entries:[Option<Grant>;N]}
impl<const N:usize> Drivers<N>{
    pub const fn new()->Self{Self{entries:[None;N]}}
    pub fn allocate(&mut self,driver:u32,generation:u64,start:u64,length:u64,irq:u16)
        ->Result<(),DriverError>{
        if driver==0||generation==0||length==0||irq==0||start%4096!=0||
            length%4096!=0{return Err(DriverError::Invalid);}
        let end=start.checked_add(length).ok_or(DriverError::Invalid)?;
        if self.entries.iter().flatten().any(|e|e.driver==driver){return Err(DriverError::Duplicate);}
        if self.entries.iter().flatten().any(|e|e.irq==irq ||
            e.region_start.checked_add(e.region_length).is_some_and(|prior_end|
                start<prior_end && e.region_start<end)){
            return Err(DriverError::Overlap);
        }
        let free=self.entries.iter_mut().find(|s|s.is_none()).ok_or(DriverError::Full)?;
        *free=Some(Grant{driver,generation,region_start:start,region_length:length,irq,active:true});
        Ok(())
    }
    pub fn validate(&self,driver:u32,generation:u64,address:u64,bytes:u64)
        ->Result<(),DriverError>{
        let e=self.entries.iter().flatten().find(|e|e.driver==driver&&driver!=0)
            .ok_or(DriverError::Stale)?;
        if !e.active{return Err(DriverError::Quarantined);}
        if e.generation!=generation||bytes==0{return Err(DriverError::Stale);}
        let end=address.checked_add(bytes).ok_or(DriverError::Invalid)?;
        if address<e.region_start||e.region_start.checked_add(e.region_length)
            .is_none_or(|bound|end>bound){
            return Err(DriverError::Invalid);
        }
        Ok(())
    }
    /// Trusted fault handler only; no user-mode caller can reset a quarantine.
    /// Reuse a quarantined software slot ONLY after privileged hardware code
    /// has disabled DMA/bus mastering, drained IRQs and unmapped MMIO.
    /// The boolean must never be derived from userspace or a mere timer.
    pub fn retire_quarantined(&mut self,driver:u32,generation:u64,
        hardware_quiesced:bool)->Result<(),DriverError>{
        if driver==0||generation==0{return Err(DriverError::Invalid);}
        let slot=self.entries.iter_mut().find(|slot|slot.as_ref().is_some_and(
            |grant|grant.driver==driver&&grant.generation==generation))
            .ok_or(DriverError::Stale)?;
        if !hardware_quiesced||slot.as_ref().is_some_and(|g|g.active){
            return Err(DriverError::Quarantined);
        }
        *slot=None;
        Ok(())
    }
    pub fn quarantine(&mut self,driver:u32,generation:u64)->Result<(),DriverError>{
        let e=self.entries.iter_mut().flatten().find(|e|e.driver==driver&&driver!=0)
            .ok_or(DriverError::Stale)?;
        if e.generation!=generation{return Err(DriverError::Stale);}
        e.active=false;Ok(())
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn exclusive_mmio_irq_grants_and_quarantine(){
        let mut t=Drivers::<2>::new();
        t.allocate(1,7,0x1000,0x2000,4).unwrap();
        assert_eq!(t.allocate(2,1,0x2000,0x1000,5),Err(DriverError::Overlap));
        assert_eq!(t.allocate(2,1,0x4000,0x1000,4),Err(DriverError::Overlap));
        t.allocate(2,1,0x4000,0x1000,5).unwrap();
        assert_eq!(t.validate(1,7,0x2000,4096),Ok(()));
        assert_eq!(t.validate(1,7,0x3000,1),Err(DriverError::Invalid));
        t.quarantine(1,7).unwrap();
        assert_eq!(t.validate(1,7,0x1000,4),Err(DriverError::Quarantined));
        assert_eq!(t.validate(2,1,0x4000,4),Ok(()));
    }
    #[test]fn stale_generation_and_overflow_denied(){
        let mut t=Drivers::<1>::new();
        t.allocate(1,3,0x1000,0x1000,1).unwrap();
        assert_eq!(t.quarantine(1,2),Err(DriverError::Stale));
        assert_eq!(t.validate(1,2,0x1000,1),Err(DriverError::Stale));
        assert_eq!(t.validate(1,3,u64::MAX,9),Err(DriverError::Invalid));
    }
}

#[cfg(test)]
mod ws21_driver_quarantine_tests{
 use super::*;
 #[test]fn retired_slot_requires_matching_generation_and_hardware_quiesce(){
  let mut d=Drivers::<1>::new();d.allocate(3,7,0x2000,0x1000,5).unwrap();
  assert_eq!(d.retire_quarantined(3,7,true),Err(DriverError::Quarantined));
  d.quarantine(3,7).unwrap();
  assert_eq!(d.retire_quarantined(3,8,true),Err(DriverError::Stale));
  assert_eq!(d.retire_quarantined(3,7,false),Err(DriverError::Quarantined));
  assert_eq!(d.retire_quarantined(3,7,true),Ok(()));
  d.allocate(4,9,0x4000,0x1000,6).unwrap();
  assert_eq!(d.validate(4,9,0x4fff,2),Err(DriverError::Invalid));
 }
}


// PR #782 WS48-50: privileged, generation-bound driver resource lifecycle.
impl<const N:usize> Drivers<N>{
 /// WS48: Do not quarantine an unrelated device by recycling an IRQ number.
 pub fn quarantine_exact_irq(&mut self,driver:u32,generation:u64,
  expected_irq:u16)->Result<(),DriverError>{
  if driver==0||generation==0||expected_irq==0{return Err(DriverError::Invalid);}
  let grant=self.entries.iter_mut().flatten().find(|g|g.driver==driver)
   .ok_or(DriverError::Stale)?;
  if grant.generation!=generation||grant.irq!=expected_irq{
   return Err(DriverError::Stale);
  }
  grant.active=false;Ok(())
 }
 /// WS49: Require complete page ownership, not just a matching byte within
 /// the grant, before a trusted driver obtains an MMIO page mapping.
 pub fn validate_whole_pages(&self,driver:u32,generation:u64,
  address:u64,bytes:u64)->Result<(),DriverError>{
  if address&4095!=0||bytes==0||bytes&4095!=0{
   return Err(DriverError::Invalid);
  }
  self.validate(driver,generation,address,bytes)
 }
 /// WS50: Rebind an already quarantined driver transactionally. The old
 /// quarantine is preserved if reallocation fails (including IRQ conflicts).
 /// Caller must independently prove hardware DMA/IRQ quiescence.
 pub fn rebind_after_quiescence(&mut self,driver:u32,old_epoch:u64,
  new_epoch:u64,start:u64,length:u64,irq:u16,hardware_quiesced:bool)
  ->Result<(),DriverError>{
  if driver==0||old_epoch==0||new_epoch<=old_epoch||!hardware_quiesced{
   return Err(DriverError::Stale);
  }
  let mut candidate=Drivers::<N>{entries:self.entries};
  candidate.retire_quarantined(driver,old_epoch,true)?;
  candidate.allocate(driver,new_epoch,start,length,irq)?;
  self.entries=candidate.entries;Ok(())
 }
}
#[cfg(test)]
mod pr782_ws48_50_tests{
 use super::*;
 #[test]fn ws48_quarantine_requires_exact_device_and_irq_identity(){
  let mut d=Drivers::<2>::new();
  d.allocate(1,3,0x1000,0x1000,4).unwrap();
  d.allocate(2,5,0x3000,0x1000,5).unwrap();
  assert_eq!(d.quarantine_exact_irq(1,3,5),Err(DriverError::Stale));
  assert_eq!(d.validate(1,3,0x1000,4),Ok(()));
  d.quarantine_exact_irq(1,3,4).unwrap();
  assert_eq!(d.validate(1,3,0x1000,4),Err(DriverError::Quarantined));
  assert_eq!(d.validate(2,5,0x3000,4),Ok(()));
 }
 #[test]fn ws49_whole_pages_rejects_unaligned_and_overbound_mmio(){
  let mut d=Drivers::<1>::new();d.allocate(1,3,0x1000,0x2000,4).unwrap();
  assert_eq!(d.validate_whole_pages(1,3,0x1000,4096),Ok(()));
  assert_eq!(d.validate_whole_pages(1,3,0x1001,4096),
   Err(DriverError::Invalid));
  assert_eq!(d.validate_whole_pages(1,3,0x2000,8192),
   Err(DriverError::Invalid));
 }
 #[test]fn ws50_failed_rebind_never_drops_quarantine(){
  let mut d=Drivers::<2>::new();
  d.allocate(1,3,0x1000,0x1000,4).unwrap();
  d.allocate(2,3,0x4000,0x1000,5).unwrap();
  d.quarantine_exact_irq(1,3,4).unwrap();
  assert_eq!(d.rebind_after_quiescence(1,3,4,0x6000,0x1000,5,true),
   Err(DriverError::Overlap));
  assert_eq!(d.validate(1,3,0x1000,4),Err(DriverError::Quarantined));
  assert_eq!(d.rebind_after_quiescence(1,3,4,0x6000,0x1000,6,false),
   Err(DriverError::Stale));
  d.rebind_after_quiescence(1,3,4,0x6000,0x1000,6,true).unwrap();
  assert_eq!(d.validate(1,4,0x6000,4),Ok(()));
 }
}


// PR782 WS116-120: transactional trusted-device reclamation and MMIO
// resource auditing. Privileged callers still must really stop device DMA.
impl<const N:usize> Drivers<N>{
 /// WS116: Snapshot all active grants in slot order, preserving the
 /// destination on short capacity or a forged zero-sized device aperture.
 pub fn snapshot_active(&self,out:&mut[Grant])->Result<usize,DriverError>{
  let count=self.entries.iter().flatten().filter(|g|g.active).count();
  if count>out.len(){return Err(DriverError::Full);}
  for g in self.entries.iter().flatten(){
   if g.driver==0||g.generation==0||g.region_length==0||
    g.region_start.checked_add(g.region_length).is_none(){
    return Err(DriverError::Invalid);
   }
  }
  let mut index=0;
  for g in self.entries.iter().flatten(){
   if g.active{out[index]=*g;index+=1;}
  }
  Ok(index)
 }
 /// WS117: Quarantine a batch only if every exact grant generation is live.
 /// Invalid, repeated or foreign members leave all grants active.
 pub fn quarantine_batch(&mut self,owners:&[(u32,u64)])
  ->Result<usize,DriverError>{
  if owners.is_empty(){return Err(DriverError::Invalid);}
  let mut staging=Drivers::<N>{entries:self.entries};
  for (i,&(driver,generation)) in owners.iter().enumerate(){
   if driver==0||generation==0{return Err(DriverError::Invalid);}
   if owners[..i].contains(&(driver,generation))||
    owners[..i].iter().any(|&(id,_)|id==driver){
    return Err(DriverError::Duplicate);
   }
   let grant=staging.entries.iter().flatten()
    .find(|g|g.driver==driver).ok_or(DriverError::Stale)?;
   if grant.generation!=generation||!grant.active{return Err(DriverError::Stale);}
   staging.quarantine(driver,generation)?;
  }
  self.entries=staging.entries;Ok(owners.len())
 }
 /// WS118: The trusted fault coordinator may retire a whole quarantined
 /// group only after hardware quiescence. No partial slot freeing is allowed.
 pub fn retire_batch_quiesced(&mut self,owners:&[(u32,u64)],
  hardware_stopped:bool)->Result<usize,DriverError>{
  if !hardware_stopped{return Err(DriverError::Quarantined);}
  if owners.is_empty(){return Err(DriverError::Invalid);}
  let mut staging=Drivers::<N>{entries:self.entries};
  for (i,&(driver,generation)) in owners.iter().enumerate(){
   if owners[..i].iter().any(|&(id,_)|id==driver){return Err(DriverError::Duplicate);}
   staging.retire_quarantined(driver,generation,true)?;
  }
  self.entries=staging.entries;Ok(owners.len())
 }
 /// WS119: An IRQ can only be claimed by one active exact-generation driver
 /// and cannot be inherited by a stale or quarantined grant.
 pub fn authorize_irq(&self,driver:u32,generation:u64,irq:u16)
  ->Result<(),DriverError>{
  if driver==0||generation==0||irq==0{return Err(DriverError::Invalid);}
  let grant=self.entries.iter().flatten().find(|g|g.driver==driver)
   .ok_or(DriverError::Stale)?;
  if grant.generation!=generation||grant.irq!=irq{return Err(DriverError::Stale);}
  if !grant.active{return Err(DriverError::Quarantined);}
  Ok(())
 }
 /// WS120: A driver MMIO subrange must lie inside its current whole-page
 /// grant and outside every trusted reserved physical memory aperture.
 pub fn authorize_mmio_outside_reserved(&self,driver:u32,generation:u64,
  address:u64,bytes:u64,reserved:&[(u64,u64)])
  ->Result<(),DriverError>{
  self.validate_whole_pages(driver,generation,address,bytes)?;
  let end=address.checked_add(bytes).ok_or(DriverError::Invalid)?;
  for &(start,length) in reserved{
   if length==0{return Err(DriverError::Invalid);}
   let stop=start.checked_add(length).ok_or(DriverError::Invalid)?;
   if address<stop&&start<end{return Err(DriverError::Overlap);}
  }
  Ok(())
 }
}
#[cfg(test)]
mod pr782_ws116_120_tests{
 use super::*;
 fn table()->Drivers<3>{
  let mut d=Drivers::<3>::new();
  d.allocate(1,3,0x1000,0x2000,4).unwrap();
  d.allocate(2,5,0x5000,0x1000,5).unwrap();d
 }
 #[test]fn ws116_bounded_active_snapshot_does_not_partially_write(){
  let d=table();let sentinel=Grant{driver:9,generation:1,
   region_start:0x9000,region_length:4096,irq:9,active:true};
  let mut short=[sentinel];
  assert_eq!(d.snapshot_active(&mut short),Err(DriverError::Full));
  assert_eq!(short,[sentinel]);
  let mut out=[sentinel;2];
  assert_eq!(d.snapshot_active(&mut out),Ok(2));
  assert_eq!([out[0].driver,out[1].driver],[1,2]);
 }
 #[test]fn ws117_quarantine_batch_is_all_or_nothing(){
  let mut d=table();
  assert_eq!(d.quarantine_batch(&[(1,3),(2,4)]),Err(DriverError::Stale));
  assert_eq!(d.validate(1,3,0x1000,4096),Ok(()));
  assert_eq!(d.quarantine_batch(&[(1,3),(1,3)]),Err(DriverError::Duplicate));
  assert_eq!(d.quarantine_batch(&[(1,3),(2,5)]),Ok(2));
  assert_eq!(d.validate(1,3,0x1000,4096),Err(DriverError::Quarantined));
 }
 #[test]fn ws118_retirement_keeps_quarantine_on_failed_batch(){
  let mut d=table();d.quarantine(1,3).unwrap();
  assert_eq!(d.retire_batch_quiesced(&[(1,3),(2,5)],true),
   Err(DriverError::Quarantined));
  assert_eq!(d.validate(1,3,0x1000,4096),Err(DriverError::Quarantined));
  d.quarantine(2,5).unwrap();
  assert_eq!(d.retire_batch_quiesced(&[(1,3),(2,5)],false),
   Err(DriverError::Quarantined));
  assert_eq!(d.retire_batch_quiesced(&[(1,3),(2,5)],true),Ok(2));
  assert_eq!(d.validate(1,3,0x1000,4096),Err(DriverError::Stale));
 }
 #[test]fn ws119_irq_identity_requires_live_exact_owner(){
  let mut d=table();
  assert_eq!(d.authorize_irq(1,3,4),Ok(()));
  assert_eq!(d.authorize_irq(1,3,5),Err(DriverError::Stale));
  assert_eq!(d.authorize_irq(1,2,4),Err(DriverError::Stale));
  d.quarantine(1,3).unwrap();
  assert_eq!(d.authorize_irq(1,3,4),Err(DriverError::Quarantined));
 }
 #[test]fn ws120_mmio_grant_rejects_firmware_reservations(){
  let d=table();
  assert_eq!(d.authorize_mmio_outside_reserved(1,3,0x1000,4096,
   &[(0x3000,4096)]),Ok(()));
  assert_eq!(d.authorize_mmio_outside_reserved(1,3,0x1000,4096,
   &[(0x1800,256)]),Err(DriverError::Overlap));
  assert_eq!(d.authorize_mmio_outside_reserved(1,3,0x1001,4096,&[]),
   Err(DriverError::Invalid));
 }
}
