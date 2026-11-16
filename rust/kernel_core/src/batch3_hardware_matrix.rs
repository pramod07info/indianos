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
    if record.passed&record.failed!=0||record.failed&record.skipped!=0||
        record.failed&REQUIRED!=0{return Err(MatrixError::Failed);}
    if record.passed&record.skipped!=0||record.skipped&REQUIRED!=0{
        return Err(MatrixError::Skipped);
    }
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

#[cfg(test)]
mod ws28_hardware_evidence_consistency_tests{
 use super::*;
 #[test]fn conflicting_evidence_bits_cannot_certify_hardware(){
  let base=Evidence{platform:Platform::QemuX86_64,commit:[3;20],
      passed:REQUIRED,failed:0,skipped:0};
  assert_eq!(qualify(base,Platform::QemuX86_64,[3;20]),Ok(()));
  assert_eq!(qualify(Evidence{failed:BOOT,..base},Platform::QemuX86_64,[3;20]),
      Err(MatrixError::Failed));
  assert_eq!(qualify(Evidence{skipped:BLOCK,..base},Platform::QemuX86_64,[3;20]),
      Err(MatrixError::Skipped));
 }
}


// PR782 WS85-87: exact binary qualification evidence. GitHub/physical
// attestations MUST come from trusted external test infrastructure.
/// WS85: Deny unexplained bits outside the documented hardware coverage
/// mask. A green required subset is not permission to invent more evidence.
pub fn qualify_exact_bits(record:Evidence,platform:Platform,sha:[u8;20])
 ->Result<(),MatrixError>{
 if (record.passed|record.failed|record.skipped)&!REQUIRED!=0{
  return Err(MatrixError::Failed);
 }
 qualify(record,platform,sha)
}
/// WS86: Require exact, non-repeated platform evidence for a caller-defined
/// platform set. QEMU evidence cannot impersonate physical CPU acceptance.
pub fn qualify_platform_set(records:&[Evidence],platforms:&[Platform],
 sha:[u8;20])->Result<(),MatrixError>{
 if records.is_empty()||records.len()!=platforms.len()||records.len()>3{
  return Err(MatrixError::Missing);
 }
 for (i,&platform) in platforms.iter().enumerate(){
  if platforms[..i].contains(&platform){return Err(MatrixError::WrongPlatform);}
  // Records can arrive unordered but each must carry a unique claimed platform.
  let mut found=None;
  for (j,record) in records.iter().enumerate(){
   if record.platform==platform{
    if found.is_some(){return Err(MatrixError::WrongPlatform);}
    found=Some(j);
   }
  }
  let index=found.ok_or(MatrixError::Missing)?;
  qualify_exact_bits(records[index],platform,sha)?;
 }
 for record in records{
  if !platforms.contains(&record.platform){return Err(MatrixError::WrongPlatform);}
 }
 Ok(())
}
/// WS87: Gate an explicitly selected, nonzero subsystem set. A partial
/// profile cannot be represented as the full REQUIRED hardware matrix.
pub fn qualify_subsystems(record:Evidence,platform:Platform,sha:[u8;20],
 subset:u32)->Result<(),MatrixError>{
 if record.platform!=platform{return Err(MatrixError::WrongPlatform);}
 if sha==[0;20]||record.commit!=sha{return Err(MatrixError::WrongCommit);}
 if subset==0||subset&!REQUIRED!=0{return Err(MatrixError::Missing);}
 if (record.passed&record.failed)!=0||(record.passed&record.skipped)!=0||
   (record.failed&record.skipped)!=0||
   (record.passed|record.failed|record.skipped)&!REQUIRED!=0{
  return Err(MatrixError::Failed);
 }
 if record.failed&subset!=0{return Err(MatrixError::Failed);}
 if record.skipped&subset!=0{return Err(MatrixError::Skipped);}
 if record.passed&subset!=subset{return Err(MatrixError::Missing);}
 Ok(())
}
#[cfg(test)]
mod pr782_ws85_87_tests{
 use super::*;
 fn e(platform:Platform)->Evidence{Evidence{platform,commit:[7;20],
  passed:REQUIRED,failed:0,skipped:0}}
 #[test]fn ws85_forged_unrecognized_bits_are_never_green(){
  let q=e(Platform::QemuX86_64);
  assert_eq!(qualify_exact_bits(q,q.platform,[7;20]),Ok(()));
  assert_eq!(qualify_exact_bits(Evidence{passed:REQUIRED|0x100,..q},
    q.platform,[7;20]),Err(MatrixError::Failed));
  assert_eq!(qualify_exact_bits(Evidence{skipped:0x100,..q},
    q.platform,[7;20]),Err(MatrixError::Failed));
 }
 #[test]fn ws86_platform_coverage_is_distinct_and_sha_bound(){
  let q=e(Platform::QemuX86_64);let hw=e(Platform::BareMetalX86_64);
  assert_eq!(qualify_platform_set(&[q,hw],
    &[Platform::BareMetalX86_64,Platform::QemuX86_64],[7;20]),Ok(()));
  assert_eq!(qualify_platform_set(&[q,q],
    &[Platform::QemuX86_64,Platform::BareMetalX86_64],[7;20]),
    Err(MatrixError::WrongPlatform));
  assert_eq!(qualify_platform_set(&[q],
    &[Platform::BareMetalX86_64],[7;20]),Err(MatrixError::Missing));
  assert_eq!(qualify_platform_set(&[q],&[Platform::QemuX86_64],[8;20]),
    Err(MatrixError::WrongCommit));
 }
 #[test]fn ws87_subsystem_proofs_cannot_launder_unknown_or_skipped(){
  let q=e(Platform::QemuX86_64);
  assert_eq!(qualify_subsystems(q,q.platform,[7;20],BOOT|MEMORY),Ok(()));
  assert_eq!(qualify_subsystems(q,q.platform,[7;20],0),Err(MatrixError::Missing));
  assert_eq!(qualify_subsystems(Evidence{skipped:BOOT,passed:REQUIRED&!BOOT,..q},
    q.platform,[7;20],BOOT),Err(MatrixError::Skipped));
  assert_eq!(qualify_subsystems(Evidence{passed:REQUIRED|128,..q},
    q.platform,[7;20],BOOT),Err(MatrixError::Failed));
 }
}
