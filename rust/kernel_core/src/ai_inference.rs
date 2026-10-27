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
pub const MAX_BATCH:usize=8;
/// Execute up to eight dense-model samples with one authenticated grant.
/// All geometry is prevalidated and output remains unchanged on errors.
pub fn run_batch_authorized<const N:usize>(
 access:&Access<N>,who:Identity,now:u64,model:&Dense<'_>,
 inputs:&[i8],samples:usize,output:&mut[i16],
)->Result<usize,Error>{
 access.require(who,VIEW,now).map_err(|_|Error::Denied)?;
 if samples==0||samples>MAX_BATCH||
    model.features.checked_mul(samples)!=Some(inputs.len())||
    model.neurons.checked_mul(samples).is_none_or(|x|output.len()<x){
  return Err(Error::Shape);
 }
 validate(model,model.features,model.neurons)?;
 let mut staged=[0i16;MAX_BATCH*MAX_NEURONS];
 for index in 0..samples{
  let in_start=index*model.features;
  let out_start=index*model.neurons;
  run_dense(model,
    &inputs[in_start..in_start+model.features],
    &mut staged[out_start..out_start+model.neurons])?;
 }
 let total=samples*model.neurons;
 output[..total].copy_from_slice(&staged[..total]);
 Ok(total)
}
/// Execute two real, bounded CPU neural network layers per sample.
/// Both layers and the entire batch are verified before writing output.
pub fn run_two_layer_batch<const N:usize>(
 access:&Access<N>,actor:Identity,now:u64,
 first:&Dense<'_>,second:&Dense<'_>,samples:usize,
 input:&[i8],output:&mut[i16])->Result<usize,Error>{
 access.require(actor,VIEW,now).map_err(|_|Error::Denied)?;
 if samples==0||samples>MAX_BATCH||
   first.neurons!=second.features||
   first.features.checked_mul(samples)!=Some(input.len())||
   second.neurons.checked_mul(samples).is_none_or(|n|n>output.len()){
   return Err(Error::Shape);
 }
 validate(first,first.features,first.neurons)?;
 validate(second,first.neurons,second.neurons)?;
 let mut staged=[0i16;MAX_BATCH*MAX_NEURONS];
 for n in 0..samples{
  let x=n*first.features;let y=n*second.neurons;
  run_two_layers(first,second,&input[x..x+first.features],
    &mut staged[y..y+second.neurons])?;
 }
 let count=samples*second.neurons;
 output[..count].copy_from_slice(&staged[..count]);
 Ok(count)
}
/// Bounded classification of local CPU model logits, not a generative AI
/// assistant or calibrated probability. Ties prefer the lowest output ID.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Prediction{pub class:usize,pub score:i16,pub margin:i32}
impl Prediction{
    pub const fn empty()->Self{Self{class:0,score:0,margin:0}}
}
pub fn top_class(logits:&[i16])->Result<Prediction,Error>{
    if logits.is_empty()||logits.len()>MAX_NEURONS{return Err(Error::Shape);}
    let mut best=0usize;
    let mut runner:Option<i16>=None;
    for i in 1..logits.len(){
        if logits[i]>logits[best]{
            runner=Some(runner.map_or(logits[best],|x|x.max(logits[best])));
            best=i;
        }else{
            runner=Some(runner.map_or(logits[i],|x|x.max(logits[i])));
        }
    }
    Ok(Prediction{class:best,score:logits[best],
        margin:runner.map_or(0,|other|logits[best] as i32-other as i32)})
}
/// Enforce live user identity and run the full real dense CPU pipeline.
pub fn classify_authorized<const N:usize>(
    acl:&Access<N>,owner:Identity,now:u64,model:&Dense<'_>,
    input:&[i8])->Result<Prediction,Error>{
    let mut scores=[0i16;MAX_NEURONS];
    let len=run_authorized(acl,owner,now,model,input,&mut scores)?;
    top_class(&scores[..len])
}
/// Recheck both actual model layers, then classify output. All computation
/// stays in bounded caller/stack memory without disclosing raw model weights.
pub fn classify_two_layer<const N:usize>(
    acl:&Access<N>,owner:Identity,now:u64,
    first:&Dense<'_>,second:&Dense<'_>,input:&[i8])
    ->Result<Prediction,Error>{
    acl.require(owner,VIEW,now).map_err(|_|Error::Denied)?;
    let mut scores=[0i16;MAX_NEURONS];
    let size=run_two_layers(first,second,input,&mut scores)?;
    top_class(&scores[..size])
}
/// Atomic multi-sample classification: a single malformed input, expired
/// grant or undersized output produces no published prediction.
pub fn classify_batch<const N:usize>(
    acl:&Access<N>,owner:Identity,now:u64,model:&Dense<'_>,
    inputs:&[i8],samples:usize,out:&mut[Prediction])
    ->Result<usize,Error>{
    acl.require(owner,VIEW,now).map_err(|_|Error::Denied)?;
    if samples==0||samples>MAX_BATCH||out.len()<samples||
       model.features.checked_mul(samples)!=Some(inputs.len()){
        return Err(Error::Shape);
    }
    validate(model,model.features,model.neurons)?;
    let mut staged=[Prediction::empty();MAX_BATCH];
    let mut scores=[0i16;MAX_NEURONS];
    for i in 0..samples{
        let from=i*model.features;
        let len=run_dense(model,&inputs[from..from+model.features],&mut scores)?;
        staged[i]=top_class(&scores[..len])?;
    }
    out[..samples].copy_from_slice(&staged[..samples]);
    Ok(samples)
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

 #[test]fn batched_inference_executes_distinct_samples_without_aliasing(){
  let weights=[2,-1,-1,4];let bias=[1,-2];
  let m=model(&weights,&bias);
  let who=Identity{pid:30,generation:7};
  let mut auth=Access::<1>::new();auth.grant(who,VIEW,20).unwrap();
  let mut out=[0i16;4];
  assert_eq!(run_batch_authorized(&auth,who,1,&m,&[2,3,1,1],2,&mut out),Ok(4));
  assert_eq!(out,[2,8,2,1]);
 }
 #[test]fn failed_batch_preflight_leaves_output_untouched(){
  let weights=[2,-1,-1,4];let bias=[1,-2];
  let m=model(&weights,&bias);
  let who=Identity{pid:30,generation:7};
  let mut auth=Access::<1>::new();auth.grant(who,VIEW,20).unwrap();
  let mut out=[99i16;4];
  assert_eq!(run_batch_authorized(&auth,who,1,&m,&[1,2],2,&mut out),
   Err(Error::Shape));
  assert_eq!(run_batch_authorized(&auth,who,1,&m,&[1,2],0,&mut out),
   Err(Error::Shape));
  assert_eq!(run_batch_authorized(&auth,who,20,&m,&[1,2,3,4],2,&mut out),
   Err(Error::Denied));
  assert_eq!(out,[99;4]);
 }


 #[test]fn two_layer_batch_executes_real_forward_pass_for_distinct_samples(){
  let w=[2,-1,-1,4];let b=[1,-2];
  let first=model(&w,&b);
  let w2=[2,1];let b2=[0];
  let second=Dense{features:2,neurons:1,
   weights:&w2,bias:&b2,shift:0,relu:true};
  let actor=Identity{pid:50,generation:2};
  let mut acl=Access::<1>::new();acl.grant(actor,VIEW,100).unwrap();
  let mut out=[0i16;2];
  assert_eq!(run_two_layer_batch(&acl,actor,1,&first,&second,2,
    &[2,3,1,1],&mut out),Ok(2));
  assert_eq!(out,[12,5]);
 }
 #[test]fn invalid_second_layer_or_expired_owner_keeps_all_outputs_intact(){
  let w=[2,-1,-1,4];let b=[1,-2];let first=model(&w,&b);
  let w2=[2,1];let b2=[0];
  let mut second=Dense{features:2,neurons:1,
   weights:&w2,bias:&b2,shift:0,relu:true};
  let actor=Identity{pid:50,generation:2};
  let mut acl=Access::<1>::new();acl.grant(actor,VIEW,10).unwrap();
  let mut out=[77i16;2];
  second.features=1;
  assert_eq!(run_two_layer_batch(&acl,actor,1,&first,&second,2,
    &[2,3,1,1],&mut out),Err(Error::Shape));
  second.features=2;
  assert_eq!(run_two_layer_batch(&acl,actor,10,&first,&second,2,
    &[2,3,1,1],&mut out),Err(Error::Denied));
  assert_eq!(out,[77;2]);
 }

 #[test]fn orbit_ai_top_class_margin_ties_and_signed_values(){
  assert_eq!(top_class(&[-2,-7,-3]),Ok(Prediction{class:0,score:-2,margin:1}));
  assert_eq!(top_class(&[4,4,0]),Ok(Prediction{class:0,score:4,margin:0}));
  assert_eq!(top_class(&[]),Err(Error::Shape));
 }
 #[test]fn orbit_ai_batched_classification_never_partially_publishes(){
  let weight=[1i8,0,0,1];let bias=[0i32,0];
  let m=model(&weight,&bias);
  let who=Identity{pid:9,generation:3};
  let mut acl=Access::<1>::new();acl.grant(who,VIEW,20).unwrap();
  let sentinel=Prediction{class:99,score:99,margin:99};
  let mut out=[sentinel;2];
  assert_eq!(classify_batch(&acl,who,1,&m,&[3,1,1,8],2,&mut out),Ok(2));
  assert_eq!(out[0].class,0);assert_eq!(out[1].class,1);
  out=[sentinel;2];
  assert_eq!(classify_batch(&acl,who,20,&m,&[3,1,1,8],2,&mut out),
             Err(Error::Denied));
  assert_eq!(out,[sentinel;2]);
  assert_eq!(classify_batch(&acl,who,1,&m,&[3,1],2,&mut out),
             Err(Error::Shape));
  assert_eq!(out,[sentinel;2]);
 }

}
