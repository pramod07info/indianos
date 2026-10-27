//! Allocation-free rectangle intersection for an authenticated desktop surface.
pub const SCREEN_WIDTH: u32 = 1280;
pub const SCREEN_HEIGHT: u32 = 800;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rectangle { pub x: u32, pub y: u32, pub width: u32, pub height: u32 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RectError { InvalidOwner, Empty, Overflow, OutsideScreen }
fn edges(r: Rectangle) -> Result<(u32, u32), RectError> {
    if r.width == 0 || r.height == 0 { return Err(RectError::Empty); }
    let right = r.x.checked_add(r.width).ok_or(RectError::Overflow)?;
    let bottom = r.y.checked_add(r.height).ok_or(RectError::Overflow)?;
    if right > SCREEN_WIDTH || bottom > SCREEN_HEIGHT { return Err(RectError::OutsideScreen); }
    Ok((right, bottom))
}
/// None means both valid rectangles do not intersect.
pub fn clip_owned(surface: Rectangle, clip: Rectangle, owner: u32, caller: u32)
    -> Result<Option<Rectangle>, RectError>
{
    if owner == 0 || owner != caller { return Err(RectError::InvalidOwner); }
    let (sr, sb) = edges(surface)?;
    let (cr, cb) = edges(clip)?;
    let left = core::cmp::max(surface.x, clip.x);
    let top = core::cmp::max(surface.y, clip.y);
    let right = core::cmp::min(sr, cr);
    let bottom = core::cmp::min(sb, cb);
    if left >= right || top >= bottom { return Ok(None); }
    Ok(Some(Rectangle { x: left, y: top, width: right - left, height: bottom - top }))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn r(x: u32,y: u32,w: u32,h: u32) -> Rectangle {
        Rectangle { x, y, width: w, height: h }
    }
    #[test]
    fn clips_intersection_at_screen_edge() {
        assert_eq!(clip_owned(r(1270,790,10,10),r(1275,795,5,5),1,1),
            Ok(Some(r(1275,795,5,5))));
        assert_eq!(clip_owned(r(0,0,5,5),r(8,8,3,3),1,1), Ok(None));
    }
    #[test]
    fn prevents_cross_owner_access() {
        assert_eq!(clip_owned(r(0,0,5,5),r(0,0,5,5),1,2), Err(RectError::InvalidOwner));
    }
    #[test]
    fn refuses_overflow_and_invalid_dimensions() {
        assert_eq!(clip_owned(r(u32::MAX,0,2,3),r(0,0,1,1),1,1), Err(RectError::Overflow));
        assert_eq!(clip_owned(r(0,0,0,1),r(0,0,1,1),1,1), Err(RectError::Empty));
        assert_eq!(clip_owned(r(1279,0,2,1),r(0,0,1,1),1,1), Err(RectError::OutsideScreen));
    }
}

/// Move an already valid surface within the screen. A caller cannot move a
/// foreign surface or overflow signed/unsigned coordinates.
pub fn move_owned(surface: Rectangle, dx: i32, dy: i32, owner: u32, caller: u32)
    -> Result<Rectangle, RectError>
{
    if owner == 0 || owner != caller { return Err(RectError::InvalidOwner); }
    edges(surface)?;
    let x = i64::from(surface.x) + i64::from(dx);
    let y = i64::from(surface.y) + i64::from(dy);
    if x < 0 || y < 0 || x > i64::from(SCREEN_WIDTH) || y > i64::from(SCREEN_HEIGHT) {
        return Err(RectError::OutsideScreen);
    }
    let moved = Rectangle { x: x as u32, y: y as u32, ..surface };
    edges(moved)?;
    Ok(moved)
}
#[cfg(test)]
mod motion_tests {
    use super::*;
    fn window() -> Rectangle {
        Rectangle { x: 50, y: 25, width: 100, height: 50 }
    }
    #[test]
    fn moves_window_only_within_screen() {
        assert_eq!(move_owned(window(), 10, -5, 1, 1).unwrap().x, 60);
        assert_eq!(move_owned(window(), 10, -5, 1, 1).unwrap().y, 20);
    }
    #[test]
    fn rejects_outside_or_overflow_attempts() {
        assert_eq!(move_owned(window(), -51, 0, 1, 1), Err(RectError::OutsideScreen));
        assert_eq!(move_owned(window(), i32::MAX, 0, 1, 1), Err(RectError::OutsideScreen));
    }
    #[test]
    fn foreign_pid_cannot_move_surface() {
        assert_eq!(move_owned(window(), 1, 1, 4, 5), Err(RectError::InvalidOwner));
    }
}


/// Conservative CPU-desktop damage region covering two authorized
/// rectangles. This is metadata only and never writes framebuffer pixels.
pub fn union_owned(a:Rectangle,b:Rectangle,owner:u32,caller:u32)
    ->Result<Rectangle,RectError>{
    if owner==0||owner!=caller{return Err(RectError::InvalidOwner)}
    let (ar,ab)=edges(a)?;
    let (br,bb)=edges(b)?;
    let left=core::cmp::min(a.x,b.x);
    let top=core::cmp::min(a.y,b.y);
    let right=core::cmp::max(ar,br);
    let bottom=core::cmp::max(ab,bb);
    Ok(Rectangle{x:left,y:top,width:right-left,height:bottom-top})
}
#[cfg(test)]
mod damage_union_tests{
    use super::*;
    fn rect(x:u32,y:u32,w:u32,h:u32)->Rectangle{
        Rectangle{x,y,width:w,height:h}
    }
    #[test]fn merging_adjacent_and_distant_damage_stays_bounded(){
        assert_eq!(union_owned(rect(4,8,20,10),rect(24,8,6,10),5,5),
                   Ok(rect(4,8,26,10)));
        assert_eq!(union_owned(rect(1270,790,10,10),rect(0,0,2,3),5,5),
                   Ok(rect(0,0,1280,800)));
    }
    #[test]fn rejects_foreign_owner_and_zero_owner(){
        assert_eq!(union_owned(rect(0,0,1,1),rect(0,0,1,1),5,6),
                   Err(RectError::InvalidOwner));
        assert_eq!(union_owned(rect(0,0,1,1),rect(0,0,1,1),0,0),
                   Err(RectError::InvalidOwner));
    }
    #[test]fn rejects_corrupted_dimensions_and_screen_escape(){
        assert_eq!(union_owned(rect(0,0,1,1),rect(1279,0,2,1),1,1),
                   Err(RectError::OutsideScreen));
        assert_eq!(union_owned(rect(u32::MAX,0,2,2),rect(0,0,1,1),1,1),
                   Err(RectError::Overflow));
        assert_eq!(union_owned(rect(0,0,1,1),rect(0,0,0,1),1,1),
                   Err(RectError::Empty));
    }
}
