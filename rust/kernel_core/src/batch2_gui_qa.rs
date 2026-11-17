//! R036/20: deterministic screenshot-buffer comparison helper.
//! Real QEMU framebuffer screenshot capture and golden-image fixtures must
//! be supplied by an independent CI harness. No screenshot is fabricated.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Difference{pub differing_pixels:usize,pub first_x:usize,pub first_y:usize}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum CompareError{InvalidGeometry,Overflow,Truncated}
/// Compare visible pixels only: ignore row padding and report first mismatch.
pub fn compare(expected:&[u32],actual:&[u32],width:usize,height:usize,
    stride:usize)->Result<Difference,CompareError>{
    if width==0||height==0||stride<width||width>4096||height>4096{
        return Err(CompareError::InvalidGeometry);
    }
    let last_row=(height-1).checked_mul(stride).ok_or(CompareError::Overflow)?;
    let needed=last_row.checked_add(width).ok_or(CompareError::Overflow)?;
    if expected.len()<needed||actual.len()<needed{return Err(CompareError::Truncated);}
    let mut count=0;let mut first_x=0;let mut first_y=0;
    for y in 0..height{
        for x in 0..width{
            let offset=y*stride+x; // previously bounded by checked needed
            if expected[offset]!=actual[offset]{
                if count==0{first_x=x;first_y=y;}
                count+=1;
            }
        }
    }
    Ok(Difference{differing_pixels:count,first_x,first_y})
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn equal_frame_has_zero_pixel_errors(){
        assert_eq!(compare(&[1,2,3,4],&[1,2,3,4],2,2,2),
            Ok(Difference{differing_pixels:0,first_x:0,first_y:0}));
    }
    #[test]fn first_diff_location_and_row_padding_are_correct(){
        let a=[1u32,2,999,3,4,999];
        let b=[1u32,5,0,3,6,0];
        assert_eq!(compare(&a,&b,2,2,3),
            Ok(Difference{differing_pixels:2,first_x:1,first_y:0}));
    }
    #[test]fn missing_pixels_or_invalid_geometry_fail_closed(){
        assert_eq!(compare(&[0;2],&[0;2],2,2,2),Err(CompareError::Truncated));
        assert_eq!(compare(&[0;4],&[0;4],2,2,1),Err(CompareError::InvalidGeometry));
        assert_eq!(compare(&[0;4],&[0;4],0,2,2),Err(CompareError::InvalidGeometry));
    }
}


/// GUI media viewer preflight. Strict metadata only: no PNG inflate,
/// audio playback, decoder execution or physical camera/mic capabilities.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum MediaHeader{Png{width:u32,height:u32},WavPcm{channels:u16,rate:u32}}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum MediaError{Unsupported,Invalid,TooLarge}
pub fn inspect_media_header(bytes:&[u8])->Result<MediaHeader,MediaError>{
 if bytes.len()>1024*1024{return Err(MediaError::TooLarge);}
 if bytes.len()>=33&&bytes[..8]==[137,80,78,71,13,10,26,10]{
  if bytes[8..12]!=13u32.to_be_bytes()||&bytes[12..16]!=b"IHDR"||
     bytes[26]!=0||bytes[27]!=0||bytes[28]>1{
   return Err(MediaError::Invalid);
  }
  let width=u32::from_be_bytes(bytes[16..20].try_into().map_err(|_|MediaError::Invalid)?);
  let height=u32::from_be_bytes(bytes[20..24].try_into().map_err(|_|MediaError::Invalid)?);
  if width==0||height==0||width>4096||height>4096{return Err(MediaError::Invalid);}
  return Ok(MediaHeader::Png{width,height});
 }
 if bytes.len()>=44&&&bytes[..4]==b"RIFF"&&&bytes[8..12]==b"WAVE"{
  let size=u32::from_le_bytes(bytes[4..8].try_into().map_err(|_|MediaError::Invalid)?);
  if size as usize !=bytes.len()-8||&bytes[12..16]!=b"fmt "||
    bytes[16..20]!=16u32.to_le_bytes()||bytes[20..22]!=1u16.to_le_bytes()||
    &bytes[36..40]!=b"data"{
   return Err(MediaError::Invalid);
  }
  let channels=u16::from_le_bytes(bytes[22..24].try_into().map_err(|_|MediaError::Invalid)?);
  let rate=u32::from_le_bytes(bytes[24..28].try_into().map_err(|_|MediaError::Invalid)?);
  let payload=u32::from_le_bytes(bytes[40..44].try_into().map_err(|_|MediaError::Invalid)?);
  if !matches!(channels,1|2)||!(8000..=192000).contains(&rate)||
     payload as usize !=bytes.len()-44{return Err(MediaError::Invalid);}
  return Ok(MediaHeader::WavPcm{channels,rate});
 }
 Err(MediaError::Unsupported)
}
/// Screenshot utility: copy the real caller-supplied framebuffer (not a
/// fabricated mockup) into packed little-endian BGRA words. Geometry is
/// fully checked before a single output byte is written.
pub fn copy_framebuffer_snapshot(pixels:&[u32],width:usize,height:usize,
 stride:usize,output:&mut[u8])->Result<usize,CompareError>{
 if width==0||height==0||width>4096||height>4096||stride<width{
  return Err(CompareError::InvalidGeometry);
 }
 let last=(height-1).checked_mul(stride).and_then(|v|v.checked_add(width))
   .ok_or(CompareError::Overflow)?;
 let bytes=width.checked_mul(height).and_then(|v|v.checked_mul(4))
   .ok_or(CompareError::Overflow)?;
 if pixels.len()<last||output.len()<bytes{return Err(CompareError::Truncated);}
 for y in 0..height{
  for x in 0..width{
   let at=(y*width+x)*4;
   output[at..at+4].copy_from_slice(&pixels[y*stride+x].to_le_bytes());
  }
 }
 Ok(bytes)
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum MathError{Overflow,DivideByZero}
pub enum Operation{Add,Subtract,Multiply,Divide}
pub fn calculate(a:i64,b:i64,op:Operation)->Result<i64,MathError>{
 let value=match op{
  Operation::Add=>a.checked_add(b),
  Operation::Subtract=>a.checked_sub(b),
  Operation::Multiply=>a.checked_mul(b),
  Operation::Divide=>{if b==0{return Err(MathError::DivideByZero);}
    a.checked_div(b)}
 };
 value.ok_or(MathError::Overflow)
}
#[cfg(test)]
mod m1_batch05_w2_media_utilities_tests{
 use super::*;
 #[test]fn media_headers_reject_bad_dimensions_and_truncated_riff(){
  let mut png=[0u8;33];png[..8].copy_from_slice(&[137,80,78,71,13,10,26,10]);
  png[8..12].copy_from_slice(&13u32.to_be_bytes());
  png[12..16].copy_from_slice(b"IHDR");
  png[16..20].copy_from_slice(&20u32.to_be_bytes());
  png[20..24].copy_from_slice(&30u32.to_be_bytes());
  assert_eq!(inspect_media_header(&png),Ok(MediaHeader::Png{width:20,height:30}));
  png[16..20].fill(0);
  assert_eq!(inspect_media_header(&png),Err(MediaError::Invalid));
  let mut wav=[0u8;44];wav[..4].copy_from_slice(b"RIFF");
  wav[4..8].copy_from_slice(&36u32.to_le_bytes());
  wav[8..12].copy_from_slice(b"WAVE");wav[12..16].copy_from_slice(b"fmt ");
  wav[16..20].copy_from_slice(&16u32.to_le_bytes());
  wav[20..22].copy_from_slice(&1u16.to_le_bytes());
  wav[22..24].copy_from_slice(&2u16.to_le_bytes());
  wav[24..28].copy_from_slice(&44100u32.to_le_bytes());
  wav[36..40].copy_from_slice(b"data");
  assert_eq!(inspect_media_header(&wav),Ok(MediaHeader::WavPcm{channels:2,rate:44100}));
  wav[4]=4;
  assert_eq!(inspect_media_header(&wav),Err(MediaError::Invalid));
 }
 #[test]fn screenshot_preflight_and_arithmetic_are_checked(){
  let pixels=[0x11223344u32,0x55667788,0,0xaabbccdd,0x12345678,0];
  let mut out=[99u8;16];
  assert_eq!(copy_framebuffer_snapshot(&pixels,2,2,3,&mut out[..15]),
    Err(CompareError::Truncated));
  assert_eq!(out,[99u8;16]);
  assert_eq!(copy_framebuffer_snapshot(&pixels,2,2,3,&mut out),Ok(16));
  assert_eq!(&out[..4],&0x11223344u32.to_le_bytes());
  assert_eq!(&out[8..12],&0xaabbccddu32.to_le_bytes());
  assert_eq!(calculate(5,2,Operation::Multiply),Ok(10));
  assert_eq!(calculate(5,0,Operation::Divide),Err(MathError::DivideByZero));
  assert_eq!(calculate(i64::MAX,1,Operation::Add),Err(MathError::Overflow));
 }
}
