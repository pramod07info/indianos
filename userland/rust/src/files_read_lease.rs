//! Files' private record of an already kernel-issued descriptor and preview.
//! This record never creates filesystem authority; every operation still goes
//! through the authenticated native descriptor ABI.
#[derive(Clone,Copy)]
pub struct ReadLease{token:u64,bytes:[u8;20],len:usize}
impl ReadLease{
    /// Additive ABI feature 8 promises coordinated unrelated-path sharing.
    /// Older kernels keep volume-wide leases; do not enable this UI on them.
    pub fn supports_abi(info:u64)->bool{
        info&0xffff==1&&(info>>16)&0xffff==64&&(info>>32)&8!=0
    }
    pub const fn new()->Self{Self{token:0,bytes:[0;20],len:0}}
    pub fn token(&self)->u64{self.token}
    pub fn held(&self)->bool{self.token!=0}
    pub fn capture(&mut self,token:u64,bytes:&[u8])->bool{
        if self.held()||token==0||token==u64::MAX||bytes.len()>self.bytes.len()||
           bytes.iter().any(|b|!(0x20..=0x7e).contains(b)){return false;}
        self.bytes.fill(0);self.bytes[..bytes.len()].copy_from_slice(bytes);
        self.len=bytes.len();self.token=token;true
    }
    pub fn verifies(&self,token:u64,bytes:&[u8])->bool{
        self.held()&&self.token==token&&bytes==&self.bytes[..self.len]
    }
    pub fn clear(&mut self)->u64{
        let token=self.token;*self=Self::new();token
    }
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn held_reader_requires_a_compatible_version_and_sharing_feature(){
        let info=1|(64u64<<16)|(15u64<<32);
        assert!(ReadLease::supports_abi(info));
        assert!(ReadLease::supports_abi(info|(16u64<<32)));
        for invalid in [0,u64::MAX,1|(64u64<<16)|(7u64<<32),
                        2|(64u64<<16)|(15u64<<32),1|(32u64<<16)|(15u64<<32)]{
            assert!(!ReadLease::supports_abi(invalid));
        }
    }
    #[test]fn bad_or_replayed_capture_preserves_the_original_descriptor(){
        let mut lease=ReadLease::new();
        for (token,data) in [(0,b"note".as_slice()),(u64::MAX,b"note"),
                            (1,b"bad\n"),(1,b"123456789012345678901")]{
            assert!(!lease.capture(token,data));assert!(!lease.held());
        }
        assert!(lease.capture(7,b"original"));
        assert!(!lease.capture(8,b"replacement"));
        assert_eq!(lease.token(),7);assert!(lease.verifies(7,b"original"));
        assert!(!lease.verifies(8,b"original"));assert!(!lease.verifies(7,b"changed"));
    }
    #[test]fn clearing_or_empty_files_cannot_revive_a_stale_lease(){
        let mut lease=ReadLease::new();assert!(lease.capture(5,b"secret"));
        assert_eq!(lease.clear(),5);assert_eq!(lease.clear(),0);
        assert!(!lease.verifies(5,b"secret"));
        assert!(lease.capture(9,b""));assert!(lease.verifies(9,b""));
        assert!(!lease.verifies(9,b"secret"));assert!(!lease.verifies(5,b""));
    }
}
