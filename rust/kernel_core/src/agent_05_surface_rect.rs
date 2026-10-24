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
