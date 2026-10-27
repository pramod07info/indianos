//! IndianOS Orbit R001: actual framebuffer UI paint contract, no allocator.
//! This is a strictly opt-in QEMU visual prototype. The assistant, snapshots
//! and search display *preview* affordances and do not perform those actions.
use crate::desktop::{App,Window};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Workspace{Build,Create,Learn,Personal}
impl Workspace{
    pub fn label(self)->&'static [u8]{
        match self{
            Self::Build=>b"BUILD",Self::Create=>b"CREATE",
            Self::Learn=>b"LEARN",Self::Personal=>b"PERSONAL"
        }
    }
    pub fn key(k:u8)->Option<Self>{
        match k{b'b'=>Some(Self::Build),b'c'=>Some(Self::Create),
            b'l'=>Some(Self::Learn),b'p'=>Some(Self::Personal),_=>None}
    }
}
/// Honest per-workspace explanatory chrome. This is not AI execution.
impl Workspace{
    pub fn subtitle(self)->&'static [u8]{
        match self{
            Self::Build=>b"KERNEL AND CODE",
            Self::Create=>b"DESIGN AND CONTENT",
            Self::Learn=>b"STUDY AND NOTES",
            Self::Personal=>b"PERSONAL WORK",
        }
    }
    pub fn shortcut(self)->&'static [u8]{
        match self{
            Self::Build=>b"PRESS B",Self::Create=>b"PRESS C",
            Self::Learn=>b"PRESS L",Self::Personal=>b"PRESS P",
        }
    }
}
#[derive(Clone,Copy)]
pub struct Colors{pub bg:u32,pub top:u32,pub bar:u32,pub title:u32,
    pub unfocused:u32,pub light:u32,pub dark:u32,
    pub panel:u32,pub white:u32,pub accent:u32}
pub trait Canvas{
    fn rect(&mut self,x:i32,y:i32,w:u32,h:u32,color:u32);
    fn label(&mut self,x:i32,y:i32,text:&[u8],color:u32);
}
/// Pure, no-heap classification shared by desktop GUI and future mobile ports.
/// Phone geometry is an offline layout contract only: IndianOS does NOT boot
/// on ARM64/phones and the present x86 VGA backend requires >=640x480.
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum DisplayClass{PhonePortrait,Compact,Desktop}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Layout{pub class:DisplayClass,pub width:u32,pub height:u32}
impl Layout{
    pub const fn new(width:u32,height:u32)->Self{
        let class=if width<640 {DisplayClass::PhonePortrait}
        else if width<1120||height<720{DisplayClass::Compact}
        else{DisplayClass::Desktop};
        Self{class,width,height}
    }
    pub const fn usable_desktop(&self)->bool{
        self.width>=640&&self.height>=480&&
        self.width<=1600&&self.height<=1600
    }
    /// Two actual independent GUI windows on supported VGA configurations.
    /// Choose sizes from usable area and keep them outside the top/ribbon.
    /// Does not claim native multiple monitors, runtime resize or touch input.
    pub fn windows(self)->Option<(crate::desktop::Rect,crate::desktop::Rect)>{
        use crate::desktop::Rect;
        if !self.usable_desktop(){return None;}
        let (w,h)=(self.width,self.height);
        if self.class==DisplayClass::Desktop{
            let area=w-202-264-32;
            let files_w=area*56/100;
            let terminal_w=area-files_w-14;
            let height=(h-246).min(482);
            return Some((
                Rect::new(204,143,files_w,height),
                Rect::new((204+files_w+14) as i32,143,terminal_w,height)
            ));
        }
        let fw=((w-110)*56/100).max(300).min(w-95);
        let tw=((w-125)*48/100).max(318).min(w-75);
        let fh=(h-183).min(490);
        let th=(h-214).min(455);
        Some((Rect::new(82,82,fw,fh),
              Rect::new((w-tw-15) as i32,128,tw,th)))
    }
}
/// Actual framebuffer rendering at small x86 QEMU resolutions.
/// Larger screens retain the existing Orbit R001 rail/assistant/ribbon.
fn compact_shell(c:&mut impl Canvas,w:u32,h:u32,workspace:Workspace,p:Colors){
    let end=h as i32;
    c.rect(0,0,w,h,p.bg);
    c.rect(0,0,w,62,p.top);
    c.rect(13,14,30,30,p.accent);c.rect(19,20,18,18,p.top);
    c.label(52,23,b"INDIANOS ORBIT",p.white);
    workspace_header(c,w,h,workspace,p);
    c.label(w as i32-148,23,b"LOCAL UI",p.accent);
    c.rect(0,62,72,h.saturating_sub(144),p.top);
    c.rect(70,62,2,h.saturating_sub(144),p.panel);
    let entries=[Workspace::Build,Workspace::Create,
                 Workspace::Learn,Workspace::Personal];
    for (i,item) in entries.iter().enumerate(){
        let y=110+i as i32*75;
        c.rect(12,y-7,49,55,if *item==workspace{p.title}else{p.bar});
        c.rect(12,y-7,4,55,if *item==workspace{p.accent}else{p.panel});
        c.label(24,y+8,&item.label()[..1],p.white);
    }
    c.rect(0,end-82,w,82,p.top);
    c.rect(0,end-83,w,1,p.accent);
    c.label(20,end-57,b"SESSION / PREVIEW",p.accent);
    c.label(20,end-28,b"FILES  TERMINAL  |  RESTORE PLANNED",p.white);
}
/// Phone layout *preview only*: not called by the current VGA backend.
/// Kept behind tested geometry so a future ARM64 renderer can reuse it.
fn phone_shell(c:&mut impl Canvas,w:u32,h:u32,workspace:Workspace,p:Colors){
    let end=h as i32;
    c.rect(0,0,w,h,p.bg);
    c.rect(0,0,w,70,p.top);
    c.label(16,24,b"INDIANOS MOBILE",p.white);
    c.label(16,49,b"ORBIT PREVIEW",p.accent);
    c.label(18,192,workspace.subtitle(),p.white);
    c.label(18,92,b"WORKSPACE",p.accent);
    c.rect(12,117,w.saturating_sub(24),75,p.title);
    c.label(26,146,workspace.label(),p.white);
    c.rect(12,216,w.saturating_sub(24),112,p.light);
    c.label(27,236,b"FILES",p.white);
    c.label(27,272,b"WINDOWS / PLANNED",p.accent);
    c.rect(12,341,w.saturating_sub(24),112,p.light);
    c.label(27,360,b"ORBIT ASSISTANT",p.white);
    c.label(27,394,b"NOT CONNECTED",p.accent);
    c.rect(0,end-78,w,78,p.top);
    c.label(15,end-48,b"HOME",p.white);
    c.label(w as i32-155,end-48,b"WORKSPACES",p.white);
}
/// Draw a visible, bounded live-workspace status area. No hidden model
/// execution, no font assets, no GPU or network calls.
pub fn workspace_header(c:&mut impl Canvas,w:u32,h:u32,ws:Workspace,p:Colors){
    if w<640||h<480{return;}
    let compact=w<1120||h<720;
    let (x,width)=if compact{(79, w.saturating_sub(88))}
                  else{(204,w.saturating_sub(480))};
    if width<200{return;}
    c.rect(x,72,width,55,p.top);
    c.rect(x,72,5,55,p.accent);
    c.label(x as i32+14,82,ws.subtitle(),p.white);
    c.label(x as i32+14,105,ws.shortcut(),p.accent);
}
pub fn shell(c:&mut impl Canvas,w:u32,h:u32,workspace:Workspace,p:Colors){
    match Layout::new(w,h).class{
        DisplayClass::PhonePortrait=>{phone_shell(c,w,h,workspace,p);return;},
        DisplayClass::Compact=>{compact_shell(c,w,h,workspace,p);return;},
        DisplayClass::Desktop=>{}
    }
    let x=w as i32;let y=h as i32;
    c.rect(0,0,w,h,p.bg);
    // A deliberate flat-color fallback: no PNG loader, GPU or fake blur.
    c.rect(0,68,w,h.saturating_sub(148),p.dark);
    c.rect(190,68,w.saturating_sub(450),h.saturating_sub(148),p.bg);
    c.rect(0,0,w,62,p.top);
    c.rect(20,15,30,30,p.accent);c.rect(25,20,20,20,p.top);
    c.label(61,23,b"INDIANOS ORBIT",p.white);
    c.rect(385,13,w.saturating_sub(700),36,p.bar);
    c.label(400,23,b"COMMAND / SEARCH [PREVIEW]",p.white);
    c.label(x-254,25,b"LOCAL-FIRST UI",p.accent);
    workspace_header(c,w,h,workspace,p);
    // Goal-first workspace navigation, selectable with b/c/l/p in R029.
    c.rect(0,63,190,h.saturating_sub(145),p.top);
    c.rect(188,63,2,h.saturating_sub(145),p.panel);
    c.label(20,86,b"YOUR WORKSPACES",p.accent);
    let entries=[Workspace::Build,Workspace::Create,
                 Workspace::Learn,Workspace::Personal];
    for (i,item) in entries.iter().enumerate(){
        let row=134+i as i32*72;
        c.rect(13,row-8,164,52,if *item==workspace{p.title}else{p.top});
        c.rect(13,row-8,if *item==workspace{4}else{1},52,p.accent);
        c.rect(25,row+4,21,21,if *item==workspace{p.accent}else{p.panel});
        c.label(57,row+8,item.label(),p.white);
    }
    c.label(22,487,b"QUICK ACCESS",p.accent);
    c.label(24,526,b"FILES",p.white);
    c.label(24,561,b"TERMINAL",p.white);
    c.label(24,596,b"SETTINGS",p.white);
    c.label(24,626,b"[PREVIEW]",p.accent);
    // Realistically label the assistant shell, don't imply an AI is running.
    let right=x-260;
    c.rect(right,77,245,h.saturating_sub(171),p.top);
    c.rect(right,77,3,h.saturating_sub(171),p.accent);
    c.label(right+18,99,b"ORBIT ASSISTANT",p.white);
    c.rect(right+18,140,210,1,p.panel);
    c.label(right+20,163,b"WORKSPACE",p.white);
    c.label(right+20,190,workspace.label(),p.accent);
    c.rect(right+15,236,215,84,p.light);
    c.label(right+26,251,b"AI ACTIONS",p.white);
    c.label(right+26,280,b"NOT CONNECTED",p.accent);
    c.rect(right+15,334,215,81,p.light);
    c.label(right+26,352,b"REVIEW FIRST",p.white);
    c.label(right+26,379,b"POLICY PREVIEW",p.accent);
    c.rect(right+15,411,215,20,p.bar);
    c.label(right+20,456,b"PRIVATE BY DESIGN",p.white);
    c.label(right+20,488,b"QEMU UI PREVIEW",p.accent);
    // Session ribbon: not a real snapshot/restore or filesystem history.
    c.rect(0,y-81,w,81,p.top);
    c.rect(0,y-82,w,2,p.accent);
    c.label(17,y-59,b"SESSION RIBBON",p.accent);
    c.label(17,y-32,b"RESTORE PLANNED",p.white);
    let entries:[(i32,i32,&[u8]);4]=[
        (220,195,b"CURRENT SESSION"),
        (427,146,b"FILES"),
        (585,159,b"TERMINAL"),
        (757,185,b"SNAPSHOT PLANNED"),
    ];
    for (i,(cx,cw,name)) in entries.iter().enumerate(){
        c.rect(*cx,y-68,*cw as u32,50,if i==0{p.title}else{p.bar});
        c.label(*cx+11,y-51,name,p.white);
    }
}
pub fn window(c:&mut impl Canvas,w:Window,focused:bool,p:Colors){
    if !w.visible||w.minimized{return;}
    let b=w.bounds;let x=b.x;let y=b.y;
    c.rect(x-2,y-2,b.width+4,b.height+4,
        if focused{p.accent}else{p.panel});
    c.rect(x,y,b.width,b.height,p.light);
    c.rect(x,y,b.width,42,if focused{p.title}else{p.bar});
    for dot in 0..3{
        c.rect(x+13+dot*16,y+16,9,9,if dot==0{p.accent}else{p.panel});
    }
    c.label(x+68,y+13,match w.app{
        App::Files=>b"FILES / PROJECTS".as_slice(),
        App::Terminal=>b"TERMINAL / SHELL".as_slice(),
        _=>b"ORBIT APP".as_slice()},p.white);
    c.rect(x+b.width as i32-30,y+15,12,12,p.accent);
    // Visible in actual QEMU frames, even when the window body is replaced
    // by the authenticated ring-3 Files or Terminal client draw commands.
    c.rect(x+8,y+42,b.width.saturating_sub(16),2,p.accent);
    match w.app{
        App::Files=>{
            c.rect(x+12,y+54,b.width.saturating_sub(24),33,p.bar);
            c.label(x+23,y+62,b"HOME / PROJECTS",p.white);
            for row in 0..4{
                // Leave the real ring-3 client paint area at y+146..y+222
                // unobstructed; static category placeholders must not
                // collide with authenticated user-content rectangles.
                let yy=y+105+if row==0{0}else{120+(row-1)*58};
                if yy+42>=y+b.height as i32{break;}
                c.rect(x+19,yy,27,24,p.title);
                c.rect(x+22,yy-4,16,7,p.accent);
                c.label(x+59,yy+6,match row{
                    0=>b"KERNEL".as_slice(),1=>b"USERLAND".as_slice(),
                    2=>b"APPS".as_slice(),_=>b"DOCUMENTS".as_slice()},p.white);
            }
        }
        App::Terminal=>{
            c.rect(x+9,y+51,b.width.saturating_sub(18),
                b.height.saturating_sub(63),p.dark);
            c.label(x+17,y+71,b"INDIANOS SHELL",p.accent);
            c.label(x+17,y+106,b"$ STATUS",p.white);
            c.label(x+17,y+139,b"QEMU PREVIEW",p.white);
            c.label(x+17,y+175,b"$ _",p.accent);
        }
        _=>{}
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn layouts_cover_phone_tablet_laptop_and_desktop_without_claiming_hardware(){
        let phone=Layout::new(390,844);
        assert_eq!(phone.class,DisplayClass::PhonePortrait);
        assert!(!phone.usable_desktop());
        assert!(phone.windows().is_none());
        for (w,h,expected) in [
            (640,480,DisplayClass::Compact),
            (800,600,DisplayClass::Compact),
            (1024,768,DisplayClass::Compact),
            (1280,800,DisplayClass::Desktop),
            (1600,900,DisplayClass::Desktop)
        ]{
            let layout=Layout::new(w,h);
            assert_eq!(layout.class,expected);
            let (files,terminal)=layout.windows().unwrap();
            for r in [files,terminal]{
                assert!(r.x>=0&&r.y>=62);
                assert!(r.width>=160&&r.height>=100);
                assert!((r.x as u32)+r.width<=w);
                assert!((r.y as u32)+r.height<=h-80);
                // The existing x86 ring-3 R029 client still uses fixed
                // pixel command bounds; smaller windows must not reject
                // valid 270px-wide Terminal and 207px-high commands.
                if r==terminal{
                    assert!(r.width>=308&&r.height>=261);
                }
            }
        }
        assert!(Layout::new(3840,2160).windows().is_none());
    }
    struct Recorded{total:u32}
    impl Canvas for Recorded{
        fn rect(&mut self,x:i32,y:i32,w:u32,h:u32,_color:u32){
            assert!(w<=4096&&h<=4096);
            assert!(x>=0&&y>=0);
            self.total+=1;
        }
        fn label(&mut self,x:i32,y:i32,text:&[u8],_color:u32){
            assert!(x>=0&&y>=0&&text.len()<=36);
            self.total+=1;
        }
    }
    #[test]fn all_three_form_factors_have_bounded_nonempty_shell_commands(){
        let color=Colors{bg:1,top:2,bar:3,title:4,unfocused:5,
                         light:6,dark:7,panel:8,white:9,accent:10};
        for (w,h) in [(390,844),(640,480),(800,600),(1024,768),
                       (1280,800),(1600,900)]{
            let mut canvas=Recorded{total:0};
            shell(&mut canvas,w,h,Workspace::Build,color);
            assert!(canvas.total>=15);
        }
    }
    #[test]fn workspace_hotkeys_are_deterministic_and_no_accidental_switch(){
        for (key,w) in [(b'b',Workspace::Build),(b'c',Workspace::Create),
                        (b'l',Workspace::Learn),(b'p',Workspace::Personal)]{
            assert_eq!(Workspace::key(key),Some(w));
        }
        assert_eq!(Workspace::key(b'q'),None);
    }
    #[test]fn orbit_goal_header_has_distinct_and_truthful_status(){
        let mut titles=[b"" as &[u8];4];
        for (i,workspace) in [Workspace::Build,Workspace::Create,
                              Workspace::Learn,Workspace::Personal]
                              .iter().enumerate(){
            titles[i]=workspace.subtitle();
            assert!(!workspace.shortcut().is_empty());
            assert!(!titles[i].is_empty());
        }
        for i in 0..4{for j in i+1..4{assert_ne!(titles[i],titles[j]);}}
    }
    #[test]fn orbit_new_status_chrome_is_bounded_on_real_desktop_sizes(){
        let p=Colors{bg:1,top:2,bar:3,title:4,unfocused:5,
                     light:6,dark:7,panel:8,white:9,accent:10};
        for (w,h) in [(640,480),(1024,768),(1280,800),(1600,900)]{
            let mut canvas=Recorded{total:0};
            workspace_header(&mut canvas,w,h,Workspace::Build,p);
            assert!(canvas.total>=3);
        }
    }

}


/// R005 optional accessible paint entrypoints. These render to the existing
/// bounded framebuffer Canvas; they do not enable a native GPU, fonts or a
/// phone build, and remain opt-in until QEMU pixel acceptance is complete.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Accessibility{
 pub high_contrast:bool,
 pub keyboard_focus:bool,
 pub scale_percent:u16,
}
impl Accessibility{
 pub const fn normal()->Self{Self{high_contrast:false,keyboard_focus:false,scale_percent:100}}
 pub fn valid(self)->bool{(100..=200).contains(&self.scale_percent)}
 pub fn colors(self,mut p:Colors)->Colors{
  if self.high_contrast{
   p.bg=0x000000;p.top=0x000000;p.bar=0x111111;
   p.title=0x202020;p.dark=0x000000;p.light=0x222222;
   p.panel=0xffffff;p.white=0xffffff;p.accent=0xffff00;
  }
  p
 }
 /// Scale target geometry without exceeding a physical frame boundary.
 pub fn focus_box(self,frame_w:u32,frame_h:u32,
  x:u32,y:u32,w:u32,h:u32)->Option<(i32,i32,u32,u32)>{
  if !self.valid()||!self.keyboard_focus||
   x>=frame_w||y>=frame_h||w==0||h==0{return None;}
  let scale=self.scale_percent as u32;
  let sw=w.checked_mul(scale)?.checked_div(100)?;
  let sh=h.checked_mul(scale)?.checked_div(100)?;
  let width=sw.min(frame_w-x);
  let height=sh.min(frame_h-y);
  if width==0||height==0{return None;}
  Some((x as i32,y as i32,width,height))
 }
}
/// Separate entrypoint for physical keyboard focus, exposing a visible
/// yellow focus boundary and high-contrast palette when explicitly selected.
pub fn shell_accessible(c:&mut impl Canvas,w:u32,h:u32,
 workspace:Workspace,p:Colors,settings:Accessibility)->bool{
 if !settings.valid()||w==0||h==0{return false;}
 let paint=settings.colors(p);
 shell(c,w,h,workspace,paint);
 if let Some((x,y,bw,bh))=settings.focus_box(w,h,12,68,w.saturating_sub(24),60){
  c.rect(x,y,bw,3,paint.accent);
  c.rect(x,y+bh as i32-3,bw,3,paint.accent);
 }
 true
}
#[cfg(test)]
mod orbit_r005_accessibility_tests{
 use super::*;
 struct Counter{rects:usize,focus_yellow:usize}
 impl Canvas for Counter{
  fn rect(&mut self,x:i32,y:i32,w:u32,h:u32,color:u32){
   assert!(x>=0&&y>=0&&w<=1600&&h<=1600);
   self.rects+=1;if color==0xffff00{self.focus_yellow+=1;}
  }
  fn label(&mut self,x:i32,y:i32,_:&[u8],_:u32){
   assert!(x>=0&&y>=0);
  }
 }
 fn colors()->Colors{Colors{bg:1,top:2,bar:3,title:4,unfocused:5,
   light:6,dark:7,panel:8,white:9,accent:10}}
 #[test]fn accessible_frame_renders_keyboard_focus_and_contrast(){
  let settings=Accessibility{high_contrast:true,keyboard_focus:true,scale_percent:150};
  let mut canvas=Counter{rects:0,focus_yellow:0};
  assert!(shell_accessible(&mut canvas,800,600,Workspace::Build,colors(),settings));
  assert!(canvas.rects>10);assert!(canvas.focus_yellow>=2);
 }
 #[test]fn focus_is_clipped_and_invalid_zoom_is_rejected(){
  let settings=Accessibility{high_contrast:false,keyboard_focus:true,scale_percent:200};
  assert_eq!(settings.focus_box(640,480,630,470,30,30),Some((630,470,10,10)));
  assert_eq!(settings.focus_box(640,480,640,0,20,20),None);
  let bad=Accessibility{scale_percent:300,..settings};
  let mut canvas=Counter{rects:0,focus_yellow:0};
  assert!(!shell_accessible(&mut canvas,800,600,Workspace::Build,colors(),bad));
  assert_eq!(canvas.rects,0);
 }
 #[test]fn ordinary_palette_stays_unchanged_without_contrast(){
  let normal=Accessibility::normal();let old=colors();let next=normal.colors(old);
  assert_eq!(next.bg,old.bg);assert_eq!(next.accent,old.accent);
 }
}
