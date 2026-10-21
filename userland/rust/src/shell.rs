//! R015: allocation-free command parser for the Rust userland shell.
//! The command language is intentionally bounded: no pipelines, quoting,
//! filesystem traversal or arbitrary executable loading.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Command { Help, Ps, RunWorker, Version, Exit }

pub const BOOT_SCRIPT: &[&[u8]] = &[
    b"help", b"ps", b"run /bin/worker", b"exit",
];

fn whitespace(b:u8)->bool{b==b' '||b==b'\t'}
pub fn parse(input:&[u8])->Option<Command>{
    let mut start=0;
    let mut end=input.len();
    while start<end&&whitespace(input[start]){start+=1;}
    while end>start&&whitespace(input[end-1]){end-=1;}
    let trimmed=&input[start..end];
    match trimmed {
        b"help"=>Some(Command::Help),
        b"ps"=>Some(Command::Ps),
        b"version"=>Some(Command::Version),
        b"exit"=>Some(Command::Exit),
        _=>{
            // Permit bounded whitespace between the two tokens, but no
            // extra arguments or shell metacharacters.
            if !trimmed.starts_with(b"run"){return None;}
            let rest=&trimmed[3..];
            let mut i=0;
            while i<rest.len()&&whitespace(rest[i]){i+=1;}
            if i==0{return None;}
            (rest[i..]==*b"/bin/worker").then_some(Command::RunWorker)
        }
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn parses_existing_script(){
        assert_eq!(parse(b"help"),Some(Command::Help));
        assert_eq!(parse(b"ps"),Some(Command::Ps));
        assert_eq!(parse(b"run /bin/worker"),Some(Command::RunWorker));
        assert_eq!(parse(b"exit"),Some(Command::Exit));
    }
    #[test]
    fn trims_and_rejects_unrecognized_input(){
        assert_eq!(parse(b"  version\t"),Some(Command::Version));
        assert_eq!(parse(b"\trun  /bin/worker "),Some(Command::RunWorker));
        for invalid in [b"".as_slice(),b"run",b"run /bin/other",b"run /bin/worker extra",b"help;exit",b"RUN /bin/worker"]{
            assert_eq!(parse(invalid),None);
        }
    }
}
