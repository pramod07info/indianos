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
            usize::from(request.count) <= ROW_PIXELS && request.generation != 0
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
