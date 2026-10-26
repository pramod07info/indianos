//! R036/16: bounded interactive terminal command line.
//! Only built-in informational commands are represented; no shell execution.
pub const LINE_BYTES:usize=80;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Command{Help,Clear,ListFiles,PrintWorkingDirectory,Unknown}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum TerminalError{InvalidCharacter,LineFull,Empty}
pub fn parse(line:&[u8])->Result<Command,TerminalError>{
    if line.len()>LINE_BYTES{return Err(TerminalError::LineFull);}
    if line.iter().any(|c|!c.is_ascii_graphic() && *c!=b' '){
        return Err(TerminalError::InvalidCharacter);
    }
    let mut start=0usize;let mut end=line.len();
    while start<end && line[start]==b' '{start+=1;}
    while end>start && line[end-1]==b' '{end-=1;}
    if start==end{return Err(TerminalError::Empty);}
    Ok(match &line[start..end]{
        b"help"=>Command::Help,
        b"clear"=>Command::Clear,
        b"ls"=>Command::ListFiles,
        b"pwd"=>Command::PrintWorkingDirectory,
        _=>Command::Unknown,
    })
}
pub struct Terminal{line:[u8;LINE_BYTES],len:usize}
impl Terminal {
    pub const fn new()->Self{Self{line:[0;LINE_BYTES],len:0}}
    pub fn type_ascii(&mut self,ch:u8)->Result<(),TerminalError>{
        if !ch.is_ascii_graphic()&&ch!=b' '{return Err(TerminalError::InvalidCharacter);}
        if self.len==LINE_BYTES{return Err(TerminalError::LineFull);}
        self.line[self.len]=ch;self.len+=1;Ok(())
    }
    pub fn backspace(&mut self){if self.len>0{self.len-=1;self.line[self.len]=0;}}
    pub fn submit(&mut self)->Result<Command,TerminalError>{
        let result=parse(&self.line[..self.len]);
        self.line=[0;LINE_BYTES];self.len=0;result
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn interactive_line_editing_and_submission(){
        let mut t=Terminal::new();
        for &ch in b"lss"{t.type_ascii(ch).unwrap();}
        t.backspace();
        assert_eq!(t.submit(),Ok(Command::ListFiles));
        assert_eq!(t.submit(),Err(TerminalError::Empty));
    }
    #[test]fn rejects_shell_metacharacters_as_unknown_commands(){
        assert_eq!(parse(b" help "),Ok(Command::Help));
        assert_eq!(parse(b"ls;rm -rf /"),Ok(Command::Unknown));
        assert_eq!(parse(b"pwd"),Ok(Command::PrintWorkingDirectory));
    }
    #[test]fn bounds_and_non_ascii_input_fail_closed(){
        assert_eq!(parse(&[b'x';LINE_BYTES+1]),Err(TerminalError::LineFull));
        let mut t=Terminal::new();
        for _ in 0..LINE_BYTES{t.type_ascii(b'a').unwrap();}
        assert_eq!(t.type_ascii(b'b'),Err(TerminalError::LineFull));
        assert_eq!(t.type_ascii(0),Err(TerminalError::InvalidCharacter));
    }
}
