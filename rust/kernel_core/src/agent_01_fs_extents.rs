//! Bounded, allocation-free block extent validation (policy helper only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent { pub start: u64, pub blocks: u64 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtentError { Empty, Overflow, OutsideDevice, Overlap, Unsorted, Capacity }

/// Validate ordered, nonoverlapping [start, start + blocks) ranges.
pub fn validate_extents(items: &[Extent], device_blocks: u64) -> Result<u64, ExtentError> {
    let mut end = 0u64;
    let mut previous_start = 0u64;
    let mut total = 0u64;
    for (index, item) in items.iter().enumerate() {
        if item.blocks == 0 { return Err(ExtentError::Empty); }
        if index != 0 && item.start < previous_start { return Err(ExtentError::Unsorted); }
        if index != 0 && item.start < end { return Err(ExtentError::Overlap); }
        let next_end = item.start.checked_add(item.blocks).ok_or(ExtentError::Overflow)?;
        if next_end > device_blocks { return Err(ExtentError::OutsideDevice); }
        total = total.checked_add(item.blocks).ok_or(ExtentError::Overflow)?;
        previous_start = item.start;
        end = next_end;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_adjacent_and_empty_ranges() {
        assert_eq!(validate_extents(&[], 100), Ok(0));
        assert_eq!(validate_extents(&[Extent { start: 0, blocks: 4 },
            Extent { start: 4, blocks: 6 }], 10), Ok(10));
    }
    #[test]
    fn rejects_overlap_unsorted_and_zero() {
        assert_eq!(validate_extents(&[Extent { start: 1, blocks: 3 },
            Extent { start: 2, blocks: 2 }], 10), Err(ExtentError::Overlap));
        assert_eq!(validate_extents(&[Extent { start: 9, blocks: 1 },
            Extent { start: 2, blocks: 1 }], 10), Err(ExtentError::Unsorted));
        assert_eq!(validate_extents(&[Extent { start: 0, blocks: 0 }], 10), Err(ExtentError::Empty));
    }
    #[test]
    fn rejects_overflow_and_device_boundary() {
        assert_eq!(validate_extents(&[Extent { start: u64::MAX, blocks: 2 }], u64::MAX),
            Err(ExtentError::Overflow));
        assert_eq!(validate_extents(&[Extent { start: 10, blocks: 1 }], 10),
            Err(ExtentError::OutsideDevice));
    }
}

/// Map a logical file block to its physical device block, after validating every
/// extent. Out-of-range file offsets are distinguishable from corrupt metadata.
pub fn logical_to_physical(extents: &[Extent], device_blocks: u64, logical: u64)
    -> Result<Option<u64>, ExtentError>
{
    let total = validate_extents(extents, device_blocks)?;
    if logical >= total { return Ok(None); }
    let mut cursor = 0u64;
    for extent in extents {
        let end = cursor.checked_add(extent.blocks).ok_or(ExtentError::Overflow)?;
        if logical < end {
            return Ok(Some(extent.start + (logical - cursor)));
        }
        cursor = end;
    }
    Ok(None)
}
#[cfg(test)]
mod mapping_tests {
    use super::*;
    #[test]
    fn maps_discontiguous_file_blocks() {
        let exts = [Extent { start: 2, blocks: 2 }, Extent { start: 10, blocks: 3 }];
        assert_eq!(logical_to_physical(&exts, 20, 0), Ok(Some(2)));
        assert_eq!(logical_to_physical(&exts, 20, 1), Ok(Some(3)));
        assert_eq!(logical_to_physical(&exts, 20, 2), Ok(Some(10)));
        assert_eq!(logical_to_physical(&exts, 20, 4), Ok(Some(12)));
    }
    #[test]
    fn rejects_past_end_without_touching_disk() {
        assert_eq!(logical_to_physical(&[Extent { start: 9, blocks: 1 }], 10, 1), Ok(None));
        assert_eq!(logical_to_physical(&[], 10, 0), Ok(None));
    }
    #[test]
    fn validates_all_extents_before_returning_mapping() {
        let corrupt = [Extent { start: 0, blocks: 1 }, Extent { start: 8, blocks: 9 }];
        assert_eq!(logical_to_physical(&corrupt, 10, 0), Err(ExtentError::OutsideDevice));
    }
}


/// Atomically convert a logical block span into physical extent runs.
/// This purely validates metadata: no device reads or writes occur.
pub fn map_logical_span(items:&[Extent],device_blocks:u64,start:u64,count:u64,
                      output:&mut [Extent])->Result<usize,ExtentError>{
    if count==0{return Err(ExtentError::Empty)}
    let total=validate_extents(items,device_blocks)?;
    let limit=start.checked_add(count).ok_or(ExtentError::Overflow)?;
    if limit>total{return Err(ExtentError::OutsideDevice)}
    let (mut cursor,mut needed)=(0u64,0usize);
    for e in items{
        let next=cursor.checked_add(e.blocks).ok_or(ExtentError::Overflow)?;
        if cursor<limit && start<next {
            needed=needed.checked_add(1).ok_or(ExtentError::Overflow)?;
        }
        cursor=next;
    }
    if needed>output.len(){return Err(ExtentError::Capacity)}
    cursor=0;
    let mut written=0;
    for e in items {
        let next=cursor+e.blocks;
        if cursor<limit && start<next{
            let from=core::cmp::max(start,cursor);
            let to=core::cmp::min(limit,next);
            output[written]=Extent{start:e.start+(from-cursor),blocks:to-from};
            written+=1;
        }
        cursor=next;
    }
    Ok(written)
}
#[cfg(test)]
mod span_tests {
    use super::*;
    #[test]fn fragmented_mapping_preserves_each_run(){
        let a=[Extent{start:3,blocks:2},Extent{start:20,blocks:3}];
        let mut out=[Extent{start:0,blocks:0};2];
        assert_eq!(map_logical_span(&a,40,1,3,&mut out),Ok(2));
        assert_eq!(out,[Extent{start:4,blocks:1},Extent{start:20,blocks:2}]);
    }
    #[test]fn small_output_never_partially_changes_data(){
        let a=[Extent{start:1,blocks:2},Extent{start:9,blocks:2}];
        let mark=Extent{start:55,blocks:55};
        let mut out=[mark;1];
        assert_eq!(map_logical_span(&a,20,0,4,&mut out),Err(ExtentError::Capacity));
        assert_eq!(out,[mark]);
    }
    #[test]fn invalid_metadata_and_overflow_fail_closed(){
        let mut out=[Extent{start:0,blocks:0};2];
        assert_eq!(map_logical_span(&[Extent{start:1,blocks:10}],10,0,1,&mut out),
                   Err(ExtentError::OutsideDevice));
        assert_eq!(map_logical_span(&[Extent{start:0,blocks:2}],10,u64::MAX,2,&mut out),
                   Err(ExtentError::Overflow));
        assert_eq!(map_logical_span(&[],10,0,0,&mut out),Err(ExtentError::Empty));
    }
}


/// Validate that all requested logical blocks are mapped before data access.
pub fn contains_logical_span(items:&[Extent],blocks:u64,start:u64,count:u64)
 ->Result<bool,ExtentError>{
 let total=validate_extents(items,blocks)?;
 let end=start.checked_add(count).ok_or(ExtentError::Overflow)?;
 Ok(count>0&&start<total&&end<=total)
}
#[cfg(test)]mod prefix_guard_tests{
 use super::*;
 #[test]fn accepts_complete_range(){assert_eq!(contains_logical_span(&[Extent{start:2,blocks:5}],20,1,4),Ok(true));}
 #[test]fn refuses_outside_or_empty(){assert_eq!(contains_logical_span(&[Extent{start:2,blocks:5}],20,5,1),Ok(false));assert_eq!(contains_logical_span(&[],20,0,0),Ok(false));}
 #[test]fn rejects_overflow_and_bad_extent(){assert_eq!(contains_logical_span(&[Extent{start:1,blocks:3}],20,u64::MAX,2),Err(ExtentError::Overflow));assert_eq!(contains_logical_span(&[Extent{start:19,blocks:2}],20,0,1),Err(ExtentError::OutsideDevice));}
}
