//! R029 QEMU-only software framebuffer desktop compositor.
//! Two independent window surfaces are drawn into Limine's mapped 32-bpp
//! framebuffer; keyboard actions and physical PS/2 mouse packets change the
//! actual pixels, z-order, position and visibility, not just a Rust model.
//! Single BSP; no direct framebuffer access from IRQs, no arbitrary DMA,
//! no general display-driver/GPU ABI or user-space isolation yet.
use core::ptr;
use crate::desktop::{App,Desktop,Rect};
use crate::{boot,console,input,ps2_mouse::{Mouse,Packet}};

const WIDTH_MIN:usize=640;
const HEIGHT_MIN:usize=480;
const LIMIT:usize=1600;
const GUI_ITERATIONS:u64=300_000_000;

#[cfg(not(test))]
fn log(message:&'static [u8]) {
    unsafe extern "C" {fn serial_write(message:*const core::ffi::c_char);}
    unsafe{serial_write(message.as_ptr().cast());}
}
#[cfg(test)]
fn log(_message:&'static [u8]) {
    // The host unit-test binary does not link the QEMU UART personality.
    // Only the real kernel uses the serial_write boot symbol.
}
#[derive(Clone,Copy)]
struct Palette{
    bg:u32,top:u32,bar:u32,title:u32,unfocused:u32,
    light:u32,dark:u32,panel:u32,white:u32,accent:u32,
}
impl Palette{
    fn new()->Option<Self>{
        let p=|r,g,b|boot::desktop_rgb(r,g,b);
        if cfg!(feature="orbit-ui-preview"){
            return Some(Self{
                bg:p(8,23,41)?,top:p(8,18,32)?,bar:p(18,40,61)?,
                title:p(19,86,118)?,unfocused:p(31,50,71)?,
                light:p(20,35,54)?,dark:p(9,17,30)?,
                panel:p(41,70,90)?,white:p(235,245,251)?,
                accent:p(248,185,70)?
            });
        }
        Some(Self{
            bg:p(16,28,55)?,top:p(12,20,43)?,
            bar:p(22,41,75)?,title:p(15,110,136)?,
            unfocused:p(75,88,117)?,light:p(236,245,248)?,
            dark:p(31,50,73)?,panel:p(205,223,231)?,
            white:p(255,255,255)?,accent:p(247,159,60)?
        })
    }
}
struct Surface {
    ptr:*mut u8,width:usize,height:usize,pitch:usize,palette:Palette,
    rgb:fn(u8,u8,u8)->Option<u32>
}
impl Surface{
    fn from_boot()->Option<Self>{
        let f=boot::console_framebuffer()?;
        if f.width<WIDTH_MIN || f.height<HEIGHT_MIN ||
           f.width>LIMIT || f.height>LIMIT ||
           f.pitch<f.width.checked_mul(4)? || f.pitch%4!=0{
            return None;
        }
        Some(Self{ptr:f.address,width:f.width,height:f.height,
            pitch:f.pitch,palette:Palette::new()?,rgb:boot::desktop_rgb})
    }
    fn fill(&mut self,x:i32,y:i32,width:u32,height:u32,color:u32){
        if width==0||height==0{return;}
        let x0=x.max(0) as usize;
        let y0=y.max(0) as usize;
        let right=((x as i64)+(width as i64)).max(0).min(self.width as i64) as usize;
        let bottom=((y as i64)+(height as i64)).max(0).min(self.height as i64) as usize;
        for cy in y0..bottom{
            for cx in x0..right{
                // Boot has validated this exact framebuffer pitch, size,
                // canonical address and RGB layout; clip BEFORE pointer math.
                let offset=cy*self.pitch+cx*4;
                unsafe{ptr::write_volatile(self.ptr.add(offset).cast::<u32>(),color);}
            }
        }
    }
    fn glyph(&mut self,x:i32,y:i32,character:u8,color:u32){
        let rows=console::glyph(character);
        for (cy,&bits) in rows.iter().enumerate(){
            for bit in 0..5{
                if bits&(1<<(4-bit))!=0{
                    self.fill(x+(bit*2) as i32,y+(cy*2) as i32,2,2,color);
                }
            }
        }
    }
    fn text(&mut self,x:i32,y:i32,bytes:&[u8],color:u32){
        for (i,&ch) in bytes.iter().take(36).enumerate(){
            self.glyph(x+(i as i32)*12,y,ch,color);
        }
    }
    #[cfg(feature="orbit-ui-preview")]
    fn orbit_palette(&self)->crate::orbit_ui::Colors{
        let p=self.palette;
        crate::orbit_ui::Colors{bg:p.bg,top:p.top,bar:p.bar,title:p.title,
          unfocused:p.unfocused,light:p.light,dark:p.dark,panel:p.panel,
          white:p.white,accent:p.accent}
    }
    #[cfg(feature="orbit-ui-preview")]
    fn orbit_shell(&mut self,selection:u8){
        use crate::orbit_ui::{shell,Workspace};
        let workspace=match selection{
            1=>Workspace::Create,2=>Workspace::Learn,
            3=>Workspace::Personal,_=>Workspace::Build
        };
        let colors=self.orbit_palette();
        let width=self.width as u32;let height=self.height as u32;
        shell(self,width,height,workspace,colors);
    }
    fn draw_window(&mut self,w:crate::desktop::Window,focused:bool){
        if !w.visible||w.minimized{return;}
        #[cfg(feature="orbit-ui-preview")]
        {
            let colors=self.orbit_palette();
            crate::orbit_ui::window(self,w,focused,colors);
            return;
        }

        let b=w.bounds;let x=b.x;let y=b.y;
        let p=self.palette;
        self.fill(x-2,y-2,b.width+4,b.height+4,p.dark);
        self.fill(x,y,b.width,b.height,p.light);
        self.fill(x,y,b.width,35,if focused{p.title}else{p.unfocused});
        self.fill(x+(b.width as i32)-30,y+8,18,18,p.accent);
        let heading=match w.app{
            App::Files=>b"FILES".as_slice(),
            App::Terminal=>b"TERMINAL".as_slice(),
            _=>b"APP".as_slice()
        };
        self.text(x+18,y+11,heading,p.white);
        match w.app{
            App::Files=>{
                self.fill(x+10,y+48,b.width-20,32,p.panel);
                self.text(x+18,y+57,b"HOME / FILES",p.dark);
                for row in 0..3{
                    let y=y+94+row*46;
                    self.fill(x+23,y,26,23,p.accent);
                    let label=match row{
                        0=>b"DOCUMENTS".as_slice(),
                        1=>b"DOWNLOADS".as_slice(),
                        _=>b"INDIANOS".as_slice(),
                    };
                    self.text(x+66,y+5,label,p.dark);
                }
            }
            App::Terminal=>{
                self.fill(x+9,y+44,b.width-18,b.height-54,p.dark);
                self.text(x+25,y+64,b"INDIANOS TERMINAL",p.accent);
                self.text(x+25,y+92,b"$ HELP",p.white);
                self.text(x+25,y+120,b"READY FOR COMMANDS",p.white);
                self.text(x+25,y+150,b"$ _",p.accent);
            }
            _=>{}
        }
    }
    fn render(&mut self,desktop:&Desktop,mouse_x:i32,mouse_y:i32,orbit:u8){
        let p=self.palette;
        #[cfg(feature="orbit-ui-preview")]
        self.orbit_shell(orbit);
        #[cfg(not(feature="orbit-ui-preview"))]
        {
            let _=orbit;
            self.fill(0,0,self.width as u32,self.height as u32,p.bg);
            self.fill(0,0,self.width as u32,48,p.top);
            self.text(20,18,b"INDIANOS DESKTOP",p.white);
            self.fill(0,(self.height-42) as i32,self.width as u32,42,p.bar);
            self.fill(20,(self.height-34) as i32,90,25,p.title);
            self.text(30,(self.height-29) as i32,b"APPS",p.white);
        }
        let focused=desktop.focused();
        for w in desktop.windows(){self.draw_window(*w,focused==Some(w.id));}
        // Crosshair mouse cursor. All positions are clipped by fill().
        self.fill(mouse_x-1,mouse_y-10,3,21,p.accent);
        self.fill(mouse_x-10,mouse_y-1,21,3,p.accent);
        self.fill(mouse_x-1,mouse_y-1,3,3,p.white);
    }
}
// R029 second acceptance: independently mapped ring-3 /bin/init owns surface
// *commands*, never the framebuffer mapping. The kernel alone clips, paints,
// validates privilege, and keeps a replayable eight-command backing store.
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
use crate::gui_protocol::{Command as UserCommand,FILL,TEXT,REPLACE_TEXT,COMMAND_LIMIT};
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
use crate::gui_broker::{Broker,Event as GuiEvent};
// The canonical retained streams are kernel-private, not pixel pointers or
// app-selected addresses. Normal Desktop gives each stream a whole payload
// page bracketed by unmapped pages; neither snapshots nor reset may touch them.
#[cfg(feature="normal-desktop")]
const COMMAND_PAGE:usize=4096;
#[cfg(feature="normal-desktop")]
#[repr(C,align(4096))]
struct GuardedCommands{
    lower:[u8;COMMAND_PAGE],
    entries:[UserCommand;COMMAND_LIMIT],
    padding:[u8;COMMAND_PAGE-core::mem::size_of::<UserCommand>()*COMMAND_LIMIT],
    upper:[u8;COMMAND_PAGE],
}
#[cfg(feature="normal-desktop")]
impl GuardedCommands{
    const fn new()->Self{Self{lower:[0;COMMAND_PAGE],
        entries:[UserCommand::EMPTY;COMMAND_LIMIT],
        padding:[0;COMMAND_PAGE-core::mem::size_of::<UserCommand>()*COMMAND_LIMIT],
        upper:[0;COMMAND_PAGE]}}
    fn layout(&self)->(u64,u64,u64,u64){
        let lower=core::ptr::addr_of!(self.lower) as usize as u64;
        let begin=core::ptr::addr_of!(self.entries) as usize as u64;
        let upper=core::ptr::addr_of!(self.upper) as usize as u64;
        (lower,begin,begin+COMMAND_PAGE as u64,upper)
    }
}
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
struct CommandStore{
    #[cfg(feature="normal-desktop")]
    surfaces:[GuardedCommands;2],
    #[cfg(not(feature="normal-desktop"))]
    entries:[[UserCommand;COMMAND_LIMIT];2],
}
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
impl CommandStore{
    const fn new()->Self{Self{
        #[cfg(feature="normal-desktop")]
        surfaces:[GuardedCommands::new(),GuardedCommands::new()],
        #[cfg(not(feature="normal-desktop"))]
        entries:[[UserCommand::EMPTY;COMMAND_LIMIT];2],
    }}
    fn clear_slot(&mut self,slot:usize){
        self[slot].fill(UserCommand::EMPTY);
        #[cfg(feature="normal-desktop")]
        self.surfaces[slot].padding.fill(0);
    }
    fn clear(&mut self){for slot in 0..2{self.clear_slot(slot);}}
    #[cfg(test)]
    fn snapshot(&self)->[[UserCommand;COMMAND_LIMIT];2]{[self[0],self[1]]}
    #[cfg(feature="normal-desktop")]
    fn layout(&self)->[(u64,u64,u64,u64);2]{
        [self.surfaces[0].layout(),self.surfaces[1].layout()]
    }
}
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
impl core::ops::Index<usize> for CommandStore{
    type Output=[UserCommand;COMMAND_LIMIT];
    fn index(&self,slot:usize)->&Self::Output{
        #[cfg(feature="normal-desktop")]
        {&self.surfaces[slot].entries}
        #[cfg(not(feature="normal-desktop"))]
        {&self.entries[slot]}
    }
}
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
impl core::ops::IndexMut<usize> for CommandStore{
    fn index_mut(&mut self,slot:usize)->&mut Self::Output{
        #[cfg(feature="normal-desktop")]
        {&mut self.surfaces[slot].entries}
        #[cfg(not(feature="normal-desktop"))]
        {&mut self.entries[slot]}
    }
}
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
struct UserSurfaceService{
    session:Session,
    commands:CommandStore,
    counts:[usize;2],
    focused:u32,
    broker:Broker<2,8>,
    #[cfg(any(feature="test-r029-frame-server",feature="normal-desktop"))]
    frames_presented:u32,
    #[cfg(feature="normal-desktop")]
    retired_streams:u8,
    #[cfg(any(feature="test-r029-desktop-suite",feature="normal-desktop"))]
    launcher_visible:bool,
    #[cfg(any(feature="test-r029-desktop-suite",feature="normal-desktop"))]
    desktop_actions:u32,
    #[cfg(feature="test-r031-supervisor")]
    restart_count:u32,
    #[cfg(any(feature="test-r029-physical-events",feature="normal-desktop"))]
    mouse:Mouse,
}
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
static mut USER_SURFACE_SERVICE:Option<UserSurfaceService>=None;
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
impl UserSurfaceService{
    #[cfg(feature="normal-desktop")]
    fn retirement_verified(&self)->bool{
        self.frames_presented>0&&self.retired_streams==3&&self.counts==[0,0]&&
        (1..=4).all(|pid|!self.broker.known(pid))
    }
    #[cfg(feature="normal-desktop")]
    fn window_frame(&self,pid:u32)->Option<crate::gui_protocol::DesktopFrame>{
        use crate::gui_protocol::{DesktopFrame,WindowFrame};
        if pid!=4{return None;}
        let mut windows=[WindowFrame{x:0,y:0,width:0,height:0,minimized:false};2];
        for (index,slot) in windows.iter_mut().enumerate(){
            let w=self.window(index as u32+1)?;
            *slot=WindowFrame{x:w.bounds.x as u16,y:w.bounds.y as u16,
                width:w.bounds.width as u16,height:w.bounds.height as u16,
                minimized:w.minimized};
        }
        Some(DesktopFrame{width:self.session.desktop.width as u16,
            height:self.session.desktop.height as u16,
            front:self.session.desktop.focused()? as u8,launcher:self.launcher_visible,
            windows,pointer_x:self.session.mouse_x as u16,pointer_y:self.session.mouse_y as u16})
    }
    #[cfg(feature="normal-desktop")]
    fn apply_window_frame(&mut self,pid:u32,bytes:&[u8;64],
        paint:impl FnOnce(&Self)->bool)->bool{
        if pid!=4||!self.broker.owns(2,1)||!self.broker.owns(3,2){return false;}
        let Some(state)=crate::gui_protocol::DesktopFrame::parse(bytes)else{return false;};
        if state.width as u32!=self.session.desktop.width||state.height as u32!=self.session.desktop.height{
            return false;
        }
        let Ok(mut layout)=Desktop::new(state.width as u32,state.height as u32)else{return false;};
        for (index,window) in state.windows.iter().enumerate(){
            let Some(old)=self.window(index as u32+1)else{return false;};
            // Retained app streams remain attached to immutable client extents.
            if window.width as u32!=old.bounds.width||window.height as u32!=old.bounds.height{
                return false;
            }
            if layout.open(old.app,Rect::new(window.x as i32,window.y as i32,
                window.width as u32,window.height as u32)).ok()!=Some(index as u32+1){return false;}
        }
        for (index,window) in state.windows.iter().enumerate(){
            if window.minimized&&layout.minimize(index as u32+1).is_err(){return false;}
        }
        if layout.focus(state.front as u32).is_err(){return false;}
        let old=core::mem::replace(&mut self.session.desktop,layout);
        let launcher=self.launcher_visible;
        let pointer=(self.session.mouse_x,self.session.mouse_y);
        self.launcher_visible=state.launcher;
        self.session.mouse_x=state.pointer_x as i32;self.session.mouse_y=state.pointer_y as i32;
        if !paint(self){
            self.session.desktop=old;self.launcher_visible=launcher;
            (self.session.mouse_x,self.session.mouse_y)=pointer;return false;
        }
        self.session.active=state.front as u32;
        self.frames_presented=self.frames_presented.saturating_add(1);
        true
    }
    fn new(w:u32,h:u32)->Option<Self>{
        let session=Session::new(w,h)?;
        if session.files!=1||session.terminal!=2{return None;}
        let mut broker=Broker::new();
        #[cfg(any(feature="test-r029-desktop-suite",feature="normal-desktop"))]
        {
            broker.grant(2,session.files).ok()?;
            broker.grant(3,session.terminal).ok()?;
        }
        #[cfg(not(any(feature="test-r029-desktop-suite",feature="normal-desktop")))]
        {
            broker.grant(1,session.files).ok()?;
            #[cfg(any(feature="test-r029-multiprocess",feature="normal-desktop"))]
            broker.grant(2,session.terminal).ok()?;
            #[cfg(not(any(feature="test-r029-multiprocess",feature="normal-desktop")))]
            broker.grant(1,session.terminal).ok()?;
        }
        #[cfg(any(feature="test-r029-physical-events",feature="normal-desktop"))]
        let mouse=Mouse::init()?;
        Some(Self{session,
            commands:CommandStore::new(),
            counts:[0,0],focused:0,broker,
            #[cfg(any(feature="test-r029-frame-server",feature="normal-desktop"))]
            frames_presented:0,
            #[cfg(feature="normal-desktop")]
            retired_streams:0,
            #[cfg(any(feature="test-r029-desktop-suite",feature="normal-desktop"))]
            launcher_visible:false,
            #[cfg(any(feature="test-r029-desktop-suite",feature="normal-desktop"))]
            desktop_actions:0,
            #[cfg(feature="test-r031-supervisor")]
            restart_count:0,
            #[cfg(any(feature="test-r029-physical-events",feature="normal-desktop"))]
            mouse})
    }
    #[cfg(feature="normal-desktop")]
    fn reset_in_place(&mut self,next:Self){
        // The existing guard pages are already unmapped. Move live metadata
        // field-by-field and clear only the payload, never copy/drop guards.
        self.commands.clear();
        self.session=next.session;self.counts=next.counts;
        self.focused=next.focused;self.broker=next.broker;
        self.frames_presented=next.frames_presented;
        self.retired_streams=0;
        self.launcher_visible=next.launcher_visible;
        self.desktop_actions=next.desktop_actions;
        self.mouse=next.mouse;
    }
    fn window(&self,id:u32)->Option<crate::desktop::Window>{
        self.session.desktop.windows().find(|w|w.id==id).copied()
    }
    fn client_bounds(&self,id:u32)->Option<(i32,i32,u32,u32)>{
        let w=self.window(id)?;
        let inner_w=w.bounds.width.checked_sub(18)?;
        let inner_h=w.bounds.height.checked_sub(54)?;
        Some((w.bounds.x+9,w.bounds.y+44,inner_w,inner_h))
    }
    fn redraw(&self)->bool{
        let Some(mut out)=Surface::from_boot()else{return false;};
        self.paint(&mut out)
    }
    // The boot framebuffer and memory-backed tests share this exact raster path.
    fn paint(&self,out:&mut Surface)->bool{
        let palette=out.palette;
        // Paint desktop chrome, then each window AND its user commands in
        // the same z-order. A rear window can never paint over front chrome.
        #[cfg(feature="orbit-ui-preview")]
        out.orbit_shell(self.session.orbit);
        #[cfg(not(feature="orbit-ui-preview"))]
        {
            out.fill(0,0,out.width as u32,out.height as u32,palette.bg);
            out.fill(0,0,out.width as u32,48,palette.top);
            out.text(20,18,b"INDIANOS DESKTOP",palette.white);
            out.fill(0,(out.height-42) as i32,out.width as u32,42,palette.bar);
            out.fill(20,(out.height-34) as i32,90,25,palette.title);
            out.text(30,(out.height-29) as i32,b"APPS",palette.white);
        }
        for window in self.session.desktop.windows(){
            // Chrome and retained client content have one visibility contract.
            // Keep backing commands intact so an explicit focus can restore them.
            if !window.visible||window.minimized{continue;}
            out.draw_window(*window,self.session.desktop.focused()==Some(window.id));
            let Some((origin_x,origin_y,_,_))=self.client_bounds(window.id)
                else{return false;};
            let slot=window.id as usize-1;
            // A revoked client must never replay old raster commands,
            // even when its window remains visible until teardown.
            if self.broker.owner(window.id).is_none(){continue;}
            for command in &self.commands[slot][..self.counts[slot]]{
                let rgb=command.rgb;
                let Some(color)=(out.rgb)(
                    ((rgb>>16)&0xff) as u8,((rgb>>8)&0xff) as u8,(rgb&0xff) as u8
                )else{return false;};
                let x=origin_x+command.x as i32;
                let y=origin_y+command.y as i32;
                match command.operation{
                    FILL=>out.fill(x,y,command.width as u32,command.height as u32,color),
                    TEXT=>out.text(x,y,&command.text[..command.text_len as usize],color),
                    _=>return false,
                }
            }
        }
        #[cfg(any(feature="test-r029-desktop-suite",feature="normal-desktop"))]
        if self.launcher_visible{
            out.fill(20,(out.height-225) as i32,340,183,palette.top);
            out.fill(26,(out.height-215) as i32,328,30,palette.title);
            out.text(36,(out.height-207) as i32,b"APPLICATIONS",palette.white);
            out.text(42,(out.height-168) as i32,b"1  FILE MANAGER",palette.white);
            out.text(42,(out.height-132) as i32,b"2  TERMINAL",palette.white);
            out.text(42,(out.height-95) as i32,b"FOCUS  MOVE  MINIMIZE",palette.white);
        }
        out.fill(self.session.mouse_x-1,self.session.mouse_y-10,3,21,palette.accent);
        out.fill(self.session.mouse_x-10,self.session.mouse_y-1,21,3,palette.accent);
        out.fill(self.session.mouse_x-1,self.session.mouse_y-1,3,3,palette.white);
        true
    }
    fn submit(&mut self,pid:u32,bytes:&[u8;64])->bool{
        // Frame-server clients stage commands; authenticated PID1 presents.
        #[cfg(any(feature="test-r029-frame-server",feature="normal-desktop"))]
        {self.submit_with(pid,bytes,|_|true)}
        #[cfg(not(any(feature="test-r029-frame-server",feature="normal-desktop")))]
        {self.submit_with(pid,bytes,Self::redraw)}
    }
    fn submit_with(&mut self,pid:u32,bytes:&[u8;64],paint:impl FnOnce(&Self)->bool)->bool{
        let Ok(mut cmd)=UserCommand::parse(bytes)else{return false;};
        if !self.broker.owns(pid,cmd.window){return false;}
        let slot=cmd.window as usize-1;
        let Some((_,_,width,height))=self.client_bounds(cmd.window)else{return false;};
        if !cmd.within_client(width,height){return false;}
        let before=self.counts[slot];
        let index=if cmd.operation==REPLACE_TEXT{
            let Some(index)=self.commands[slot][..before].iter().position(|old|
                old.operation==TEXT&&old.x==cmd.x&&old.y==cmd.y&&
                old.width==cmd.width&&old.height==cmd.height)else{return false;};
            cmd.operation=TEXT;index
        }else{
            if before>=COMMAND_LIMIT{return false;}
            self.counts[slot]+=1;before
        };
        let previous=self.commands[slot][index];
        self.commands[slot][index]=cmd;
        if paint(self){true}else{
            self.commands[slot][index]=previous;self.counts[slot]=before;false
        }
    }
    fn focus(&mut self,pid:u32,id:u32)->bool{
        if !self.broker.can_post(pid,id){return false;}
        if !self.session.focus(id){return false;}
        #[cfg(not(any(feature="test-r029-frame-server",feature="normal-desktop")))]
        if !self.redraw(){return false;}
        if self.broker.post_focus(pid,id).is_err(){return false;}
        self.focused+=1;true
    }
    #[cfg(any(feature="test-r029-physical-events",feature="normal-desktop"))]
    fn pump_mouse(&mut self){
        // Read physical emulated 8042 packets only in a serialized ring-0
        // syscall; never trust a client-supplied event owner.
        for _ in 0..4 {
            let Some(packet)=self.mouse.poll()else{break;};
            let click=packet.left&&!self.session.was_down;
            let hit=if click {
                let x=(self.session.mouse_x+packet.dx as i32)
                    .clamp(0,self.session.desktop.width as i32-1);
                let y=(self.session.mouse_y-packet.dy as i32)
                    .clamp(0,self.session.desktop.height as i32-1);
                self.session.desktop.hit_test(x,y)
            }else{None};
            let target=hit.and_then(|w|self.broker.owner(w).map(|p|(p,w)));
            if let Some((pid,window))=target{
                if !self.broker.can_post(pid,window){continue;}
            }
            let changed=self.session.on_mouse(packet);
            if let Some((pid,window))=target{
                if self.session.desktop.focused()==Some(window) &&
                    self.broker.post_pointer_focus(pid,window).is_ok(){
                    log(b"IndianOS: R029.2 physical mouse focus event queued\n\0");
                }
            }
            #[cfg(not(any(feature="test-r029-frame-server",feature="normal-desktop")))]
            if changed{let _=self.redraw();}
            #[cfg(any(feature="test-r029-frame-server",feature="normal-desktop"))]
            let _=changed;
        }
    }
    /// Scheduler-owned crash teardown. Pending input and window grants are
    /// revoked as one serialized kernel action. In-progress mouse gesture
    /// state tied to the dead owner must not carry over to a new incarnation.
    fn retire_client(&mut self,pid:u32){
        if pid==0{return;}
        if self.session.dragging.is_some_and(|(window,_,_)|
           self.broker.owns(pid,window)){
            self.session.dragging=None;
            self.session.was_down=false;
        }
        // Scrub the canonical backing before withdrawing its authority.
        // Peers retain their own stream and no stale command can replay.
        #[cfg(feature="normal-desktop")]
        for window in 1..=2{
            if self.broker.owns(pid,window){
                if self.counts[window as usize-1]>0{
                    self.retired_streams|=1<<(window-1);
                }
                self.commands.clear_slot(window as usize-1);
                self.counts[window as usize-1]=0;
            }
        }
        self.broker.revoke(pid);
    }
    #[cfg(feature="test-r031-supervisor")]
    fn rotate_terminal(&mut self,pid:u32)->bool{
        if pid!=4||self.restart_count!=0||
           self.broker.owner(self.session.terminal).is_some()||
           self.broker.known(3){return false;}
        // Process3 was reaped, including its queued input. Old commands
        // must not survive into a new process generation.
        if self.broker.grant(4,self.session.terminal).is_err(){return false;}
        self.commands[1].fill(UserCommand::EMPTY);
        self.counts[1]=0;
        self.restart_count=1;
        true
    }
    #[cfg(any(feature="test-r031-boot-desktop",feature="normal-desktop"))]
    fn pump_desktop(&mut self,pid:u32)->Option<u64>{
        if pid!=1{return None;}
        #[cfg(feature="normal-desktop")]
        {
            // Preserve every button edge. Collapsing several complete
            // packets loses a quick press/release before ring-3 sees it.
            let packet=self.mouse.poll();
            return Some(self.desktop_pointer_packet(packet));
        }
        #[cfg(not(feature="normal-desktop"))]
        if self.frame_state(pid).is_none(){return None;}
        #[cfg(any(feature="test-r029-physical-events",feature="normal-desktop"))]
        {
            let before=(self.session.mouse_x,self.session.mouse_y,
                        self.session.desktop.focused());
            self.pump_mouse();
            let after=(self.session.mouse_x,self.session.mouse_y,
                       self.session.desktop.focused());
            let focus=self.session.desktop.focused()?;
            // Updated state is represented as a changed bit plus focused
            // window. No raw kernel pointers or forged mouse packets.
            return Some((focus as u64)|if before!=after{0x100}else{0});
        }
        #[cfg(not(any(feature="test-r029-physical-events",feature="normal-desktop")))]
        {Some(self.session.desktop.focused()? as u64)}
    }
    #[cfg(feature="normal-desktop")]
    fn desktop_pointer_packet(&mut self,packet:Option<Packet>)->u64{
        let before=(self.session.mouse_x,self.session.mouse_y,self.session.was_down);
        if let Some(packet)=packet{
            self.session.mouse_x=(self.session.mouse_x+packet.dx as i32)
                .clamp(0,self.session.desktop.width as i32-1);
            self.session.mouse_y=(self.session.mouse_y-packet.dy as i32)
                .clamp(0,self.session.desktop.height as i32-1);
            self.session.was_down=packet.left;
        }
        let changed=before!=(self.session.mouse_x,self.session.mouse_y,self.session.was_down);
        self.session.mouse_x as u64|((self.session.mouse_y as u64)<<16)|
            ((self.session.was_down as u64)<<32)|((changed as u64)<<33)
    }
    #[cfg(any(feature="test-r029-desktop-suite",feature="normal-desktop"))]
    fn desktop_action(&mut self,pid:u32,action:u32)->bool{
        if pid!=1||self.frame_state(pid).is_none(){return false;}
        let result=match action{
            1=>self.session.focus(self.session.files),
            2=>self.session.focus(self.session.terminal),
            3=>{
                let id=self.session.files;
                let window=self.session.desktop.windows().find(|w|w.id==id).copied();
                window.is_some_and(|w|
                    self.session.desktop.move_window(id,w.bounds.x+36,w.bounds.y).is_ok())
            },
            4=>self.session.desktop.minimize(self.session.terminal).is_ok(),
            5=>self.session.focus(self.session.terminal),
            6=>{self.launcher_visible=!self.launcher_visible;true},
            _=>false
        };
        if result{self.desktop_actions+=1;}
        result
    }
    #[cfg(any(feature="test-r029-frame-server",feature="normal-desktop"))]
    fn frame_state(&self,pid:u32)->Option<u64>{
        #[cfg(feature="normal-desktop")]
        {let _=pid;return None;}
        // The frame server is an explicit kernel-granted identity. PID2
        // cannot read or present the combined display even though it owns
        // its own drawing stream.
        #[cfg(any(feature="test-r029-desktop-suite",feature="normal-desktop"))]
        if pid!=1{return None;}
        #[cfg(not(any(feature="test-r029-desktop-suite",feature="normal-desktop")))]
        if pid!=1 || !self.broker.owns(pid,self.session.files){return None;}
        Some(self.counts[0] as u64 |
             ((self.counts[1] as u64)<<8) |
             ((self.frames_presented as u64)<<16))
    }
    #[cfg(any(feature="test-r029-frame-server",feature="normal-desktop"))]
    fn present_frame(&mut self,pid:u32,front:u32)->bool{
        self.present_with(pid,front,Self::redraw)
    }
    #[cfg(any(feature="test-r029-frame-server",feature="normal-desktop"))]
    fn present_with(&mut self,pid:u32,front:u32,paint:impl FnOnce(&Self)->bool)->bool{
        let limit=if cfg!(any(feature="test-r031-boot-desktop",feature="normal-desktop")){64}
                  else if cfg!(any(feature="test-r029-desktop-suite",feature="normal-desktop")){5}else{2};
        if self.frame_state(pid).is_none() ||
           !matches!(front,1|2) ||
           !(if cfg!(any(feature="test-r031-files-gui",feature="normal-desktop")){
               // PID2's protected Files client may submit 3 initial
               // entries and 2 refresh operations, but never exceed the
               // same bounded command backing store.
               (5..=7).contains(&self.counts[0])&&self.counts[1]==2
           }else{self.counts==[2,2]}) ||
           self.frames_presented>=limit{return false;}
        // Window stacking order is selected by the ring-3 compositor
        // policy. The privileged kernel remains only a clipped raster
        // backend in this opt-in implementation.
        if !self.session.focus(front){return false;}
        if !paint(self){return false;}
        self.frames_presented+=1;
        log(b"IndianOS: R029.4 user-controlled framebuffer frame presented\n\0");
        true
    }
}
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
fn user_service()->Option<&'static mut UserSurfaceService>{
    // Single BSP, interrupt masked, and no recursive caller permitted.
    unsafe{(&mut *ptr::addr_of_mut!(USER_SURFACE_SERVICE)).as_mut()}
}
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
pub(crate) fn user_surface_start()->bool{
    #[cfg(feature="normal-desktop")]
    input::reset_physical_session();
    let Some(f)=Surface::from_boot()else{return false;};
    let Some(svc)=UserSurfaceService::new(f.width as u32,f.height as u32)else{return false;};
    if !svc.redraw(){return false;}
    #[cfg(feature="normal-desktop")]
    unsafe{
        let service=&mut *ptr::addr_of_mut!(USER_SURFACE_SERVICE);
        if let Some(current)=service.as_mut(){current.reset_in_place(svc);}
        else{*service=Some(svc);}
    }
    #[cfg(not(feature="normal-desktop"))]
    unsafe{USER_SURFACE_SERVICE=Some(svc);}
    log(b"IndianOS: R029 ring3 surface server shows two framebuffer windows\n\0");
    true
}
#[cfg(feature="normal-desktop")]
pub(crate) fn surface_guard_layout()->Option<[(u64,u64,u64,u64);2]>{
    Some(user_service()?.commands.layout())
}
#[cfg(feature="normal-desktop")]
pub(crate) fn desktop_window_query(pid:u32)->Option<[u8;64]>{
    user_service()?.window_frame(pid).map(|frame|frame.wire())
}
#[cfg(feature="normal-desktop")]
pub(crate) fn desktop_window_apply(pid:u32,bytes:&[u8;64])->bool{
    user_service().is_some_and(|svc|svc.apply_window_frame(pid,bytes,UserSurfaceService::redraw))
}
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
pub(crate) fn user_surface_submit(pid:u32,bytes:&[u8;64])->bool{
    user_service().is_some_and(|svc|svc.submit(pid,bytes))
}
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
pub(crate) fn user_surface_focus(pid:u32,id:u32)->bool{
    user_service().is_some_and(|svc|svc.focus(pid,id))
}
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
pub(crate) fn user_surface_status(pid:u32)->Option<u64>{
    let svc=user_service()?;
    if !svc.broker.known(pid){return None;}
    Some((svc.counts[0] as u64) |
         ((svc.counts[1] as u64)<<8) |
         ((svc.focused as u64)<<16) |
         ((svc.session.desktop.focused()? as u64)<<24))
}
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
pub(crate) fn user_surface_peek(pid:u32)->Option<GuiEvent>{
    let svc=user_service()?;
    #[cfg(all(feature="test-r029-physical-events",not(feature="normal-desktop")))]
    svc.pump_mouse();
    svc.broker.peek(pid).ok()
}
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
pub(crate) fn user_surface_terminate(pid:u32){
    if let Some(svc)=user_service(){
        svc.retire_client(pid);
        #[cfg(any(feature="test-r029-physical-events",feature="normal-desktop"))]
        if !svc.broker.known(1) && !svc.broker.known(2)
            && !svc.broker.known(3)
            && !(cfg!(feature="test-r031-supervisor") && svc.broker.known(4)){
            svc.mouse.disable();
        }
    }
}
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
pub(crate) fn user_surface_ack(pid:u32,sequence:u32)->bool{
    user_service().is_some_and(|svc|svc.broker.ack(pid,sequence).is_ok())
}
#[cfg(feature="test-r031-supervisor")]
pub(crate) fn user_surface_restart_terminal(pid:u32)->bool{
    user_service().is_some_and(|svc|svc.rotate_terminal(pid))
}
#[cfg(any(feature="test-r031-boot-desktop",feature="normal-desktop"))]
pub(crate) fn user_desktop_pump(pid:u32)->Option<u64>{
    user_service()?.pump_desktop(pid)
}
#[cfg(any(feature="test-r029-desktop-suite",feature="normal-desktop"))]
pub(crate) fn user_desktop_action(pid:u32,action:u32)->bool{
    user_service().is_some_and(|svc|svc.desktop_action(pid,action))
}
#[cfg(any(feature="test-r029-frame-server",feature="normal-desktop"))]
pub(crate) fn user_frame_state(pid:u32)->Option<u64>{
    user_service()?.frame_state(pid)
}
#[cfg(any(feature="test-r029-frame-server",feature="normal-desktop"))]
pub(crate) fn user_frame_present(pid:u32,front:u32)->bool{
    user_service().is_some_and(|svc|svc.present_frame(pid,front))
}
#[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
pub(crate) fn user_surface_verified()->bool{
    user_service().is_some_and(|svc|{
        #[cfg(feature="normal-desktop")]
        {svc.retirement_verified()}
        #[cfg(not(feature="normal-desktop"))]
        {
        #[cfg(any(feature="test-r031-boot-desktop",feature="normal-desktop"))]
        let frames_ok=svc.frames_presented>=5 && svc.frames_presented<=64 &&
           svc.desktop_actions>=5 && !svc.launcher_visible &&
           !svc.broker.known(3);
        #[cfg(feature="test-r031-supervisor")]
        let frames_ok=frames_ok && svc.restart_count==1 &&
            !svc.broker.known(4);
        #[cfg(all(any(feature="test-r029-desktop-suite",feature="normal-desktop"),not(any(feature="test-r031-boot-desktop",feature="normal-desktop"))))]
        let frames_ok=svc.frames_presented==5 && svc.desktop_actions==6 &&
           !svc.launcher_visible && !svc.broker.known(3);
        #[cfg(all(any(feature="test-r029-frame-server",feature="normal-desktop"),not(any(feature="test-r029-desktop-suite",feature="normal-desktop"))))]
        let frames_ok=svc.frames_presented==2;
        #[cfg(not(any(feature="test-r029-frame-server",feature="normal-desktop")))]
        let frames_ok=svc.focused>=1;
        (if cfg!(any(feature="test-r031-files-gui",feature="normal-desktop")){
            // Initial persistent directory/file view is 5 commands;
            // the single IPC refresh adds 2 more before shutdown.
            matches!(svc.counts,[5,2]|[7,2])
        }else{svc.counts==[2,2]}) && frames_ok &&
        !svc.broker.known(1) && !svc.broker.known(2) &&
        svc.session.desktop.focused()==Some(
            if cfg!(any(feature="test-r029-physical-events",feature="normal-desktop")) {
                svc.session.terminal
            }else{svc.session.files}
        )
        }
    })
}
pub(crate) struct Session{
    desktop:Desktop,files:u32,terminal:u32,active:u32,
    orbit:u8,
    mouse_x:i32,mouse_y:i32,was_down:bool,
    dragging:Option<(u32,i32,i32)>,
    focused_mouse:u32,
    focused_keyboard:u32,
    moves:u32,minimizations:u32,restores:u32,closures:u32,
}
impl Session{
    fn new(w:u32,h:u32)->Option<Self>{
        let mut desktop=Desktop::new(w,h).ok()?;
        #[cfg(feature="orbit-ui-preview")]
        let (files_rect,terminal_rect)=
            crate::orbit_ui::Layout::new(w,h).windows()?;
        #[cfg(not(feature="orbit-ui-preview"))]
        let (files_rect,terminal_rect)={
            let ww=(w*49/100).max(320).min(w-80);
            let wh=(h*50/100).max(240).min(h-110);
            (Rect::new(45,75,ww,wh),
             Rect::new((w-ww-36) as i32,(h-wh-65) as i32,ww,wh))
        };
        let files=desktop.open(App::Files,files_rect).ok()?;
        let terminal=desktop.open(App::Terminal,terminal_rect).ok()?;
        Some(Self{desktop,files,terminal,active:terminal,orbit:0,
            mouse_x:(w/2) as i32,mouse_y:(h/2) as i32,
            was_down:false,dragging:None,focused_mouse:0,focused_keyboard:0,
            moves:0,minimizations:0,restores:0,closures:0})
    }
    fn focus(&mut self,id:u32)->bool{
        if self.desktop.focus(id).is_err(){return false;}
        self.active=id;true
    }
    fn on_mouse(&mut self,p:Packet)->bool{
        let old_x=self.mouse_x;
        let old_y=self.mouse_y;
        self.mouse_x=(old_x+p.dx as i32).clamp(0,self.desktop.width as i32-1);
        self.mouse_y=(old_y-p.dy as i32).clamp(0,self.desktop.height as i32-1);
        let clicked=p.left&&!self.was_down;
        if clicked{
            self.dragging=None;
            if let Some(id)=self.desktop.hit_test(self.mouse_x,self.mouse_y){
                if let Some(w)=self.desktop.windows().find(|w|w.id==id){
                    let title=self.mouse_y<w.bounds.y+35;
                    if title{
                        self.dragging=Some((id,self.mouse_x-w.bounds.x,
                            self.mouse_y-w.bounds.y));
                    }
                }
                if self.focus(id){
                    self.focused_mouse+=1;
                    if id==self.files {
                        log(b"IndianOS: R029 PS2 mouse selected Files window\n\0");
                    }else{
                        log(b"IndianOS: R029 PS2 mouse selected Terminal window\n\0");
                    }
                }
            }
        }
        if !p.left{self.dragging=None;}
        if p.left{
            if let Some((id,offset_x,offset_y))=self.dragging{
                let moving_window=self.desktop.windows().find(|w|w.id==id).copied();
                if let Some(w)=moving_window{
                    let x=(self.mouse_x-offset_x).clamp(0,
                        self.desktop.width as i32-w.bounds.width as i32);
                    let y=(self.mouse_y-offset_y).clamp(48,
                        self.desktop.height as i32-w.bounds.height as i32);
                    if self.desktop.move_window(id,x,y).is_ok(){
                        self.moves+=1;
                    }
                }
            }
        }
        self.was_down=p.left;
        self.mouse_x!=old_x || self.mouse_y!=old_y || clicked
    }
    fn on_key(&mut self,ch:u8)->bool{
        #[cfg(feature="orbit-ui-preview")]
        if let Some(next)=crate::orbit_ui::Workspace::key(ch){
            self.orbit=next as u8;
            return false;
        }
        match ch{
            // Polling QEMU desktop shortcut: cycle all visible windows,
            // then use the same focus path as direct selection.
            b'n'=>{if let Ok(id)=self.desktop.focus_next_visible(){
                if self.focus(id){
                    self.focused_keyboard+=1;
                    log(b"IndianOS: R029 keyboard cycled visible window\n\0");
                }
            }}
            b'1'=>{if self.focus(self.files){
                self.focused_keyboard+=1;
                log(b"IndianOS: R029 keyboard focused Files window\n\0");
            }}
            b'2'=>{if self.focus(self.terminal){
                self.focused_keyboard+=1;
                log(b"IndianOS: R029 keyboard focused Terminal window\n\0");
            }}
            b'h'=>{
                let moving_window=self.desktop.windows()
                    .find(|w|w.id==self.active).copied();
                if let Some(window)=moving_window{
                    let max=self.desktop.width as i32-window.bounds.width as i32;
                    let x=(window.bounds.x+36).min(max);
                    if self.desktop.move_window(self.active,x,window.bounds.y).is_ok(){
                        self.moves+=1;
                        log(b"IndianOS: R029 window moved on framebuffer\n\0");
                    }
                }
            }
            b'm'=>{
                if self.desktop.minimize(self.active).is_ok(){
                    self.minimizations+=1;
                    log(b"IndianOS: R029 window minimized on framebuffer\n\0");
                }
            }
            b'r'=>{
                if self.desktop.focus(self.active).is_ok(){
                    self.restores+=1;
                    log(b"IndianOS: R029 window restored on framebuffer\n\0");
                }
            }
            b'x'=>{
                if self.desktop.close(self.active).is_ok(){
                    self.closures+=1;
                    log(b"IndianOS: R029 independent window closed on framebuffer\n\0");
                    self.active=self.desktop.focused().unwrap_or(0);
                }
            }
            b'q'=>return true,
            _=>{}
        }
        false
    }
    fn passed(&self)->bool{
        self.focused_mouse>=1 && self.focused_keyboard>=2 &&
        self.moves>=1 && self.minimizations==1 &&
        self.restores==1 && self.closures==1 &&
        self.desktop.windows().count()==1 &&
        self.desktop.windows().all(|w|w.id==self.files)
    }
}
/// R029 is a privileged QEMU demonstration; a future milestone will move
/// compositor ownership and per-window surfaces into isolated userspace.
pub(crate) fn run()->bool{
    let Some(mut surface)=Surface::from_boot()else{return false;};
    let Some(mut session)=Session::new(surface.width as u32,surface.height as u32)
        else{return false;};
    let Some(mut mouse)=Mouse::init()else{
        log(b"IndianOS: FATAL R029 auxiliary PS2 mouse enable failed\n\0");
        return false;
    };
    surface.render(&session.desktop,session.mouse_x,session.mouse_y,session.orbit);
    log(b"IndianOS: R029 two graphical framebuffer windows visible\n\0");
    log(b"IndianOS: R029 PS2 mouse and serial keyboard ready\n\0");
    let mut completed=false;
    for _ in 0..GUI_ITERATIONS{
        if let Some(packet)=mouse.poll(){
            if session.on_mouse(packet){
                surface.render(&session.desktop,session.mouse_x,session.mouse_y,session.orbit);
            }
        }
        if let Some(ch)=input::poll_byte(){
            if session.on_key(ch){
                completed=true;
                break;
            }
            surface.render(&session.desktop,session.mouse_x,session.mouse_y,session.orbit);
        }
        core::hint::spin_loop();
    }
    mouse.disable();
    if completed && session.passed(){
        log(b"IndianOS: R029 QEMU real framebuffer two interactive windows PASSED\n\0");
        true
    }else{
        log(b"IndianOS: R029 FATAL window interaction evidence incomplete\n\0");
        false
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[cfg(feature="normal-desktop")]
    fn isolated_service()->UserSurfaceService{
        let mut broker=Broker::new();broker.grant(2,1).unwrap();broker.grant(3,2).unwrap();
        UserSurfaceService{session:Session::new(800,600).unwrap(),
            commands:CommandStore::new(),counts:[0,0],focused:0,broker,
            frames_presented:0,launcher_visible:false,desktop_actions:0,
            retired_streams:0,
            #[cfg(feature="test-r031-supervisor")]restart_count:0,
            // State-only fixture; no hardware initialization is invoked.
            mouse:unsafe{core::mem::zeroed()},}
    }
    #[cfg(feature="normal-desktop")]
    #[test]fn desktop_pointer_preserves_press_and_release_as_individual_events(){
        let mut svc=isolated_service();
        let idle=svc.desktop_pointer_packet(None);assert_eq!(idle>>32,0);
        let pressed=svc.desktop_pointer_packet(Some(Packet{dx:0,dy:0,left:true}));
        let released=svc.desktop_pointer_packet(Some(Packet{dx:0,dy:0,left:false}));
        assert_eq!((pressed>>32)&3,3);assert_eq!((released>>32)&3,2);
        assert_eq!(pressed&0xffff_ffff,released&0xffff_ffff);
        // Holding with no motion is not another press. A later release still
        // survives a stationary packet and an idle poll.
        svc.desktop_pointer_packet(Some(Packet{dx:0,dy:0,left:true}));
        assert_eq!(svc.desktop_pointer_packet(Some(Packet{dx:0,dy:0,left:true}))>>32,1);
        assert_eq!(svc.desktop_pointer_packet(None)>>32,1);
        assert_eq!(svc.desktop_pointer_packet(Some(Packet{dx:0,dy:0,left:false}))>>32,2);
        for (dx,dy,x,y) in [(i16::MAX,i16::MIN,799u64,599u64),
                            (i16::MIN,i16::MAX,0,0)]{
            let event=svc.desktop_pointer_packet(Some(Packet{dx,dy,left:false}));
            assert_eq!(event&0xffff,x);assert_eq!((event>>16)&0xffff,y);
            assert_eq!(event>>32,2);
        }
    }
    #[cfg(feature="normal-desktop")]
    #[test]fn command_pages_have_two_real_guard_slots_and_clear_payload_only(){
        let mut store=CommandStore::new();
        let layout=store.layout();
        assert_eq!(core::mem::size_of::<GuardedCommands>(),3*COMMAND_PAGE);
        for (index,(lower,begin,end,upper)) in layout.iter().copied().enumerate(){
            assert_eq!(lower&4095,0);assert_eq!(begin,lower+4096);
            assert_eq!(end,begin+4096);assert_eq!(upper,end);
            store.surfaces[index].lower.fill(0xa5);
            store.surfaces[index].upper.fill(0x5a);
            store.surfaces[index].padding.fill(0xcc);
            store[index][0].rgb=index as u32+1;
        }
        assert_eq!(layout[1].0,layout[0].3+4096);
        let peer=store[1];store.clear_slot(0);
        assert_eq!(store[0],[UserCommand::EMPTY;COMMAND_LIMIT]);assert_eq!(store[1],peer);
        assert!(store.surfaces[0].padding.iter().all(|&b|b==0));
        assert!(store.surfaces[1].padding.iter().all(|&b|b==0xcc));
        for surface in &store.surfaces{
            assert!(surface.lower.iter().all(|&b|b==0xa5));
            assert!(surface.upper.iter().all(|&b|b==0x5a));
        }
    }
    #[cfg(feature="normal-desktop")]
    #[test]fn service_restart_preserves_guard_addresses_and_scrubs_payload_without_guard_access(){
        let mut svc=isolated_service();let layout=svc.commands.layout();
        for surface in &mut svc.commands.surfaces{
            surface.lower.fill(0xa5);surface.upper.fill(0x5a);
            surface.padding.fill(0xcc);surface.entries[0].rgb=0x123456;
        }
        svc.counts=[1,1];svc.frames_presented=23;svc.launcher_visible=true;svc.retired_streams=3;
        svc.reset_in_place(isolated_service());
        assert_eq!(svc.commands.layout(),layout);assert_eq!(svc.counts,[0,0]);
        assert_eq!(svc.commands.snapshot(),[[UserCommand::EMPTY;COMMAND_LIMIT];2]);
        assert_eq!(svc.frames_presented,0);assert!(!svc.launcher_visible);
        assert_eq!(svc.retired_streams,0);assert!(!svc.retirement_verified());
        for surface in &svc.commands.surfaces{
            assert!(surface.lower.iter().all(|&b|b==0xa5));
            assert!(surface.upper.iter().all(|&b|b==0x5a));
            assert!(surface.padding.iter().all(|&b|b==0));
        }
    }
    #[cfg(feature="normal-desktop")]
    #[test]fn teardown_evidence_survives_scrubbing_but_requires_both_live_streams(){
        let mut svc=isolated_service();svc.frames_presented=1;
        svc.counts=[1,1];
        assert!(!svc.retirement_verified());
        svc.retire_client(99);assert_eq!(svc.retired_streams,0);
        svc.retire_client(2);assert_eq!(svc.retired_streams,1);
        assert!(!svc.retirement_verified());
        svc.retire_client(2);assert_eq!(svc.retired_streams,1);
        svc.retire_client(3);
        assert_eq!(svc.counts,[0,0]);assert!(svc.retirement_verified());
        assert_eq!(svc.commands.snapshot(),[[UserCommand::EMPTY;COMMAND_LIMIT];2]);
        let mut empty=isolated_service();empty.frames_presented=1;
        empty.retire_client(2);empty.retire_client(3);
        assert!(!empty.retirement_verified());
    }
    #[cfg(feature="normal-desktop")]
    #[test]fn command_owner_and_retirement_isolate_backings_and_failed_submission_rolls_back(){
        fn fill(window:u32)->[u8;64]{
            let mut b=[0;64];b[..4].copy_from_slice(b"R29U");
            b[4..8].copy_from_slice(&1u32.to_le_bytes());b[8..12].copy_from_slice(&window.to_le_bytes());
            b[12..16].copy_from_slice(&FILL.to_le_bytes());
            b[20..22].copy_from_slice(&1u16.to_le_bytes());b[22..24].copy_from_slice(&1u16.to_le_bytes());b
        }
        let mut svc=isolated_service();
        assert!(svc.submit_with(2,&fill(1),|_|true));assert!(svc.submit_with(3,&fill(2),|_|true));
        let snapshot=svc.commands.snapshot();let counts=svc.counts;
        assert!(!svc.submit_with(3,&fill(1),|_|panic!("foreign owner painted")));
        assert!(!svc.submit_with(2,&fill(2),|_|panic!("foreign owner painted")));
        assert!(!svc.submit_with(2,&fill(1),|_|false));
        assert_eq!(svc.commands.snapshot(),snapshot);assert_eq!(svc.counts,counts);
        svc.retire_client(3);
        assert_eq!(svc.commands[0],snapshot[0]);
        assert_eq!(svc.commands[1],[UserCommand::EMPTY;COMMAND_LIMIT]);assert_eq!(svc.counts,[1,0]);
        assert!(!svc.submit_with(3,&fill(2),|_|panic!("retired owner painted")));
    }
    #[cfg(feature="normal-desktop")]
    #[test]fn compositor_state_requires_current_app_grants_and_rolls_back_failed_paint(){
        let mut broker=Broker::new();broker.grant(2,1).unwrap();broker.grant(3,2).unwrap();
        let mut svc=UserSurfaceService{session:Session::new(800,600).unwrap(),
            commands:CommandStore::new(),counts:[0,0],
            focused:0,broker,frames_presented:0,launcher_visible:false,desktop_actions:0,
            retired_streams:0,
            #[cfg(feature="test-r031-supervisor")]restart_count:0,
            mouse:unsafe{core::mem::zeroed()},};
        let before=svc.window_frame(4).unwrap().wire();
        for pid in [0,1,2,3,5,u32::MAX]{
            assert!(svc.window_frame(pid).is_none());
            assert!(!svc.apply_window_frame(pid,&before,|_|panic!("foreign owner painted")));
        }
        let mut requested=crate::gui_protocol::DesktopFrame::parse(&before).unwrap();
        requested.windows[0].x+=10;requested.front=1;requested.launcher=true;
        assert!(!svc.apply_window_frame(4,&requested.wire(),|_|false));
        assert_eq!(svc.window_frame(4).unwrap().wire(),before);assert_eq!(svc.frames_presented,0);
        assert!(svc.apply_window_frame(4,&requested.wire(),|_|true));
        assert_eq!(svc.window_frame(4).unwrap(),requested);assert_eq!(svc.frames_presented,1);
        let good=requested.wire();requested.windows[0].width+=1;
        assert!(!svc.apply_window_frame(4,&requested.wire(),|_|panic!("resized backing painted")));
        assert_eq!(svc.window_frame(4).unwrap().wire(),good);
        svc.broker.revoke(3);
        assert!(!svc.apply_window_frame(4,&good,|_|panic!("retired app painted")));
    }
    #[cfg(any(feature="test-r029-desktop-suite",feature="normal-desktop"))]
    #[test]fn owned_text_replacement_is_bounded_transactional_and_erases_shortened_glyphs(){
        let mut broker=Broker::new();broker.grant(2,1).unwrap();broker.grant(3,2).unwrap();
        let mut svc=UserSurfaceService{
            session:Session::new(800,600).unwrap(),
            commands:CommandStore::new(),counts:[0,0],
            focused:0,broker,frames_presented:0,launcher_visible:false,desktop_actions:0,
            #[cfg(feature="normal-desktop")]retired_streams:0,
            #[cfg(feature="test-r031-supervisor")]
            restart_count:0,
            mouse:unsafe{core::mem::zeroed()},
        };
        fn text(operation:u32,bytes:&[u8])->[u8;64]{
            let mut b=[0;64];b[..4].copy_from_slice(b"R29U");
            b[4..8].copy_from_slice(&1u32.to_le_bytes());
            b[8..12].copy_from_slice(&2u32.to_le_bytes());
            b[12..16].copy_from_slice(&operation.to_le_bytes());
            b[16..18].copy_from_slice(&24u16.to_le_bytes());
            b[18..20].copy_from_slice(&181u16.to_le_bytes());
            b[20..22].copy_from_slice(&270u16.to_le_bytes());
            b[22..24].copy_from_slice(&14u16.to_le_bytes());
            b[24..28].copy_from_slice(&0xffffffu32.to_le_bytes());
            b[32..32+bytes.len()].copy_from_slice(bytes);b
        }
        let replace=text(REPLACE_TEXT,b"A");
        assert!(!svc.submit(3,&replace)); // No existing field.
        assert!(svc.submit(3,&text(TEXT,b"LONGER")));
        let original=svc.commands.snapshot();
        assert!(!svc.submit(2,&replace));
        let mut moved=replace;moved[16]=25;
        assert!(!svc.submit(3,&moved));
        assert!(!svc.submit_with(3,&replace,|_|false));
        assert_eq!(svc.commands.snapshot(),original);assert_eq!(svc.counts,[0,1]);
        let p=Palette{bg:2,top:3,bar:4,title:5,unfocused:6,light:7,dark:8,panel:9,white:10,accent:11};
        const PITCH:usize=804;const GUARD:u32=0xfeedface;
        let mut pixels=std::vec![GUARD;PITCH*600+16];
        let mut out=Surface{ptr:pixels.as_mut_ptr().cast(),width:800,height:600,pitch:PITCH*4,
            palette:p,rgb:|r,g,b|Some(((r as u32)<<16)|((g as u32)<<8)|b as u32)};
        assert!(svc.paint(&mut out));let long=pixels.clone();
        for _ in 0..100{assert!(svc.submit(3,&replace));}
        assert_eq!(svc.counts,[0,1]);assert_eq!(svc.commands[1][0].operation,TEXT);
        assert!(svc.paint(&mut out));
        let (x,y,_,_)=svc.client_bounds(2).unwrap();
        // The old second-through-sixth glyphs disappear from actual pixels.
        let mut erased=0;
        for py in y as usize+181..y as usize+195{
            for px in x as usize+36..x as usize+24+6*12{
                if long[py*PITCH+px]==0xffffff&&pixels[py*PITCH+px]!=0xffffff{erased+=1;}
            }
        }
        assert!(erased>10);
        for row in 0..600{assert!(pixels[row*PITCH+800..(row+1)*PITCH].iter().all(|&v|v==GUARD));}
        assert!(pixels[PITCH*600..].iter().all(|&v|v==GUARD));
        svc.session.dragging=Some((2,13,17));
        svc.session.was_down=true;
        let last_peer=svc.broker.grant_epoch_for(2,1).unwrap();
        svc.retire_client(3);
        assert_eq!(svc.session.dragging,None);
        assert!(!svc.session.was_down);
        assert_eq!(svc.broker.grant_epoch_for(2,1),Ok(last_peer));
        assert!(!svc.submit(3,&replace));
    }
    #[cfg(any(feature="test-r029-desktop-suite",feature="normal-desktop"))]
    #[test]fn retained_clients_obey_visibility_through_authenticated_present(){
        // No firmware mapping or PS/2 I/O: only the destination and RGB packing
        // differ from boot. present_with and paint are the production paths.
        let mut broker=Broker::new();
        broker.grant(2,1).unwrap();broker.grant(3,2).unwrap();
        let mut svc=UserSurfaceService{
            session:Session::new(800,600).unwrap(),
            commands:CommandStore::new(),counts:[0,0],
            focused:0,broker,frames_presented:0,launcher_visible:false,
            #[cfg(feature="normal-desktop")]retired_streams:0,
            desktop_actions:0,
            #[cfg(feature="test-r031-supervisor")]
            restart_count:0,
            // Mouse is integer-only state; this test never polls hardware.
            mouse:unsafe{core::mem::zeroed()},
        };
        svc.session.desktop.restore_bounds(1,Rect::new(100,100,400,300)).unwrap();
        svc.session.desktop.restore_bounds(2,Rect::new(100,50,400,300)).unwrap();
        svc.session.mouse_x=790;svc.session.mouse_y=590;
        fn wire(id:u32,operation:u32,rgb:u32)->[u8;64]{
            let mut b=[0;64];
            b[..4].copy_from_slice(b"R29U");
            b[4..8].copy_from_slice(&1u32.to_le_bytes());
            b[8..12].copy_from_slice(&id.to_le_bytes());
            b[12..16].copy_from_slice(&operation.to_le_bytes());
            b[20..22].copy_from_slice(&382u16.to_le_bytes());
            b[22..24].copy_from_slice(&246u16.to_le_bytes());
            b[24..28].copy_from_slice(&rgb.to_le_bytes());
            if operation==TEXT{b[32]=b'X';}
            b
        }
        let presenter=if cfg!(feature="normal-desktop"){4}else{1};
        fn present(svc:&mut UserSurfaceService,pid:u32,front:u32,out:&mut Surface)->bool{
            #[cfg(feature="normal-desktop")]
            {
                let Some(mut frame)=svc.window_frame(pid)else{return false;};
                if !(1..=2).contains(&front){return false;}
                frame.front=front as u8;frame.windows[front as usize-1].minimized=false;
                svc.apply_window_frame(pid,&frame.wire(),|s|s.paint(out))
            }
            #[cfg(not(feature="normal-desktop"))]
            {svc.present_with(pid,front,|s|s.paint(out))}
        }
        const FILES:u32=0x123456;
        const TERMINAL:u32=0xabcdef;
        for (pid,id,color) in [(2,1,FILES),(3,2,TERMINAL)]{
            assert!(svc.submit(pid,&wire(id,FILL,color)));
            assert!(svc.submit(pid,&wire(id,TEXT,color)));
        }
        // Files-GUI presents require the real client startup threshold.
        #[cfg(any(feature="test-r031-files-gui",feature="normal-desktop"))]
        for _ in 0..3{assert!(svc.submit(2,&wire(1,FILL,FILES)));}
        let retained=svc.commands.snapshot();
        let retained_counts=svc.counts;
        let p=Palette{bg:2,top:3,bar:4,title:5,unfocused:6,
            light:7,dark:8,panel:9,white:10,accent:11};
        const PITCH:usize=804;
        const GUARD:u32=0xfeedface;
        let mut pixels=std::vec![GUARD;PITCH*600+16];
        let mut out=Surface{ptr:pixels.as_mut_ptr().cast(),width:800,height:600,
            pitch:PITCH*4,palette:p,
            rgb:|r,g,b|Some(((r as u32)<<16)|((g as u32)<<8)|b as u32)};
        let pixel=|data:&[u32],x:usize,y:usize|data[y*PITCH+x];
        assert!(present(&mut svc,presenter,2,&mut out));
        // Terminal content overlaps Files' titlebar while Terminal is in front.
        assert_eq!(pixel(&pixels,115,105),TERMINAL);
        svc.session.desktop.minimize(2).unwrap();
        // A redraw after minimization retains the old stacking order: this
        // catches the original ghost client painting over the exposed peer.
        assert!(svc.paint(&mut out));
        assert_eq!(pixel(&pixels,115,105),p.title);
        assert_eq!(pixel(&pixels,115,160),FILES);
        assert!(!pixels.contains(&TERMINAL));
        let before=pixels.clone();
        let frame=svc.frames_presented;
        // Foreign applications can neither alter another backing stream nor
        // trigger composition or restore its hidden window via frame present.
        assert!(!svc.submit(2,&wire(2,FILL,0xff0000)));
        for pid in [0,2,3,99]{
            assert!(!present(&mut svc,pid,2,&mut out));
        }
        assert!(!present(&mut svc,presenter,99,&mut out));
        assert_eq!(pixels,before);assert_eq!(svc.frames_presented,frame);
        assert!(svc.window(2).unwrap().minimized);
        assert_eq!(svc.commands.snapshot(),retained);assert_eq!(svc.counts,retained_counts);
        assert!(present(&mut svc,presenter,1,&mut out));
        assert_eq!(pixel(&pixels,115,105),p.title);
        assert_eq!(pixel(&pixels,115,160),FILES);
        assert!(!pixels.contains(&TERMINAL));
        // An explicit server-selected focus restores the independent backing.
        assert!(present(&mut svc,presenter,2,&mut out));
        assert_eq!(pixel(&pixels,115,105),TERMINAL);
        // With both visible, back-to-front replay preserves peer chrome too.
        assert!(present(&mut svc,presenter,1,&mut out));
        assert_eq!(pixel(&pixels,115,105),p.title);
        assert_eq!(pixel(&pixels,115,160),FILES);
        // Even with its desktop window still visible, a process that
        // exits must lose the right to replay retained display commands.
        svc.session.dragging=Some((2,50,60));
        svc.session.was_down=true;
        svc.retire_client(3);
        assert_eq!(svc.session.dragging,None);
        assert!(!svc.session.was_down);
        assert!(svc.paint(&mut out));
        assert!(!pixels.contains(&TERMINAL));
        #[cfg(not(feature="normal-desktop"))]
        assert_eq!(svc.counts,retained_counts);
        #[cfg(feature="normal-desktop")]
        assert_eq!(svc.counts,[retained_counts[0],0]);
        svc.session.desktop.close(2).unwrap();
        assert!(!svc.submit(3,&wire(2,FILL,0xff0000)));
        #[cfg(not(feature="normal-desktop"))]
        assert!(present(&mut svc,presenter,1,&mut out));
        #[cfg(feature="normal-desktop")]
        {assert!(!present(&mut svc,presenter,1,&mut out));assert!(svc.paint(&mut out));}
        assert!(!pixels.contains(&TERMINAL));
        #[cfg(not(feature="normal-desktop"))]
        assert_eq!(svc.commands.snapshot(),retained);
        #[cfg(feature="normal-desktop")]
        assert_eq!(svc.commands.snapshot(),[retained[0],[UserCommand::EMPTY;COMMAND_LIMIT]]);
        assert_eq!(&pixels[PITCH*600..],&[GUARD;16]);
        for row in 0..600{assert_eq!(&pixels[row*PITCH+800..(row+1)*PITCH],&[GUARD;4]);}
    }
    #[test]fn physical_character_cycle_updates_session_target_and_live_geometry(){
        let mut s=Session::new(800,600).unwrap();
        let third=s.desktop.open(App::Settings,Rect::new(20,60,320,240)).unwrap();
        for id in [s.files,s.terminal,third,s.files,s.terminal,third]{
            assert!(!s.on_key(b'n'));
            assert_eq!(s.active,id);assert_eq!(s.desktop.focused(),Some(id));
        }
        s.desktop.minimize(s.terminal).unwrap();
        s.on_key(b'n');assert_eq!(s.active,s.files);
        s.on_key(b'n');assert_eq!(s.active,third);
        let before=s.desktop.windows().find(|w|w.id==third).unwrap().bounds;
        s.on_key(b'h');
        assert!(s.desktop.windows().find(|w|w.id==third).unwrap().bounds.x>before.x);
        s.on_key(b'x');assert!(!s.desktop.windows().any(|w|w.id==third));
    }
    #[test]fn windows_keyboard_focus_move_minimize_restore_close(){
        let mut s=Session::new(1024,768).unwrap();
        assert_eq!(s.desktop.windows().count(),2);
        assert_eq!(s.desktop.focused(),Some(s.terminal));
        s.on_key(b'1');
        assert_eq!(s.desktop.focused(),Some(s.files));
        s.on_key(b'2');
        s.on_key(b'h');
        s.on_key(b'm');
        assert_eq!(s.desktop.focused(),Some(s.files));
        s.on_key(b'r');
        assert_eq!(s.desktop.focused(),Some(s.terminal));
        s.on_key(b'x');
        assert_eq!(s.desktop.windows().count(),1);
        assert_eq!(s.desktop.focused(),Some(s.files));
        assert!(!s.passed()); // no physical mouse evidence
    }
    #[test]fn actual_framebuffer_clips_writes_to_allocated_pixels(){
        let mut data=std::vec![0xccu8;80*48*4+32];
        let p=Palette{bg:2,top:3,bar:4,title:5,unfocused:6,
            light:7,dark:8,panel:9,white:10,accent:11};
        let mut surface=Surface{ptr:data.as_mut_ptr(),width:80,height:48,
            pitch:80*4,palette:p,rgb:|_,_,_|None};
        surface.fill(-200,-200,400,400,0x0123_4567);
        assert_eq!(&data[80*48*4..],&[0xccu8;32]);
        assert_eq!(&data[..4],&0x0123_4567u32.to_ne_bytes());
        surface.fill(i32::MAX,i32::MAX,u32::MAX,u32::MAX,22);
        assert_eq!(&data[80*48*4..],&[0xccu8;32]);
    }
    #[cfg(feature="orbit-ui-preview")]
    #[test]fn orbit_workspace_hotkeys_preserve_live_window_ownership(){
        let mut s=Session::new(1280,800).unwrap();
        let original=s.desktop.focused();
        for (hotkey,slot) in [(b'c',1),(b'l',2),(b'p',3),(b'b',0)]{
            assert!(!s.on_key(hotkey));
            assert_eq!(s.orbit,slot);
            assert_eq!(s.desktop.focused(),original);
        }
        assert_eq!(s.desktop.windows().count(),2);
        assert!(s.desktop.windows().all(|w|w.bounds.x>=190
            &&w.bounds.y>=62));
    }
    #[test]fn mouse_click_changes_z_order_and_drag_is_bounded(){
        let mut s=Session::new(800,600).unwrap();
        // Initial mouse position is screen center. Move onto the first
        // window's exposed titlebar and press the actual PS/2 left button.
        assert!(s.on_mouse(Packet{dx:-300,dy:210,left:false}));
        assert!(s.on_mouse(Packet{dx:0,dy:0,left:true}));
        assert_eq!(s.desktop.focused(),Some(s.files));
        s.on_mouse(Packet{dx:1000,dy:-1000,left:true});
        assert!(s.desktop.windows().find(|w|w.id==s.files)
            .is_some_and(|w|w.bounds.x>=0));
        s.on_mouse(Packet{dx:0,dy:0,left:false});
    }
}

#[cfg(feature="orbit-ui-preview")]
impl crate::orbit_ui::Canvas for Surface{
    fn rect(&mut self,x:i32,y:i32,w:u32,h:u32,color:u32){
        Surface::fill(self,x,y,w,h,color);
    }
    fn label(&mut self,x:i32,y:i32,text:&[u8],color:u32){
        Surface::text(self,x,y,text,color);
    }
}
