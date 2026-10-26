//! R037/29: deterministic hardware qualification gates by tested platform.
//! A passed simulated QEMU test cannot certify untested physical hardware.
pub const REQUIRED:u32=0b11_1111;
pub const BOOT:u32=1;
pub const MEMORY:u32=2;
pub const BLOCK:u32=4;
pub const NETWORK:u32=8;
pub const INPUT:u32=16;
pub const RECOVERY:u32=32;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Platform{QemuX86_64,BareMetalX86_64,BareMetalAArch64}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Evidence{pub platform:Platform,pub commit:[u8;20],
    pub passed:u32,pub failed:u32,pub skipped:u32}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum MatrixError{WrongPlatform,WrongCommit,Missing,Failed,Skipped}
/// Require *same* 20-byte Git SHA, correct platform and all tests truly passed.
pub fn qualify(record:Evidence,requested:Platform,commit:[u8;20])
    ->Result<(),MatrixError>{
    if record.platform!=requested{return Err(MatrixError::WrongPlatform);}
    if commit==[0;20]||record.commit!=commit{return Err(MatrixError::WrongCommit);}
    if record.failed&REQUIRED!=0{return Err(MatrixError::Failed);}
    if record.skipped&REQUIRED!=0{return Err(MatrixError::Skipped);}
    if record.passed&REQUIRED!=REQUIRED{return Err(MatrixError::Missing);}
    Ok(())
}
#[cfg(test)]
mod tests{
    use super::*;
    fn record()->Evidence{Evidence{platform:Platform::QemuX86_64,
        commit:[1;20],passed:REQUIRED,failed:0,skipped:0}}
    #[test]fn qemu_green_does_not_certify_bare_metal(){
        assert_eq!(qualify(record(),Platform::QemuX86_64,[1;20]),Ok(()));
        assert_eq!(qualify(record(),Platform::BareMetalX86_64,[1;20]),
            Err(MatrixError::WrongPlatform));
    }
    #[test]fn stale_missing_and_skipped_fail_closed(){
        assert_eq!(qualify(record(),Platform::QemuX86_64,[2;20]),
            Err(MatrixError::WrongCommit));
        let mut r=record();r.passed&=!NETWORK;
        assert_eq!(qualify(r,Platform::QemuX86_64,[1;20]),
            Err(MatrixError::Missing));
        r=record();r.skipped=RECOVERY;
        assert_eq!(qualify(r,Platform::QemuX86_64,[1;20]),
            Err(MatrixError::Skipped));
    }
}
