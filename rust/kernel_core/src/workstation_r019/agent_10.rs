//! R019/10: compact line-oriented source-diff preview.
//! Hunk output never copies untrusted source text into privileged memory.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind{Unchanged,Removed,Added}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Delta{pub kind:Kind,pub old_line:u32,pub new_line:u32}
fn boundaries<const N:usize>(data:&[u8],out:&mut[(usize,usize);N])
 ->Result<usize,Error>{
 let mut count=0;let mut start=0;
 for (i,&b) in data.iter().enumerate(){
  if b==b'\n'{
   if count>=N{return Err(Error::Full);}
   out[count]=(start,i);count+=1;start=i+1;
  }
 }
 if start<data.len(){
  if count>=N{return Err(Error::Full);}
  out[count]=(start,data.len());count+=1;
 }Ok(count)
}
pub fn compare<const L:usize,const O:usize>(old:&[u8],new:&[u8],
 out:&mut[Delta;O])->Result<usize,Error>{
 let mut a=[(0,0);L];let mut b=[(0,0);L];
 let an=boundaries(old,&mut a)?;let bn=boundaries(new,&mut b)?;
 let mut stage=[Delta{kind:Kind::Unchanged,old_line:0,new_line:0};O];
 let mut x=0;let mut y=0;let mut n=0;
 // Bounded lookahead keeps moved unrelated lines from becoming false matches.
 while x<an||y<bn{
  if n>=O{return Err(Error::Full);}
  if x<an&&y<bn&&old[a[x].0..a[x].1]==new[b[y].0..b[y].1]{
   stage[n]=Delta{kind:Kind::Unchanged,old_line:(x+1) as u32,
    new_line:(y+1) as u32};x+=1;y+=1;
  }else if x<an&&(y==bn||x+1<an&&y<bn&&
    old[a[x+1].0..a[x+1].1]==new[b[y].0..b[y].1]){
   stage[n]=Delta{kind:Kind::Removed,old_line:(x+1) as u32,new_line:0};x+=1;
  }else if y<bn{
   stage[n]=Delta{kind:Kind::Added,old_line:0,new_line:(y+1) as u32};y+=1;
  }else{
   stage[n]=Delta{kind:Kind::Removed,old_line:(x+1) as u32,new_line:0};x+=1;
  }
  n+=1;
 }
 *out=stage;Ok(n)
}
pub fn change_count(deltas:&[Delta])->usize{
 deltas.iter().filter(|d|d.kind!=Kind::Unchanged).count()
}
#[cfg(test)]mod tests{
 use super::*;
 const EMPTY:Delta=Delta{kind:Kind::Unchanged,old_line:0,new_line:0};
 #[test]fn identifies_added_line_in_source_preview(){
  let mut d=[EMPTY;8];
  let n=compare::<8,8>(b"one\nthree\n",b"one\ntwo\nthree\n",&mut d).unwrap();
  assert_eq!(n,3);
  assert_eq!(d[1].kind,Kind::Added);assert_eq!(d[2].kind,Kind::Unchanged);
 }
 #[test]fn unchanged_content_has_no_changed_lines(){
  let mut d=[EMPTY;4];
  let n=compare::<4,4>(b"a\nb\n",b"a\nb\n",&mut d).unwrap();
  assert_eq!(change_count(&d[..n]),0);
 }
 #[test]fn insufficient_hunk_capacity_never_publishes_first_hunk(){
  let sentinel=Delta{kind:Kind::Added,old_line:99,new_line:99};
  let mut d=[sentinel;1];
  assert_eq!(compare::<4,1>(b"a\nb\n",b"x\ny\n",&mut d),Err(Error::Full));
  assert_eq!(d,[sentinel]);
 }
}
