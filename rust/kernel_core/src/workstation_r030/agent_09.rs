//! R030/09: bounded lexical Rust function outline for code navigation.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Function{pub byte:usize,pub line:usize,pub name:[u8;48],pub len:usize}
pub const EMPTY:Function=Function{byte:0,line:0,name:[0;48],len:0};
fn ident(b:u8)->bool{b.is_ascii_alphanumeric()||b==b'_'}
pub fn outline<const N:usize>(source:&[u8],out:&mut[Function;N])
 ->Result<usize,Error>{
 let mut staged=[EMPTY;N];let(mut n,mut i,mut line)=(0,0,1);
 while i<source.len(){
  if i+3<=source.len()&&&source[i..i+3]==b"fn "&&
   (i==0||!ident(source[i-1])){
   let start=i+3;let mut end=start;
   while end<source.len()&&ident(source[end]){end+=1;}
   if end>start&&end-start<=48{
    if n==N{return Err(Error::Full);}
    let mut f=EMPTY;f.byte=i;f.line=line;f.len=end-start;
    f.name[..f.len].copy_from_slice(&source[start..end]);
    staged[n]=f;n+=1;
   }
  }
  if source[i]==b'\n'{line+=1;}i+=1;
 }
 *out=staged;Ok(n)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn source_outline_tracks_actual_function_names_and_lines(){
  let mut found=[EMPTY;2];assert_eq!(outline(b"fn first(){}\npub fn next(){}", &mut found),Ok(2));
  assert_eq!(&found[1].name[..found[1].len],b"next");assert_eq!(found[1].line,2);
 }
 #[test]fn outline_overflow_preserves_callers_buffer(){
  let mut found=[EMPTY;1];
  assert_eq!(outline(b"fn a(){}\nfn b(){}", &mut found),Err(Error::Full));
  assert_eq!(found,[EMPTY]);
 }
}