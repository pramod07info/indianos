//! R037/24: validate VirtIO split-ring descriptor chains before trusting them.
//! No MMIO transport negotiation, DMA ownership or actual device operations.
pub const NEXT:u16=1;
pub const WRITE:u16=2;
pub const INDIRECT:u16=4;
pub const MAX_CHAIN:usize=32;
pub const MAX_BYTES:u64=1024*1024;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Descriptor{pub address:u64,pub length:u32,pub flags:u16,pub next:u16}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum VirtError{Invalid,TooLong,Cycle,OutOfBounds,Unsupported}
pub fn validate_chain(descriptors:&[Descriptor],head:usize,device_writes:bool)
    ->Result<u64,VirtError>{
    if descriptors.is_empty()||descriptors.len()>256||head>=descriptors.len(){
        return Err(VirtError::OutOfBounds);
    }
    let mut seen=[false;256];
    let mut current=head;let mut count=0usize;let mut total=0u64;
    loop{
        if current>=descriptors.len(){return Err(VirtError::OutOfBounds);}
        if seen[current]{return Err(VirtError::Cycle);}
        seen[current]=true;count+=1;
        if count>MAX_CHAIN{return Err(VirtError::TooLong);}
        let d=descriptors[current];
        if d.flags & !(NEXT|WRITE|INDIRECT)!=0 ||
            d.flags&INDIRECT!=0{return Err(VirtError::Unsupported);}
        if d.address==0||d.length==0||
            d.address.checked_add(u64::from(d.length)).is_none(){
            return Err(VirtError::Invalid);
        }
        if (d.flags&WRITE!=0)!=device_writes{
            return Err(VirtError::Unsupported);
        }
        total=total.checked_add(u64::from(d.length)).ok_or(VirtError::TooLong)?;
        if total>MAX_BYTES{return Err(VirtError::TooLong);}
        if d.flags&NEXT==0{return Ok(total);}
        current=d.next as usize;
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn descriptor(next:u16,flags:u16)->Descriptor{
        Descriptor{address:0x1000,length:512,flags,next}
    }
    #[test]fn accepts_bounded_write_chain(){
        let chain=[descriptor(1,NEXT|WRITE),descriptor(0,WRITE)];
        assert_eq!(validate_chain(&chain,0,true),Ok(1024));
    }
    #[test]fn cycle_outside_ring_and_untrusted_flags_rejected(){
        assert_eq!(validate_chain(&[descriptor(0,NEXT)],0,false),Err(VirtError::Cycle));
        assert_eq!(validate_chain(&[descriptor(2,NEXT)],0,false),
            Err(VirtError::OutOfBounds));
        assert_eq!(validate_chain(&[descriptor(0,INDIRECT)],0,false),
            Err(VirtError::Unsupported));
    }
    #[test]fn refuses_direction_mismatch(){
        assert_eq!(validate_chain(&[descriptor(0,WRITE)],0,false),
            Err(VirtError::Unsupported));
    }
}
