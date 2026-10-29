//! R034/07: Explorer drag destination validation.
use super::Error;
pub fn destination(root:&[u8],from:&[u8],to:&[u8])->Result<(),Error>{if root.len()<2||from==to||!from.starts_with(root)||!to.starts_with(root)||from.get(root.len())!=Some(&b'/')||to.get(root.len())!=Some(&b'/')||to.starts_with(from)&&to.get(from.len())==Some(&b'/')||to.windows(2).any(|x|x==b".."){return Err(Error::Denied);}Ok(())}
#[cfg(test)] mod tests {use super::*;#[test]fn functional_and_boundary_cases(){assert_eq!(destination(b"/p",b"/p/a",b"/p/b"),Ok(()));assert_eq!(destination(b"/p",b"/p/a",b"/p/a/b"),Err(Error::Denied));}}
