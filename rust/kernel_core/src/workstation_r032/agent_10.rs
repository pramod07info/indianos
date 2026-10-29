//! R032/10: source code folding positions, honoring comments and strings.
use super::Error;
use crate::workstation_r031::agent_06::{self,Span,Kind,EMPTY};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Fold{pub start_line:usize,pub end_line:usize,pub open_byte:usize,
 pub close_byte:usize}
pub const NONE:Fold=Fold{start_line:0,end_line:0,open_byte:0,close_byte:0};
pub fn find<const N:usize,const T:usize>(source:&[u8],out:&mut[Fold;N])
 ->Result<usize,Error>{
 let mut spans=[EMPTY;T];let n=agent_06::tokenize(source,&mut spans)
  .map_err(|_|Error::Invalid)?;
 let mut stack=[(0usize,0usize);128];let(mut depth,mut count,mut line)=(0,0,1usize);
 let mut stage=[NONE;N];
 let mut cursor=0usize;
 for span in &spans[..n]{
  for (offset,b) in source[span.start..span.end].iter().enumerate(){
   let pos=span.start+offset;
   while cursor<pos{if source[cursor]==b'\n'{line+=1;}cursor+=1;}
   if span.kind==Kind::Code{
    if *b==b'{'{
     if depth==stack.len(){return Err(Error::Full);}
     stack[depth]=(pos,line);depth+=1;
    }else if *b==b'}'{
     if depth==0{return Err(Error::Conflict);}
     depth-=1;let(begin,first)=stack[depth];
     if line>first{
      if count==N{return Err(Error::Full);}
      stage[count]=Fold{start_line:first,end_line:line,open_byte:begin,close_byte:pos};
      count+=1;
     }
    }
   }
   cursor=pos;
  }
 }
 if depth!=0{return Err(Error::Conflict);}
 stage[..count].sort_unstable_by_key(|f|f.open_byte);
 *out=stage;Ok(count)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn function_braces_generate_one_real_fold_ignoring_quoted_braces(){
  let source=b"fn a(){\n println!(\"}\");\n}\n";
  let mut out=[NONE;4];assert_eq!(find::<4,8>(source,&mut out),Ok(1));
  assert_eq!(out[0].start_line,1);assert_eq!(out[0].end_line,3);
 }
 #[test]fn malformed_braces_and_short_capacity_fail_closed(){
  assert_eq!(find::<0,2>(b"{\n}\n",&mut[]),Err(Error::Full));
  assert_eq!(find::<1,2>(b"}",&mut[NONE;1]),Err(Error::Conflict));
 }
}