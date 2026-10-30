#[path = "../src/bootstrap_line.rs"]
mod line;
use line::{Command, Line};
fn commands(bytes: &[u8]) -> Vec<Command> {
    let mut line = Line::new();
    bytes.iter().filter_map(|b| line.feed(*b)).collect()
}
#[test]
fn normal_commands_crlf_whitespace_and_unknown() {
    assert_eq!(
        commands(b" help \r\nstatus\nhalt\r"),
        vec![Command::Help, Command::Status, Command::Halt]
    );
    assert_eq!(
        commands(b"\n\r\nformat\nHALT\n"),
        vec![Command::Unknown, Command::Unknown]
    );
}
#[test]
fn normal_backspace_and_delete_are_bounded() {
    assert_eq!(
        commands(b"\x08\x7fhelpx\x08\nhalx\x7ft\n"),
        vec![Command::Help, Command::Halt]
    );
}
#[test]
fn normal_overflow_is_poisoned_until_newline() {
    let mut bytes = vec![b'x'; 33];
    bytes.extend_from_slice(b"\x08halt\nstatus\n");
    assert_eq!(commands(&bytes), vec![Command::Rejected, Command::Status]);
    let mut exact = vec![b'x'; 32];
    exact.push(b'\n');
    assert_eq!(commands(&exact), vec![Command::Unknown]);
}
#[test]
fn normal_controls_and_non_ascii_cannot_form_commands() {
    for byte in (0..=255u8).filter(|b| !matches!(b, 8 | 10 | 13 | 127 | 32..=126)) {
        let mut bytes = b"hel".to_vec();
        bytes.extend_from_slice(&[byte, 8, b'p', b'\n']);
        bytes.extend_from_slice(b"status\n");
        assert_eq!(commands(&bytes), vec![Command::Rejected, Command::Status]);
    }
}
