//! Native app-owned pixel upload ABI. Shared by clients and the privileged
//! copy boundary; ownership and real page mappings are checked by the runtime.
pub const REGISTER:u32=0;
pub const BEGIN:u32=1;
pub const TILE:u32=2;
pub const COMMIT:u32=3;
pub const ABORT:u32=4;
pub const MAX_WIDTH:u32=768;
pub const MAX_HEIGHT:u32=768;
pub const TILE_PIXELS:u32=256;
pub const SCRATCH_BYTES:u64=4096;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Request{
 pub operation:u32,pub window:u32,
 pub x:u16,pub y:u16,pub width:u16,pub height:u16,
 pub address:u64,pub bytes:u64,pub token:u64,pub lower:u64,pub upper:u64,
}
impl Request{
 pub const fn new(operation:u32,window:u32)->Self{
  Self{operation,window,x:0,y:0,width:0,height:0,address:0,bytes:0,token:0,lower:0,upper:0}
 }
 pub fn parse(bytes:&[u8;64])->Option<Self>{
  if &bytes[..4]!=b"IPX1"||bytes[4..8]!=1u32.to_le_bytes(){return None;}
  let get16=|i|u16::from_le_bytes([bytes[i],bytes[i+1]]);
  let get32=|i|u32::from_le_bytes(bytes[i..i+4].try_into().unwrap());
  let get64=|i|u64::from_le_bytes(bytes[i..i+8].try_into().unwrap());
  let request=Self{operation:get32(8),window:get32(12),x:get16(16),y:get16(18),
   width:get16(20),height:get16(22),address:get64(24),bytes:get64(32),
   token:get64(40),lower:get64(48),upper:get64(56)};
  request.valid().then_some(request)
 }
 fn valid(self)->bool{
  if !matches!(self.window,1|2){return false;}
  let empty_rect=self.x==0&&self.y==0&&self.width==0&&self.height==0;
  let empty_data=self.address==0&&self.bytes==0&&self.lower==0&&self.upper==0;
  match self.operation{
   REGISTER=>empty_rect&&self.token==0&&self.address%SCRATCH_BYTES==0&&
    self.lower!=0&&self.lower.checked_add(SCRATCH_BYTES)==Some(self.address)&&
    self.bytes==SCRATCH_BYTES&&self.address.checked_add(SCRATCH_BYTES)==Some(self.upper)&&
    self.upper.checked_add(SCRATCH_BYTES).is_some(),
   BEGIN=>empty_data&&self.token==0&&self.width!=0&&self.height!=0&&
    self.x as u32+self.width as u32<=MAX_WIDTH&&
    self.y as u32+self.height as u32<=MAX_HEIGHT,
   TILE=>self.lower==0&&self.upper==0&&self.token!=0&&self.address!=0&&
    self.address&3==0&&self.height==1&&(1..=TILE_PIXELS).contains(&(self.width as u32))&&
    self.x as u32+self.width as u32<=MAX_WIDTH&&(self.y as u32)<MAX_HEIGHT&&
    self.bytes==self.width as u64*4&&self.address.checked_add(self.bytes).is_some(),
   COMMIT|ABORT=>empty_rect&&empty_data&&self.token!=0,
   _=>false,
  }
 }
 pub fn wire(self)->[u8;64]{
  let mut bytes=[0u8;64];bytes[..4].copy_from_slice(b"IPX1");
  bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
  bytes[8..12].copy_from_slice(&self.operation.to_le_bytes());
  bytes[12..16].copy_from_slice(&self.window.to_le_bytes());
  for (offset,value) in [(16,self.x),(18,self.y),(20,self.width),(22,self.height)]{
   bytes[offset..offset+2].copy_from_slice(&value.to_le_bytes());
  }
  for (offset,value) in [(24,self.address),(32,self.bytes),(40,self.token),(48,self.lower),(56,self.upper)]{
   bytes[offset..offset+8].copy_from_slice(&value.to_le_bytes());
  }
  bytes
 }
}
pub const fn window_for_pid(pid:u32)->Option<u32>{match pid{2=>Some(1),3=>Some(2),_=>None}}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn operation_shapes_roundtrip_and_reject_hidden_fields(){
  let mut register=Request::new(REGISTER,1);
  register.lower=0x401000;register.address=0x402000;register.upper=0x403000;register.bytes=4096;
  let mut begin=Request::new(BEGIN,2);begin.width=768;begin.height=768;
  let mut tile=Request::new(TILE,2);tile.address=0x402000;tile.width=256;tile.height=1;
  tile.bytes=1024;tile.token=7;
  let mut commit=Request::new(COMMIT,2);commit.token=7;
  let mut abort=commit;abort.operation=ABORT;
  for good in [register,begin,tile,commit,abort]{
   assert_eq!(Request::parse(&good.wire()),Some(good));
   let mut bad=good;bad.window=3;assert!(Request::parse(&bad.wire()).is_none());
   let mut wire=good.wire();wire[4]=2;assert!(Request::parse(&wire).is_none());
  }
  tile.height=2;assert!(Request::parse(&tile.wire()).is_none());
  tile.height=1;tile.address=u64::MAX-3;assert!(Request::parse(&tile.wire()).is_none());
  begin.x=1;assert!(Request::parse(&begin.wire()).is_none());
  register.upper+=4096;assert!(Request::parse(&register.wire()).is_none());
  commit.bytes=4;assert!(Request::parse(&commit.wire()).is_none());
 }
 #[test]fn pid_mapping_has_no_broker_or_compositor_surface(){
  for pid in [0,1,4,u32::MAX]{assert_eq!(window_for_pid(pid),None);}
  assert_eq!(window_for_pid(2),Some(1));assert_eq!(window_for_pid(3),Some(2));
 }
}
