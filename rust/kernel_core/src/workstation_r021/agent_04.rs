//! R021/04: source outline of Rust functions/types from real RamFs file bytes.
//! Lexical navigation only; it is NOT a Rust parser or semantic compiler index.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind{Function,Structure,Enumeration,Trait}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Symbol{pub name:[u8;48],pub len:usize,pub line:usize,
 pub offset:usize,pub kind:Kind}
pub const EMPTY:Symbol=Symbol{name:[0;48],len:0,line:0,offset:0,kind:Kind::Function};
pub fn outline<const O:usize>(source:&[u8],out:&mut[Symbol;O])
 ->Result<usize,Error>{
 let mut staged=[EMPTY;O];let mut count=0;let mut start=0;let mut line=1;
 while start<source.len(){
  let end=source[start..].iter().position(|b|*b==b'\n')
   .map(|i|start+i).unwrap_or(source.len());
  let text=&source[start..end];
  let trimmed=text.iter().position(|b|*b!=b' '&&*b!=b'\t')
   .map(|x|&text[x..]).unwrap_or(&[]);
  let prefix=[(b"pub fn ".as_slice(),Kind::Function),
   (b"fn ".as_slice(),Kind::Function),
   (b"pub struct ".as_slice(),Kind::Structure),
   (b"struct ".as_slice(),Kind::Structure),
   (b"pub enum ".as_slice(),Kind::Enumeration),
   (b"enum ".as_slice(),Kind::Enumeration),
   (b"trait ".as_slice(),Kind::Trait)];
  for (key,kind) in prefix{
   if trimmed.starts_with(key){
    let ident=&trimmed[key.len()..];
    let len=ident.iter().take_while(|&&b|b.is_ascii_alphanumeric()||b==b'_').count();
    if len==0||len>48{return Err(Error::Invalid);}
    if count>=O{return Err(Error::Full);}
    let mut name=[0u8;48];name[..len].copy_from_slice(&ident[..len]);
    staged[count]=Symbol{name,len,line,offset:start,kind};count+=1;break;
   }
  }
  if end==source.len(){break;}start=end+1;line+=1;
 }
 *out=staged;Ok(count)
}
pub fn from_file<const F:usize,const O:usize>(fs:&RamFs<F>,
 path:&[u8],out:&mut[Symbol;O])->Result<usize,Error>{
 let (_,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if size>CONTENT_MAX{return Err(Error::Full);}
 let mut data=[0u8;CONTENT_MAX];
 let n=fs.read(path,0,&mut data[..size]).map_err(|_|Error::Missing)?;
 outline(&data[..n],out)
}
pub fn first_named<'a>(items:&'a[Symbol],name:&[u8])->Option<&'a Symbol>{
 items.iter().find(|s|&s.name[..s.len]==name)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn real_source_file_lists_functions_and_types_for_navigator(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/src/lib.rs",b"pub struct Book {}\nfn open() {}\n// fn no\ntrait Store {}\n").unwrap();
  let mut out=[EMPTY;4];
  assert_eq!(from_file(&fs,b"/src/lib.rs",&mut out),Ok(3));
  assert_eq!(first_named(&out,b"Book").unwrap().kind,Kind::Structure);
  assert_eq!(first_named(&out,b"open").unwrap().line,2);
 }
 #[test]fn excess_symbols_leave_existing_navigator_untouched(){
  let mut out=[Symbol{line:99,..EMPTY};1];
  assert_eq!(outline(b"fn a(){}\nfn b(){}\n",&mut out),Err(Error::Full));
  assert_eq!(out[0].line,99);
 }
 #[test]fn comment_lines_and_empty_source_produce_no_symbols(){
  let mut out=[EMPTY;2];
  assert_eq!(outline(b"// fn fake()\n",&mut out),Ok(0));
  assert_eq!(outline(b"",&mut out),Ok(0));
 }
}
