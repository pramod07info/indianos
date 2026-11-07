//! Bounded Terminal requests. Names are relative to the broker's /shared root;
//! this codec carries neither process identities nor filesystem authority.
pub const MAX_NAME: usize = 15;
pub const MAX_DATA: usize = 20;
pub const WIRE_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    bytes: [u8; MAX_NAME],
    len: u8,
}
impl Name {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() || bytes.len() > MAX_NAME || bytes == b"." || bytes == b".."
            || bytes.iter().any(|b| !b.is_ascii_alphanumeric() && !matches!(b, b'_' | b'-' | b'.'))
        {
            return None;
        }
        let mut name = Self { bytes: [0; MAX_NAME], len: bytes.len() as u8 };
        name.bytes[..bytes.len()].copy_from_slice(bytes);
        Some(name)
    }
    pub fn as_bytes(&self) -> &[u8] { &self.bytes[..self.len as usize] }
    /// Zero-padded absolute path for the existing 24-byte syscall descriptor.
    pub fn path(&self, out: &mut [u8; 24]) -> usize {
        out.fill(0);
        out[..8].copy_from_slice(b"/shared/");
        let end = 8 + self.len as usize;
        out[8..end].copy_from_slice(self.as_bytes());
        end
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Touch(Name),
    Read(Name),
    Remove(Name),
    Mkdir(Name),
    Move { from: Name, to: Name },
    Put { name: Name, bytes: [u8; MAX_DATA], len: u8 },
}

/// Preserve exact payload bytes, including trailing spaces and additional '='.
pub fn parse(line: &[u8]) -> Option<Command> {
    for (prefix, operation) in [(b"touch ".as_slice(), 1), (b"cat ", 2),
        (b"rm ", 3), (b"mkdir ", 4)]
    {
        if let Some(tail) = line.strip_prefix(prefix) {
            let name = Name::parse(tail)?;
            return Some(match operation {
                1 => Command::Touch(name), 2 => Command::Read(name),
                3 => Command::Remove(name), _ => Command::Mkdir(name),
            });
        }
    }
    if let Some(tail) = line.strip_prefix(b"mv ") {
        let separator = tail.iter().position(|b| *b == b' ')?;
        return Some(Command::Move {
            from: Name::parse(&tail[..separator])?,
            to: Name::parse(&tail[separator + 1..])?,
        });
    }
    if let Some(tail) = line.strip_prefix(b"put ") {
        let separator = tail.iter().position(|b| *b == b'=')?;
        let name = Name::parse(&tail[..separator])?;
        let payload = &tail[separator + 1..];
        if payload.is_empty() || payload.len() > MAX_DATA
            || payload.iter().any(|b| !(0x20..=0x7e).contains(b))
        {
            return None;
        }
        let mut bytes = [0; MAX_DATA];
        bytes[..payload.len()].copy_from_slice(payload);
        return Some(Command::Put { name, bytes, len: payload.len() as u8 });
    }
    None
}

/// Canonical fixed-size IPC: F, opcode, name/target/data lengths, reserved;
/// name[15] at8, target[15] at23, data[20] at38, all unused bytes zero.
pub fn encode(command: Command, out: &mut [u8; WIRE_BYTES]) -> Option<usize> {
    out.fill(0);
    let (operation, first, second, payload): (u8, Name, Option<Name>, &[u8]) = match &command {
        Command::Touch(name) => (1, *name, None, &[]),
        Command::Read(name) => (2, *name, None, &[]),
        Command::Remove(name) => (3, *name, None, &[]),
        Command::Mkdir(name) => (4, *name, None, &[]),
        Command::Move { from, to } => (5, *from, Some(*to), &[]),
        Command::Put { name, bytes, len } => {
            if *len == 0 || *len as usize > MAX_DATA { return None; }
            let data = &bytes[..*len as usize];
            if data.iter().any(|b| !(0x20..=0x7e).contains(b)) { return None; }
            (6, *name, None, data)
        },
    };
    out[0] = b'F'; out[1] = operation; out[2] = first.len;
    out[4] = payload.len() as u8;
    out[8..8 + first.len as usize].copy_from_slice(first.as_bytes());
    if let Some(second) = second {
        out[3] = second.len;
        out[23..23 + second.len as usize].copy_from_slice(second.as_bytes());
    }
    out[38..38 + payload.len()].copy_from_slice(payload);
    Some(WIRE_BYTES)
}

/// The broker independently validates the full wire; a successful editor
/// parse is never trusted as admission for a request from another process.
pub fn decode(wire: &[u8]) -> Option<Command> {
    if wire.len() != WIRE_BYTES || wire[0] != b'F'
        || wire[5..8].iter().chain(&wire[58..]).any(|b| *b != 0)
    {
        return None;
    }
    let (a, b, n) = (wire[2] as usize, wire[3] as usize, wire[4] as usize);
    if a > MAX_NAME || b > MAX_NAME || n > MAX_DATA
        || wire[8 + a..23].iter().chain(&wire[23 + b..38])
            .chain(&wire[38 + n..58]).any(|byte| *byte != 0)
    {
        return None;
    }
    let name = Name::parse(&wire[8..8 + a])?;
    match wire[1] {
        1..=4 if b == 0 && n == 0 => Some(match wire[1] {
            1 => Command::Touch(name), 2 => Command::Read(name),
            3 => Command::Remove(name), _ => Command::Mkdir(name),
        }),
        5 if n == 0 => Some(Command::Move {
            from: name, to: Name::parse(&wire[23..23 + b])?,
        }),
        6 if b == 0 && n != 0 && wire[38..38 + n].iter()
            .all(|byte| (0x20..=0x7e).contains(byte)) =>
        {
            let mut bytes = [0; MAX_DATA];
            bytes[..n].copy_from_slice(&wire[38..38 + n]);
            Some(Command::Put { name, bytes, len: n as u8 })
        },
        _ => None,
    }
}
