//! R020/18 versioned, checksum-protected workstation settings interchange.
//! Imported data is applied only to the authenticated matching Store user.
use super::Error;
use crate::batch2_settings::{Store,Settings,Theme};
pub const LENGTH:usize=19;
fn hash(b:&[u8])->u32{let mut h=0x811c9dc5u32;
 for &x in b{h=(h^(x as u32)).wrapping_mul(0x01000193);}h}
pub fn encode<const N:usize>(store:&Store<N>,user:u32,generation:u64,
 out:&mut[u8])->Result<usize,Error>{
 let settings=store.read(user,generation).map_err(|_|Error::Denied)?;
 if out.len()<LENGTH{return Err(Error::Full);}
 let mut raw=[0u8;LENGTH];
 raw[..4].copy_from_slice(b"ISET");
 raw[4]=match settings.theme{Theme::Light=>1,Theme::Dark=>2};
 raw[5]=settings.pointer_speed;raw[6]=settings.key_repeat;
 raw[7..15].copy_from_slice(&generation.to_le_bytes());
 raw[15..19].copy_from_slice(&hash(&raw[..15]).to_le_bytes());
 out[..LENGTH].copy_from_slice(&raw);Ok(LENGTH)
}
pub fn decode<const N:usize>(store:&mut Store<N>,user:u32,
 generation:u64,bytes:&[u8])->Result<Settings,Error>{
 if user==0||generation==0||bytes.len()!=LENGTH||
  &bytes[..4]!=b"ISET"{return Err(Error::Invalid);}
 let recorded=u64::from_le_bytes(bytes[7..15].try_into()
  .map_err(|_|Error::Invalid)?);
 let checksum=u32::from_le_bytes(bytes[15..19].try_into()
  .map_err(|_|Error::Invalid)?);
 if recorded!=generation||hash(&bytes[..15])!=checksum{
  return Err(Error::Denied);
 }
 let theme=match bytes[4]{1=>Theme::Light,2=>Theme::Dark,_=>return Err(Error::Invalid)};
 let settings=Settings{theme,pointer_speed:bytes[5],key_repeat:bytes[6]};
 store.update(user,generation,settings).map_err(|_|Error::Denied)?;
 Ok(settings)
}
pub fn settings_changed(a:Settings,b:Settings)->bool{a!=b}
#[cfg(test)]mod tests{
 use super::*;
 fn options()->Settings{Settings{theme:Theme::Dark,pointer_speed:5,key_repeat:10}}
 #[test]fn saves_and_restores_verified_user_theme_settings(){
  let mut s=Store::<1>::new();s.register(7,1,options()).unwrap();
  let mut raw=[0u8;LENGTH];assert_eq!(encode(&s,7,1,&mut raw),Ok(LENGTH));
  s.update(7,1,Settings{theme:Theme::Light,..options()}).unwrap();
  assert_eq!(decode(&mut s,7,1,&raw),Ok(options()));
 }
 #[test]fn corrupted_preferences_do_not_change_current_values(){
  let mut s=Store::<1>::new();s.register(7,1,options()).unwrap();
  let mut raw=[0u8;LENGTH];encode(&s,7,1,&mut raw).unwrap();
  raw[5]^=6;
  assert_eq!(decode(&mut s,7,1,&raw),Err(Error::Denied));
  assert_eq!(s.read(7,1),Ok(options()));
 }
 #[test]fn recycled_identity_and short_buffer_cannot_leak_settings(){
  let mut s=Store::<1>::new();s.register(7,1,options()).unwrap();
  let mut short=[99u8;3];
  assert_eq!(encode(&s,7,1,&mut short),Err(Error::Full));
  assert_eq!(short,[99;3]);
  assert_eq!(encode(&s,7,2,&mut [0u8;LENGTH]),Err(Error::Denied));
 }
}
