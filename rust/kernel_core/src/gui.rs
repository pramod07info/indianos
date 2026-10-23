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
    ptr:*mut u8,width:usize,height:usize,pitch:usize,palette:Palette
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
            pitch:f.pitch,palette:Palette::new()?})
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
    fn draw_window(&mut self,w:crate::desktop::Window,focused:bool){
        if !w.visible||w.minimized{return;}
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
    fn render(&mut self,desktop:&Desktop,mouse_x:i32,mouse_y:i32){
        let p=self.palette;
        self.fill(0,0,self.width as u32,self.height as u32,p.bg);
        self.fill(0,0,self.width as u32,48,p.top);
        self.text(20,18,b"INDIANOS DESKTOP",p.white);
        self.fill(0,(self.height-42) as i32,self.width as u32,42,p.bar);
        self.fill(20,(self.height-34) as i32,90,25,p.title);
        self.text(30,(self.height-29) as i32,b"APPS",p.white);
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
#[cfg(feature="test-r029-userspace")]
use crate::gui_protocol::{Command as UserCommand,FILL,TEXT,COMMAND_LIMIT};
#[cfg(feature="test-r029-userspace")]
use crate::gui_broker::{Broker,Event as GuiEvent};
#[cfg(feature="test-r029-userspace")]
struct UserSurfaceService{
    session:Session,
    commands:[[UserCommand;COMMAND_LIMIT];2],
    counts:[usize;2],
    focused:u32,
    broker:Broker<2,8>,
    #[cfg(feature="test-r029-physical-events")]
    mouse:Mouse,
}
#[cfg(feature="test-r029-userspace")]
static mut USER_SURFACE_SERVICE:Option<UserSurfaceService>=None;
#[cfg(feature="test-r029-userspace")]
impl UserSurfaceService{
    fn new(w:u32,h:u32)->Option<Self>{
        let session=Session::new(w,h)?;
        if session.files!=1||session.terminal!=2{return None;}
        let mut broker=Broker::new();
        broker.grant(1,session.files).ok()?;
        #[cfg(feature="test-r029-multiprocess")]
        broker.grant(2,session.terminal).ok()?;
        #[cfg(not(feature="test-r029-multiprocess"))]
        broker.grant(1,session.terminal).ok()?;
        #[cfg(feature="test-r029-physical-events")]
        let mouse=Mouse::init()?;
        Some(Self{session,
            commands:[[UserCommand::EMPTY;COMMAND_LIMIT];2],
            counts:[0,0],focused:0,broker,
            #[cfg(feature="test-r029-physical-events")]
            mouse})
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
        let palette=out.palette;
        // Paint desktop chrome, then each window AND its user commands in
        // the same z-order. A rear window can never paint over front chrome.
        out.fill(0,0,out.width as u32,out.height as u32,palette.bg);
        out.fill(0,0,out.width as u32,48,palette.top);
        out.text(20,18,b"INDIANOS DESKTOP",palette.white);
        out.fill(0,(out.height-42) as i32,out.width as u32,42,palette.bar);
        out.fill(20,(out.height-34) as i32,90,25,palette.title);
        out.text(30,(out.height-29) as i32,b"APPS",palette.white);
        for window in self.session.desktop.windows(){
            out.draw_window(*window,self.session.desktop.focused()==Some(window.id));
            let Some((origin_x,origin_y,_,_))=self.client_bounds(window.id)
                else{return false;};
            let slot=window.id as usize-1;
            for command in &self.commands[slot][..self.counts[slot]]{
                let rgb=command.rgb;
                let Some(color)=boot::desktop_rgb(
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
        out.fill(self.session.mouse_x-1,self.session.mouse_y-10,3,21,palette.accent);
        out.fill(self.session.mouse_x-10,self.session.mouse_y-1,21,3,palette.accent);
        out.fill(self.session.mouse_x-1,self.session.mouse_y-1,3,3,palette.white);
        true
    }
    fn submit(&mut self,pid:u32,bytes:&[u8;64])->bool{
        let Ok(cmd)=UserCommand::parse(bytes)else{return false;};
        if !self.broker.owns(pid,cmd.window){return false;}
        let slot=cmd.window as usize-1;
        let Some((_,_,width,height))=self.client_bounds(cmd.window)else{return false;};
        if !cmd.within_client(width,height) || self.counts[slot]>=COMMAND_LIMIT{
            return false;
        }
        self.commands[slot][self.counts[slot]]=cmd;
        self.counts[slot]+=1;
        if self.redraw(){true}else{self.counts[slot]-=1;false}
    }
    fn focus(&mut self,pid:u32,id:u32)->bool{
        if !self.broker.can_post(pid,id){return false;}
        if !self.session.focus(id){return false;}
        if !self.redraw(){return false;}
        if self.broker.post_focus(pid,id).is_err(){return false;}
        self.focused+=1;true
    }
    #[cfg(feature="test-r029-physical-events")]
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
            if changed{let _=self.redraw();}
        }
    }
}
#[cfg(feature="test-r029-userspace")]
fn user_service()->Option<&'static mut UserSurfaceService>{
    // Single BSP, interrupt masked, and no recursive caller permitted.
    unsafe{(&mut *ptr::addr_of_mut!(USER_SURFACE_SERVICE)).as_mut()}
}
#[cfg(feature="test-r029-userspace")]
pub(crate) fn user_surface_start()->bool{
    let Some(f)=Surface::from_boot()else{return false;};
    let Some(svc)=UserSurfaceService::new(f.width as u32,f.height as u32)else{return false;};
    if !svc.redraw(){return false;}
    unsafe{USER_SURFACE_SERVICE=Some(svc);}
    log(b"IndianOS: R029 ring3 surface server shows two framebuffer windows\n\0");
    true
}
#[cfg(feature="test-r029-userspace")]
pub(crate) fn user_surface_submit(pid:u32,bytes:&[u8;64])->bool{
    user_service().is_some_and(|svc|svc.submit(pid,bytes))
}
#[cfg(feature="test-r029-userspace")]
pub(crate) fn user_surface_focus(pid:u32,id:u32)->bool{
    user_service().is_some_and(|svc|svc.focus(pid,id))
}
#[cfg(feature="test-r029-userspace")]
pub(crate) fn user_surface_status(pid:u32)->Option<u64>{
    let svc=user_service()?;
    if !svc.broker.known(pid){return None;}
    Some((svc.counts[0] as u64) |
         ((svc.counts[1] as u64)<<8) |
         ((svc.focused as u64)<<16) |
         ((svc.session.desktop.focused()? as u64)<<24))
}
#[cfg(feature="test-r029-userspace")]
pub(crate) fn user_surface_peek(pid:u32)->Option<GuiEvent>{
    let svc=user_service()?;
    #[cfg(feature="test-r029-physical-events")]
    svc.pump_mouse();
    svc.broker.peek(pid).ok()
}
#[cfg(feature="test-r029-userspace")]
pub(crate) fn user_surface_terminate(pid:u32){
    if let Some(svc)=user_service(){
        svc.broker.revoke(pid);
        #[cfg(feature="test-r029-physical-events")]
        if !svc.broker.known(1) && !svc.broker.known(2){
            svc.mouse.disable();
        }
    }
}
#[cfg(feature="test-r029-userspace")]
pub(crate) fn user_surface_ack(pid:u32,sequence:u32)->bool{
    user_service().is_some_and(|svc|svc.broker.ack(pid,sequence).is_ok())
}
#[cfg(feature="test-r029-userspace")]
pub(crate) fn user_surface_verified()->bool{
    user_service().is_some_and(|svc|{
        svc.counts==[2,2] && svc.focused>=1 &&
        !svc.broker.known(1) && !svc.broker.known(2) &&
        svc.session.desktop.focused()==Some(
            if cfg!(feature="test-r029-physical-events") {
                svc.session.terminal
            }else{svc.session.files}
        )
    })
}
pub(crate) struct Session{
    desktop:Desktop,files:u32,terminal:u32,active:u32,
    mouse_x:i32,mouse_y:i32,was_down:bool,
    dragging:Option<(u32,i32,i32)>,
    focused_mouse:u32,
    focused_keyboard:u32,
    moves:u32,minimizations:u32,restores:u32,closures:u32,
}
impl Session{
    fn new(w:u32,h:u32)->Option<Self>{
        let mut desktop=Desktop::new(w,h).ok()?;
        let win_w=(w*49/100).max(320).min(w-80);
        let win_h=(h*50/100).max(240).min(h-110);
        let left_x=45;
        let top_y=75;
        let right_x=(w-win_w-36) as i32;
        let right_y=(h-win_h-65) as i32;
        let files=desktop.open(App::Files,Rect::new(left_x,top_y,win_w,win_h)).ok()?;
        let terminal=desktop.open(App::Terminal,
            Rect::new(right_x,right_y,win_w,win_h)).ok()?;
        Some(Self{desktop,files,terminal,active:terminal,
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
        match ch{
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
    surface.render(&session.desktop,session.mouse_x,session.mouse_y);
    log(b"IndianOS: R029 two graphical framebuffer windows visible\n\0");
    log(b"IndianOS: R029 PS2 mouse and serial keyboard ready\n\0");
    let mut completed=false;
    for _ in 0..GUI_ITERATIONS{
        if let Some(packet)=mouse.poll(){
            if session.on_mouse(packet){
                surface.render(&session.desktop,session.mouse_x,session.mouse_y);
            }
        }
        if let Some(ch)=input::poll_byte(){
            if session.on_key(ch){
                completed=true;
                break;
            }
            surface.render(&session.desktop,session.mouse_x,session.mouse_y);
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
            pitch:80*4,palette:p};
        surface.fill(-200,-200,400,400,0x0123_4567);
        assert_eq!(&data[80*48*4..],&[0xccu8;32]);
        assert_eq!(&data[..4],&0x0123_4567u32.to_ne_bytes());
        surface.fill(i32::MAX,i32::MAX,u32::MAX,u32::MAX,22);
        assert_eq!(&data[80*48*4..],&[0xccu8;32]);
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
