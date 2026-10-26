//! M2 dependency-free SHA-256, usable in bare-metal no_std kernels.
//! This provides content integrity ONLY. SHA-256 is not Ed25519 and cannot
//! independently authenticate a publisher or replace trusted public keys.
//! The current M2P4 package loader bounds native ELF inputs to 3,914 bytes.
const INITIAL:[u32;8]=[
 0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,
 0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19
];
const ROUND:[u32;64]=[
 0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,
 0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
 0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,
 0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
 0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,
 0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
 0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,
 0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
 0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,
 0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
 0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,
 0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
 0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,
 0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
 0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,
 0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2
];
fn compress(h:&mut[u32;8],data:&[u8]){
 debug_assert_eq!(data.len(),64);
 let mut w=[0u32;64];
 for i in 0..16{
  let j=i*4;
  w[i]=u32::from_be_bytes([data[j],data[j+1],data[j+2],data[j+3]]);
 }
 for i in 16..64{
  let s0=w[i-15].rotate_right(7)^w[i-15].rotate_right(18)^(w[i-15]>>3);
  let s1=w[i-2].rotate_right(17)^w[i-2].rotate_right(19)^(w[i-2]>>10);
  w[i]=w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
 }
 let (mut a,mut b,mut c,mut d,mut e,mut f,mut g,mut hh)=
  (h[0],h[1],h[2],h[3],h[4],h[5],h[6],h[7]);
 for i in 0..64{
  let s1=e.rotate_right(6)^e.rotate_right(11)^e.rotate_right(25);
  let ch=(e&f)^(!e&g);
  let t1=hh.wrapping_add(s1).wrapping_add(ch)
    .wrapping_add(ROUND[i]).wrapping_add(w[i]);
  let s0=a.rotate_right(2)^a.rotate_right(13)^a.rotate_right(22);
  let maj=(a&b)^(a&c)^(b&c);
  let t2=s0.wrapping_add(maj);
  hh=g;g=f;f=e;e=d.wrapping_add(t1);
  d=c;c=b;b=a;a=t1.wrapping_add(t2);
 }
 for (dst,delta) in h.iter_mut().zip([a,b,c,d,e,f,g,hh]){
  *dst=dst.wrapping_add(delta);
 }
}
/// Bounded-memory incremental SHA-256 for multi-sector packages.
/// The same NIST padding and compression as digest(), without heap allocation.
pub struct Stream{
 h:[u32;8],pending:[u8;64],used:usize,length:u64,
}
impl Stream{
 pub const fn new()->Self{
  Self{h:INITIAL,pending:[0;64],used:0,length:0}
 }
 pub fn update(&mut self,mut data:&[u8])->Result<(),()>{
  self.length=self.length.checked_add(data.len() as u64).ok_or(())?;
  if self.length>u64::MAX/8{return Err(());}
  if self.used!=0{
   let n=(64-self.used).min(data.len());
   self.pending[self.used..self.used+n].copy_from_slice(&data[..n]);
   self.used+=n;
   data=&data[n..];
   if self.used==64{
    compress(&mut self.h,&self.pending);
    self.used=0;
   }
  }
  while data.len()>=64{
   compress(&mut self.h,&data[..64]);
   data=&data[64..];
  }
  if !data.is_empty(){
   self.pending[..data.len()].copy_from_slice(data);
   self.used=data.len();
  }
  Ok(())
 }
 pub fn finish(mut self)->[u8;32]{
  let mut pad=[0u8;128];
  pad[..self.used].copy_from_slice(&self.pending[..self.used]);
  pad[self.used]=0x80;
  let size=if self.used<56{64}else{128};
  pad[size-8..size].copy_from_slice(&(self.length*8).to_be_bytes());
  for block in pad[..size].chunks_exact(64){compress(&mut self.h,block);}
  let mut out=[0u8;32];
  for (i,word) in self.h.iter().enumerate(){
   out[4*i..4*i+4].copy_from_slice(&word.to_be_bytes());
  }
  out
 }
}

pub fn digest(bytes:&[u8])->[u8;32]{
 let mut h=INITIAL;
 let mut index=0usize;
 while bytes.len()-index>=64{
  compress(&mut h,&bytes[index..index+64]);
  index+=64;
 }
 let remaining=bytes.len()-index;
 let mut pad=[0u8;128];
 pad[..remaining].copy_from_slice(&bytes[index..]);
 pad[remaining]=0x80;
 let padded_len=if remaining<56{64}else{128};
 let bit_len=(bytes.len() as u64).wrapping_mul(8);
 pad[padded_len-8..padded_len].copy_from_slice(&bit_len.to_be_bytes());
 for block in pad[..padded_len].chunks_exact(64){compress(&mut h,block);}
 let mut result=[0u8;32];
 for (i,word) in h.iter().enumerate(){
  result[i*4..i*4+4].copy_from_slice(&word.to_be_bytes());
 }
 result
}
/// Fixed-work comparison to avoid exiting early on digest mismatches.
pub fn matches(bytes:&[u8],expected:&[u8;32])->bool{
 let actual=digest(bytes);
 let mut difference=0u8;
 for i in 0..32{difference|=actual[i]^expected[i];}
 difference==0
}
#[cfg(test)]
mod m2_tests{
 use super::*;
 fn bytes(s:&str)->[u8;32]{
  let mut result=[0u8;32];
  for i in 0..32{
   result[i]=u8::from_str_radix(&s[i*2..i*2+2],16).unwrap();
  }
  result
 }
 #[test]fn m2_sha256_nist_empty_abc_and_multiblock(){
  assert_eq!(digest(b""),bytes("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"));
  assert_eq!(digest(b"abc"),bytes("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
  let million=std::vec![b'a';1_000_000];
  assert_eq!(digest(&million),bytes("cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"));
 }
 #[test]fn m2_sha256_digest_comparison_denies_corruption(){
  let digest=digest(b"trusted");
  assert!(matches(b"trusted",&digest));
  assert!(!matches(b"trusteD",&digest));
  assert!(!matches(b"trusted!",&digest));
 } #[test]fn m2_sha256_stream_chunks_match_one_shot_at_block_and_sector_boundaries(){
  for len in [0usize,1,7,55,56,63,64,65,127,128,129,511,512,
   513,2048,3914,16384,65536]{
   let values:std::vec::Vec<u8>=(0..len).map(|i|(i.wrapping_mul(113)%251) as u8)
     .collect();
   let expected=digest(&values);
   for step in [1usize,7,63,64,65,511,512,4096]{
    let mut h=Stream::new();
    for piece in values.chunks(step){h.update(piece).unwrap();}
    assert_eq!(h.finish(),expected,"len={len} step={step}");
   }
  }
 }
 #[test]fn m2_sha256_stream_nist_vectors(){
  let mut h=Stream::new();
  h.update(b"a").unwrap();h.update(b"b").unwrap();h.update(b"c").unwrap();
  assert_eq!(h.finish(),digest(b"abc"));
  let h=Stream::new();
  assert_eq!(h.finish(),digest(b""));
 }

}
