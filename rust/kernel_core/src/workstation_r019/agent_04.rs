//! R019/04: strict compiler log source navigation for IDE diagnostics.
//! Only parses text. It never starts a process or trusts a parsed path.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Severity{Error,Warning,Note}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Diagnostic{pub path:[u8;80],pub path_len:usize,
 pub line:u32,pub column:u32,pub severity:Severity,
 pub message:[u8;128],pub message_len:usize}
fn number(s:&[u8])->Result<u32,Error>{
 if s.is_empty()||s.len()>10{return Err(Error::Invalid);}
 let mut v=0u32;
 for &b in s{if !b.is_ascii_digit(){return Err(Error::Invalid);}
  v=v.checked_mul(10).and_then(|x|x.checked_add((b-b'0') as u32))
   .ok_or(Error::Overflow)?;
 }
 if v==0{return Err(Error::Invalid);}Ok(v)
}
pub fn parse(input:&[u8])->Result<Diagnostic,Error>{
 let mut fields=input.splitn(4,|b|*b==b':');
 let path=fields.next().ok_or(Error::Invalid)?;
 let line=number(fields.next().ok_or(Error::Invalid)?)?;
 let column=number(fields.next().ok_or(Error::Invalid)?)?;
 let body=fields.next().ok_or(Error::Invalid)?;
 if path.is_empty()||path.len()>80||body.is_empty()||body.len()>128||
  path.iter().any(|b|!b.is_ascii_graphic()||*b==b'\\')||
  path.windows(2).any(|w|w==b"..")||
  body.iter().any(|b|!b.is_ascii_graphic()&&*b!=b' '){return Err(Error::Invalid);}
 let severity=if body.starts_with(b"error "){Severity::Error}
  else if body.starts_with(b"warning "){Severity::Warning}
  else{Severity::Note};
 let mut output=Diagnostic{path:[0;80],path_len:path.len(),line,column,
  severity,message:[0;128],message_len:body.len()};
 output.path[..path.len()].copy_from_slice(path);
 output.message[..body.len()].copy_from_slice(body);
 Ok(output)
}
pub fn parse_batch<const N:usize>(lines:&[&[u8]],out:&mut[Diagnostic;N])
 ->Result<usize,Error>{
 if lines.is_empty()||lines.len()>N{return Err(Error::Full);}
 let empty=Diagnostic{path:[0;80],path_len:0,line:0,column:0,
  severity:Severity::Note,message:[0;128],message_len:0};
 let mut staged=[empty;N];
 for (i,&line) in lines.iter().enumerate(){staged[i]=parse(line)?;}
 *out=staged;Ok(lines.len())
}
#[cfg(test)]mod tests{
 use super::*;
 fn empty()->Diagnostic{Diagnostic{path:[0;80],path_len:0,
  line:0,column:0,severity:Severity::Note,message:[0;128],message_len:0}}
 #[test]fn maps_rust_errors_and_warnings_to_editor_locations(){
  let d=parse(b"src/main.rs:12:4:error missing type").unwrap();
  assert_eq!(d.line,12);assert_eq!(d.column,4);
  assert_eq!(d.severity,Severity::Error);
  assert_eq!(parse(b"src/lib.rs:3:2:warning unused").unwrap().severity,Severity::Warning);
 }
 #[test]fn malformed_second_log_keeps_original_screen(){
  let mut out=[empty();2];
  assert_eq!(parse_batch(&[b"src/a.rs:1:2:error bad",b"../evil:1:2:error bad"],&mut out),
    Err(Error::Invalid));
  assert_eq!(out,[empty();2]);
 }
 #[test]fn overflow_line_numbers_and_nonprinting_strings_are_denied(){
  assert!(parse(b"src/a:4294967296:1:error fail").is_err());
  assert!(parse(b"src/a:1:2:error\nfail").is_err());
 }
}
