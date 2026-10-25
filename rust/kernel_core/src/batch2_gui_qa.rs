//! R036/20: deterministic screenshot-buffer comparison helper.
//! Real QEMU framebuffer screenshot capture and golden-image fixtures must
//! be supplied by an independent CI harness. No screenshot is fabricated.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Difference{pub differing_pixels:usize,pub first_x:usize,pub first_y:usize}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum CompareError{InvalidGeometry,Overflow,Truncated}
/// Compare visible pixels only: ignore row padding and report first mismatch.
pub fn compare(expected:&[u32],actual:&[u32],width:usize,height:usize,
    stride:usize)->Result<Difference,CompareError>{
    if width==0||height==0||stride<width||width>4096||height>4096{
        return Err(CompareError::InvalidGeometry);
    }
    let last_row=(height-1).checked_mul(stride).ok_or(CompareError::Overflow)?;
    let needed=last_row.checked_add(width).ok_or(CompareError::Overflow)?;
    if expected.len()<needed||actual.len()<needed{return Err(CompareError::Truncated);}
    let mut count=0;let mut first_x=0;let mut first_y=0;
    for y in 0..height{
        for x in 0..width{
            let offset=y*stride+x; // previously bounded by checked needed
            if expected[offset]!=actual[offset]{
                if count==0{first_x=x;first_y=y;}
                count+=1;
            }
        }
    }
    Ok(Difference{differing_pixels:count,first_x,first_y})
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn equal_frame_has_zero_pixel_errors(){
        assert_eq!(compare(&[1,2,3,4],&[1,2,3,4],2,2,2),
            Ok(Difference{differing_pixels:0,first_x:0,first_y:0}));
    }
    #[test]fn first_diff_location_and_row_padding_are_correct(){
        let a=[1u32,2,999,3,4,999];
        let b=[1u32,5,0,3,6,0];
        assert_eq!(compare(&a,&b,2,2,3),
            Ok(Difference{differing_pixels:2,first_x:1,first_y:0}));
    }
    #[test]fn missing_pixels_or_invalid_geometry_fail_closed(){
        assert_eq!(compare(&[0;2],&[0;2],2,2,2),Err(CompareError::Truncated));
        assert_eq!(compare(&[0;4],&[0;4],2,2,1),Err(CompareError::InvalidGeometry));
        assert_eq!(compare(&[0;4],&[0;4],0,2,2),Err(CompareError::InvalidGeometry));
    }
}
