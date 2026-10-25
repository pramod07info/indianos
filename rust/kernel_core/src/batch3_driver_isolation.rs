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
            start<e.region_start+e.region_length && e.region_start<end){
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
        if address<e.region_start||end>e.region_start+e.region_length{
            return Err(DriverError::Invalid);
        }
        Ok(())
    }
    /// Trusted fault handler only; no user-mode caller can reset a quarantine.
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
