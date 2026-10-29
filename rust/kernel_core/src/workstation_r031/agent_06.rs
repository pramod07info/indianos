//! R031/06: bounded Rust lexical tokens for editor colorization, not a Rust parser.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind{Code,LineComment,BlockComment,String}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Span{pub start:usize,pub end:usize,pub kind:Kind}
pub const EMPTY:Span=Span{start:0,end:0,kind:Kind::Code};
fn emit<const N:usize>(list:&mut[Span;N],n:&mut usize,
 start:usize,end:usize,kind:Kind)->Result<(),Error>{
 if end==start{return Ok(());}
 if *n==N{return Err(Error::Full);}
 list[*n]=Span{start,end,kind};*n+=1;Ok(())
}
pub fn tokenize<const N:usize>(source:&[u8],out:&mut[Span;N])
 ->Result<usize,Error>{
 if core::str::from_utf8(source).is_err(){return Err(Error::Invalid);}
 let mut stage=[EMPTY;N];let(mut p,mut i,mut n)=(0usize,0usize,0usize);
 while i<source.len(){
  let start=i;
  if source[i..].starts_with(b"//"){
   emit(&mut stage,&mut n,p,start,Kind::Code)?;
   i+=2;while i<source.len()&&source[i]!=b'\n'{i+=1;}
   emit(&mut stage,&mut n,start,i,Kind::LineComment)?;p=i;
  }else if source[i..].starts_with(b"/*"){
   emit(&mut stage,&mut n,p,start,Kind::Code)?;
   i+=2;let mut depth=1usize;
   while i<source.len()&&depth>0{
    if source[i..].starts_with(b"/*"){depth+=1;i+=2;}
    else if source[i..].starts_with(b"*/"){depth-=1;i+=2;}
    else{i+=1;}
   }
   if depth!=0{return Err(Error::Conflict);}
   emit(&mut stage,&mut n,start,i,Kind::BlockComment)?;p=i;
  }else if source[i]==b'"'{
   emit(&mut stage,&mut n,p,start,Kind::Code)?;
   i+=1;let mut ended=false;
   while i<source.len(){
    if source[i]==b'\\'{i=(i+2).min(source.len());continue;}
    if source[i]==b'"'{i+=1;ended=true;break;}
    i+=1;
   }
   if !ended{return Err(Error::Conflict);}
   emit(&mut stage,&mut n,start,i,Kind::String)?;p=i;
  }else{i+=1;}
 }
 emit(&mut stage,&mut n,p,i,Kind::Code)?;
 *out=stage;Ok(n)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn lexical_editor_ignores_comment_syntax_inside_quoted_strings(){
  let mut s=[EMPTY;8];
  let n=tokenize(br#"let x="// not comment"; /* actual */"#,&mut s).unwrap();
  assert_eq!(n,4);assert_eq!(s[1].kind,Kind::String);
  assert_eq!(s[3].kind,Kind::BlockComment);
 }
 #[test]fn nested_unclosed_comment_cannot_produce_misleading_token_stream(){
  let mut s=[EMPTY;2];
  assert_eq!(tokenize(b"/* outer /* inner */",&mut s),Err(Error::Conflict));
  assert_eq!(s,[EMPTY;2]);
 }
}