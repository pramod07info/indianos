//! Retained app pixels. Clients rasterize; this backend only validates rows,
//! stages them privately and atomically publishes a complete transaction.
const PAGE:usize=4096;
pub(super) const MAX_WIDTH:usize=crate::pixel_protocol::MAX_WIDTH as usize;
pub(super) const MAX_HEIGHT:usize=crate::pixel_protocol::MAX_HEIGHT as usize;
const TILE_PIXELS:usize=crate::pixel_protocol::TILE_PIXELS as usize;
const PIXELS:usize=MAX_WIDTH*MAX_HEIGHT;
const MASK_WORDS:usize=PIXELS/64;
pub(super) const PAYLOAD_PAGES:u64=((PIXELS*4+MASK_WORDS*8)*2/PAGE) as u64;
const _:()=assert!((PIXELS*4+MASK_WORDS*8)*2%PAGE==0);
#[repr(C)]
struct Plane{pixels:[u32;PIXELS],coverage:[u64;MASK_WORDS]}
#[cfg(not(test))]
impl Plane{const fn new()->Self{Self{pixels:[0;PIXELS],coverage:[0;MASK_WORDS]}}}
#[repr(C,align(4096))]
struct GuardedPixels{lower:[u8;PAGE],planes:[Plane;2],upper:[u8;PAGE]}
#[cfg(not(test))]
impl GuardedPixels{const fn new()->Self{Self{lower:[0;PAGE],planes:[Plane::new(),Plane::new()],upper:[0;PAGE]}}}
#[derive(Clone,Copy)]
struct Transaction{token:u64,rect:[u16;4],cursor:usize,revision:u64}
#[derive(Clone,Copy)]
struct State{width:usize,height:usize,active:usize,next:u64,metadata_revision:u64,published_revision:u64,published_generation:u64,transaction:Option<Transaction>}
impl State{const fn new()->Self{Self{width:0,height:0,active:0,next:1,metadata_revision:0,published_revision:0,published_generation:0,transaction:None}}}
#[cfg(not(test))]
static mut PLANES:[GuardedPixels;2]=[GuardedPixels::new(),GuardedPixels::new()];
#[cfg(not(test))]
static mut STATES:[State;2]=[State::new();2];
#[cfg(test)]
struct TestPlane{pixels:std::vec::Vec<u32>,coverage:std::vec::Vec<u64>}
#[cfg(test)]
impl TestPlane{const fn new()->Self{Self{pixels:std::vec::Vec::new(),coverage:std::vec::Vec::new()}}}
#[cfg(test)]
struct TestSurface{planes:[TestPlane;2],state:State}
#[cfg(test)]
impl TestSurface{const fn new()->Self{Self{planes:[TestPlane::new(),TestPlane::new()],state:State::new()}}}
/// A production handle to boot-pinned backing. Host fixtures own independent
/// heap planes, never megabyte stack values or a shared test singleton.
pub(super) struct PixelCache{
    #[cfg(test)] surfaces:core::cell::RefCell<[TestSurface;2]>,
}
impl PixelCache{
    pub(super) const fn new()->Self{Self{
        #[cfg(test)] surfaces:core::cell::RefCell::new([TestSurface::new(),TestSurface::new()]),
    }}
    fn with<R>(&self,slot:usize,operation:impl FnOnce(&mut[u32],&mut[u64],&mut[u32],&mut[u64],&mut State)->R)->Option<R>{
        if slot>=2{return None;}
        #[cfg(not(test))]
        unsafe{
            // Single BSP, exclusive graphics syscall context. The guarded
            // allocation is never assigned/copied after its guards unmap.
            let [a,b]=&mut (*core::ptr::addr_of_mut!(PLANES))[slot].planes;
            let state=&mut (*core::ptr::addr_of_mut!(STATES))[slot];
            Some(operation(&mut a.pixels,&mut a.coverage,&mut b.pixels,&mut b.coverage,state))
        }
        #[cfg(test)]
        {
            let mut surfaces=self.surfaces.borrow_mut();let surface=&mut surfaces[slot];
            for plane in &mut surface.planes{
                if plane.pixels.is_empty(){plane.pixels.resize(PIXELS,0);plane.coverage.resize(MASK_WORDS,0);}
            }
            let [a,b]=&mut surface.planes;
            Some(operation(&mut a.pixels,&mut a.coverage,&mut b.pixels,&mut b.coverage,&mut surface.state))
        }
    }
    pub(super) fn clear_slot(&self,slot:usize){
        self.with(slot,|a,am,b,bm,state|{
            a.fill(0);am.fill(0);b.fill(0);bm.fill(0);
            let next=state.next;*state=State::new();state.next=next;
        });
    }
    pub(super) fn clear(&self){for slot in 0..2{self.clear_slot(slot);}}
    pub(super) fn metadata_changed(&self,slot:usize)->bool{
        self.with(slot,|_,_,_,_,state|{
            let Some(next)=state.metadata_revision.checked_add(1)else{return false;};
            state.metadata_revision=next;true
        }).unwrap_or(false)
    }
    pub(super) fn ready(&self,slot:usize)->bool{
        self.with(slot,|_,_,_,_,state|state.width!=0&&state.height!=0&&
            state.metadata_revision!=0&&state.metadata_revision==state.published_revision&&state.transaction.is_none()).unwrap_or(false)
    }
    pub(super) fn begin(&self,slot:usize,width:usize,height:usize,rect:[u16;4])->Option<u64>{
        if width==0||height==0||width>MAX_WIDTH||height>MAX_HEIGHT||rect[2]==0||rect[3]==0||
            rect[0] as usize+rect[2] as usize>width||rect[1] as usize+rect[3] as usize>height{return None;}
        self.with(slot,|a,am,b,bm,state|{
            if state.transaction.is_some()||(state.width!=0&&(state.width!=width||state.height!=height)){return None;}
            let next=state.next.checked_add(1)?;let token=state.next;
            if state.active==0{b.copy_from_slice(a);bm.copy_from_slice(am);}
            else{a.copy_from_slice(b);am.copy_from_slice(bm);}
            state.width=width;state.height=height;state.next=next;
            state.transaction=Some(Transaction{token,rect,cursor:0,revision:state.metadata_revision});Some(token)
        })?
    }
    pub(super) fn tile(&self,slot:usize,token:u64,x:u16,y:u16,pixels:&[u32],rgb:fn(u8,u8,u8)->Option<u32>)->bool{
        self.with(slot,|a,am,b,bm,state|{
            let Some(transaction)=state.transaction else{return false;};
            if token==0||transaction.token!=token{return false;}
            let [left,top,width,height]=transaction.rect;
            let width=width as usize;let count=width*height as usize;
            if transaction.cursor>=count{return false;}
            let column=transaction.cursor%width;
            let expected=(width-column).min(TILE_PIXELS);
            if x as usize!=left as usize+column||y as usize!=top as usize+transaction.cursor/width||pixels.len()!=expected{return false;}
            // Validate the ENTIRE row before changing staging data or cursor.
            let mut encoded=[0u32;TILE_PIXELS];
            for (index,&value) in pixels.iter().enumerate(){
                if value==0{continue;}
                if value>>24!=0xff{return false;}
                let Some(color)=rgb((value>>16) as u8,(value>>8) as u8,value as u8)else{return false;};
                encoded[index]=color;
            }
            let (data,mask)=if state.active==0{(b,bm)}else{(a,am)};
            let start=y as usize*state.width+x as usize;
            for (index,&value) in pixels.iter().enumerate(){
                let at=start+index;data[at]=encoded[index];
                if value==0{mask[at/64]&=!(1u64<<(at%64));}else{mask[at/64]|=1u64<<(at%64);}
            }
            state.transaction=Some(Transaction{cursor:transaction.cursor+expected,..transaction});true
        }).unwrap_or(false)
    }
    pub(super) fn commit(&self,slot:usize,token:u64)->bool{
        self.with(slot,|_,_,_,_,state|{
            let Some(transaction)=state.transaction else{return false;};
            if token==0||transaction.token!=token||transaction.revision!=state.metadata_revision||
                transaction.cursor!=transaction.rect[2] as usize*transaction.rect[3] as usize{return false;}
            state.active=1-state.active;
            state.published_revision=transaction.revision;
            state.published_generation=transaction.token;
            state.transaction=None;true
        }).unwrap_or(false)
    }
    pub(super) fn abort(&self,slot:usize,token:u64)->bool{
        self.with(slot,|a,am,b,bm,state|{
            let Some(transaction)=state.transaction else{return false;};
            if token==0||transaction.token!=token{return false;}
            if state.active==0{b.fill(0);bm.fill(0);}else{a.fill(0);am.fill(0);}
            state.transaction=None;true
        }).unwrap_or(false)
    }
    pub(super) fn snapshot(
        &self, slot: usize, request: crate::compositor_pixels::Request,
    ) -> Option<[u8; crate::compositor_pixels::REPLY_BYTES]> {
        self.snapshot_sized::<{crate::compositor_pixels::REPLY_BYTES}>(slot,request)
    }
    pub(super) fn snapshot_sized<const BYTES:usize>(
        &self, slot:usize, request:crate::compositor_pixels::Request,
    )->Option<[u8;BYTES]>{
        use crate::compositor_pixels::{HEADER_BYTES,OPAQUE};
        if BYTES<HEADER_BYTES{return None;}
        self.with(slot, |a, am, b, bm, state| {
            let generation = state.published_generation;
            if generation == 0 {
                return None;
            }
            let x = usize::from(request.x);
            let y = usize::from(request.y);
            let count = usize::from(request.count);
            if count != 0 && (count > (BYTES-HEADER_BYTES)/8 || request.generation != generation ||
                              y >= state.height || x + count > state.width) {
                return None;
            }
            let mut out = [0; BYTES];
            out[..8].copy_from_slice(&generation.to_le_bytes());
            out[8..10].copy_from_slice(&(state.width as u16).to_le_bytes());
            out[10..12].copy_from_slice(&(state.height as u16).to_le_bytes());
            if count != 0 {
                let (pixels, mask) = if state.active == 0 { (a, am) } else { (b, bm) };
                let row = y * state.width + x;
                for offset in 0..count {
                    let at = row + offset;
                    if mask[at / 64] & (1 << (at % 64)) != 0 {
                        let start = HEADER_BYTES + 8 * offset;
                        let pixel = OPAQUE | u64::from(pixels[at]);
                        out[start..start + 8].copy_from_slice(&pixel.to_le_bytes());
                    }
                }
            }
            Some(out)
        })?
    }
    pub(super) fn blit(&self,slot:usize,width:usize,height:usize,mut pixel:impl FnMut(usize,usize,u32))->bool{
        if width==0||height==0||width>MAX_WIDTH||height>MAX_HEIGHT{return false;}
        self.with(slot,|a,am,b,bm,state|{
            if state.width==0{return true;}
            if state.width!=width||state.height!=height{return false;}
            let (data,mask)=if state.active==0{(a,am)}else{(b,bm)};
            for (index,&color) in data[..width*height].iter().enumerate(){
                if mask[index/64]&(1u64<<(index%64))!=0{pixel(index%width,index/width,color);}
            }
            true
        }).unwrap_or(false)
    }
}
#[cfg(not(test))]
pub(super) fn guard_layout()->[(u64,u64,u64,u64);2]{
    core::array::from_fn(|slot|unsafe{
        let plane=&(*core::ptr::addr_of!(PLANES))[slot];
        (core::ptr::addr_of!(plane.lower) as u64,core::ptr::addr_of!(plane.planes) as u64,
         core::ptr::addr_of!(plane.upper) as u64,core::ptr::addr_of!(plane.upper) as u64)
    })
}
#[cfg(test)]
pub(super) fn guard_layout()->[(u64,u64,u64,u64);2]{[(0,0,0,0);2]}

#[cfg(test)]
mod tests{
    use super::*;
    fn rgb(r:u8,g:u8,b:u8)->Option<u32>{Some((r as u32)<<16|(g as u32)<<8|b as u32)}
    fn visible(cache:&PixelCache,slot:usize)->[u32;16]{let mut out=[9;16];assert!(cache.blit(slot,4,4,|x,y,p|out[y*4+x]=p));out}
    #[test]fn guarded_double_backing_has_exact_full_page_payload_geometry(){
        assert_eq!(core::mem::offset_of!(GuardedPixels,planes),PAGE);
        assert_eq!(core::mem::offset_of!(GuardedPixels,upper),PAGE+PAYLOAD_PAGES as usize*PAGE);
        assert_eq!(core::mem::size_of::<GuardedPixels>(),(PAYLOAD_PAGES as usize+2)*PAGE);
        assert_eq!(PAYLOAD_PAGES,1188);
    }
    #[test]fn partial_and_failed_uploads_cannot_replace_committed_pixels(){
        let cache=PixelCache::new();let token=cache.begin(0,4,4,[1,1,2,2]).unwrap();
        assert!(cache.tile(0,token,1,1,&[0xff123456;2],rgb));
        assert!(!cache.commit(0,token));assert_eq!(visible(&cache,0),[9;16]);
        assert!(!cache.tile(0,token,1,2,&[0x01123456;2],rgb));
        assert!(!cache.tile(0,token,2,2,&[0xff123456;2],rgb));
        assert!(cache.tile(0,token,1,2,&[0xff123456;2],rgb));assert!(cache.commit(0,token));
        assert_eq!(visible(&cache,0)[5],0x123456);assert!(!cache.commit(0,token));
        let next=cache.begin(0,4,4,[1,1,2,2]).unwrap();assert_ne!(next,token);
        assert!(!cache.tile(0,token,1,1,&[0;2],rgb));assert!(cache.tile(0,next,1,1,&[0;2],rgb));
        assert_eq!(visible(&cache,0)[5],0x123456);assert!(cache.abort(0,next));
        assert_eq!(visible(&cache,0)[5],0x123456);
    }
    #[test]fn transparent_replacement_isolated_planes_and_epoch_reset(){
        let cache=PixelCache::new();
        let first=cache.begin(0,4,4,[0,0,1,1]).unwrap();assert!(cache.tile(0,first,0,0,&[0xff112233],rgb));assert!(cache.commit(0,first));
        let peer=cache.begin(1,4,4,[0,0,1,1]).unwrap();assert!(cache.tile(1,peer,0,0,&[0xff445566],rgb));assert!(cache.commit(1,peer));
        let replace=cache.begin(0,4,4,[0,0,1,1]).unwrap();assert!(cache.tile(0,replace,0,0,&[0],rgb));assert!(cache.commit(0,replace));
        assert_eq!(visible(&cache,0)[0],9);assert_eq!(visible(&cache,1)[0],0x445566);
        let stale=cache.begin(0,4,4,[0,0,1,1]).unwrap();cache.clear_slot(0);
        let fresh=cache.begin(0,4,4,[0,0,1,1]).unwrap();assert!(fresh>stale);assert!(!cache.abort(0,stale));
        assert!(cache.abort(0,fresh));assert_eq!(visible(&cache,1)[0],0x445566);
        assert!(cache.begin(2,4,4,[0,0,1,1]).is_none());assert!(cache.begin(0,4,4,[3,3,2,2]).is_none());
        cache.clear();assert_eq!(visible(&cache,1),[9;16]);
    }
    #[test]
    fn compositor_reads_only_committed_generation_and_distinguishes_opaque_black() {
        use crate::compositor_pixels::{Request, HEADER_BYTES, OPAQUE};
        let cache = PixelCache::new();
        let query = Request { window: 1, x: 0, y: 0, count: 0, generation: 0 };
        assert!(cache.snapshot(0, query).is_none());
        let generation = cache.begin(0, 4, 4, [0, 0, 2, 1]).unwrap();
        assert!(cache.tile(0, generation, 0, 0, &[0, 0xff000000], rgb));
        assert!(cache.snapshot(0, query).is_none());
        assert!(cache.commit(0, generation));

        let read = Request { count: 2, generation, ..query };
        let before = cache.snapshot(0, read).unwrap();
        let covered = HEADER_BYTES + 8;
        assert_eq!(&before[HEADER_BYTES..covered], &[0; 8]);
        assert_eq!(&before[covered..covered + 8], &OPAQUE.to_le_bytes());
        assert!(before[covered + 8..].iter().all(|&byte| byte == 0));
        assert!(cache.snapshot(1, query).is_none());
        assert!(cache.snapshot(0, Request { x: 3, ..read }).is_none());

        let next = cache.begin(0, 4, 4, [0, 0, 2, 1]).unwrap();
        assert!(cache.tile(0, next, 0, 0, &[0xff112233; 2], rgb));
        assert_eq!(cache.snapshot(0, read), Some(before));
        assert!(cache.commit(0, next));
        assert!(cache.snapshot(0, read).is_none());
        assert!(cache.snapshot(0, Request { generation: next, ..read }).is_some());
        cache.clear_slot(0);
        assert!(cache.snapshot(0, query).is_none());
    }
    #[test]fn readiness_requires_a_complete_current_revision_and_no_pending_upload(){
        let cache=PixelCache::new();assert!(!cache.ready(0));assert!(cache.metadata_changed(0));
        let first=cache.begin(0,4,4,[0,0,1,1]).unwrap();assert!(!cache.ready(0));
        assert!(cache.tile(0,first,0,0,&[0xff112233],rgb));assert!(cache.commit(0,first));assert!(cache.ready(0));
        let pending=cache.begin(0,4,4,[0,0,1,1]).unwrap();assert!(!cache.ready(0));
        assert!(cache.tile(0,pending,0,0,&[0xff445566],rgb));assert!(cache.metadata_changed(0));
        assert!(!cache.commit(0,pending));assert!(cache.abort(0,pending));assert!(!cache.ready(0));
        let fresh=cache.begin(0,4,4,[0,0,1,1]).unwrap();assert!(cache.tile(0,fresh,0,0,&[0xff778899],rgb));
        assert!(cache.commit(0,fresh));assert!(cache.ready(0));cache.clear_slot(0);assert!(!cache.ready(0));
    }
}
