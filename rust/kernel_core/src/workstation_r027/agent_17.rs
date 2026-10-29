//! R027/17: safe typed debugger arithmetic on trusted integer observations.
//! No eval, arbitrary code execution, memory access, scripts or dynamic loading.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Operator{Add,Subtract,Multiply,Divide,Equal,Greater}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ResultValue{pub number:i64,pub boolean:bool}
pub fn evaluate(left:i64,right:i64,op:Operator)->Result<ResultValue,Error>{
 let value=match op{
  Operator::Add=>left.checked_add(right).ok_or(Error::Overflow)?,
  Operator::Subtract=>left.checked_sub(right).ok_or(Error::Overflow)?,
  Operator::Multiply=>left.checked_mul(right).ok_or(Error::Overflow)?,
  Operator::Divide=>left.checked_div(right).ok_or(Error::Invalid)?,
  Operator::Equal=>i64::from(left==right),
  Operator::Greater=>i64::from(left>right)
 };
 Ok(ResultValue{number:value,boolean:value!=0})
}
pub fn parse_decimal(bytes:&[u8])->Result<i64,Error>{
 if bytes.is_empty()||bytes.len()>20{return Err(Error::Invalid);}
 let mut negative=false;let mut index=0;
 if bytes[0]==b'-'{negative=true;index=1;}
 if index==bytes.len(){return Err(Error::Invalid);}
 let mut value=0i64;
 for &b in &bytes[index..]{
  if !b.is_ascii_digit(){return Err(Error::Invalid);}
  let digit=(b-b'0') as i64;
  value=if negative{value.checked_mul(10).and_then(|v|v.checked_sub(digit))}
   else{value.checked_mul(10).and_then(|v|v.checked_add(digit))}
   .ok_or(Error::Overflow)?;
 }
 Ok(value)
}
pub fn evaluate_text(left:&[u8],right:&[u8],
 op:Operator)->Result<ResultValue,Error>{
 evaluate(parse_decimal(left)?,parse_decimal(right)?,op)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn debugger_calculates_bounded_step_variable_expression(){
  assert_eq!(evaluate_text(b"41",b"1",Operator::Add).unwrap().number,42);
  assert_eq!(evaluate_text(b"4",b"5",Operator::Greater).unwrap().boolean,false);
 }
 #[test]fn integer_overflow_and_division_by_zero_are_rejected(){
  assert_eq!(evaluate(i64::MAX,1,Operator::Add),Err(Error::Overflow));
  assert_eq!(evaluate(5,0,Operator::Divide),Err(Error::Invalid));
 }
 #[test]fn arbitrary_non_numeric_code_is_not_executed(){
  assert_eq!(parse_decimal(b"1;rm -rf /"),Err(Error::Invalid));
  assert_eq!(parse_decimal(b""),Err(Error::Invalid));
 }
}
