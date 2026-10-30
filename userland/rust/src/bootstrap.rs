//! Minimal production PID1 bootstrap console; no disks or device privileges.
#[path = "bootstrap_line.rs"]
mod line;
use line::{Command, Line};
pub fn run() -> ! {
    if super::call(0, 0, 0) != 1 || super::call(4, 0, 0) != 1 {
        super::exit(126);
    }
    super::write(b"normal PID1 ready: ABI=1 PID=1 ring3 bootstrap console\n");
    super::write(b"bootstrap> ");
    let mut line = Line::new();
    loop {
        let mut byte = 0u8;
        match super::call(6, core::ptr::addr_of_mut!(byte) as u64, 1) {
            0 => {
                unsafe { core::arch::asm!("pause", options(nomem, nostack, preserves_flags)) };
                continue;
            }
            1 => {}
            _ => super::exit(127),
        }
        let Some(command) = line.feed(byte) else {
            continue;
        };
        match command {
            Command::Help => super::write(b"bootstrap commands: help status halt\n"),
            Command::Status => {
                if super::call(0, 0, 0) != 1 || super::call(4, 0, 0) != 1 {
                    super::exit(126);
                }
                super::write(b"bootstrap status: PID=1 ABI=1 filesystem=offline desktop=offline\n");
            }
            Command::Halt => {
                super::write(b"bootstrap stopping PID1\n");
                super::exit(0);
            }
            Command::Unknown => super::write(b"bootstrap: unsupported command\n"),
            Command::Rejected => super::write(b"bootstrap: input rejected\n"),
        }
        super::write(b"bootstrap> ");
    }
}
