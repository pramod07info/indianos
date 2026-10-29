//! R034/14: Build stage dependency DAG validation.
use super::Error;
pub fn validate(dependencies:&[u32])->Result<(),Error>{if dependencies.len()>32{return Err(Error::Full);}let all=if dependencies.len()==32{u32::MAX}else{(1u32<<dependencies.len())-1};for(i,mask)in dependencies.iter().enumerate(){if mask&!all!=0||mask&(1u32<<i)!=0{return Err(Error::Invalid);}}let mut complete=0;for _ in 0..dependencies.len(){let mut changed=false;for(i,mask)in dependencies.iter().enumerate(){if complete&(1<<i)==0&&mask&!complete==0{complete|=1<<i;changed=true;}}if !changed{break;}}if complete!=all{Err(Error::Conflict)}else{Ok(())}}
#[cfg(test)] mod tests {use super::*;#[test]fn functional_and_boundary_cases(){assert_eq!(validate(&[0,1,2]),Ok(()));assert_eq!(validate(&[2,1]),Err(Error::Invalid));}}
