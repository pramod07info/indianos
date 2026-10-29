//! R034/15: Incremental compilation changed source selector.
use super::Error;
pub fn dirty(old:&[u64],new:&[u64],out:&mut[usize])->Result<usize,Error>{if old.len()!=new.len(){return Err(Error::Invalid);}let n=old.iter().zip(new).filter(|(a,b)|a!=b).count();if n>out.len(){return Err(Error::Full);}let mut k=0;for(i,(a,b))in old.iter().zip(new).enumerate(){if a!=b{out[k]=i;k+=1;}}Ok(n)}
#[cfg(test)] mod tests {use super::*;#[test]fn functional_and_boundary_cases(){let mut out=[0usize;2];assert_eq!(dirty(&[1,2,3],&[1,4,6],&mut out),Ok(2));assert_eq!(out,[1,2]);}}
