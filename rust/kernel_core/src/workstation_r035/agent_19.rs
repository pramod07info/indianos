//! R035/19 — Desktop four-region graphical IDE layout model.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub struct Rect{pub x:u16,pub y:u16,pub w:u16,pub h:u16}pub fn layout(width:u16,height:u16)->Result<[Rect;4],Error>{if width<800||height<480{return Err(Error::Full);}let nav=width/5;let diagnostics=width/4;let main=width-nav-diagnostics;let editor=height*3/4;Ok([Rect{x:0,y:0,w:nav,h:height},Rect{x:nav,y:0,w:main,h:editor},Rect{x:nav,y:editor,w:main,h:height-editor},Rect{x:nav+main,y:0,w:diagnostics,h:height}])}
#[cfg(test)]mod tests{use super::*;#[test]fn real_workflow_and_failure_guard(){let p=layout(1200,800).unwrap();assert_eq!(p.iter().map(|v|v.w as u32*v.h as u32).sum::<u32>(),960000);assert_eq!(layout(100,100),Err(Error::Full));}}
