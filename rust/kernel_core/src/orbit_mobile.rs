//! IndianOS Orbit mobile: pure input/layout contracts (not a phone port).
//! This module neither accesses multitouch hardware nor boots on AArch64.
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Point{pub x:u32,pub y:u32}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Box2D{pub x:u32,pub y:u32,pub width:u32,pub height:u32}
impl Box2D{
    pub fn contains(self,p:Point)->bool{
        self.width>0&&self.height>0&&
        (p.x as u64)>=self.x as u64&&
        (p.y as u64)>=self.y as u64&&
        (p.x as u64)<self.x as u64+self.width as u64&&
        (p.y as u64)<self.y as u64+self.height as u64
    }
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Insets{pub left:u32,pub right:u32,pub top:u32,pub bottom:u32}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum Error{Invalid,TooSmall,Busy,Missing,Denied}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum Form{Portrait,Landscape}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Layout{pub screen:Box2D,pub safe:Box2D,pub form:Form}
impl Layout{
    /// M1: phone/tablet viewport classification + display bounds.
    /// An orientation change requires a new physical mode negotiation.
    pub fn new(width:u32,height:u32,insets:Insets)->Result<Self,Error>{
        if width<320||height<320||width>4096||height>4096{
            return Err(Error::Invalid);
        }
        let usable_width=width.checked_sub(insets.left)
            .and_then(|n|n.checked_sub(insets.right)).ok_or(Error::Invalid)?;
        let usable_height=height.checked_sub(insets.top)
            .and_then(|n|n.checked_sub(insets.bottom)).ok_or(Error::Invalid)?;
        if usable_width<272||usable_height<272{return Err(Error::TooSmall);}
        Ok(Self{screen:Box2D{x:0,y:0,width,height},
            safe:Box2D{x:insets.left,y:insets.top,width:usable_width,
                       height:usable_height},
            form:if width<height{Form::Portrait}else{Form::Landscape}})
    }
    /// M2: bottom navigation stays inside the safe area, including notches
    /// and gesture exclusion zones.
    /// Screen below the system header and above bottom gestures. Never
    /// assumes fullscreen access through a device notch or keyboard.
    pub fn content_area(self,header:u32)->Result<Box2D,Error>{
        let nav=self.bottom_nav();
        let top=self.safe.y.checked_add(header).ok_or(Error::Invalid)?;
        if top>=nav.y{return Err(Error::TooSmall);}
        Ok(Box2D{x:self.safe.x,y:top,width:self.safe.width,height:nav.y-top})
    }
    /// Accessible controls from one horizontal toolbar, at least 44 pixels
    /// tall and wide each. Fail before returning any invalid hit target.
    pub fn toolbar_target(self,index:usize,count:usize)
        ->Result<Box2D,Error>{
        if count==0||count>8||index>=count{return Err(Error::Invalid);}
        let area=self.content_area(60)?;
        if area.width/(count as u32)<44{return Err(Error::TooSmall);}
        let width=area.width as u64;
        let x0=width*index as u64/count as u64;
        let x1=width*(index+1) as u64/count as u64;
        Ok(Box2D{x:area.x+x0 as u32,y:area.y,width:(x1-x0) as u32,height:52})
    }
    /// New display orientation requires a new viewport and insets. Keep
    /// this pure; no false claim of automatic accelerometer activation.
    pub fn relayout(self,width:u32,height:u32,insets:Insets)
        ->Result<Self,Error>{Self::new(width,height,insets)}
    /// Percent-based position for migration of an on-screen pointer to a
    /// new viewport. No raw pointer/coordinate is used as authority.
    pub fn normalized(self,position:Point)->Option<(u16,u16)>{
        if !self.safe.contains(position){return None;}
        let x=(position.x-self.safe.x) as u64*1000/self.safe.width as u64;
        let y=(position.y-self.safe.y) as u64*1000/self.safe.height as u64;
        Some((x as u16,y as u16))
    }
    pub fn bottom_nav(self)->Box2D{
        let height=64.min(self.safe.height);
        Box2D{x:self.safe.x,y:self.safe.y+self.safe.height-height,
              width:self.safe.width,height}
    }
    /// M3: three large touch targets. No client-provided pixels can choose
    /// a window PID or bypass the trusted permission broker.
    pub fn nav_item(self,index:usize)->Result<Box2D,Error>{
        if index>=3{return Err(Error::Invalid);}
        let bar=self.bottom_nav();
        let left=bar.width*(index as u32)/3;
        let right=bar.width*((index as u32)+1)/3;
        Ok(Box2D{x:bar.x+left,y:bar.y,width:right-left,height:bar.height})
    }
    /// M4: map a bounded raw device coordinate to the safe-area grid.
    /// This is geometry only: it is NOT a touch controller implementation.
    pub fn hit_nav(self,position:Point)->Option<u8>{
        if !self.safe.contains(position){return None;}
        for index in 0..3{
            if self.nav_item(index).ok()?.contains(position){
                return Some(index as u8);
            }
        }
        None
    }
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum Gesture{Tap(Point),SwipeLeft,SwipeRight,SwipeUp,SwipeDown}
/// M5: one bounded touch contact; rejects a second/recycled contact and
/// does not emit gestures outside the initial trusted safe region.
pub struct Contact{
    id:Option<u32>,start:Point,last:Point
}
impl Contact{
    pub const fn new()->Self{Self{id:None,start:Point{x:0,y:0},last:Point{x:0,y:0}}}
    pub fn begin(&mut self,id:u32,point:Point,viewport:Layout)->Result<(),Error>{
        if id==0||!viewport.safe.contains(point){return Err(Error::Invalid);}
        if self.id.is_some(){return Err(Error::Busy);}
        self.id=Some(id);self.start=point;self.last=point;Ok(())
    }
    pub fn move_to(&mut self,id:u32,point:Point,viewport:Layout)
        ->Result<(),Error>{
        if self.id!=Some(id){return Err(Error::Denied);}
        if !viewport.safe.contains(point){return Err(Error::Invalid);}
        self.last=point;Ok(())
    }
    /// On screen lock/rotation, atomically discard the touch contact; an
    /// old contact ID can no longer complete a gesture.
    pub fn reset(&mut self){self.id=None;}
    pub fn active(&self)->bool{self.id.is_some()}
    pub fn cancel(&mut self,id:u32)->Result<(),Error>{
        if self.id!=Some(id){return Err(Error::Denied);}
        self.id=None;Ok(())
    }
    pub fn end(&mut self,id:u32,point:Point,viewport:Layout)
        ->Result<Gesture,Error>{
        if self.id!=Some(id){return Err(Error::Denied);}
        if !viewport.safe.contains(point){return Err(Error::Invalid);}
        let begin=self.start;
        self.id=None;
        let dx=point.x as i64-begin.x as i64;
        let dy=point.y as i64-begin.y as i64;
        let (ax,ay)=(dx.unsigned_abs(),dy.unsigned_abs());
        if ax<32&&ay<32{return Ok(Gesture::Tap(point));}
        // Avoid accidental diagonals: the dominant movement must exceed
        // the secondary movement by at least 50%.
        if ax>=(ay*3)/2&&ax>=48{
            return Ok(if dx>0{Gesture::SwipeRight}else{Gesture::SwipeLeft});
        }
        if ay>=(ax*3)/2&&ay>=48{
            return Ok(if dy>0{Gesture::SwipeDown}else{Gesture::SwipeUp});
        }
        Err(Error::Invalid)
    }
}
#[cfg(test)]mod tests{
    use super::*;
    fn insets()->Insets{Insets{left:0,right:0,top:24,bottom:20}}
    #[test]fn m1_realistic_phone_tablet_and_landscape_geometry(){
        for (w,h,form) in [(360,800,Form::Portrait),
                           (412,915,Form::Portrait),
                           (1024,768,Form::Landscape)]{
            let layout=Layout::new(w,h,insets()).unwrap();
            assert_eq!(layout.form,form);
            assert!(layout.safe.width<=w&&layout.safe.height<=h);
        }
        assert_eq!(Layout::new(390,844,Insets{left:220,right:220,
                      top:0,bottom:0}),Err(Error::Invalid));
    }
    #[test]fn m2_safe_bottom_nav_never_overlaps_system_insets(){
        let l=Layout::new(390,844,insets()).unwrap();
        let bar=l.bottom_nav();
        assert_eq!(bar.y+bar.height,824);
        assert_eq!(bar.height,64);
        assert!(bar.y>=l.safe.y);
    }
    #[test]fn m3_three_tappable_targets_have_accessible_bounds(){
        let l=Layout::new(360,800,insets()).unwrap();
        let mut previous=0;
        for i in 0..3{
            let b=l.nav_item(i).unwrap();
            assert!(b.width>=44&&b.height>=44);
            if i>0{assert_eq!(b.x,previous);}
            previous=b.x+b.width;
        }
        assert_eq!(previous,l.safe.x+l.safe.width);
        assert_eq!(l.nav_item(3),Err(Error::Invalid));
    }
    #[test]fn m4_hit_testing_clamps_to_safe_zone_and_exact_target(){
        let l=Layout::new(390,844,insets()).unwrap();
        assert_eq!(l.hit_nav(Point{x:0,y:0}),None);
        for i in 0..3{
            let target=l.nav_item(i).unwrap();
            assert_eq!(l.hit_nav(Point{x:target.x+10,y:target.y+10}),
                       Some(i as u8));
        }
    }
    #[test]fn m5_touch_identity_and_gestures_are_fail_closed(){
        let l=Layout::new(390,844,insets()).unwrap();
        let mut t=Contact::new();
        t.begin(7,Point{x:200,y:300},l).unwrap();
        assert_eq!(t.begin(8,Point{x:201,y:300},l),Err(Error::Busy));
        assert_eq!(t.move_to(8,Point{x:210,y:310},l),Err(Error::Denied));
        t.move_to(7,Point{x:100,y:302},l).unwrap();
        assert_eq!(t.end(7,Point{x:90,y:301},l),Ok(Gesture::SwipeLeft));
        assert_eq!(t.end(7,Point{x:90,y:301},l),Err(Error::Denied));
        t.begin(9,Point{x:100,y:100},l).unwrap();
        assert_eq!(t.end(9,Point{x:100,y:180},l),Ok(Gesture::SwipeDown));
        t.begin(10,Point{x:100,y:100},l).unwrap();
        t.cancel(10).unwrap();
        assert_eq!(t.end(10,Point{x:100,y:100},l),Err(Error::Denied));
    }
    #[test]fn orbit_mobile_rotation_and_content_area_are_safe(){
        let old=Layout::new(390,844,insets()).unwrap();
        let content=old.content_area(60).unwrap();
        assert_eq!(content.y,84);
        assert!(content.y+content.height<=old.bottom_nav().y);
        let changed=old.relayout(844,390,Insets{left:12,right:12,
                                     top:0,bottom:12}).unwrap();
        assert_eq!(changed.form,Form::Landscape);
        assert!(changed.content_area(60).is_ok());
        assert_eq!(old.normalized(Point{x:0,y:0}),None);
        assert_eq!(old.normalized(Point{x:195,y:424}),Some((500,500)));
    }
    #[test]fn orbit_mobile_accessible_targets_and_rotation_invalidate_contacts(){
        let layout=Layout::new(412,915,insets()).unwrap();
        for index in 0..5{
            let b=layout.toolbar_target(index,5).unwrap();
            assert!(b.width>=44&&b.height>=44);
            assert!(layout.safe.contains(Point{x:b.x,y:b.y}));
        }
        assert_eq!(layout.toolbar_target(8,9),Err(Error::Invalid));
        let mut c=Contact::new();
        c.begin(8,Point{x:120,y:120},layout).unwrap();
        assert!(c.active());
        c.reset();
        assert!(!c.active());
        assert_eq!(c.end(8,Point{x:150,y:120},layout),Err(Error::Denied));
    }

}
