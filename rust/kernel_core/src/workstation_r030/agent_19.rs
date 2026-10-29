//! R030/19: spatial keyboard focus traversal for tiled workstation windows.
use super::Error;
use crate::workstation_r029::agent_18::Rect;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Window{pub id:u8,pub rect:Rect}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Direction{Left,Right,Up,Down}
fn center(r:Rect)->(i64,i64){
 (r.x as i64*2+r.w as i64,r.y as i64*2+r.h as i64)
}
pub fn next(windows:&[Window],active:u8,dir:Direction)->Result<u8,Error>{
 let source=windows.iter().find(|w|w.id==active).ok_or(Error::Missing)?;
 if source.rect.w==0||source.rect.h==0{return Err(Error::Invalid);}
 let (x,y)=center(source.rect);let mut best=None;
 for candidate in windows{
  if candidate.id==active||candidate.rect.w==0||candidate.rect.h==0{continue;}
  let (cx,cy)=center(candidate.rect);let dx=cx-x;let dy=cy-y;
  let valid=match dir{Direction::Left=>dx<0,Direction::Right=>dx>0,
   Direction::Up=>dy<0,Direction::Down=>dy>0};
  if !valid{continue;}
  let primary=if matches!(dir,Direction::Left|Direction::Right){dx.abs()}else{dy.abs()};
  let cross=if matches!(dir,Direction::Left|Direction::Right){dy.abs()}else{dx.abs()};
  let score=primary.saturating_mul(2).saturating_add(cross);
  if best.is_none_or(|(s,id)|score<s||score==s&&candidate.id<id){
   best=Some((score,candidate.id));
  }
 }
 best.map(|(_,id)|id).ok_or(Error::Missing)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn arrow_navigation_selects_adjacent_real_window_geometry(){
  let w=[Window{id:1,rect:Rect{x:0,y:0,w:300,h:300}},
   Window{id:2,rect:Rect{x:300,y:0,w:300,h:300}}];
  assert_eq!(next(&w,1,Direction::Right),Ok(2));
  assert_eq!(next(&w,2,Direction::Left),Ok(1));
 }
 #[test]fn missing_neighbor_and_unknown_current_window_are_safe(){
  let w=[Window{id:1,rect:Rect{x:0,y:0,w:300,h:300}}];
  assert_eq!(next(&w,1,Direction::Right),Err(Error::Missing));
  assert_eq!(next(&w,3,Direction::Right),Err(Error::Missing));
 }
}