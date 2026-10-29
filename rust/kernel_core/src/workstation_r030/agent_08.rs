//! R030/08: lexical bracket validation with quoted-string escape handling.
use super::Error;
pub fn balanced(source:&[u8])->Result<usize,Error>{
 let mut stack=[0u8;128];let(mut depth,mut peak,mut quote,mut escape)=(0usize,0usize,0u8,false);
 for &b in source{
  if quote!=0{
   if escape{escape=false;}else if b==b'\\'{escape=true;}
   else if b==quote{quote=0;}
   continue;
  }
  if b==b'"'{quote=b;continue;}
  match b{
   b'(' | b'[' | b'{' => {
    if depth==stack.len(){return Err(Error::Full);}
    stack[depth]=b;depth+=1;peak=peak.max(depth);
   }
   b')' | b']' | b'}' => {
    if depth==0{return Err(Error::Conflict);}
    depth-=1;
    if !matches!((stack[depth],b),(b'(',b')')|(b'[',b']')|(b'{',b'}')){
     return Err(Error::Conflict);
    }
   }
   _=>{}
  }
 }
 if quote!=0||depth!=0{return Err(Error::Conflict);}Ok(peak)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn nested_braces_and_delimiters_in_strings_do_not_conflict(){
  assert_eq!(balanced(br#"fn main(){print("}"); [1,2]}"#),Ok(2));
 }
 #[test]fn bad_nesting_and_unclosed_quote_fail_closed(){
  assert_eq!(balanced(b"{(})"),Err(Error::Conflict));
  assert_eq!(balanced(b"\"unclosed"),Err(Error::Conflict));
 }
}