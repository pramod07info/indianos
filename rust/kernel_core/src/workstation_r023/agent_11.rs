//! R023/11: bounded terminal command-line lexer with quoting and escaping.
//! This parses text only; it never executes or expands arbitrary shell commands.
use super::Error;
pub const TOKEN_MAX:usize=96;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Token{pub bytes:[u8;TOKEN_MAX],pub len:usize}
pub const EMPTY:Token=Token{bytes:[0;TOKEN_MAX],len:0};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Quote{None,Single,Double}
pub fn lex<const N:usize>(line:&[u8],out:&mut[Token;N])
 ->Result<usize,Error>{
 if line.len()>1024{return Err(Error::Full);}
 let mut stage=[EMPTY;N];let mut count=0usize;
 let mut word=EMPTY;let mut active=false;
 let mut quote=Quote::None;let mut escape=false;
 for &ch in line{
  if escape{
   if word.len>=TOKEN_MAX{return Err(Error::Full);}
   word.bytes[word.len]=ch;word.len+=1;escape=false;active=true;
   continue;
  }
  if ch==b'\\'&&quote!=Quote::Single{escape=true;active=true;continue;}
  if ch==b'\''&&quote!=Quote::Double{
   quote=if quote==Quote::Single{Quote::None}else{Quote::Single};
   active=true;continue;
  }
  if ch==b'"'&&quote!=Quote::Single{
   quote=if quote==Quote::Double{Quote::None}else{Quote::Double};
   active=true;continue;
  }
  if (ch==b' '||ch==b'\t')&&quote==Quote::None{
   if active{
    if count>=N{return Err(Error::Full);}
    stage[count]=word;count+=1;word=EMPTY;active=false;
   }continue;
  }
  if ch==0||ch==b'\n'||ch==b'\r'||ch==0x1b{return Err(Error::Invalid);}
  if word.len>=TOKEN_MAX{return Err(Error::Full);}
  word.bytes[word.len]=ch;word.len+=1;active=true;
 }
 if escape||quote!=Quote::None{return Err(Error::Invalid);}
 if active{
  if count>=N{return Err(Error::Full);}
  stage[count]=word;count+=1;
 }
 *out=stage;Ok(count)
}
pub fn equals(token:&Token,text:&[u8])->bool{
 &token.bytes[..token.len]==text
}
pub fn as_str(token:&Token)->Result<&str,Error>{
 core::str::from_utf8(&token.bytes[..token.len]).map_err(|_|Error::Invalid)
}
pub fn render<const N:usize>(tokens:&[Token],out:&mut[u8;N])
 ->Result<usize,Error>{
 let mut stage=[0u8;N];let mut n=0;
 for (i,t) in tokens.iter().enumerate(){
  let sep=if i==0{0}else{1};
  if n.checked_add(sep+t.len).is_none_or(|end|end>N){return Err(Error::Full);}
  if sep>0{stage[n]=b' ';n+=1;}
  stage[n..n+t.len].copy_from_slice(&t.bytes[..t.len]);n+=t.len;
 }
 *out=stage;Ok(n)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn quoted_source_path_and_escaped_spaces_are_single_arguments(){
  let mut args=[EMPTY;4];
  let n=lex(b"cat \"my project/file.rs\" a\\ b",&mut args).unwrap();
  assert_eq!(n,3);assert!(equals(&args[1],b"my project/file.rs"));
  assert!(equals(&args[2],b"a b"));
 }
 #[test]fn unterminated_quote_preserves_original_command_arguments(){
  let mut args=[Token{len:55,..EMPTY};2];
  assert_eq!(lex(b"cat 'broken",&mut args),Err(Error::Invalid));
  assert_eq!(args[0].len,55);
 }
 #[test]fn overlong_argument_and_command_injection_escape_rejected(){
  let mut args=[EMPTY;1];
  assert_eq!(lex(&[b'x';TOKEN_MAX+1],&mut args),Err(Error::Full));
  assert_eq!(lex(b"ls\nrm -rf /",&mut args),Err(Error::Invalid));
 }
}
