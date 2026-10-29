//! R030/10: bounded wildcard project-path selector for file explorer.
use super::Error;
pub fn matches(pattern:&[u8],path:&[u8])->Result<bool,Error>{
 if pattern.is_empty()||pattern.len()>96||path.len()>96||pattern.contains(&0)||
  path.contains(&0){return Err(Error::Invalid);}
 // Iterative '*' backtracking, no recursion, heap, or regex execution.
 let(mut pi,mut si,mut star,mut checkpoint)=(0usize,0usize,None,0usize);
 while si<path.len(){
  if pi<pattern.len()&&(pattern[pi]==path[si]||
   pattern[pi]==b'?'&&path[si]!=b'/'){
   pi+=1;si+=1;
  }else if pi<pattern.len()&&pattern[pi]==b'*'{
   star=Some(pi);pi+=1;checkpoint=si;
  }else if let Some(s)=star{
   if path.get(checkpoint)==Some(&b'/'){return Ok(false);}
   checkpoint+=1;si=checkpoint;pi=s+1;
  }else{return Ok(false);}
 }
 while pi<pattern.len()&&pattern[pi]==b'*'{pi+=1;}
 Ok(pi==pattern.len())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn glob_matches_rust_files_only_in_selected_directory(){
  assert_eq!(matches(b"/p/*.rs",b"/p/main.rs"),Ok(true));
  assert_eq!(matches(b"/p/*.rs",b"/p/sub/main.rs"),Ok(false));
 }
 #[test]fn question_mark_cannot_cross_directories_and_nul_rejected(){
  assert_eq!(matches(b"/p/?.rs",b"/p/a.rs"),Ok(true));
  assert_eq!(matches(b"/p/?.rs",b"/p//.rs"),Ok(false));
  assert_eq!(matches(b"/p/\0",b"/p/a"),Err(Error::Invalid));
 }
}