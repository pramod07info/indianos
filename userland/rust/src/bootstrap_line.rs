//! Bounded command decoder for the normal, unprivileged PID1 bootstrap console.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Status,
    Halt,
    Unknown,
    Rejected,
}
pub struct Line {
    bytes: [u8; 32],
    len: usize,
    rejected: bool,
    after_cr: bool,
}
impl Line {
    pub const fn new() -> Self {
        Self {
            bytes: [0; 32],
            len: 0,
            rejected: false,
            after_cr: false,
        }
    }
    pub fn feed(&mut self, byte: u8) -> Option<Command> {
        if byte == b'\n' && self.after_cr {
            self.after_cr = false;
            return None;
        }
        self.after_cr = byte == b'\r';
        if matches!(byte, b'\r' | b'\n') {
            let command = if self.rejected {
                Some(Command::Rejected)
            } else {
                match core::str::from_utf8(&self.bytes[..self.len])
                    .unwrap()
                    .trim()
                {
                    "" => None,
                    "help" => Some(Command::Help),
                    "status" => Some(Command::Status),
                    "halt" => Some(Command::Halt),
                    _ => Some(Command::Unknown),
                }
            };
            self.bytes.fill(0);
            self.len = 0;
            self.rejected = false;
            return command;
        }
        if self.rejected {
            return None;
        }
        if matches!(byte, 8 | 127) {
            if self.len > 0 {
                self.len -= 1;
                self.bytes[self.len] = 0;
            }
        } else if (32..=126).contains(&byte) && self.len < self.bytes.len() {
            self.bytes[self.len] = byte;
            self.len += 1;
        } else {
            // Once invalid or full, backspace cannot turn a rejected command
            // into a privileged-looking suffix. Only newline resets state.
            self.bytes.fill(0);
            self.len = 0;
            self.rejected = true;
        }
        None
    }
}
