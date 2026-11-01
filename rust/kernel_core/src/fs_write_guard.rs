//! S0 defense-in-depth for the existing R031 per-process file-handle write ABI.
//!
//! This is NOT a complete ransomware detector or a global VFS ACL.
//! It defends the current FileHandles::open/write syscall surface only.
//! Direct privileged VFS/ATA callers remain separate S1 migration work.
//!
//! All checks are allocation-free and fail closed. A process cannot
//! evade the monotonic write budget simply by closing/reopening a handle.
//! Budget resets if the FileHandles kernel service itself is recreated;
//! this is deliberately NOT claimed to be durable ransomware protection.
use crate::vfs_path::normalize_absolute;

pub const MAX_WRITE_OPERATIONS:u32=32;
pub const MAX_WRITE_BYTES:u32=4096;
const PROTECTED:&[&[u8]]=&[
 b"/boot",b"/system",b"/security",b"/recovery",
 b"/snapshots",b"/keys",b"/firmware",
];
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum Error{InvalidPath,Protected,RateLimited}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Statistics{
 pub completed_writes:u32,pub changed_bytes:u32,pub denials:u32,
}
/// Kernel-owned service-scoped counter, never stored in a user process.
/// Caller must serialize access; existing file handle table is single-core.
pub struct WriteGuard{
 completed:u32,bytes:u32,denials:u32,
 ops_limit:u32,bytes_limit:u32,locked:bool,
}
impl WriteGuard{
 pub const fn new()->Self{Self{completed:0,bytes:0,denials:0,
 ops_limit:MAX_WRITE_OPERATIONS,bytes_limit:MAX_WRITE_BYTES,locked:false}}
 pub fn statistics(&self)->Statistics{Statistics{
  completed_writes:self.completed,changed_bytes:self.bytes,
  denials:self.denials
 }}
 pub fn deny(&mut self){
  self.denials=self.denials.saturating_add(1);
 }
 /// Protect exact trusted namespaces and all descendants, but avoid
 /// matching innocent names such as /systematic or /bootcamp.
 pub fn allow_path(path:&str)->Result<(),Error>{
  let mut canonical=[0u8;64];
  let n=normalize_absolute(path,&mut canonical).map_err(|_|Error::InvalidPath)?;
  let name=&canonical[..n];
  if PROTECTED.iter().any(|p|{
   name==*p||(name.starts_with(p)&&name.get(p.len())==Some(&b'/'))
  }){
   return Err(Error::Protected);
  }
  Ok(())
 }
 /// Before any mutation: stop writes to sensitive namespaces and
 /// large/repeated write attempts through this specific file ABI.
 pub fn preflight(&mut self,path:&str,length:usize)->Result<(),Error>{
  let error=(||{
   Self::allow_path(path)?;
   if length==0||length>u32::MAX as usize{
    return Err(Error::RateLimited);
   }
   let next=self.completed.checked_add(1).ok_or(Error::RateLimited)?;
   let bytes=self.bytes.checked_add(length as u32).ok_or(Error::RateLimited)?;
   if self.locked||next>self.ops_limit||bytes>self.bytes_limit{
    return Err(Error::RateLimited);
   }
   Ok(())
  })();
  if error.is_err(){self.deny();}
  error
 }
 /// Called ONLY after successful durable filesystem write.
 /// The file-handle service verifies the actual count never exceeds
 /// the number of bytes admitted by preflight.
 pub fn completed(&mut self,written:usize){
  self.completed=self.completed.saturating_add(1);
  self.bytes=self.bytes.saturating_add(written as u32);
 }
}
#[cfg(test)]
mod security_tests{
 use super::*;
 #[test]fn security_sensitive_directories_are_write_protected(){
  for path in [
   "/system","/system/driver","/boot/init","/security/keys",
   "/recovery/snapshot","/snapshots/history","/keys/public",
   "/firmware/update","//system/./driver",
  ]{
   assert_eq!(WriteGuard::allow_path(path),Err(Error::Protected),"{path}");
  }
  for path in ["/etc/note","/shared/notes","/systematic/file",
   "/bootcamp/notes","/documents/security","/user/boot"]{
   assert_eq!(WriteGuard::allow_path(path),Ok(()),"{path}");
  }
 }
 #[test]fn security_traversal_nul_and_invalid_names_fail_closed(){
  for path in ["/system/../etc/note","/etc/../../system",
   "system/file","/etc/\0evil",""]{
   assert_eq!(WriteGuard::allow_path(path),Err(Error::InvalidPath));
  }
 }
 #[test]fn security_write_budget_cannot_reset_by_new_open_or_short_operation(){
  let mut g=WriteGuard::new();
  for _ in 0..MAX_WRITE_OPERATIONS{
   g.preflight("/shared/notes",1).unwrap();
   g.completed(1);
  }
  assert_eq!(g.preflight("/shared/notes",1),Err(Error::RateLimited));
  assert_eq!(g.statistics(),Statistics{
   completed_writes:MAX_WRITE_OPERATIONS,
   changed_bytes:MAX_WRITE_OPERATIONS,denials:1,
  });
  assert_eq!(g.preflight("/boot/startup",1),Err(Error::Protected));
  assert_eq!(g.statistics().denials,2);
 }
 #[test]fn security_failed_write_is_not_charged_and_byte_budget_enforced(){
  let mut g=WriteGuard::new();
  assert_eq!(g.preflight("/etc/note",MAX_WRITE_BYTES as usize+1),
   Err(Error::RateLimited));
  assert_eq!(g.statistics().changed_bytes,0);
  assert_eq!(g.preflight("/etc/note",128),Ok(()));
  assert_eq!(g.statistics().completed_writes,0);
  g.completed(128);
  assert_eq!(g.statistics().changed_bytes,128);
  assert_eq!(g.preflight("/shared/docs",3970),Err(Error::RateLimited));
 }
}


// R084 / filesystem security workstream: narrowable lifetime write budgets,
// atomic transaction preflights and protected namespace rename/delete gates.
// Still not a global filesystem ACL or ransomware detector: only callers
// which use this kernel-owned guard are covered.
impl WriteGuard{
    /// Number of remaining mutation operations in this guard lifetime.
    pub fn operations_left(&self)->u32{
        self.ops_limit.saturating_sub(self.completed)
    }
    /// Number of remaining admitted bytes; not a persisted filesystem quota.
    pub fn bytes_left(&self)->u32{
        self.bytes_limit.saturating_sub(self.bytes)
    }
    pub fn exhausted(&self)->bool{
        self.locked||self.operations_left()==0||self.bytes_left()==0
    }
    /// Return current narrowed runtime limits for audited kernel policy.
    pub fn limits(&self)->(u32,u32){(self.ops_limit,self.bytes_limit)}
    /// Narrow but never widen trusted write budgets; zero deliberately locks
    /// the corresponding budget. Failures preserve all existing limits.
    pub fn reduce_limits(&mut self,operations:u32,bytes:u32)->bool{
        if operations>self.ops_limit||bytes>self.bytes_limit{return false;}
        self.ops_limit=operations;self.bytes_limit=bytes;true
    }
    /// Fail-closed permanent lockdown until the kernel constructs a new guard.
    pub fn lock_down(&mut self){self.locked=true;}
    pub fn is_locked(&self)->bool{self.locked}
    /// Validate deletion authority: metadata deletion counts as one mutation
    /// even when zero file bytes are copied.
    pub fn preflight_delete(&mut self,path:&str)->Result<(),Error>{
        self.preflight(path,1)
    }
    /// Rename must authorize *both* source and destination before any write;
    /// a protected source may not be moved into an unprotected directory.
    pub fn preflight_rename(&mut self,source:&str,target:&str)->Result<(),Error>{
        if let Err(e)=Self::allow_path(source){self.deny();return Err(e);}
        if source==target{self.deny();return Err(Error::InvalidPath);}
        self.preflight(target,1)
    }
    /// Atomic preflight for multiple actual pending mutations, with exact
    /// operation and byte accounting. This step only checks policy; caller
    /// MUST commit each successful operation through completed() or use
    /// completed_batch() after an actually atomic COW transaction.
    pub fn preflight_batch(&mut self,operations:&[(&str,usize)])->Result<(),Error>{
        let result=(||{
            if self.locked||operations.is_empty(){return Err(Error::RateLimited);}
            let mut bytes=0u32;
            for (i,&(path,length)) in operations.iter().enumerate(){
                Self::allow_path(path)?;
                if length==0||length>u32::MAX as usize{
                    return Err(Error::RateLimited);
                }
                if operations[..i].iter().any(|(previous,_)|*previous==path){
                    return Err(Error::RateLimited);
                }
                bytes=bytes.checked_add(length as u32).ok_or(Error::RateLimited)?;
            }
            let op_count=u32::try_from(operations.len()).map_err(|_|Error::RateLimited)?;
            if self.completed.checked_add(op_count).is_none_or(|n|n>self.ops_limit)||
               self.bytes.checked_add(bytes).is_none_or(|n|n>self.bytes_limit){
                return Err(Error::RateLimited);
            }
            Ok(())
        })();
        if result.is_err(){self.deny();}
        result
    }
    /// Record an all-or-nothing COW multi-file transaction ONLY after the
    /// entire transaction has actually committed, not after a partial write.
    /// It is a trusted kernel hook, not exposed directly to untrusted apps.
    pub fn completed_batch(&mut self,lengths:&[usize]){
        self.completed=self.completed.saturating_add(lengths.len() as u32);
        for &length in lengths{
            self.bytes=self.bytes.saturating_add(length as u32);
        }
    }
    /// Return a normalized, policy-validated mutable destination path. A
    /// short caller buffer does not receive partial canonicalized names.
    pub fn canonical_writable_path(&mut self,path:&str,
        out:&mut[u8])->Result<usize,Error>{
        let mut normalized=[0u8;64];
        let result=(||{
            let n=normalize_absolute(path,&mut normalized)
                .map_err(|_|Error::InvalidPath)?;
            let canonical=core::str::from_utf8(&normalized[..n])
                .map_err(|_|Error::InvalidPath)?;
            Self::allow_path(canonical)?;
            if out.len()<n{return Err(Error::RateLimited);}
            out[..n].copy_from_slice(&normalized[..n]);Ok(n)
        })();
        if result.is_err(){self.deny();}
        result
    }
}
#[cfg(test)]
mod r084_guard_hardening_tests{
    use super::*;
    #[test]
    fn narrowing_lockdown_and_namespace_gates_are_one_way(){
        let mut g=WriteGuard::new();
        assert_eq!(g.limits(),(32,4096));
        assert!(g.reduce_limits(4,100));
        assert!(!g.reduce_limits(5,100));
        assert_eq!(g.limits(),(4,100));
        assert_eq!(g.operations_left(),4);
        assert_eq!(g.bytes_left(),100);
        assert_eq!(g.preflight_rename("/system/config","/home/config"),
            Err(Error::Protected));
        assert_eq!(g.preflight_rename("/home/config","/boot/init"),
            Err(Error::Protected));
        assert_eq!(g.preflight_delete("/home/file"),Ok(()));
        g.completed(1);
        assert_eq!(g.operations_left(),3);
        assert_eq!(g.bytes_left(),99);
        let mut out=[0xaau8;64];
        let n=g.canonical_writable_path("//home/./file",&mut out).unwrap();
        assert_eq!(&out[..n],b"/home/file");
        g.lock_down();
        assert!(g.is_locked());assert!(g.exhausted());
        assert_eq!(g.preflight("/home/file",1),Err(Error::RateLimited));
    }
    #[test]
    fn atomic_multi_file_policy_never_charges_on_partial_preflight(){
        let mut g=WriteGuard::new();
        assert!(g.reduce_limits(3,12));
        assert_eq!(g.preflight_batch(&[("/work/a",4),("/boot/init",4)]),
            Err(Error::Protected));
        assert_eq!(g.statistics().completed_writes,0);
        assert_eq!(g.preflight_batch(&[("/work/a",4),("/work/a",4)]),
            Err(Error::RateLimited));
        assert_eq!(g.preflight_batch(&[("/work/a",8),("/work/b",8)]),
            Err(Error::RateLimited));
        assert_eq!(g.preflight_batch(&[("/work/a",4),("/work/b",6)]),Ok(()));
        g.completed_batch(&[4,6]);
        assert_eq!(g.statistics().completed_writes,2);
        assert_eq!(g.statistics().changed_bytes,10);
        assert_eq!(g.operations_left(),1);assert_eq!(g.bytes_left(),2);
        assert_eq!(g.preflight_batch(&[("/work/a",2),("/work/b",1)]),
            Err(Error::RateLimited));
    }
    #[test]
    fn too_short_canonical_output_does_not_publish_prefix(){
        let mut g=WriteGuard::new();
        let mut out=[0xa5u8;3];
        assert_eq!(g.canonical_writable_path("/home/file",&mut out),
            Err(Error::RateLimited));
        assert_eq!(out,[0xa5;3]);
        assert_eq!(g.canonical_writable_path("/recovery/log",&mut out),
            Err(Error::Protected));
        assert_eq!(out,[0xa5;3]);
    }
}


// R084 extended write-guard service: safe preflight planners and bounded
// postcommit accounting. This is still a per-service kernel guard and is
// not presented as a global VFS/ATA access-control system.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct WriteBudgetAudit{
    pub committed_operations:u32,pub committed_bytes:u32,pub denied_operations:u32,
    pub remaining_operations:u32,pub remaining_bytes:u32,pub locked:bool,
}
impl WriteGuard{
    /// 1. Immutable audit snapshot without modifying denial counters.
    pub fn audit_snapshot(&self)->WriteBudgetAudit{
        WriteBudgetAudit{
            committed_operations:self.completed,committed_bytes:self.bytes,
            denied_operations:self.denials,
            remaining_operations:self.operations_left(),
            remaining_bytes:self.bytes_left(),locked:self.locked,
        }
    }
    /// 2. Maximum number of fixed-size writes affordable in this session.
    pub fn fixed_write_capacity(&self,length:u32)->u32{
        if self.locked||length==0{return 0;}
        self.operations_left().min(self.bytes_left()/length)
    }
    /// 3. Largest single write allowed by both byte and operation budgets.
    pub fn maximum_next_write(&self)->u32{
        if self.locked||self.operations_left()==0{0}else{self.bytes_left()}
    }
    /// 4. Non-mutating one-write policy inquiry, never grants authority.
    pub fn would_accept_write(&self,path:&str,length:usize)->Result<(),Error>{
        Self::allow_path(path)?;
        if self.locked||length==0||length>u32::MAX as usize||
            self.operations_left()==0||length as u32>self.bytes_left(){
            return Err(Error::RateLimited);
        }
        Ok(())
    }
    /// 5. Non-mutating atomic preview of a multi-write commit.
    pub fn would_accept_batch(&self,requests:&[(&str,usize)])->Result<(),Error>{
        if self.locked||requests.is_empty(){return Err(Error::RateLimited);}
        let mut bytes=0u32;
        let count=u32::try_from(requests.len()).map_err(|_|Error::RateLimited)?;
        if count>self.operations_left(){return Err(Error::RateLimited);}
        for (i,&(path,length)) in requests.iter().enumerate(){
            Self::allow_path(path)?;
            if length==0||length>u32::MAX as usize||
                requests[..i].iter().any(|(p,_)|*p==path){
                return Err(Error::RateLimited);
            }
            bytes=bytes.checked_add(length as u32).ok_or(Error::RateLimited)?;
            if bytes>self.bytes_left(){return Err(Error::RateLimited);}
        }
        Ok(())
    }
    /// 6. Distinguish a locked/exhausted policy without charging denials.
    pub fn cannot_write(&self)->bool{
        self.locked||self.operations_left()==0||self.bytes_left()==0
    }
    /// 7. Check protected source or target for a non-destructive preview.
    pub fn would_allow_rename(&self,from:&str,to:&str)->Result<(),Error>{
        Self::allow_path(from)?;
        if from==to{return Err(Error::InvalidPath);}
        self.would_accept_write(to,1)
    }
    /// 8. Check a metadata-only deletion without decrementing budgets.
    pub fn would_allow_delete(&self,path:&str)->Result<(),Error>{
        self.would_accept_write(path,1)
    }
    /// 9. Report committed operation budget utilization, rounded down.
    pub fn operation_utilization_percent(&self)->u8{
        if self.ops_limit==0{return 100;}
        ((self.completed as u64*100/self.ops_limit as u64).min(100)) as u8
    }
    /// 10. Report committed changed-byte utilization, rounded down.
    pub fn byte_utilization_percent(&self)->u8{
        if self.bytes_limit==0{return 100;}
        ((self.bytes as u64*100/self.bytes_limit as u64).min(100)) as u8
    }
    /// 11. Narrow the operations budget alone; cannot replenish spent quota.
    pub fn reduce_operation_limit(&mut self,limit:u32)->bool{
        self.reduce_limits(limit,self.bytes_limit)
    }
    /// 12. Narrow byte budget alone; cannot widen current write quota.
    pub fn reduce_byte_limit(&mut self,limit:u32)->bool{
        self.reduce_limits(self.ops_limit,limit)
    }
    /// 13. Compare-and-narrow both limits; reject stale policy updates.
    pub fn reduce_limits_if_equal(&mut self,expect_ops:u32,expect_bytes:u32,
        ops:u32,bytes:u32)->bool{
        self.ops_limit==expect_ops&&self.bytes_limit==expect_bytes&&
            self.reduce_limits(ops,bytes)
    }
    /// 14. File creation consumes one real filesystem metadata operation.
    pub fn preflight_create(&mut self,path:&str)->Result<(),Error>{
        self.preflight(path,1)
    }
    /// 15. Truncation charges at least one operation despite zero payload.
    pub fn preflight_truncate(&mut self,path:&str)->Result<(),Error>{
        self.preflight(path,1)
    }
    /// 16. Append budget applies to actual new bytes, not file size.
    pub fn preflight_append(&mut self,path:&str,appended:usize)->Result<(),Error>{
        self.preflight(path,appended)
    }
    /// 17. Full overwrite charges the exact resulting payload size.
    pub fn preflight_overwrite(&mut self,path:&str,new_bytes:usize)->Result<(),Error>{
        self.preflight(path,new_bytes)
    }
    /// 18. Batch deletion validates every path and commits no counters.
    pub fn preflight_delete_batch(&mut self,paths:&[&str])->Result<(),Error>{
        let result=(||{
            if paths.is_empty()||paths.len()>MAX_WRITE_OPERATIONS as usize{
                return Err(Error::RateLimited);
            }
            let mut staged=[("",0usize);MAX_WRITE_OPERATIONS as usize];
            for (i,&path) in paths.iter().enumerate(){staged[i]=(path,1);}
            self.would_accept_batch(&staged[..paths.len()])
        })();
        if result.is_err(){self.deny();}
        result
    }
    /// 19. Multiple renames must authorize both source and destination,
    /// detect duplicate path use, and preserve the current guard on failure.
    pub fn preflight_rename_batch(&mut self,pairs:&[(&str,&str)])->Result<(),Error>{
        let result=(||{
            if pairs.is_empty()||pairs.len()>MAX_WRITE_OPERATIONS as usize{
                return Err(Error::RateLimited);
            }
            let mut staged=[("",0usize);MAX_WRITE_OPERATIONS as usize];
            for (i,&(from,to)) in pairs.iter().enumerate(){
                Self::allow_path(from)?;
                if from==to||pairs[..i].iter().any(|(a,b)|
                    *a==from||*b==to||*a==to||*b==from){
                    return Err(Error::InvalidPath);
                }
                staged[i]=(to,1);
            }
            self.would_accept_batch(&staged[..pairs.len()])
        })();
        if result.is_err(){self.deny();}
        result
    }
    /// 20. Swapping two distinct writable namespace entries accounts for
    /// two metadata operations; never allow protected-path swap bypass.
    pub fn preflight_swap(&mut self,left:&str,right:&str)->Result<(),Error>{
        let result=(||{
            if left==right{return Err(Error::InvalidPath);}
            Self::allow_path(left)?;
            Self::allow_path(right)?;
            self.would_accept_batch(&[(left,1),(right,1)])
        })();
        if result.is_err(){self.deny();}
        result
    }
}
#[cfg(test)]
mod r084_extended_write_guard_tests{
    use super::*;
    #[test]
    fn twenty_budget_operations_fail_closed_without_partial_accounting(){
        let mut g=WriteGuard::new();
        assert_eq!(g.audit_snapshot().remaining_operations,32);
        assert_eq!(g.fixed_write_capacity(1000),4);
        assert_eq!(g.maximum_next_write(),4096);
        assert_eq!(g.would_accept_write("/work/file",12),Ok(()));
        assert_eq!(g.would_accept_batch(&[("/work/a",2),("/work/b",2)]),Ok(()));
        assert!(!g.cannot_write());
        assert_eq!(g.would_allow_rename("/boot/init","/work/x"),Err(Error::Protected));
        assert_eq!(g.would_allow_delete("/work/x"),Ok(()));
        assert_eq!(g.operation_utilization_percent(),0);
        assert_eq!(g.byte_utilization_percent(),0);
        assert!(g.reduce_operation_limit(6));
        assert!(g.reduce_byte_limit(30));
        assert!(!g.reduce_limits_if_equal(32,4096,3,20));
        assert!(g.reduce_limits_if_equal(6,30,4,20));
        assert_eq!(g.preflight_create("/work/a"),Ok(()));
        assert_eq!(g.preflight_truncate("/work/b"),Ok(()));
        assert_eq!(g.preflight_append("/work/c",5),Ok(()));
        assert_eq!(g.preflight_overwrite("/work/d",5),Ok(()));
        assert_eq!(g.preflight_delete_batch(&["/work/a","/boot/init"]),
            Err(Error::Protected));
        assert_eq!(g.preflight_rename_batch(&[("/work/a","/work/b")]),Ok(()));
        assert_eq!(g.preflight_swap("/work/a","/work/b"),Ok(()));
        assert_eq!(g.preflight_swap("/boot/init","/work/b"),Err(Error::Protected));
        assert_eq!(g.statistics().completed_writes,0);
        g.completed_batch(&[1,1,5,5]);
        assert_eq!(g.operation_utilization_percent(),100);
        assert_eq!(g.byte_utilization_percent(),60);
        assert!(g.cannot_write());
        assert_eq!(g.fixed_write_capacity(1),0);
        assert_eq!(g.maximum_next_write(),0);
    }
    #[test]
    fn stale_limits_duplicate_paths_and_short_budget_do_not_charge(){
        let mut g=WriteGuard::new();assert!(g.reduce_limits(2,3));
        assert_eq!(g.preflight_delete_batch(&["/work/a","/work/a"]),Err(Error::RateLimited));
        assert_eq!(g.preflight_rename_batch(&[("/work/a","/work/b"),("/work/a","/work/c")]),
            Err(Error::InvalidPath));
        assert_eq!(g.preflight_append("/work/x",4),Err(Error::RateLimited));
        assert_eq!(g.statistics().completed_writes,0);
        assert_eq!(g.audit_snapshot().remaining_bytes,3);
    }
}


// R084 extension III: canonical namespace, metadata, and multi-write
// authorization preflights against live WriteGuard counters.
// These are kernel-service write checks, NOT claims of a global VFS ACL.
impl WriteGuard{
    /// 1. Compare normalized paths; aliases such as /work/./file are equal.
    pub fn same_canonical_path(a:&str,b:&str)->Result<bool,Error>{
        let mut left=[0u8;64];let mut right=[0u8;64];
        let al=normalize_absolute(a,&mut left).map_err(|_|Error::InvalidPath)?;
        let bl=normalize_absolute(b,&mut right).map_err(|_|Error::InvalidPath)?;
        Ok(left[..al]==right[..bl])
    }
    /// 2. Strict descendant check prevents /work matching /workspace.
    pub fn canonical_child_of(parent:&str,child:&str)->Result<bool,Error>{
        let mut p=[0u8;64];let mut c=[0u8;64];
        let n=normalize_absolute(parent,&mut p).map_err(|_|Error::InvalidPath)?;
        let m=normalize_absolute(child,&mut c).map_err(|_|Error::InvalidPath)?;
        Ok(m>n&&c[..m].starts_with(&p[..n])&&
            (n==1&&p[0]==b'/'||c[n]==b'/'))
    }
    /// 3. Canonical basename length for path capacity estimates.
    pub fn basename_size(path:&str)->Result<usize,Error>{
        let mut out=[0u8;64];
        let n=normalize_absolute(path,&mut out).map_err(|_|Error::InvalidPath)?;
        let i=out[..n].iter().rposition(|b|*b==b'/').unwrap_or(0);
        Ok(n.saturating_sub(i+1))
    }
    /// 4. Canonical parent path in a bounded caller buffer, no partial copy.
    pub fn write_parent_path(path:&str,out:&mut[u8])->Result<usize,Error>{
        let mut full=[0u8;64];
        let n=normalize_absolute(path,&mut full).map_err(|_|Error::InvalidPath)?;
        let end=full[..n].iter().rposition(|b|*b==b'/')
            .ok_or(Error::InvalidPath)?.max(1);
        if out.len()<end{return Err(Error::RateLimited);}
        out[..end].copy_from_slice(&full[..end]);Ok(end)
    }
    /// 5. Non-mutating budget capacity for a specific metadata transaction.
    pub fn would_accept_metadata(&self,path:&str)->Result<(),Error>{
        self.would_accept_write(path,1)
    }
    /// 6. Audit whether one path may store an exact output payload length.
    pub fn would_accept_payload(&self,path:&str,bytes:usize)->Result<(),Error>{
        self.would_accept_write(path,bytes)
    }
    /// 7. Canonical-aware rename validation: no self-alias or protected move.
    pub fn would_rename_canonical(&self,from:&str,to:&str)->Result<(),Error>{
        if Self::same_canonical_path(from,to)?{return Err(Error::InvalidPath);}
        Self::allow_path(from)?;self.would_accept_write(to,1)
    }
    /// 8. Root/cycle-safe directory move: a parent cannot move into its child.
    pub fn would_move_directory(&self,from:&str,to:&str)->Result<(),Error>{
        self.would_rename_canonical(from,to)?;
        if Self::canonical_child_of(from,to)?{return Err(Error::InvalidPath);}
        Ok(())
    }
    /// 9. Test source and destination namespace policy for a copy operation.
    pub fn would_copy_bytes(&self,source:&str,destination:&str,
        size:usize)->Result<(),Error>{
        if Self::same_canonical_path(source,destination)?{
            return Err(Error::InvalidPath);
        }
        Self::allow_path(source)?;
        self.would_accept_write(destination,size)
    }
    /// 10. Test two-file swap with canonical alias and protected scope checks.
    pub fn would_swap_paths(&self,left:&str,right:&str)->Result<(),Error>{
        if Self::same_canonical_path(left,right)?{
            return Err(Error::InvalidPath);
        }
        Self::allow_path(left)?;Self::allow_path(right)?;
        self.would_accept_batch(&[(left,1),(right,1)])
    }
    /// 11. Charge the correct preflight for a canonical-safe metadata rename.
    pub fn preflight_rename_canonical(&mut self,from:&str,to:&str)
        ->Result<(),Error>{
        let result=self.would_rename_canonical(from,to);
        if result.is_err(){self.deny();}
        result
    }
    /// 12. Deny moving a directory into itself/child or protected namespace.
    pub fn preflight_directory_move(&mut self,from:&str,to:&str)
        ->Result<(),Error>{
        let result=self.would_move_directory(from,to);
        if result.is_err(){self.deny();}
        result
    }
    /// 13. Archive/export copy preflight checks source and output policy.
    pub fn preflight_copy_bytes(&mut self,src:&str,dst:&str,size:usize)
        ->Result<(),Error>{
        let result=self.would_copy_bytes(src,dst,size);
        if result.is_err(){self.deny();}
        result
    }
    /// 14. Metadata-only folder creation consumes one operation.
    pub fn preflight_directory_create(&mut self,path:&str)->Result<(),Error>{
        self.preflight(path,1)
    }
    /// 15. Metadata-only folder removal consumes an audited write operation.
    pub fn preflight_directory_remove(&mut self,path:&str)->Result<(),Error>{
        self.preflight(path,1)
    }
    /// 16. Validate an entire copy destination batch or none of the entries.
    pub fn preflight_copies(&mut self,pairs:&[(&str,&str,usize)])
        ->Result<(),Error>{
        let result=(||{
            if pairs.is_empty()||pairs.len()>MAX_WRITE_OPERATIONS as usize{
                return Err(Error::RateLimited);
            }
            let mut batch=[("",0usize);MAX_WRITE_OPERATIONS as usize];
            for (i,&(from,to,len)) in pairs.iter().enumerate(){
                if Self::same_canonical_path(from,to)?{
                    return Err(Error::InvalidPath);
                }
                Self::allow_path(from)?;
                for &(old_from,old_to,_) in &pairs[..i]{
                    let _=old_from;
                    if Self::same_canonical_path(old_to,to)?{
                        return Err(Error::InvalidPath);
                    }
                }
                batch[i]=(to,len);
            }
            self.would_accept_batch(&batch[..pairs.len()])
        })();
        if result.is_err(){self.deny();}
        result
    }
    /// 17. Atomic directory-move authorization across multiple entries.
    pub fn preflight_directory_moves(&mut self,pairs:&[(&str,&str)])
        ->Result<(),Error>{
        let result=(||{
            if pairs.is_empty()||pairs.len()>MAX_WRITE_OPERATIONS as usize{
                return Err(Error::RateLimited);
            }
            let mut batch=[("",0usize);MAX_WRITE_OPERATIONS as usize];
            for (i,&(from,to)) in pairs.iter().enumerate(){
                self.would_move_directory(from,to)?;
                for &(old_from,old_to) in &pairs[..i]{
                    if Self::same_canonical_path(old_from,from)?||
                       Self::same_canonical_path(old_to,to)?{
                        return Err(Error::InvalidPath);
                    }
                }
                batch[i]=(to,1);
            }
            self.would_accept_batch(&batch[..pairs.len()])
        })();
        if result.is_err(){self.deny();}
        result
    }
    /// 18. Two-path swap preflight with full canonical name validation.
    pub fn preflight_canonical_swap(&mut self,left:&str,right:&str)
        ->Result<(),Error>{
        let result=self.would_swap_paths(left,right);
        if result.is_err(){self.deny();}
        result
    }
    /// 19. Return projected consumed totals if a batch succeeds, with no
    /// effect on real counters until the durable filesystem commits.
    pub fn forecast_batch_budget(&self,requests:&[(&str,usize)])
        ->Result<(u32,u32),Error>{
        self.would_accept_batch(requests)?;
        let ops=self.completed.checked_add(requests.len() as u32)
            .ok_or(Error::RateLimited)?;
        let mut bytes=self.bytes;
        for &(_,len) in requests{
            bytes=bytes.checked_add(len as u32).ok_or(Error::RateLimited)?;
        }
        Ok((ops,bytes))
    }
    /// 20. One-way quota narrowing after checked optimistic audit snapshot.
    pub fn narrow_if_snapshot(&mut self,prior:WriteBudgetAudit,
        ops:u32,bytes:u32)->bool{
        self.audit_snapshot()==prior&&self.reduce_limits(ops,bytes)
    }
}
#[cfg(test)]
mod r084_canonical_guard_batch_tests{
    use super::*;
    #[test]
    fn twenty_path_guards_enforce_canonical_transactional_namespace_policy(){
        let mut g=WriteGuard::new();
        assert_eq!(WriteGuard::same_canonical_path("/work/./a","//work/a"),Ok(true));
        assert_eq!(WriteGuard::canonical_child_of("/work","/workspace/file"),Ok(false));
        assert_eq!(WriteGuard::canonical_child_of("/work","/work/a"),Ok(true));
        assert_eq!(WriteGuard::basename_size("/work/file"),Ok(4));
        let mut out=[0xaau8;64];
        assert_eq!(WriteGuard::write_parent_path("/work/file",&mut out),Ok(5));
        assert_eq!(&out[..5],b"/work");
        assert_eq!(g.would_accept_metadata("/work/entry"),Ok(()));
        assert_eq!(g.would_accept_payload("/work/data",6),Ok(()));
        assert_eq!(g.would_rename_canonical("/work/./a","/work/a"),
            Err(Error::InvalidPath));
        assert_eq!(g.would_move_directory("/work","/work/child"),
            Err(Error::InvalidPath));
        assert_eq!(g.would_copy_bytes("/work/src","/work/dst",20),Ok(()));
        assert_eq!(g.would_swap_paths("/work/a","/work/b"),Ok(()));
        assert_eq!(g.preflight_rename_canonical("/work/a","/work/b"),Ok(()));
        assert_eq!(g.preflight_directory_move("/work","/work/child"),
            Err(Error::InvalidPath));
        assert_eq!(g.preflight_copy_bytes("/work/a","/work/b",16),Ok(()));
        assert_eq!(g.preflight_directory_create("/work/new"),Ok(()));
        assert_eq!(g.preflight_directory_remove("/work/old"),Ok(()));
        assert_eq!(g.preflight_copies(&[
            ("/work/a","/work/b",5),("/work/c","/work/d",7)]),Ok(()));
        assert_eq!(g.preflight_directory_moves(&[
            ("/work/a","/archive/a"),("/work/b","/archive/b")]),Ok(()));
        assert_eq!(g.preflight_canonical_swap("/work/a","/work/b"),Ok(()));
        assert_eq!(g.forecast_batch_budget(&[
            ("/work/a",5),("/work/b",7)]),Ok((2,12)));
        let old=g.audit_snapshot();
        assert!(g.narrow_if_snapshot(old,10,40));
        assert!(!g.narrow_if_snapshot(old,20,40));
        assert_eq!(g.statistics().completed_writes,0);
    }
    #[test]
    fn duplicate_destination_and_protected_path_fail_before_any_publish(){
        let mut g=WriteGuard::new();
        assert_eq!(g.preflight_copies(&[
            ("/work/a","/work/c",1),("/work/b","/work/./c",1)]),
            Err(Error::InvalidPath));
        assert_eq!(g.preflight_directory_moves(&[
            ("/work","/work/child")]),Err(Error::InvalidPath));
        assert_eq!(g.preflight_canonical_swap("/boot/init","/work/init"),
            Err(Error::Protected));
        let mut short=[0xaau8;1];
        assert_eq!(WriteGuard::write_parent_path("/work/file",&mut short),
            Err(Error::RateLimited));
        assert_eq!(short,[0xaa]);
        assert_eq!(g.statistics().completed_writes,0);
    }
}


// M1/R084 extension IV — 20 namespace, quota and atomic mutation checks.
// These guard real FileHandles/VFS writes only when called at the boundary;
// none of them is a global filesystem ACL or a durable journal.
impl WriteGuard{
    /// 1. Check whether all requested source paths pass canonical write policy.
    pub fn allow_many_sources(paths:&[&str])->Result<(),Error>{
        if paths.is_empty()||paths.len()>MAX_WRITE_OPERATIONS as usize{
            return Err(Error::RateLimited);
        }
        for (i,&p) in paths.iter().enumerate(){
            Self::allow_path(p)?;
            for &older in &paths[..i]{
                if Self::same_canonical_path(older,p)?{return Err(Error::InvalidPath);}
            }
        }
        Ok(())
    }
    /// 2. Check distinct canonical destination names before any mutation.
    pub fn allow_many_destinations(paths:&[&str])->Result<(),Error>{
        Self::allow_many_sources(paths)
    }
    /// 3. Return the shortest time-independent outstanding byte budget.
    pub fn remaining_quota_after(&self,operations:u32,bytes:u32)
        ->Result<(u32,u32),Error>{
        if self.locked||operations>self.operations_left()||bytes>self.bytes_left(){
            return Err(Error::RateLimited);
        }
        Ok((self.operations_left()-operations,self.bytes_left()-bytes))
    }
    /// 4. Nonmutating optimistic quota comparison against exact observations.
    pub fn quota_matches(&self,expected_ops:u32,expected_bytes:u32)->bool{
        self.completed==expected_ops&&self.bytes==expected_bytes
    }
    /// 5. Report whether a prospective exact operation would exhaust quota.
    pub fn would_exhaust_quota(&self,operations:u32,bytes:u32)->Result<bool,Error>{
        let (ops,left)=self.remaining_quota_after(operations,bytes)?;
        Ok(ops==0||left==0)
    }
    /// 6. Remaining mutation budget in percentage points.
    pub fn operation_headroom_percent(&self)->u8{
        if self.locked||self.ops_limit==0{return 0;}
        ((self.operations_left() as u64*100/self.ops_limit as u64).min(100)) as u8
    }
    /// 7. Remaining byte budget in percentage points.
    pub fn byte_headroom_percent(&self)->u8{
        if self.locked||self.bytes_limit==0{return 0;}
        ((self.bytes_left() as u64*100/self.bytes_limit as u64).min(100)) as u8
    }
    /// 8. Calculate allowed write count for a fixed record size.
    pub fn affordable_records(&self,record_bytes:u32)->u32{
        self.fixed_write_capacity(record_bytes)
    }
    /// 9. Check immutable audit record consistency before a policy update.
    pub fn matches_audit(&self,prior:WriteBudgetAudit)->bool{
        self.audit_snapshot()==prior
    }
    /// 10. Lock down only if the observed audit epoch is still current.
    pub fn lock_down_if_audit(&mut self,prior:WriteBudgetAudit)->bool{
        if !self.matches_audit(prior)||self.locked{return false;}
        self.lock_down();true
    }
    /// 11. Compare-and-narrow operations only if no write happened since audit.
    pub fn narrow_operations_if_audit(&mut self,prior:WriteBudgetAudit,
        limit:u32)->bool{
        self.matches_audit(prior)&&self.reduce_operation_limit(limit)
    }
    /// 12. Compare-and-narrow bytes only if exact audit still matches.
    pub fn narrow_bytes_if_audit(&mut self,prior:WriteBudgetAudit,
        limit:u32)->bool{
        self.matches_audit(prior)&&self.reduce_byte_limit(limit)
    }
    /// 13. Preview a whole directory rename before changing VFS metadata.
    pub fn would_relocate_directory(&self,src:&str,dst:&str)
        ->Result<(),Error>{
        self.would_move_directory(src,dst)
    }
    /// 14. Verify no writable destination overlaps a source subtree.
    pub fn no_destination_inside_source(&self,source:&str,destinations:&[&str])
        ->Result<(),Error>{
        Self::allow_path(source)?;
        if destinations.is_empty(){return Err(Error::InvalidPath);}
        for &dest in destinations{
            Self::allow_path(dest)?;
            if Self::same_canonical_path(source,dest)?||
               Self::canonical_child_of(source,dest)?{
                return Err(Error::InvalidPath);
            }
        }
        Ok(())
    }
    /// 15. Validate a set of future writes to disjoint canonical destinations.
    pub fn would_write_distinct(&self,requests:&[(&str,usize)])
        ->Result<(),Error>{
        self.would_accept_batch(requests)?;
        for (i,&(path,_)) in requests.iter().enumerate(){
            for &(other,_) in &requests[..i]{
                if Self::same_canonical_path(path,other)?{
                    return Err(Error::InvalidPath);
                }
            }
        }
        Ok(())
    }
    /// 16. Validate all canonical write destinations atomically and charge
    /// a denial only on failure; caller records successes after durable COW.
    pub fn preflight_distinct_writes(&mut self,requests:&[(&str,usize)])
        ->Result<(),Error>{
        let result=self.would_write_distinct(requests);
        if result.is_err(){self.deny();}
        result
    }
    /// 17. Transactional check for a directory's separate child insertions.
    pub fn preflight_directory_children(&mut self,parent:&str,
        children:&[(&str,usize)])->Result<(),Error>{
        let result=(||{
            if children.is_empty(){return Err(Error::InvalidPath);}
            Self::allow_path(parent)?;
            for &(path,_) in children{
                if !Self::canonical_child_of(parent,path)?{
                    return Err(Error::InvalidPath);
                }
            }
            self.would_write_distinct(children)
        })();
        if result.is_err(){self.deny();}
        result
    }
    /// 18. Preview byte budget for a set of new destination files.
    pub fn forecast_distinct_writes(&self,requests:&[(&str,usize)])
        ->Result<(u32,u32),Error>{
        self.would_write_distinct(requests)?;
        self.forecast_batch_budget(requests)
    }
    /// 19. Estimate remaining byte quota after a validated metadata copy.
    pub fn forecast_copy_remaining(&self,src:&str,dst:&str,bytes:usize)
        ->Result<(u32,u32),Error>{
        self.would_copy_bytes(src,dst,bytes)?;
        self.remaining_quota_after(1,bytes as u32)
    }
    /// 20. Explicitly prohibit a destination collision in a copy batch.
    pub fn preflight_unique_copies(&mut self,items:&[(&str,&str,usize)])
        ->Result<(),Error>{
        let result=(||{
            if items.is_empty()||items.len()>MAX_WRITE_OPERATIONS as usize{
                return Err(Error::RateLimited);
            }
            let mut requests=[("",0usize);MAX_WRITE_OPERATIONS as usize];
            for (i,&(src,dst,size)) in items.iter().enumerate(){
                Self::allow_path(src)?;
                if Self::same_canonical_path(src,dst)?{
                    return Err(Error::InvalidPath);
                }
                requests[i]=(dst,size);
            }
            self.would_write_distinct(&requests[..items.len()])
        })();
        if result.is_err(){self.deny();}
        result
    }
}

#[cfg(test)]
mod r084_write_guard_quota_batch_tests{
    use super::*;
    #[test]
    fn quota_and_canonical_path_batch_enforces_real_limits(){
        let mut g=WriteGuard::new();
        assert_eq!(WriteGuard::allow_many_sources(&["/work/a","/work/./a"]),
            Err(Error::InvalidPath));
        assert_eq!(WriteGuard::allow_many_destinations(&["/work/a","/work/b"]),Ok(()));
        assert_eq!(g.remaining_quota_after(2,10),Ok((30,4086)));
        assert!(g.quota_matches(0,0));
        assert_eq!(g.would_exhaust_quota(32,1),Ok(true));
        assert_eq!(g.operation_headroom_percent(),100);
        assert_eq!(g.byte_headroom_percent(),100);
        assert_eq!(g.affordable_records(1024),4);
        let before=g.audit_snapshot();
        assert!(g.matches_audit(before));
        assert!(g.narrow_operations_if_audit(before,8));
        assert!(!g.narrow_bytes_if_audit(before,500));
        let after=g.audit_snapshot();
        assert!(g.narrow_bytes_if_audit(after,500));
        assert_eq!(g.would_relocate_directory("/work","/work/sub"),
            Err(Error::InvalidPath));
        assert_eq!(g.no_destination_inside_source("/work", &["/archive/a"]),Ok(()));
        assert_eq!(g.no_destination_inside_source("/work", &["/work/child"]),
            Err(Error::InvalidPath));
        assert_eq!(g.would_write_distinct(&[("/work/a",4),("/work/./a",5)]),
            Err(Error::InvalidPath));
        assert_eq!(g.preflight_distinct_writes(&[("/work/a",4),("/work/b",5)]),Ok(()));
        assert_eq!(g.preflight_directory_children("/work",
            &[("/work/a",4),("/work/b",5)]),Ok(()));
        assert_eq!(g.forecast_distinct_writes(&[("/work/a",4),("/work/b",5)]),Ok((2,9)));
        assert_eq!(g.forecast_copy_remaining("/work/a","/work/b",10),Ok((7,490)));
        assert_eq!(g.preflight_unique_copies(&[
            ("/work/a","/work/b",4),("/work/c","/work/d",5)]),Ok(()));
        let latest=g.audit_snapshot();
        assert!(g.lock_down_if_audit(latest));
        assert!(!g.lock_down_if_audit(latest));
        assert_eq!(g.preflight("/work/a",1),Err(Error::RateLimited));
    }
    #[test]
    fn protected_paths_and_copy_aliases_leave_budget_counters_uncommitted(){
        let mut g=WriteGuard::new();
        assert_eq!(g.preflight_unique_copies(&[
            ("/work/a","/work/c",1),("/work/b","/work/./c",1)]),
            Err(Error::InvalidPath));
        assert_eq!(g.preflight_directory_children("/work",
            &[("/boot/init",5)]),Err(Error::InvalidPath));
        assert_eq!(g.statistics().completed_writes,0);
        assert_eq!(g.statistics().changed_bytes,0);
    }
}


// R084 extension V: 20 canonical-path write authorization and stale-quota
// controls for the existing FileHandles mutation boundary. No global VFS ACL.
impl WriteGuard {
    /// Canonical path depth (root zero), with no input-path alias bypass.
    pub fn canonical_depth(path:&str)->Result<usize,Error>{
        let mut name=[0u8;64];
        let len=normalize_absolute(path,&mut name).map_err(|_|Error::InvalidPath)?;
        Ok(name[..len].iter().filter(|b|**b==b'/').count()
            .saturating_sub(usize::from(len==1)))
    }
    /// Check whether the two paths share the exact canonical parent.
    pub fn same_canonical_parent(a:&str,b:&str)->Result<bool,Error>{
        let mut pa=[0u8;64];let mut pb=[0u8;64];
        let la=Self::write_parent_path(a,&mut pa)?;
        let lb=Self::write_parent_path(b,&mut pb)?;
        Ok(pa[..la]==pb[..lb])
    }
    /// True if destination is a strict descendant, never just a prefix alias.
    pub fn is_strict_descendant(parent:&str,child:&str)->Result<bool,Error>{
        Self::canonical_child_of(parent,child)
    }
    /// Classify an existing path as protected without changing denial budget.
    pub fn path_is_protected(path:&str)->Result<bool,Error>{
        match Self::allow_path(path){
            Ok(())=>Ok(false),
            Err(Error::Protected)=>Ok(true),
            Err(e)=>Err(e)
        }
    }
    /// Non-mutating check that the path and payload fit the *current* budget.
    pub fn has_write_budget_for(&self,path:&str,length:usize)->bool{
        self.would_accept_write(path,length).is_ok()
    }
    /// Return the budgeted number of writes at exactly this byte length.
    pub fn budgeted_writes_of(&self,length:u32)->u32{
        self.fixed_write_capacity(length)
    }
    /// Compute projected completion counters after one admitted operation.
    pub fn project_single_write(&self,path:&str,length:usize)
        ->Result<(u32,u32),Error>{
        self.would_accept_write(path,length)?;
        Ok((self.completed.checked_add(1).ok_or(Error::RateLimited)?,
            self.bytes.checked_add(length as u32).ok_or(Error::RateLimited)?))
    }
    /// Validate a read-only checksum request path without charging budget.
    pub fn validate_non_mutating_path(path:&str)->Result<usize,Error>{
        let mut out=[0u8;64];
        normalize_absolute(path,&mut out).map_err(|_|Error::InvalidPath)
    }
    /// Validate that a writable new destination is outside its source subtree.
    pub fn can_copy_tree_to(&self,from:&str,to:&str)->Result<(),Error>{
        Self::allow_path(from)?;
        if Self::same_canonical_path(from,to)?||
           Self::canonical_child_of(from,to)?{return Err(Error::InvalidPath);}
        self.would_accept_write(to,1)
    }
    /// Validate two distinct source paths with one writable destination.
    pub fn can_merge_into(&self,left:&str,right:&str,dest:&str,bytes:usize)
        ->Result<(),Error>{
        Self::allow_path(left)?;Self::allow_path(right)?;
        if Self::same_canonical_path(left,right)?||
           Self::same_canonical_path(left,dest)?||
           Self::same_canonical_path(right,dest)?{return Err(Error::InvalidPath);}
        self.would_accept_write(dest,bytes)
    }
    /// Authorize a one-byte metadata touch through the existing path guard.
    pub fn preflight_touch(&mut self,path:&str)->Result<(),Error>{
        self.preflight(path,1)
    }
    /// Authorize a nonempty payload replacement with unchanged namespace.
    pub fn preflight_replace_payload(&mut self,path:&str,len:usize)
        ->Result<(),Error>{
        self.preflight(path,len)
    }
    /// Authorize safe copy-tree target and account one metadata operation.
    pub fn preflight_copy_tree(&mut self,from:&str,to:&str)->Result<(),Error>{
        let result=self.can_copy_tree_to(from,to);
        if result.is_err(){self.deny();}
        result
    }
    /// Authorize multi-source merge into one destination before VFS mutation.
    pub fn preflight_merge(&mut self,left:&str,right:&str,dst:&str,len:usize)
        ->Result<(),Error>{
        let result=self.can_merge_into(left,right,dst,len);
        if result.is_err(){self.deny();}
        result
    }
    /// Check canonical alias-safe rename and same-parent requirement.
    pub fn preflight_rename_same_parent(&mut self,from:&str,to:&str)
        ->Result<(),Error>{
        let result=(||{
            if !Self::same_canonical_parent(from,to)?{return Err(Error::InvalidPath);}
            self.would_rename_canonical(from,to)
        })();
        if result.is_err(){self.deny();}
        result
    }
    /// Reject all rename operations when source is a parent of destination.
    pub fn preflight_rename_not_into_child(&mut self,from:&str,to:&str)
        ->Result<(),Error>{
        self.preflight_directory_move(from,to)
    }
    /// Validate an exact audit and write quota before the real mutation.
    pub fn preflight_at_audit(&mut self,expected:WriteBudgetAudit,
        path:&str,len:usize)->Result<(),Error>{
        if !self.matches_audit(expected){
            self.deny();return Err(Error::RateLimited);
        }
        self.preflight(path,len)
    }
    /// Validate all paths before any batch is approved; canonical duplicates
    /// fail even when paths have different textual spellings.
    pub fn preflight_canonical_batch(&mut self,items:&[(&str,usize)])
        ->Result<(),Error>{
        self.preflight_distinct_writes(items)
    }
    /// Forecast an exact canonical write batch only at the observed audit.
    pub fn forecast_at_audit(&self,prior:WriteBudgetAudit,
        items:&[(&str,usize)])->Result<(u32,u32),Error>{
        if !self.matches_audit(prior){return Err(Error::RateLimited);}
        self.forecast_distinct_writes(items)
    }
    /// Lock the guard permanently once the byte quota is actually exhausted.
    pub fn lock_if_bytes_exhausted(&mut self)->bool{
        if self.locked||self.bytes_left()!=0{return false;}
        self.lock_down();true
    }
}
#[cfg(test)]
mod r084_v5_guard_tests{
    use super::*;
    #[test]
    fn canonical_audit_preflight_fails_closed_for_protected_namespace(){
        let mut g=WriteGuard::new();
        assert_eq!(WriteGuard::canonical_depth("/work/sub/file"),Ok(3));
        assert_eq!(WriteGuard::same_canonical_parent("/work/a","/work/./b"),Ok(true));
        assert_eq!(WriteGuard::is_strict_descendant("/work","/workspace"),Ok(false));
        assert_eq!(WriteGuard::path_is_protected("/boot/start"),Ok(true));
        assert!(g.has_write_budget_for("/work/a",1));
        assert_eq!(g.budgeted_writes_of(1024),4);
        assert_eq!(g.project_single_write("/work/a",5),Ok((1,5)));
        assert_eq!(WriteGuard::validate_non_mutating_path("/work/./file"),Ok(10));
        assert_eq!(g.can_copy_tree_to("/work/a","/work/a/child"),Err(Error::InvalidPath));
        assert_eq!(g.can_merge_into("/work/a","/work/b","/work/c",5),Ok(()));
        assert_eq!(g.preflight_touch("/work/a"),Ok(()));
        assert_eq!(g.preflight_replace_payload("/work/a",8),Ok(()));
        assert_eq!(g.preflight_copy_tree("/work/a","/archive/a"),Ok(()));
        assert_eq!(g.preflight_merge("/work/a","/work/b","/archive/c",5),Ok(()));
        assert_eq!(g.preflight_rename_same_parent("/work/a","/work/b"),Ok(()));
        assert_eq!(g.preflight_rename_not_into_child("/work/a","/work/a/child"),
            Err(Error::InvalidPath));
        let prior=g.audit_snapshot();
        assert_eq!(g.preflight_at_audit(prior,"/work/file",1),Ok(()));
        assert_eq!(g.preflight_canonical_batch(&[("/work/a",2),("/work/./a",2)]),
            Err(Error::InvalidPath));
        let latest=g.audit_snapshot();
        assert_eq!(g.forecast_at_audit(latest,&[("/work/a",2),("/work/b",3)]),
            Ok((2,5)));
        assert!(!g.lock_if_bytes_exhausted());
    }
    #[test]
    fn stale_quota_and_protected_directory_never_complete_any_write(){
        let mut g=WriteGuard::new();
        let first=g.audit_snapshot();
        g.deny();
        assert_eq!(g.preflight_at_audit(first,"/work/a",1),Err(Error::RateLimited));
        assert_eq!(g.preflight_copy_tree("/boot","/work/out"),Err(Error::Protected));
        assert_eq!(g.preflight_rename_same_parent("/boot/start","/boot/other"),
            Err(Error::Protected));
        assert_eq!(g.statistics().completed_writes,0);
    }
}


// R084 batch VI: 20 namespace-bound transaction checks and policy audits.
// They act on the FileHandles write guard; global VFS policy is NOT implied.
impl WriteGuard{
    // 1. Canonical input path length, not its potentially aliased raw length.
    pub fn canonical_length(path:&str)->Result<usize,Error>{
        let mut canonical=[0u8;64];
        normalize_absolute(path,&mut canonical).map_err(|_|Error::InvalidPath)
    }
    // 2. Check that a canonical basename is nonempty and within a limit.
    pub fn basename_within(path:&str,maximum:usize)->Result<bool,Error>{
        Ok(Self::basename_size(path)?<=maximum)
    }
    // 3. Check exact normalized name equality; no textual alias bypass.
    pub fn paths_identical(a:&str,b:&str)->Result<bool,Error>{
        Self::same_canonical_path(a,b)
    }
    // 4. Check a canonical descendant without prefix collisions.
    pub fn descendant_of(parent:&str,child:&str)->Result<bool,Error>{
        Self::canonical_child_of(parent,child)
    }
    // 5. Verify a set of paths all belong to an authorized canonical directory.
    pub fn verify_paths_in_directory(parent:&str,items:&[&str])->Result<(),Error>{
        if items.is_empty()||items.len()>MAX_WRITE_OPERATIONS as usize{
            return Err(Error::InvalidPath);
        }
        Self::allow_path(parent)?;
        for (i,&path) in items.iter().enumerate(){
            Self::allow_path(path)?;
            if !Self::canonical_child_of(parent,path)?{return Err(Error::InvalidPath);}
            if items[..i].iter().any(|prior|
                Self::same_canonical_path(prior,path)==Ok(true)){
                return Err(Error::InvalidPath);
            }
        }
        Ok(())
    }
    // 6. Non-mutating canonical selection of paths with a common parent.
    pub fn verify_common_parent(items:&[&str])->Result<(),Error>{
        if items.is_empty()||items.len()>MAX_WRITE_OPERATIONS as usize{
            return Err(Error::InvalidPath);
        }
        for (i,&path) in items.iter().enumerate(){
            Self::allow_path(path)?;
            if i>0&&!Self::same_canonical_parent(items[0],path)?{
                return Err(Error::InvalidPath);
            }
            if items[..i].iter().any(|prior|
                Self::same_canonical_path(prior,path)==Ok(true)){
                return Err(Error::InvalidPath);
            }
        }
        Ok(())
    }
    // 7. Verify that all requested write entries are safe AND have budget.
    pub fn allow_canonical_writes(&self,items:&[(&str,usize)])->Result<(),Error>{
        self.would_write_distinct(items)
    }
    // 8. Quota-headroom after a plan's estimated exact operation and bytes.
    pub fn quota_after_plan(&self,items:&[(&str,usize)])
        ->Result<(u32,u32),Error>{
        self.would_write_distinct(items)?;
        let bytes=items.iter().try_fold(0u32,|n,(_,size)|
            n.checked_add(u32::try_from(*size).map_err(|_|Error::RateLimited)?)
                .ok_or(Error::RateLimited))?;
        self.remaining_quota_after(items.len() as u32,bytes)
    }
    // 9. Maximum remaining single write size after a plan.
    pub fn max_write_after_plan(&self,items:&[(&str,usize)])->Result<u32,Error>{
        let (operations,bytes)=self.quota_after_plan(items)?;
        Ok(if operations>0{bytes}else{0})
    }
    // 10. Number of one-byte metadata operations available after a plan.
    pub fn metadata_slots_after_plan(&self,items:&[(&str,usize)])
        ->Result<u32,Error>{
        let (operations,bytes)=self.quota_after_plan(items)?;
        Ok(operations.min(bytes))
    }
    // 11. Audit comparison that also rejects changes to lock/limit fields.
    pub fn audit_unchanged(&self,snapshot:WriteBudgetAudit)->bool{
        self.audit_snapshot()==snapshot
    }
    // 12. A compare-and-preflight to reject stale userspace write proposals.
    pub fn preflight_exact_audit(&mut self,prior:WriteBudgetAudit,
        path:&str,length:usize)->Result<(),Error>{
        self.preflight_at_audit(prior,path,length)
    }
    // 13. Canonical owner directory metadata-create validation.
    pub fn preflight_child_create(&mut self,parent:&str,child:&str)
        ->Result<(),Error>{
        let outcome=(||{
            if !Self::canonical_child_of(parent,child)?{return Err(Error::InvalidPath);}
            Self::allow_path(parent)?;
            self.would_accept_metadata(child)
        })();
        if outcome.is_err(){self.deny();}
        outcome
    }
    // 14. A full destination subtree move may never target source descendants.
    pub fn preflight_tree_relocate(&mut self,from:&str,to:&str)
        ->Result<(),Error>{
        self.preflight_directory_move(from,to)
    }
    // 15. All-or-nothing rename of two distinct siblings.
    pub fn preflight_sibling_swap(&mut self,one:&str,two:&str)->Result<(),Error>{
        let outcome=(||{
            if !Self::same_canonical_parent(one,two)?{
                return Err(Error::InvalidPath);
            }
            self.would_swap_paths(one,two)
        })();
        if outcome.is_err(){self.deny();}
        outcome
    }
    // 16. Validate multiple new sibling destinations and their byte budgets.
    pub fn preflight_sibling_writes(&mut self,items:&[(&str,usize)])
        ->Result<(),Error>{
        let outcome=(||{
            if items.is_empty(){return Err(Error::InvalidPath);}
            let mut paths=["";MAX_WRITE_OPERATIONS as usize];
            if items.len()>paths.len(){return Err(Error::RateLimited);}
            for (i,&(name,_)) in items.iter().enumerate(){paths[i]=name;}
            Self::verify_common_parent(&paths[..items.len()])?;
            self.would_write_distinct(items)
        })();
        if outcome.is_err(){self.deny();}
        outcome
    }
    // 17. Preflight a multi-file move to a completely separate destination.
    pub fn preflight_cross_directory_moves(&mut self,items:&[(&str,&str)])
        ->Result<(),Error>{
        let outcome=(||{
            if items.is_empty()||items.len()>MAX_WRITE_OPERATIONS as usize{
                return Err(Error::RateLimited);
            }
            for (i,&(src,dst)) in items.iter().enumerate(){
                Self::allow_path(src)?;
                if Self::same_canonical_path(src,dst)?||
                    Self::same_canonical_parent(src,dst)?{
                    return Err(Error::InvalidPath);
                }
                for &(prior_src,prior_dst) in &items[..i]{
                    if Self::same_canonical_path(src,prior_src)?||
                        Self::same_canonical_path(dst,prior_dst)?{
                        return Err(Error::InvalidPath);
                    }
                }
            }
            let mut requests=[("",0usize);MAX_WRITE_OPERATIONS as usize];
            for (i,&(_,dst)) in items.iter().enumerate(){requests[i]=(dst,1);}
            self.would_write_distinct(&requests[..items.len()])
        })();
        if outcome.is_err(){self.deny();}
        outcome
    }
    // 18. Narrow quotas only if the complete original audit remains current.
    pub fn narrow_at_audit(&mut self,observed:WriteBudgetAudit,
        max_operations:u32,max_bytes:u32)->bool{
        self.narrow_if_snapshot(observed,max_operations,max_bytes)
    }
    // 19. Lock down when either operation or byte budget is exhausted.
    pub fn lock_on_exhaustion(&mut self)->bool{
        if self.locked||self.operations_left()>0&&self.bytes_left()>0{return false;}
        self.lock_down();true
    }
    // 20. Verify lifetime counters and monotonic limits still consistent.
    pub fn validate_guard_accounting(&self)->bool{
        self.ops_limit<=MAX_WRITE_OPERATIONS&&
        self.bytes_limit<=MAX_WRITE_BYTES&&
        self.completed<=self.ops_limit&&
        self.bytes<=self.bytes_limit
    }
}
#[cfg(test)]
mod r084_batch_vi_guard_tests{
    use super::*;
    #[test]
    fn quota_planning_and_canonical_sibling_authorization_are_atomic(){
        let mut g=WriteGuard::new();
        assert_eq!(WriteGuard::canonical_length("//work/./a"),Ok(7));
        assert_eq!(WriteGuard::basename_within("/work/file",4),Ok(true));
        assert_eq!(WriteGuard::paths_identical("/work/./a","/work/a"),Ok(true));
        assert_eq!(WriteGuard::descendant_of("/work","/workspace"),Ok(false));
        assert_eq!(WriteGuard::verify_paths_in_directory("/work",&["/work/a","/work/b"]),Ok(()));
        assert_eq!(WriteGuard::verify_common_parent(&["/work/a","/work/b"]),Ok(()));
        let edits=[("/work/a",4usize),("/work/b",5usize)];
        assert_eq!(g.allow_canonical_writes(&edits),Ok(()));
        assert_eq!(g.quota_after_plan(&edits),Ok((30,4087)));
        assert_eq!(g.max_write_after_plan(&edits),Ok(4087));
        assert_eq!(g.metadata_slots_after_plan(&edits),Ok(30));
        let prior=g.audit_snapshot();
        assert!(g.audit_unchanged(prior));
        assert_eq!(g.preflight_exact_audit(prior,"/work/a",1),Ok(()));
        assert_eq!(g.preflight_child_create("/work","/work/a"),Ok(()));
        assert_eq!(g.preflight_tree_relocate("/work","/work/a"),Err(Error::InvalidPath));
        assert_eq!(g.preflight_sibling_swap("/work/a","/work/b"),Ok(()));
        assert_eq!(g.preflight_sibling_writes(&edits),Ok(()));
        assert_eq!(g.preflight_cross_directory_moves(&[("/work/a","/archive/a")]),Ok(()));
        let now=g.audit_snapshot();
        assert!(g.narrow_at_audit(now,3,100));
        assert!(!g.lock_on_exhaustion());
        assert!(g.validate_guard_accounting());
        assert_eq!(g.statistics().completed_writes,0);
    }
    #[test]
    fn bad_namespace_or_stale_audit_never_approves_writes(){
        let mut g=WriteGuard::new();
        let original=g.audit_snapshot();g.deny();
        assert_eq!(g.preflight_exact_audit(original,"/work/file",1),Err(Error::RateLimited));
        assert_eq!(g.preflight_child_create("/work","/boot/init"),Err(Error::InvalidPath));
        assert_eq!(g.preflight_sibling_writes(&[("/work/a",1),("/work/./a",1)]),Err(Error::InvalidPath));
        assert_eq!(g.statistics().completed_writes,0);
        assert!(g.validate_guard_accounting());
    }
}
