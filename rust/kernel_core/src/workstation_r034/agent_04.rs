//! R034/04: UTF-8 editor line-range selection.
use super::Error;
pub fn select_lines(source:&[u8],first:usize,last:usize)->Result<(usize,usize),Error>{if first==0||last<first||core::str::from_utf8(source).is_err(){return Err(Error::Invalid);}let(mut line,mut begin,mut end)=(1usize,None,None);for i in 0..=source.len(){if line==first&&begin.is_none(){begin=Some(i);}if i==source.len()||source[i]==b'\n'{if line==last{end=Some((i+usize::from(i<source.len())).min(source.len()));break;}line+=1;}}Ok((begin.ok_or(Error::Missing)?,end.ok_or(Error::Missing)?))}
#[cfg(test)] mod tests {use super::*;#[test]fn functional_and_boundary_cases(){assert_eq!(select_lines(b"a\nb\nc",2,3),Ok((2,5)));assert_eq!(select_lines(b"x",3,3),Err(Error::Missing));}}
