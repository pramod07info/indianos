//! R034/06: Editor semantic-independent symbol index.
use super::Error;
pub fn identifier(source:&[u8],cursor:usize,out:&mut[u8])->Result<usize,Error>{if cursor>=source.len()||!source[cursor].is_ascii_alphanumeric()&&source[cursor]!=b'_'{return Err(Error::Invalid);}let mut a=cursor;while a>0&&(source[a-1].is_ascii_alphanumeric()||source[a-1]==b'_'){a-=1;}let mut b=cursor+1;while b<source.len()&&(source[b].is_ascii_alphanumeric()||source[b]==b'_'){b+=1;}if b-a>out.len(){return Err(Error::Full);}out[..b-a].copy_from_slice(&source[a..b]);Ok(b-a)}
#[cfg(test)] mod tests {use super::*;#[test]fn functional_and_boundary_cases(){let mut out=[0;16];let n=identifier(b"let user_name = 1",7,&mut out).unwrap();assert_eq!(&out[..n],b"user_name");}}
