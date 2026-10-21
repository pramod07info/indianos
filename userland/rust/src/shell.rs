//! R013 bounded scripted shell parser, no heap and no dependency on std.
//! Not interactive: no TTY/keyboard, shell pipelines, or arbitrary exec.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Command { Help, Ps, RunWorker, Exit }

pub const BOOT_SCRIPT: &[&[u8]] = &[
    b"help",
    b"ps",
    b"run /bin/worker",
    b"exit",
];

pub fn parse(input: &[u8]) -> Option<Command> {
    match input {
        b"help" => Some(Command::Help),
        b"ps" => Some(Command::Ps),
        b"run /bin/worker" => Some(Command::RunWorker),
        b"exit" => Some(Command::Exit),
        _ => None,
    }
}
