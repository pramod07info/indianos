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


/// Validate all physical extents and then enforce an explicit metadata and
/// allocation budget. This is a pure policy helper; it never touches a disk.
/// Corrupt extent metadata takes precedence over a capacity rejection.
pub fn validate_extent_budget(
    items: &[Extent],
    device_blocks: u64,
    max_extents: usize,
    max_blocks: u64,
) -> Result<u64, ExtentError> {
    let total = validate_extents(items, device_blocks)?;
    if items.len() > max_extents || total > max_blocks {
        return Err(ExtentError::Capacity);
    }
    Ok(total)
}

#[cfg(test)]
mod extent_budget_tests {
    use super::*;

    #[test]
    fn exact_extent_and_block_budget_is_allowed() {
        let items = [Extent { start: 2, blocks: 3 }, Extent { start: 9, blocks: 2 }];
        assert_eq!(validate_extent_budget(&items, 12, 2, 5), Ok(5));
        assert_eq!(validate_extent_budget(&[], 0, 0, 0), Ok(0));
    }

    #[test]
    fn extent_count_and_total_block_quota_fail_closed() {
        let items = [Extent { start: 2, blocks: 3 }, Extent { start: 9, blocks: 2 }];
        assert_eq!(validate_extent_budget(&items, 12, 1, 5), Err(ExtentError::Capacity));
        assert_eq!(validate_extent_budget(&items, 12, 2, 4), Err(ExtentError::Capacity));
        assert_eq!(validate_extent_budget(&items, 12, 0, 0), Err(ExtentError::Capacity));
    }

    #[test]
    fn corrupt_metadata_is_rejected_before_quota_decision() {
        let overlapping = [Extent { start: 1, blocks: 3 }, Extent { start: 2, blocks: 1 }];
        assert_eq!(validate_extent_budget(&overlapping, 20, 0, 0), Err(ExtentError::Overlap));
        let beyond = [Extent { start: 19, blocks: 2 }];
        assert_eq!(validate_extent_budget(&beyond, 20, 0, 0), Err(ExtentError::OutsideDevice));
        let overflow = [Extent { start: u64::MAX, blocks: 2 }];
        assert_eq!(validate_extent_budget(&overflow, u64::MAX, 0, 0), Err(ExtentError::Overflow));
    }
}


/// Compare two independently validated extent owners without allocating.
/// Returns false when any physical block is claimed by both owners.
/// Corrupt metadata in either owner is always an error, never a disjoint result.
/// Policy helper only: callers must authenticate ownership before using it.
pub fn extent_owners_disjoint(
    left: &[Extent],
    right: &[Extent],
    device_blocks: u64,
) -> Result<bool, ExtentError> {
    validate_extents(left, device_blocks)?;
    validate_extents(right, device_blocks)?;
    let (mut i, mut j) = (0usize, 0usize);
    while i < left.len() && j < right.len() {
        // Validation above proves these additions cannot overflow.
        let left_end = left[i].start + left[i].blocks;
        let right_end = right[j].start + right[j].blocks;
        if left[i].start < right_end && right[j].start < left_end {
            return Ok(false);
        }
        if left_end <= right_end {
            i += 1;
        } else {
            j += 1;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod cross_owner_tests {
    use super::*;

    #[test]
    fn rejects_partial_and_contained_cross_owner_aliases() {
        let left = [Extent { start: 2, blocks: 5 }];
        assert_eq!(extent_owners_disjoint(
            &left, &[Extent { start: 6, blocks: 3 }], 20), Ok(false));
        assert_eq!(extent_owners_disjoint(
            &left, &[Extent { start: 3, blocks: 1 }], 20), Ok(false));
    }

    #[test]
    fn accepts_adjacent_fragmented_and_empty_owners() {
        let left = [Extent { start: 1, blocks: 2 },
                    Extent { start: 8, blocks: 2 }];
        let right = [Extent { start: 3, blocks: 5 },
                     Extent { start: 10, blocks: 2 }];
        assert_eq!(extent_owners_disjoint(&left, &right, 12), Ok(true));
        assert_eq!(extent_owners_disjoint(&left, &[], 12), Ok(true));
        assert_eq!(extent_owners_disjoint(&[], &right, 12), Ok(true));
    }

    #[test]
    fn rejects_corruption_in_either_owner_before_comparing() {
        let good = [Extent { start: 0, blocks: 2 }];
        let bad = [Extent { start: 9, blocks: 2 }];
        assert_eq!(extent_owners_disjoint(&good, &bad, 10),
                   Err(ExtentError::OutsideDevice));
        assert_eq!(extent_owners_disjoint(&bad, &good, 10),
                   Err(ExtentError::OutsideDevice));
        let overlap = [Extent { start: 3, blocks: 2 },
                       Extent { start: 4, blocks: 2 }];
        assert_eq!(extent_owners_disjoint(&good, &overlap, 10),
                   Err(ExtentError::Overlap));
    }

    #[test]
    fn handles_last_device_boundary_without_wrapping() {
        let left = [Extent { start: u64::MAX - 4, blocks: 2 }];
        let adjacent = [Extent { start: u64::MAX - 2, blocks: 2 }];
        let overlapping = [Extent { start: u64::MAX - 3, blocks: 2 }];
        assert_eq!(extent_owners_disjoint(&left, &adjacent, u64::MAX), Ok(true));
        assert_eq!(extent_owners_disjoint(&left, &overlapping, u64::MAX), Ok(false));
    }
}


/// Metadata-only admission check for a candidate against existing owners.
/// Validates every owner even after a collision; no device I/O or allocation.
/// Existing owners must be checked against each other by their caller.
pub fn candidate_disjoint_from_owners(
    candidate: &[Extent], owners: &[&[Extent]], device_blocks: u64,
    max_owners: usize, max_owner_extents: usize,
    max_candidate_extents: usize, max_candidate_blocks: u64,
) -> Result<bool, ExtentError> {
    if owners.len() > max_owners { return Err(ExtentError::Capacity); }
    validate_extent_budget(candidate, device_blocks,
                           max_candidate_extents, max_candidate_blocks)?;
    let mut disjoint = true;
    for owner in owners {
        validate_extent_budget(owner, device_blocks, max_owner_extents, device_blocks)?;
        if !extent_owners_disjoint(candidate, owner, device_blocks)? {
            disjoint = false;
        }
    }
    Ok(disjoint)
}

#[cfg(test)]
mod owner_batch_tests {
    use super::*;
    const C: [Extent; 1] = [Extent { start: 4, blocks: 2 }];

    #[test]
    fn adjacent_and_empty_owners_are_disjoint() {
        let a = [Extent { start: 0, blocks: 4 }];
        let b = [Extent { start: 6, blocks: 2 }];
        assert_eq!(candidate_disjoint_from_owners(
            &C, &[&a, &b], 10, 2, 1, 1, 2), Ok(true));
        assert_eq!(candidate_disjoint_from_owners(
            &C, &[], 10, 0, 0, 1, 2), Ok(true));
    }

    #[test]
    fn alias_is_reported_but_later_corruption_is_not_masked() {
        let a = [Extent { start: 5, blocks: 1 }];
        let b = [Extent { start: 9, blocks: 2 }];
        assert_eq!(candidate_disjoint_from_owners(
            &C, &[&a], 10, 1, 1, 1, 2), Ok(false));
        assert_eq!(candidate_disjoint_from_owners(
            &C, &[&a, &b], 10, 2, 1, 1, 2),
            Err(ExtentError::OutsideDevice));
    }

    #[test]
    fn every_quota_fails_closed() {
        let a = [Extent { start: 0, blocks: 2 }];
        assert_eq!(candidate_disjoint_from_owners(
            &C, &[&a], 10, 0, 1, 1, 2), Err(ExtentError::Capacity));
        assert_eq!(candidate_disjoint_from_owners(
            &C, &[&a], 10, 1, 0, 1, 2), Err(ExtentError::Capacity));
        assert_eq!(candidate_disjoint_from_owners(
            &C, &[&a], 10, 1, 1, 0, 2), Err(ExtentError::Capacity));
        assert_eq!(candidate_disjoint_from_owners(
            &C, &[&a], 10, 1, 1, 1, 1), Err(ExtentError::Capacity));
    }

    #[test]
    fn corrupt_candidate_is_rejected_before_comparison() {
        let bad = [Extent { start: u64::MAX, blocks: 2 }];
        assert_eq!(candidate_disjoint_from_owners(
            &bad, &[], u64::MAX, 0, 0, 1, 2),
            Err(ExtentError::Overflow));
    }
}

/* Metadata-only, bounded cross-owner consistency audit. */
/// Validate all owners and quotas before testing pairwise aliases.
/// No device I/O, allocation, or ownership authentication is performed.
pub fn validate_owner_set_disjoint(
    owners: &[&[Extent]],
    device_blocks: u64,
    max_owners: usize,
    max_extents_per_owner: usize,
    max_total_blocks: u64,
) -> Result<bool, ExtentError> {
    if owners.len() > max_owners {
        return Err(ExtentError::Capacity);
    }
    let mut total = 0u64;
    for owner in owners {
        let blocks = validate_extent_budget(
            owner, device_blocks, max_extents_per_owner, device_blocks,
        )?;
        total = total.checked_add(blocks).ok_or(ExtentError::Overflow)?;
    }
    if total > max_total_blocks {
        return Err(ExtentError::Capacity);
    }

    let mut disjoint = true;
    for (i, left) in owners.iter().enumerate() {
        for right in owners.iter().skip(i + 1) {
            if !extent_owners_disjoint(left, right, device_blocks)? {
                disjoint = false;
            }
        }
    }
    Ok(disjoint)
}

#[cfg(test)]
mod owner_set_consistency_tests {
    use super::*;

    #[test]
    fn disjoint_adjacent_and_empty_owners_are_allowed() {
        let a = [Extent { start: 0, blocks: 2 }];
        let b = [Extent { start: 2, blocks: 3 }];
        assert_eq!(validate_owner_set_disjoint(&[&a, &b, &[]], 10, 3, 1, 5), Ok(true));
        assert_eq!(validate_owner_set_disjoint(&[], 0, 0, 0, 0), Ok(true));
    }

    #[test]
    fn cross_owner_alias_is_detected() {
        let a = [Extent { start: 1, blocks: 4 }];
        let b = [Extent { start: 3, blocks: 2 }];
        assert_eq!(validate_owner_set_disjoint(&[&a, &b], 10, 2, 1, 6), Ok(false));
    }

    #[test]
    fn later_corrupt_metadata_is_not_masked_by_alias() {
        let a = [Extent { start: 1, blocks: 4 }];
        let b = [Extent { start: 3, blocks: 2 }];
        let bad = [Extent { start: 9, blocks: 2 }];
        assert_eq!(validate_owner_set_disjoint(&[&a, &b, &bad], 10, 3, 1, 8),
            Err(ExtentError::OutsideDevice));
    }

    #[test]
    fn owner_count_extent_count_and_total_budget_are_bounded() {
        let a = [Extent { start: 0, blocks: 2 }, Extent { start: 3, blocks: 1 }];
        assert_eq!(validate_owner_set_disjoint(&[&a], 10, 0, 2, 3),
            Err(ExtentError::Capacity));
        assert_eq!(validate_owner_set_disjoint(&[&a], 10, 1, 1, 3),
            Err(ExtentError::Capacity));
        assert_eq!(validate_owner_set_disjoint(&[&a], 10, 1, 2, 2),
            Err(ExtentError::Capacity));
    }

    #[test]
    fn aggregate_block_count_overflow_fails_closed() {
        let all = [Extent { start: 0, blocks: u64::MAX }];
        assert_eq!(validate_owner_set_disjoint(&[&all, &all], u64::MAX, 2, 1, u64::MAX),
            Err(ExtentError::Overflow));
    }
}

#[cfg(test)]
mod owner_set_boundary_regression_tests {
    use super::*;

    #[test]
    fn rejects_zero_length_owner_even_when_other_owners_overlap() {
        let a = [Extent { start: 1, blocks: 3 }];
        let alias = [Extent { start: 2, blocks: 1 }];
        let empty_extent = [Extent { start: 5, blocks: 0 }];
        assert_eq!(validate_owner_set_disjoint(
            &[&a, &alias, &empty_extent], 10, 3, 1, 4),
            Err(ExtentError::Empty));
    }

    #[test]
    fn accepts_adjacent_owners_at_last_device_block() {
        let a = [Extent { start: u64::MAX - 3, blocks: 1 }];
        let b = [Extent { start: u64::MAX - 2, blocks: 2 }];
        assert_eq!(validate_owner_set_disjoint(
            &[&a, &b], u64::MAX, 2, 1, 3), Ok(true));
    }

    #[test]
    fn rejects_overlapping_owners_at_last_device_block() {
        let a = [Extent { start: u64::MAX - 3, blocks: 2 }];
        let b = [Extent { start: u64::MAX - 2, blocks: 2 }];
        assert_eq!(validate_owner_set_disjoint(
            &[&a, &b], u64::MAX, 2, 1, 4), Ok(false));
    }
}

#[cfg(test)]
mod span_atomicity_boundary_regression_tests {
    use super::*;

    #[test]
    fn invalid_later_extent_leaves_output_unchanged() {
        let items = [
            Extent { start: 1, blocks: 2 },
            Extent { start: 9, blocks: 2 },
        ];
        let sentinel = Extent { start: 77, blocks: 88 };
        let mut output = [sentinel; 2];
        assert_eq!(map_logical_span(&items, 10, 0, 1, &mut output),
            Err(ExtentError::OutsideDevice));
        assert_eq!(output, [sentinel; 2]);
    }

    #[test]
    fn zero_capacity_output_fails_without_mutation() {
        let items = [Extent { start: 4, blocks: 2 }];
        let mut output: [Extent; 0] = [];
        assert_eq!(map_logical_span(&items, 10, 0, 1, &mut output),
            Err(ExtentError::Capacity));
    }

    #[test]
    fn maps_last_valid_device_block() {
        let items = [Extent { start: u64::MAX - 1, blocks: 1 }];
        let mut output = [Extent { start: 0, blocks: 0 }];
        assert_eq!(map_logical_span(&items, u64::MAX, 0, 1, &mut output), Ok(1));
        assert_eq!(output, items);
    }

    #[test]
    fn past_logical_end_leaves_output_unchanged() {
        let items = [Extent { start: 3, blocks: 2 }];
        let sentinel = Extent { start: 71, blocks: 72 };
        let mut output = [sentinel];
        assert_eq!(map_logical_span(&items, 10, 1, 2, &mut output),
            Err(ExtentError::OutsideDevice));
        assert_eq!(output, [sentinel]);
    }
}

/* Agent01: admission against a globally consistent owner set. */
/// Admit a new owner only when the existing owners are mutually disjoint
/// AND the candidate does not alias any existing owner. The simpler
/// candidate_disjoint_from_owners helper does not audit existing owners.
/// This is metadata-only; authenticated ownership and device I/O remain
/// the responsibility of the caller. Corrupt candidate metadata is rejected
/// before reporting any existing-owner collision.
pub fn candidate_disjoint_from_consistent_owners(
    candidate: &[Extent],
    owners: &[&[Extent]],
    device_blocks: u64,
    max_owners: usize,
    max_owner_extents: usize,
    max_existing_blocks: u64,
    max_candidate_extents: usize,
    max_candidate_blocks: u64,
) -> Result<bool, ExtentError> {
    validate_extent_budget(
        candidate, device_blocks, max_candidate_extents, max_candidate_blocks,
    )?;
    let existing_disjoint = validate_owner_set_disjoint(
        owners, device_blocks, max_owners, max_owner_extents, max_existing_blocks,
    )?;
    let candidate_disjoint = candidate_disjoint_from_owners(
        candidate, owners, device_blocks, max_owners, max_owner_extents,
        max_candidate_extents, max_candidate_blocks,
    )?;
    Ok(existing_disjoint && candidate_disjoint)
}

#[cfg(test)]
mod consistent_admission_tests {
    use super::*;

    #[test]
    fn admits_disjoint_candidate_with_adjacent_existing_owners() {
        let a = [Extent { start: 0, blocks: 2 }];
        let b = [Extent { start: 2, blocks: 2 }];
        let c = [Extent { start: 4, blocks: 2 }];
        assert_eq!(candidate_disjoint_from_consistent_owners(
            &c, &[&a, &b], 10, 2, 1, 4, 1, 2), Ok(true));
    }

    #[test]
    fn rejects_preexisting_alias_even_when_candidate_is_disjoint() {
        let a = [Extent { start: 0, blocks: 3 }];
        let b = [Extent { start: 2, blocks: 2 }];
        let c = [Extent { start: 7, blocks: 1 }];
        assert_eq!(candidate_disjoint_from_consistent_owners(
            &c, &[&a, &b], 10, 2, 1, 5, 1, 1), Ok(false));
    }

    #[test]
    fn rejects_candidate_alias_and_corrupt_metadata() {
        let a = [Extent { start: 1, blocks: 3 }];
        let alias = [Extent { start: 3, blocks: 2 }];
        let bad = [Extent { start: 9, blocks: 2 }];
        assert_eq!(candidate_disjoint_from_consistent_owners(
            &alias, &[&a], 10, 1, 1, 3, 1, 2), Ok(false));
        assert_eq!(candidate_disjoint_from_consistent_owners(
            &bad, &[&a], 10, 1, 1, 3, 1, 2),
            Err(ExtentError::OutsideDevice));
        assert_eq!(candidate_disjoint_from_consistent_owners(
            &alias, &[&a, &bad], 10, 2, 1, 5, 1, 2),
            Err(ExtentError::OutsideDevice));
    }

    #[test]
    fn enforces_existing_aggregate_and_candidate_budgets() {
        let a = [Extent { start: 0, blocks: 3 }];
        let c = [Extent { start: 6, blocks: 2 }];
        assert_eq!(candidate_disjoint_from_consistent_owners(
            &c, &[&a], 10, 1, 1, 2, 1, 2),
            Err(ExtentError::Capacity));
        assert_eq!(candidate_disjoint_from_consistent_owners(
            &c, &[&a], 10, 1, 1, 3, 1, 1),
            Err(ExtentError::Capacity));
    }
}
