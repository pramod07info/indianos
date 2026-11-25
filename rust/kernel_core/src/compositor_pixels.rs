//! Bounded compositor access to committed client pixels.
//! Each opaque pixel carries framebuffer-native RGB and a separate coverage
//! bit. Zero is transparent, and opaque black is representable.
pub const OPERATION: u64 = 2;
pub const REQUEST_BYTES: usize = 24;
pub const REPLY_BYTES: usize = 72;
pub const HEADER_BYTES: usize = 16;
pub const ROW_PIXELS: usize = (REPLY_BYTES - HEADER_BYTES) / 8;
pub const OPAQUE: u64 = 1 << 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub window: u16,
    pub x: u16,
    pub y: u16,
    pub count: u16,
    pub generation: u64,
}

impl Request {
    pub fn parse(bytes: &[u8; REQUEST_BYTES]) -> Option<Self> {
        Self::parse_limit(bytes, ROW_PIXELS)
    }

    pub fn parse_limit(bytes: &[u8; REQUEST_BYTES], limit: usize) -> Option<Self> {
        if &bytes[..4] != b"IPR1" || bytes[12..16] != [0; 4] {
            return None;
        }
        let word = |offset| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        let request = Self {
            window: word(4), x: word(6), y: word(8), count: word(10),
            generation: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        };
        if !matches!(request.window, 1 | 2) {
            return None;
        }
        let valid = if request.count == 0 {
            request.x == 0 && request.y == 0 && request.generation == 0
        } else {
            usize::from(request.count) <= limit && request.generation != 0
        };
        valid.then_some(request)
    }

    pub fn wire(self) -> [u8; REPLY_BYTES] {
        let mut bytes = [0; REPLY_BYTES];
        bytes[..4].copy_from_slice(b"IPR1");
        for (offset, word) in [(4, self.window), (6, self.x), (8, self.y), (10, self.count)] {
            bytes[offset..offset + 2].copy_from_slice(&word.to_le_bytes());
        }
        bytes[16..24].copy_from_slice(&self.generation.to_le_bytes());
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readback_request_rejects_hidden_fields_and_overlong_spans() {
        let mut request = Request { window: 1, x: 0, y: 0, count: 0, generation: 0 };
        let parse = |request: Request| {
            Request::parse(request.wire()[..REQUEST_BYTES].try_into().unwrap())
        };
        assert_eq!(parse(request), Some(request));
        request.x = 1;
        assert_eq!(parse(request), None);
        request.count = ROW_PIXELS as u16;
        assert_eq!(parse(request), None);
        request.generation = 1;
        assert_eq!(parse(request), Some(request));
        request.count += 1;
        assert_eq!(parse(request), None);
        request.count = 1;
        let mut wire = request.wire();
        wire[12] = 1;
        assert_eq!(Request::parse(wire[..REQUEST_BYTES].try_into().unwrap()), None);
    }
}

/// Extended transport shares the existing generation-bound snapshot format.
pub const BULK_OPERATION:u64=3;
pub const BULK_PIXELS:usize=256;
pub const BULK_REPLY_BYTES:usize=HEADER_BYTES+BULK_PIXELS*8;
pub const FRAME_BEGIN:u64=4;
pub const FRAME_ROW:u64=5;
pub const FRAME_COMMIT:u64=6;
pub const FRAME_ABORT:u64=7;
pub const FRAME_INFO:u64=8;
pub const FRAME_BYTES:usize=128;
pub const FRAME_LIMIT:usize=1600;
pub const FRAME_COPY_LIMIT:usize=FRAME_LIMIT*4;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct FrameRequest{
 pub width:u16,pub height:u16,pub y:u16,pub count:u16,pub token:u64,
 pub epoch:u64,pub pixels:u64,pub bytes:u64,pub sequence:u64,pub state:[u8;64],
}
impl FrameRequest{
 pub fn parse(bytes:&[u8;FRAME_BYTES])->Option<Self>{
  if &bytes[..4]!=b"IFR1"||u32::from_le_bytes(bytes[12..16].try_into().ok()?)!=1||
     bytes[56..64]!=[0;8]{return None;}
  let w=|at|u16::from_le_bytes([bytes[at],bytes[at+1]]);
  let q=|at|u64::from_le_bytes(bytes[at..at+8].try_into().unwrap());
  let width=w(4);let height=w(6);
  if !(640..=FRAME_LIMIT as u16).contains(&width)||
     !(480..=FRAME_LIMIT as u16).contains(&height){return None;}
  Some(Self{width,height,y:w(8),count:w(10),token:q(16),epoch:q(24),
   pixels:q(32),bytes:q(40),sequence:q(48),state:bytes[64..].try_into().ok()?})
 }
}

#[cfg(test)]
mod frame_transport_tests{
 use super::*;
 fn packet()->[u8;FRAME_BYTES]{
  let mut bytes=[0;FRAME_BYTES];bytes[..4].copy_from_slice(b"IFR1");
  bytes[4..6].copy_from_slice(&800u16.to_le_bytes());
  bytes[6..8].copy_from_slice(&600u16.to_le_bytes());
  bytes[12..16].copy_from_slice(&1u32.to_le_bytes());bytes
 }
 #[test]fn g4_frame_geometry_and_wire_fields_are_bounded_before_any_copy(){
  assert!(FrameRequest::parse(&packet()).is_some());
  for (offset,value) in [(4,639u16),(4,1601),(6,479),(6,1601),(4,u16::MAX)]{
   let mut bytes=packet();bytes[offset..offset+2].copy_from_slice(&value.to_le_bytes());
   assert!(FrameRequest::parse(&bytes).is_none());
  }
  for offset in [0,12,15,56,63]{let mut bytes=packet();bytes[offset]=255;
   assert!(FrameRequest::parse(&bytes).is_none());}
  for (width,height) in [(640u16,480u16),(1600,1600)]{
   let mut bytes=packet();bytes[4..6].copy_from_slice(&width.to_le_bytes());
   bytes[6..8].copy_from_slice(&height.to_le_bytes());
   let parsed=FrameRequest::parse(&bytes).unwrap();
   assert!(usize::from(parsed.width)*usize::from(parsed.height)<=FRAME_LIMIT*FRAME_LIMIT);
   assert!(usize::from(parsed.width)*4<=FRAME_COPY_LIMIT);
  }
  let read=Request{window:1,x:0,y:0,count:256,generation:1}.wire();
  assert!(Request::parse(read[..REQUEST_BYTES].try_into().unwrap()).is_none());
  assert!(Request::parse_limit(read[..REQUEST_BYTES].try_into().unwrap(),BULK_PIXELS).is_some());
 }
}
