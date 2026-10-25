//! R038/31: bounded, explainable AI catalog eligibility; models have no install authority.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct App{pub id:u32,pub arch:u16,pub trusted:bool,pub permissions:u32,pub risk:u8,pub score:u8}
#[derive(Debug,PartialEq,Eq)]pub enum Error{Invalid,NoMatch}
pub fn recommend(items:&[App],arch:u16,grants:u32,max_risk:u8)->Result<u32,Error>{
 if arch==0||items.len()>128||max_risk>10{return Err(Error::Invalid);}
 let mut best=None;
 for &a in items{
  if a.id==0||a.score>100||a.risk>10{return Err(Error::Invalid);}
  if a.arch!=arch||!a.trusted||a.permissions&!grants!=0||a.risk>max_risk{continue;}
  if best.is_none_or(|b:App|a.score>b.score||(a.score==b.score&&a.id<b.id)){best=Some(a);}
 }
 best.map(|a|a.id).ok_or(Error::NoMatch)
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn no_unsigned_or_overprivileged_suggestions(){
  let a=App{id:1,arch:62,trusted:false,permissions:0,risk:1,score:100};
  let b=App{id:2,arch:62,trusted:true,permissions:4,risk:1,score:90};
  assert_eq!(recommend(&[a,b],62,4,2),Ok(2));
  assert_eq!(recommend(&[a,b],62,0,2),Err(Error::NoMatch));
 }
}