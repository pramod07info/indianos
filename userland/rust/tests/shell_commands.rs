#[path = "../src/shell.rs"]
mod shell;
use shell::{Command, BOOT_SCRIPT, parse};

#[test]
fn launch_script_contains_allowlisted_commands_and_terminates() {
    let actual: Vec<Command> = BOOT_SCRIPT.iter().map(|line|parse(line).unwrap()).collect();
    assert_eq!(actual,[Command::Help,Command::Ps,Command::RunWorker,Command::Exit]);
}
#[test]
fn reject_arbitrary_programs_traversal_prefixes_or_injection() {
    for candidate in [
        b"".as_slice(), b"rm -rf /", b"run /bin/../worker",
        b"run /bin/init", b"run /bin/worker extra", b"spawn /bin/worker",
        b"run /bin/worker\n", b"help ", b"ps all", b"exit now",
        b"run /bin/worker\0",
    ] {
        assert!(parse(candidate).is_none(),"accepted: {candidate:?}");
    }
}
