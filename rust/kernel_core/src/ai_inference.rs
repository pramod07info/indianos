//! Issue #262: first real allocation-free CPU inference primitive for IndianOS.
//! Performs signed int8 dense-layer MAC operations on actual caller input,
//! with bounded model geometry and explicit process-grant checks.
//! This is not an LLM, trained model loader, GPU/NPU executor or QPU driver.
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,VIEW};

pub const MAX_FEATURES:usize=64;
pub const MAX_NEURONS:usize=16;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Shape,Unsupported,Denied}

/// Weights are row-major [neuron][input]. Biases are signed int32 and
/// each MAC is accumulated in i64 before quantizing. No heap allocations.
pub struct Dense<'a>{
 pub features:usize,
 pub neurons:usize,
 pub weights:&'a[i8],
 pub bias:&'a[i32],
 pub shift:u8,
 pub relu:bool,
}
fn validate(m:&Dense<'_>,input_len:usize,out_len:usize)->Result<(),Error>{
 if m.features==0||m.features>MAX_FEATURES||
    m.neurons==0||m.neurons>MAX_NEURONS||
    m.shift>15||input_len!=m.features||out_len<m.neurons||
    m.features.checked_mul(m.neurons)!=Some(m.weights.len())||
    m.bias.len()!=m.neurons{return Err(Error::Shape);}
 Ok(())
}
/// Exact deterministic int8 dense-layer inference. Preflights every shape
/// before modifying output, so invalid models never expose partial results.
pub fn run_dense(model:&Dense<'_>,input:&[i8],output:&mut[i16])
 ->Result<usize,Error>{
 validate(model,input.len(),output.len())?;
 let mut staged=[0i16;MAX_NEURONS];
 for neuron in 0..model.neurons{
  let mut acc=i64::from(model.bias[neuron]);
  for feature in 0..model.features{
   let weight=model.weights[neuron*model.features+feature];
   acc+=i64::from(weight)*i64::from(input[feature]);
  }
  let quant=acc>>model.shift;
  let activated=if model.relu{quant.max(0)}else{quant};
  staged[neuron]=activated.clamp(i16::MIN as i64,i16::MAX as i64) as i16;
 }
 output[..model.neurons].copy_from_slice(&staged[..model.neurons]);
 Ok(model.neurons)
}
/// Model composition for two bounded CPU layers. Intermediate results are
/// requantized to int8. A failed shape check leaves the caller output intact.
pub fn run_two_layers(first:&Dense<'_>,second:&Dense<'_>,
 input:&[i8],output:&mut[i16])->Result<usize,Error>{
 validate(first,input.len(),first.neurons)?;
 validate(second,first.neurons,output.len())?;
 let mut tmp=[0i16;MAX_NEURONS];
 run_dense(first,input,&mut tmp[..first.neurons])?;
 let mut features=[0i8;MAX_NEURONS];
 for i in 0..first.neurons{
  features[i]=tmp[i].clamp(i8::MIN as i16,i8::MAX as i16) as i8;
 }
 run_dense(second,&features[..first.neurons],output)
}
/// A trusted runtime derives the Identity from its process table. The
/// resource grant must remain live at invocation; no user-supplied PID allowed.
pub fn run_authorized<const N:usize>(access:&Access<N>,who:Identity,now:u64,
 model:&Dense<'_>,input:&[i8],output:&mut[i16])->Result<usize,Error>{
 access.require(who,VIEW,now).map_err(|_|Error::Denied)?;
 run_dense(model,input,output)
}
#[cfg(test)]
mod tests{
 use super::*;
 fn model<'a>(weights:&'a[i8],bias:&'a[i32])->Dense<'a>{
  Dense{features:2,neurons:2,weights,bias,shift:0,relu:false}
 }
 #[test]fn actual_dense_cpu_math_is_deterministic(){
  let weights=[2,-1,-1,4];let bias=[1,-2];
  let m=model(&weights,&bias);
  let mut out=[0i16;2];
  assert_eq!(run_dense(&m,&[2,3],&mut out),Ok(2));
  assert_eq!(out,[2,8]);
  let mut second=[0i16;1];
  let weights2=[2,1];let bias2=[0];
  let hidden=Dense{features:2,neurons:1,weights:&weights2,
   bias:&bias2,shift:0,relu:true};
  assert_eq!(run_two_layers(&m,&hidden,&[2,3],&mut second),Ok(1));
  assert_eq!(second,[12]);
 }
 #[test]fn invalid_geometry_never_partially_writes_model_results(){
  let weights=[2,-1,-1,4];let bias=[1,-2];
  let mut m=model(&weights,&bias);
  let mut out=[99i16;2];
  m.features=3;
  assert_eq!(run_dense(&m,&[2,3,4],&mut out),Err(Error::Shape));
  assert_eq!(out,[99;2]);
  m.features=2;m.shift=16;
  assert_eq!(run_dense(&m,&[2,3],&mut out),Err(Error::Shape));
  assert_eq!(out,[99;2]);
 }
 #[test]fn saturation_relu_and_output_size_are_bounded(){
  let weights=[127,127];let bias=[i32::MAX];
  let m=Dense{features:2,neurons:1,weights:&weights,bias:&bias,
   shift:0,relu:false};
  let mut out=[0i16;1];
  assert_eq!(run_dense(&m,&[127,127],&mut out),Ok(1));
  assert_eq!(out,[i16::MAX]);
  let bias=[-100];let relu=Dense{bias:&bias,relu:true,..m};
  assert_eq!(run_dense(&relu,&[-1,-1],&mut out),Ok(1));
  assert_eq!(out,[0]);
 }
 #[test]fn pid_reuse_expiry_and_missing_rights_prevent_inference(){
  let weights=[2,-1,-1,4];let bias=[1,-2];
  let m=model(&weights,&bias);
  let actor=Identity{pid:30,generation:7};
  let mut access=Access::<1>::new();access.grant(actor,VIEW,20).unwrap();
  let mut out=[8i16;2];
  let stale=Identity{generation:8,..actor};
  assert_eq!(run_authorized(&access,stale,1,&m,&[2,3],&mut out),
    Err(Error::Denied));
  assert_eq!(run_authorized(&access,actor,20,&m,&[2,3],&mut out),
    Err(Error::Denied));
  assert_eq!(out,[8,8]);
  assert_eq!(run_authorized(&access,actor,1,&m,&[2,3],&mut out),Ok(2));
  assert_eq!(out,[2,8]);
 }
}
