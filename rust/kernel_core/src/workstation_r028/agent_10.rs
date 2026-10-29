//! R028/10: typed terminal command parsing without shell.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Command{Cat,Stat,Wc,Head,Grep}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Parsed{pub command:Command,pub path:[u8;96],pub len:usize,
 pub argument:[u8;64],pub arg_len:usize}
pub fn parse(input:&[u8])->Result<Parsed,Error>{
 if input.is_empty()||input.len()>180||input.iter().any(|&b|
  matches!(b,b';'|b'|'|b'>'|b'<'|b'$'|0x60|b'&'|0|b'\n')){return Err(Error::Invalid);}
 let mut words=input.split(|&b|b==b' ').filter(|x|!x.is_empty());
 let command=match words.next().ok_or(Error::Invalid)?{
  b"cat"=>Command::Cat,b"stat"=>Command::Stat,b"wc"=>Command::Wc,
  b"head"=>Command::Head,b"grep"=>Command::Grep,_=>return Err(Error::Denied)
 };
 let path=words.next().ok_or(Error::Invalid)?;
 if path.len()<2||path.len()>96||path[0]!=b'/'||
  path.windows(2).any(|x|x==b".."){return Err(Error::Invalid);}
 let argument=words.next().unwrap_or(b"");
 if words.next().is_some()||argument.len()>64||
  matches!(command,Command::Head|Command::Grep)&&argument.is_empty(){
  return Err(Error::Invalid);
 }
 let mut parsed=Parsed{command,path:[0;96],len:path.len(),argument:[0;64],
  arg_len:argument.len()};
 parsed.path[..path.len()].copy_from_slice(path);
 parsed.argument[..argument.len()].copy_from_slice(argument);Ok(parsed)
}
pub fn read_only(_:Command)->bool{true}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn parses_typed_grep_with_fixed_file_and_pattern(){
  let c=parse(b"grep /app/log error").unwrap();
  assert_eq!(c.command,Command::Grep);
  assert_eq!(&c.argument[..c.arg_len],b"error");
 }
 #[test]fn shell_injection_and_traversal_fail_closed(){
  assert_eq!(parse(b"cat /app/x;rm /app/y"),Err(Error::Invalid));
  assert_eq!(parse(b"cat /app/../secret"),Err(Error::Invalid));
  assert_eq!(parse(b"exec /app/a"),Err(Error::Denied));
 }
}