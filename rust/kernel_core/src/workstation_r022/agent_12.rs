//! R022/12: discover Rust unit test declarations from actual project RamFs files.
//! Lexical hints, not an AST compiler; quarantines malformed test identifiers.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Test{pub path:[u8;PATH_MAX],pub len:usize,
 pub name:[u8;64],pub name_len:usize,pub line:usize}
pub const EMPTY:Test=Test{path:[0;PATH_MAX],len:0,name:[0;64],
 name_len:0,line:0};
fn ident(b:u8)->bool{b.is_ascii_alphanumeric()||b==b'_'}
pub fn discover<const T:usize>(source:&[u8],path:&[u8],
 out:&mut[Test;T])->Result<usize,Error>{
 if path.is_empty()||path.len()>PATH_MAX{return Err(Error::Invalid);}
 let mut staged=[EMPTY;T];let mut count=0usize;let mut line=1usize;let mut last_attr=false;
 for text in source.split(|b|*b==b'\n'){
  let trimmed=&text[text.iter().position(|b|*b!=b' '&&*b!=b'\t')
   .unwrap_or(text.len())..];
  if trimmed.starts_with(b"#[test]"){last_attr=true;}
  else if last_attr{
   let decl=if trimmed.starts_with(b"pub fn "){&trimmed[7..]}
    else if trimmed.starts_with(b"fn "){&trimmed[3..]}
    else{last_attr=false;line+=1;continue;};
   let n=decl.iter().take_while(|&&b|ident(b)).count();
   if n==0||n>64||decl.get(n)!=Some(&b'('){return Err(Error::Invalid);}
   if count==T{return Err(Error::Full);}
   let mut t=EMPTY;t.len=path.len();t.path[..path.len()].copy_from_slice(path);
   t.name[..n].copy_from_slice(&decl[..n]);t.name_len=n;t.line=line;
   staged[count]=t;count+=1;last_attr=false;
  }
  line+=1;
 }
 *out=staged;Ok(count)
}
pub fn discover_file<const F:usize,const T:usize>(fs:&RamFs<F>,
 path:&[u8],out:&mut[Test;T])->Result<usize,Error>{
 let (_,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut source=[0u8;CONTENT_MAX];
 let n=fs.read(path,0,&mut source[..size]).map_err(|_|Error::Missing)?;
 discover(&source[..n],path,out)
}
pub fn named<'a>(items:&'a[Test],name:&[u8])->Option<&'a Test>{
 items.iter().find(|x|&x.name[..x.name_len]==name)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn discovers_tests_from_real_rust_file_with_original_lines(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/src/tests.rs",
   b"#[test]\nfn first() {}\n// ignored\n#[test]\nfn second() {}\n").unwrap();
  let mut out=[EMPTY;3];
  assert_eq!(discover_file(&fs,b"/src/tests.rs",&mut out),Ok(2));
  assert_eq!(out[0].line,2);assert_eq!(named(&out[..2],b"second").unwrap().line,5);
 }
 #[test]fn malformed_test_name_rejected_before_output_publish(){
  let marked=Test{line:99,..EMPTY};let mut out=[marked;1];
  assert_eq!(discover(b"#[test]\nfn broken name(){}\n",b"/a",&mut out),
    Err(Error::Invalid));assert_eq!(out,[marked]);
 }
 #[test]fn test_count_limit_blocks_partial_discovery(){
  let mut out=[EMPTY;1];
  assert_eq!(discover(b"#[test]\nfn a(){}\n#[test]\nfn b(){}\n",b"/a",&mut out),
    Err(Error::Full));assert_eq!(out,[EMPTY]);
 }
}
