//! Ring-3 command retention and private pixel-row rasterization.
//! The same pure implementation is used by the app and host integration tests.
use crate::gui_protocol::{Command,COMMAND_LIMIT,FILL,TEXT,REPLACE_TEXT};
#[path="../../../rust/kernel_core/src/font5x7.rs"]
mod font5x7;
pub const MAX_EXTENT:u16=crate::pixel_protocol::MAX_WIDTH as u16;
pub const ROW_PIXELS:usize=crate::pixel_protocol::TILE_PIXELS as usize;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Dirty{pub x:u16,pub y:u16,pub width:u16,pub height:u16}
#[derive(Clone,Copy)]
pub struct State{window:u32,commands:[Command;COMMAND_LIMIT],count:usize}
impl State{
 pub const fn new(window:u32)->Self{Self{window,commands:[Command::EMPTY;COMMAND_LIMIT],count:0}}
 pub fn from_commands(window:u32,commands:&[Command])->Option<Self>{
  if !matches!(window,1|2)||commands.len()>COMMAND_LIMIT{return None;}
  let mut state=Self::new(window);
  for &command in commands{
   if !valid(command,window)||!matches!(command.operation,FILL|TEXT){return None;}
   state.commands[state.count]=command;state.count+=1;
  }
  Some(state)
 }
 /// Validate the next retained command before changing the private model.
 /// A replacement changes only its existing field, including old glyph pixels.
 pub fn prepare(&mut self,wire:&[u8;64])->Option<Dirty>{
  let mut command=Command::parse(wire).ok()?;
  if !valid(command,self.window){return None;}
  let index=if command.operation==REPLACE_TEXT{
   let index=self.commands[..self.count].iter().position(|old|
    old.operation==TEXT&&old.x==command.x&&old.y==command.y&&
    old.width==command.width&&old.height==command.height)?;
   command.operation=TEXT;index
  }else{if self.count==COMMAND_LIMIT{return None;}self.count};
  self.commands[index]=command;
  if index==self.count{self.count+=1;}
  Some(Dirty{x:command.x,y:command.y,width:command.width,height:command.height})
 }
 /// Emit canonical RGBA: zero means uncovered, ffRRGGBB means an app pixel.
 /// Replaying all retained commands in the dirty rectangle restores underlying
 /// fills after shorter text, instead of leaving old glyphs on the surface.
 pub fn row(&self,x:u16,y:u16,out:&mut[u32])->bool{
  if out.is_empty()||out.len()>ROW_PIXELS||y as u32>=crate::pixel_protocol::MAX_HEIGHT||
     x as usize+out.len()>MAX_EXTENT as usize{return false;}
  out.fill(0);
  for command in &self.commands[..self.count]{
   let cy=y as u32;
   if cy<command.y as u32||cy>=command.y as u32+command.height as u32{continue;}
   for (index,pixel) in out.iter_mut().enumerate(){
    let cx=x as u32+index as u32;
    if cx<command.x as u32||cx>=command.x as u32+command.width as u32{continue;}
    if command.operation==FILL{*pixel=0xff00_0000|command.rgb;continue;}
    let tx=(cx-command.x as u32) as usize;let ty=(cy-command.y as u32) as usize;
    let character=tx/12;
    if character>=command.text_len as usize||tx%12>=10||ty>=14{continue;}
    let bits=font5x7::glyph(command.text[character])[ty/2];
    if bits&(1<<(4-(tx%12)/2))!=0{*pixel=0xff00_0000|command.rgb;}
   }
  }
  true
 }
}
fn valid(command:Command,window:u32)->bool{
 matches!(window,1|2)&&command.window==window&&command.width!=0&&command.height!=0&&
 command.within_client(crate::pixel_protocol::MAX_WIDTH,crate::pixel_protocol::MAX_HEIGHT)&&command.rgb<=0xffffff&&
 matches!(command.operation,FILL|TEXT|REPLACE_TEXT)&&
 (command.operation==FILL||((1..=24).contains(&command.text_len)&&
    command.text[..command.text_len as usize].iter().all(|b|(32..=126).contains(b))&&
    command.text_len as u32*12<=command.width as u32&&command.height>=14))
}
