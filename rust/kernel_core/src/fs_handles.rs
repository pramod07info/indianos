//! R031.6-8: allocation-free, per-process durable file handle table.
//! Supports four opaque handles and basic runtime rights, not POSIX ACLs.
//! Shared files are read-only for PID2; PID1 alone may create or modify files.
//! The table is transient; all data persists through the existing R027 COW VFS.
use crate::block::{BlockDevice,BlockWindow};
use crate::durablefs::{FsError,MAX_FILE_BYTES,MIN_SECTORS};
use crate::hier_vfs::HierVfs;
use crate::vfs_path::normalize_absolute;
use crate::ata_pio::AtaPio;
use crate::batch5_session::Identity;
use crate::fs_write_guard::{WriteGuard,Statistics};
use core::marker::PhantomData;

pub const READ:u64=1;
pub const WRITE:u64=2;
/// Versioned descriptor ABI query, returned by filesystem operation 37.
pub const FS_ABI_VERSION:u16=1;
pub const FS_ABI_DESCRIPTOR_BYTES:u16=64;
pub const FS_ABI_FEATURES:u32=7; // descriptors, range locks, persistent mode permissions
pub const FS_ABI_INFO:u64=FS_ABI_VERSION as u64|
    ((FS_ABI_DESCRIPTOR_BYTES as u64)<<16)|((FS_ABI_FEATURES as u64)<<32);
const SLOTS:usize=4;
#[derive(Clone,Copy)]
struct Handle {
    token:u64, pid:u32, generation:u64, mode:u64, offset:u64,
    // Durable metadata generation at last validated I/O. A path by itself
    // is not a persistent file-object identity: unlink/recreate or rename
    // could otherwise redirect this handle to unrelated content.
    filesystem_generation:u64,
    path:[u8;24], len:usize,
    lock_kind:u8,lock_start:u64,lock_end:u64,
}
impl Handle{
    const EMPTY:Self=Self{token:0,pid:0,generation:0,mode:0,offset:0,
        filesystem_generation:0,path:[0;24],len:0,
        lock_kind:0,lock_start:0,lock_end:0};
    fn pathname(&self)->&str{
        core::str::from_utf8(&self.path[..self.len]).unwrap()
    }
}
/// Kernel-minted, per-incarnation filesystem namespace root.
#[derive(Clone,Copy)]
struct RootGrant { epoch:u64,path:[u8;24],len:usize }
impl RootGrant {
    const EMPTY:Self=Self{epoch:0,path:[0;24],len:0};
    fn as_str(&self)->&str{core::str::from_utf8(&self.path[..self.len]).unwrap()}
}
/// Opaque kernel-issued receipt for a single process-namespace revision.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct NamespaceReceipt { owner:Identity, namespace_id:u64, revision:u64 }
/// A kernel-only, read-only proof of the exact mounted volume, root
/// namespace and mount revision at the beginning of one selected-file call.
/// This snapshot is deliberately !Send and !Sync: a local BSP filesystem
/// proof must not be sent to an AP without real cross-core synchronization.
/// This protects cooperative remount/rebind paths, NOT inter-CPU locking.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct SelectedOperationGuard{
    namespace:NamespaceReceipt,
    revision:u64,
    selected_volume:Option<crate::m1_partition::PartitionReceipt>,
    _single_bsp:PhantomData<*mut ()>,
}
/// Kernel-minted alias scoped to a process incarnation and namespace revision.
/// String path comparison alone is not an authority to access a mounted tree.
#[derive(Clone,Copy)]
pub struct MountAlias {
    receipt:NamespaceReceipt, root:[u8;24], len:usize,
    // This alias remains BSP-local, never implicitly sendable to an AP.
    _single_bsp:PhantomData<*mut ()>,
    // A kernel-selected physical mount is a different authority from its
    // pathname. Every remount or recovery issues a new monotonic revision.
    volume_revision:u64,
    selected_volume:Option<crate::m1_partition::PartitionReceipt>,
}
impl MountAlias {
    fn root(&self)->&str {
        core::str::from_utf8(&self.root[..self.len]).unwrap()
    }
}
pub struct FileHandles{
    entries:[Handle;SLOTS],next:u64,
    owners:[u64;2],next_generation:u64,
    credentials:[crate::fs_permissions::Credentials;2],
    roots:[RootGrant;2],
    root_revisions:[u64;2],
    last_syscall_namespace:Option<NamespaceReceipt>,
    // Kernel-owned service-wide budget: closing/opening handles must NOT
    // reset it. Process epochs do not implement general per-user ACLs.
    write_guard:WriteGuard,
    // One selected partition receipt per kernel-owned file service session.
    // A stale descriptor may NEVER silently move to a different disk that
    // happens to reuse the same filesystem generation and pathname.
    selected_volume:Option<crate::m1_partition::PartitionReceipt>,
    // A selected physical volume's write error may have reached the
    // controller despite returning failure. Do not reuse any cached file
    // handles until privileged, full-receipt remount verification succeeds.
    selected_quarantined:bool,
    volume_revision:u64,
}
impl FileHandles{
    /// Private kernel staging copy for all-or-none retirement publication.
    /// Do not publish two independent live copies of a FileHandles service:
    /// the descriptor-token and write-budget counters must remain singular.
    /// Callers must hold the same single-BSP scheduler admission permit for
    /// snapshot, validation and publication.
    pub(crate) fn staged_copy_kernel(&self)->Self{
        Self{
            entries:self.entries,next:self.next,
            owners:self.owners,next_generation:self.next_generation,
            credentials:self.credentials,
            roots:self.roots,root_revisions:self.root_revisions,
            last_syscall_namespace:self.last_syscall_namespace,
            write_guard:self.write_guard.clone(),
            selected_volume:self.selected_volume,
            selected_quarantined:self.selected_quarantined,
            volume_revision:self.volume_revision,
        }
    }
    pub const fn new()->Self{
        Self{entries:[Handle::EMPTY;SLOTS],next:1,
            owners:[0;2],next_generation:1,
            credentials:[crate::fs_permissions::Credentials::kernel(0,0);2],
            roots:[RootGrant::EMPTY;2],
            root_revisions:[0;2],last_syscall_namespace:None,
            write_guard:WriteGuard::new(),selected_volume:None,
            selected_quarantined:false,volume_revision:1}
    }
    pub fn security_statistics(&self)->Statistics{
        self.write_guard.statistics()
    }

    /// R128 guest audit: unlike retired-owner token tests, this exercises an
    /// ACTIVE kernel-authenticated PID2 through the same file syscall service
    /// while real live handles can exist. The candidate values start at the
    /// next never-issued token, so not even a valid handle's cursor is used.
    /// All 100 malformed calls must reject before ATA probing or mutation.
    ///
    /// SAFETY CONTRACT: only called while the kernel BSP's R120 filesystem
    /// permit excludes scheduler retirement and other file-service writers.
    #[cfg(feature="test-r031-files-gui")]
    pub(crate) fn r128_audit_active_forged_handles(
        &mut self,owner:Identity
    )->bool{
        if owner.pid!=2||!self.owns(owner)||self.selected_quarantined{
            return false;
        }
        let prior=self.staged_copy_kernel();
        let prior_statistics=self.security_statistics();
        let prior_namespace=self.last_syscall_namespace;
        let mut buffer=[0xa5u8;72];
        for delta in 0..20u64 {
            let Some(token)=self.next.checked_add(delta)else{return false;};
            if token==0 || self.entries.iter().any(|h|h.token==token){
                return false;
            }
            for action in [2u64,3,4,5,6]{
                if operation_authenticated(self,action,owner,token,0,
                    "",&mut buffer)!=Err(FsError::InvalidName){
                    return false;
                }
                if buffer!=[0xa5u8;72] {return false;}
            }
        }
        self.entries.iter().zip(prior.entries.iter()).all(|(a,b)|
            a.token==b.token && a.pid==b.pid &&
            a.generation==b.generation && a.mode==b.mode &&
            a.offset==b.offset &&
            a.filesystem_generation==b.filesystem_generation &&
            a.path==b.path && a.len==b.len) &&
        self.next==prior.next &&
        self.owners==prior.owners &&
        self.roots.iter().zip(prior.roots.iter()).all(|(a,b)|
            a.epoch==b.epoch&&a.len==b.len&&a.path==b.path) &&
        self.root_revisions==prior.root_revisions &&
        self.last_syscall_namespace==prior_namespace &&
        self.volume_revision==prior.volume_revision &&
        self.selected_volume==prior.selected_volume &&
        self.selected_quarantined==prior.selected_quarantined &&
        self.security_statistics()==prior_statistics &&
        self.owns(owner)
    }

    /// Kernel-only tightening of the single mounted file service's write
    /// budget. A signed-in PID1 incarnation can REDUCE limits, never reset,
    /// widen or transfer consumed credits by closing descriptors or reusing
    /// a PID after restart. All existing handles share this same guard.
    pub fn narrow_service_write_budget(&mut self,admin:Identity,
        operations:u32,bytes:u32)->bool{
        if admin.pid!=1||!self.owns(admin)||self.selected_quarantined{
            return false;
        }
        self.write_guard.reduce_limits(operations,bytes)
    }
    /// Bind a freshly prepared kernel process incarnation. The persistent
    /// runtime table must not be reset between scheduler invocations.
    /// Exhaustion leaves the previous owner and its handles unchanged.
    pub fn bind_process(&mut self,pid:u32)->Result<Identity,FsError>{
        if !(1..=2).contains(&pid){return Err(FsError::InvalidName);}
        let generation=self.next_generation;
        let next=generation.checked_add(1).ok_or(FsError::Full)?;
        if generation==0{return Err(FsError::Full);}
        for h in &mut self.entries{
            if h.pid==pid{*h=Handle::EMPTY;}
        }
        self.owners[(pid-1) as usize]=generation;
        self.credentials[(pid-1) as usize]=if pid==1{
            crate::fs_permissions::Credentials::kernel(0,0)
        }else{crate::fs_permissions::Credentials::kernel(1000,1000)};
        // Default roots are minted only by the trusted kernel process bind.
        // PID2 is read-only and cannot escape the /shared namespace.
        let mut grant=RootGrant::EMPTY;
        let default=if pid==1{b"/".as_slice()}else{b"/shared".as_slice()};
        grant.path[..default.len()].copy_from_slice(default);
        grant.len=default.len();
        grant.epoch=generation;
        self.roots[(pid-1) as usize]=grant;
        self.root_revisions[(pid-1) as usize]=1;
        self.next_generation=next;
        Ok(Identity{pid,generation})
    }
    pub fn owns(&self,owner:Identity)->bool{
        (1..=2).contains(&owner.pid)&&owner.generation!=0&&
            self.owners[(owner.pid-1) as usize]==owner.generation
    }
    /// Trusted credential admission, never a userspace UID claim. Revoke old
    /// handles before changing identity; rebinding starts from role defaults.
    pub(crate) fn set_credentials_kernel(&mut self,owner:Identity,uid:u16,gid:u16)
        ->Result<(),FsError>{
        if !self.owns(owner){return Err(FsError::InvalidName);}
        let revision=self.root_revisions[(owner.pid-1) as usize].checked_add(1)
            .ok_or(FsError::Full)?;
        for handle in &mut self.entries{
            if handle.pid==owner.pid&&handle.generation==owner.generation{*handle=Handle::EMPTY;}
        }
        self.credentials[(owner.pid-1) as usize]=crate::fs_permissions::Credentials::kernel(uid,gid);
        self.root_revisions[(owner.pid-1) as usize]=revision;
        Ok(())
    }
    fn credentials_for(&self,owner:Identity)->Result<crate::fs_permissions::Credentials,FsError>{
        if !self.owns(owner){return Err(FsError::InvalidName);}
        Ok(self.credentials[(owner.pid-1) as usize])
    }
    /// Delayed/repeated teardown cannot retire a replacement with the same PID.
    pub fn retire_process(&mut self,owner:Identity)->bool{
        if !self.owns(owner){return false;}
        self.owners[(owner.pid-1) as usize]=0;
        self.roots[(owner.pid-1) as usize]=RootGrant::EMPTY;
        self.root_revisions[(owner.pid-1) as usize]=0;
        for h in &mut self.entries{
            if h.pid==owner.pid&&h.generation==owner.generation{
                *h=Handle::EMPTY;
            }
        }
        true
    }
    /// Narrow only: a process can never widen its root or reset the grant
    /// by reopening a file. Every existing handle is revoked before the
    /// new policy is exposed to the authenticated service.
    pub fn narrow_root_kernel(&mut self,owner:Identity,root:&str)
        ->Result<(),FsError>{
        if !self.owns(owner){return Err(FsError::InvalidName);}
        let mut normalized=[0u8;24];
        let len=normalize_absolute(root,&mut normalized[..23])
            .map_err(|_|FsError::InvalidName)?;
        let root=core::str::from_utf8(&normalized[..len])
            .map_err(|_|FsError::InvalidName)?;
        let slot=(owner.pid-1) as usize;
        if crate::vfs_path::is_within_tree(root,self.roots[slot].as_str())!=Ok(true){
            return Err(FsError::InvalidName);
        }
        let revision=self.root_revisions[slot].checked_add(1)
            .ok_or(FsError::Full)?;
        self.roots[slot]=RootGrant{epoch:owner.generation,path:normalized,len};
        self.root_revisions[slot]=revision;
        for handle in &mut self.entries{
            if handle.pid==owner.pid && handle.generation==owner.generation {
                *handle=Handle::EMPTY;
            }
        }
        Ok(())
    }
    pub fn namespace_receipt(&self,owner:Identity)
        ->Result<NamespaceReceipt,FsError>{
        if !self.owns(owner){return Err(FsError::InvalidName);}
        let revision=self.root_revisions[(owner.pid-1) as usize];
        if revision==0{return Err(FsError::InvalidName);}
        // Each process incarnation is minted a distinct namespace ID by
        // the kernel owner-generation allocator. Revision alone is not
        // sufficient to prevent a same-number cross-process transplant.
        let namespace_id=self.roots[(owner.pid-1) as usize].epoch;
        if namespace_id!=owner.generation{return Err(FsError::InvalidName);}
        Ok(NamespaceReceipt{owner,namespace_id,revision})
    }
    pub fn namespace_receipt_is_current(&self,receipt:NamespaceReceipt)->bool{
        self.namespace_receipt(receipt.owner)==Ok(receipt)
    }
    /// The trusted kernel creates an alias only beneath the current root.
    /// Never follow a symlink or resolve user-controlled mount spelling here.
    pub fn mount_alias_kernel(&self,owner:Identity,root:&str)
        ->Result<MountAlias,FsError>{
        let receipt=self.namespace_receipt(owner)?;
        let mut path=[0u8;24];
        let len=normalize_absolute(root,&mut path[..23])
            .map_err(|_|FsError::InvalidName)?;
        let canonical=core::str::from_utf8(&path[..len])
            .map_err(|_|FsError::InvalidName)?;
        if !self.root_allows(owner,canonical){return Err(FsError::InvalidName);}
        Ok(MountAlias{receipt,root:path,len,
            volume_revision:self.volume_revision,
            selected_volume:self.selected_volume,
            _single_bsp:PhantomData})
    }
    /// A trusted kernel entry point may mint only the caller's currently
    /// bound namespace root. It never accepts a ring-3 selected mount path.
    pub fn mount_alias_for_current_root_kernel(&self,owner:Identity)
        ->Result<MountAlias,FsError>{
        let receipt=self.namespace_receipt(owner)?;
        let root=self.roots[(owner.pid-1) as usize];
        if root.epoch!=owner.generation||root.len==0{
            return Err(FsError::InvalidName)
        }
        Ok(MountAlias{receipt,root:root.path,len:root.len,
            volume_revision:self.volume_revision,
            selected_volume:self.selected_volume,
            _single_bsp:PhantomData})
    }
    /// Authenticate both the alias origin and the target for every operation.
    /// Any PID reuse or root revision revokes previously minted aliases.
    pub fn mount_alias_allows(&self,alias:MountAlias,owner:Identity,path:&str)->bool{
        alias.receipt.owner==owner &&
        self.namespace_receipt_is_current(alias.receipt) &&
        !self.selected_quarantined &&
        alias.volume_revision==self.volume_revision &&
        alias.selected_volume==self.selected_volume &&
        self.root_allows(owner,path) &&
        crate::vfs_path::is_within_tree(path,alias.root())==Ok(true)
    }
    /// Last admitted syscall's namespace snapshot. Stale receipts never
    /// authorize access after a root narrowing or PID incarnation change.
    pub fn last_syscall_namespace(&self)->Option<NamespaceReceipt>{
        self.last_syscall_namespace
    }
    /// Freeze exact namespace and selected partition state, without disk I/O.
    /// Callers must re-check after any fallible mount/recovery probe.
    pub fn selected_operation_guard(
        &self,owner:Identity
    )->Result<SelectedOperationGuard,FsError>{
        if self.selected_quarantined{return Err(FsError::InvalidVolume);}
        Ok(SelectedOperationGuard{
            namespace:self.namespace_receipt(owner)?,
            revision:self.volume_revision,
            selected_volume:self.selected_volume,
            _single_bsp:PhantomData,
        })
    }
    pub fn selected_operation_guard_is_current(
        &self,guard:SelectedOperationGuard
    )->bool{
        !self.selected_quarantined &&
        self.namespace_receipt_is_current(guard.namespace) &&
        self.volume_revision==guard.revision &&
        self.selected_volume==guard.selected_volume
    }

    fn root_allows(&self,owner:Identity,path:&str)->bool{
        if owner.generation==0{return true;} // legacy non-authenticated compatibility path
        if !self.owns(owner){return false;}
        let root=self.roots[(owner.pid-1) as usize];
        root.epoch==owner.generation &&
        crate::vfs_path::is_within_tree(path,root.as_str())==Ok(true)
    }
    fn index(&self,owner:Identity,token:u64)->Result<usize,FsError>{
        if token==0{return Err(FsError::InvalidName);}
        self.entries.iter().position(|e|e.token==token&&e.pid==owner.pid&&
            e.generation==owner.generation).ok_or(FsError::InvalidName)
    }
    fn policy(pid:u32,path:&str,mode:u64)->bool{
        match pid{
            // PID1 is trusted init/service owner. Rights requested by a
            // particular handle can still only be reduced, not escalated.
            1=>(1..=3).contains(&mode),
            // PID2 reads *only* explicitly shared subtree files.
            2=>mode==READ && crate::vfs_path::is_within_tree(path,"/shared")==Ok(true)
                && path!="/shared",
            _=>false,
        }
    }
    /// Destination authority is independent of the source descriptor. A
    /// writable source inside a narrowed root never grants rename/swap access
    /// to a sibling tree or a globally writable but foreign namespace.
    fn write_destination<'a>(&self,owner:Identity,target:&str,
        output:&'a mut[u8;24])->Result<&'a str,FsError>{
        let len=normalize_absolute(target,&mut output[..23])
            .map_err(|_|FsError::InvalidName)?;
        let target=core::str::from_utf8(&output[..len])
            .map_err(|_|FsError::InvalidName)?;
        if !Self::policy(owner.pid,target,WRITE)||!self.root_allows(owner,target)||
           WriteGuard::allow_path(target).is_err(){return Err(FsError::InvalidName);}
        Ok(target)
    }
    // The current COW identity is volume-wide. Every mutation changes its
    // generation, including descriptor edits to an unrelated file. Preserve
    // foreign open descriptors rather than turning them into stale tokens.
    fn has_foreign_descriptors(&self,owner:Identity)->bool{
        self.entries.iter().any(|handle|handle.token!=0 &&
            (handle.pid!=owner.pid||handle.generation!=owner.generation))
    }
    fn descriptor_action_mutates(action:u64)->bool{
        matches!(action,3|6..=13|16|24|26|28..=32|38)
    }
    fn lock_range(argument:u64)->Result<(u64,u64),FsError>{
        let start=argument&0xffff_ffff;let length=argument>>32;
        let end=if length==0{MAX_FILE_BYTES as u64}
            else{start.checked_add(length).ok_or(FsError::TooLarge)?};
        if start>=end||end>MAX_FILE_BYTES as u64{return Err(FsError::TooLarge);}
        Ok((start,end))
    }
    fn conflicting_lock(&self,owner:Identity,path:&str,start:u64,end:u64,write:bool)->bool{
        self.entries.iter().any(|peer|peer.token!=0&&peer.lock_kind!=0&&
            (peer.pid!=owner.pid||peer.generation!=owner.generation)&&
            peer.pathname()==path&&start<peer.lock_end&&peer.lock_start<end&&
            (write||peer.lock_kind==2))
    }
    /// Locks are cached capabilities, owned by the descriptor's authenticated
    /// process incarnation. Closing/revoking a handle removes its lock too.
    fn descriptor_control(&mut self,action:u64,owner:Identity,token:u64,
        argument:u64,path:&str,buffer:&[u8])->Result<u64,FsError>{
        if !self.owns(owner)||!path.is_empty()||!buffer.is_empty(){
            return Err(FsError::InvalidName);
        }
        if self.selected_volume.is_some(){self.selected_operation_guard(owner)?;}
        if action==37{
            if token!=0||argument!=0{return Err(FsError::InvalidName);}
            return Ok(FS_ABI_INFO);
        }
        let slot=self.index(owner,token)?;let handle=self.entries[slot];
        if !self.root_allows(owner,handle.pathname()){return Err(FsError::InvalidName);}
        if action==36{
            if argument!=0{return Err(FsError::InvalidName);}
            self.entries[slot].lock_kind=0;
            self.entries[slot].lock_start=0;self.entries[slot].lock_end=0;
            return Ok(0);
        }
        if !matches!(action,34|35)||(action==34&&handle.mode&READ==0)||
            (action==35&&handle.mode&WRITE==0){return Err(FsError::InvalidName);}
        let (start,end)=Self::lock_range(argument)?;
        if self.conflicting_lock(owner,handle.pathname(),start,end,action==35){
            return Ok(1); // contention preserves the caller's prior lock
        }
        self.entries[slot].lock_kind=if action==34{1}else{2};
        self.entries[slot].lock_start=start;self.entries[slot].lock_end=end;
        Ok(0)
    }
    /// Trusted preflight on a selected physical volume: refuse forged
    /// handles, foreign process epochs and protected namespace requests
    /// BEFORE disk discovery, mount, filesystem reads or any DMA-backed I/O.
    /// The same checks are repeated by the individual operation after mount.
    fn preflight_selected(&self,action:u64,owner:Identity,token:u64,
                          argument:u64,path:&str,buffer:&[u8])
                          ->Result<(),FsError>{
        if !self.owns(owner){return Err(FsError::InvalidName);}
        if action==38{
            let handle=self.entries[self.index(owner,token)?];
            if owner.pid!=1||!path.is_empty()||!buffer.is_empty()||argument>0o777||
               !self.root_allows(owner,handle.pathname())||
               WriteGuard::allow_path(handle.pathname()).is_err()||self.has_foreign_descriptors(owner){
                return Err(FsError::InvalidName);
            }
            return Ok(());
        }
        if matches!(action,34..=37){
            if !path.is_empty()||!buffer.is_empty(){return Err(FsError::InvalidName);}
            if action==37{return if token==0&&argument==0{Ok(())}else{Err(FsError::InvalidName)};}
            let handle=self.entries[self.index(owner,token)?];
            if !self.root_allows(owner,handle.pathname())||
                (action==34&&handle.mode&READ==0)||(action==35&&handle.mode&WRITE==0){
                return Err(FsError::InvalidName);
            }
            if action==36{return if argument==0{Ok(())}else{Err(FsError::InvalidName)};}
            Self::lock_range(argument)?;return Ok(());
        }
        match action{
            1|29=>{
                if token!=0{return Err(FsError::InvalidName);}
                if action==29 && (owner.pid!=1||argument!=0||buffer.len()>256){
                    return Err(FsError::InvalidName);
                }
                let mode=if action==1{argument}else{WRITE};
                let mut name=[0u8;24];
                let len=normalize_absolute(path,&mut name[..23])
                    .map_err(|_|FsError::InvalidName)?;
                let name=core::str::from_utf8(&name[..len])
                    .map_err(|_|FsError::InvalidName)?;
                if !Self::policy(owner.pid,name,mode) ||
                   !self.root_allows(owner,name) ||
                   (mode&WRITE!=0 && WriteGuard::allow_path(name).is_err()){
                    return Err(FsError::InvalidName);
                }
            },
            32=>{
                if owner.pid!=1||token!=0||argument!=0||
                   buffer.is_empty()||buffer.len()>23{
                    return Err(FsError::InvalidName);
                }
                let target=core::str::from_utf8(buffer)
                    .map_err(|_|FsError::InvalidName)?;
                let mut src=[0u8;24];let mut dst=[0u8;24];
                let a=normalize_absolute(path,&mut src[..23])
                    .map_err(|_|FsError::InvalidName)?;
                let b=normalize_absolute(target,&mut dst[..23])
                    .map_err(|_|FsError::InvalidName)?;
                let source=core::str::from_utf8(&src[..a])
                    .map_err(|_|FsError::InvalidName)?;
                let dest=core::str::from_utf8(&dst[..b])
                    .map_err(|_|FsError::InvalidName)?;
                if !self.root_allows(owner,source)||!self.root_allows(owner,dest)||
                   !source.starts_with("/shared/")||
                   !dest.starts_with("/shared/")||
                   WriteGuard::allow_path(source).is_err()||
                   WriteGuard::allow_path(dest).is_err(){
                    return Err(FsError::InvalidName);
                }
            },
            2..=28|30|31=>{
                let slot=self.index(owner,token)?;
                let handle=self.entries[slot];
                // An old descriptor cannot survive an out-of-band namespace
                // root change. Fail BEFORE selected ATA/media probing rather
                // than trusting the pathname recorded at its original open.
                // Normal narrow_root_kernel already revokes those handles;
                // this is a second independent admission barrier.
                if !self.root_allows(owner,handle.pathname()){
                    return Err(FsError::InvalidName);
                }
                // Inspect only the kernel-issued token and its immutable
                // access mode. Never trust user-supplied "requested" rights.
                let needs_read=matches!(action,2|14|15|17..=23|25);
                let needs_write=matches!(action,3|6|7..=13|16|24|26|27|28|30|31);
                if (needs_read&&handle.mode&READ==0) ||
                   (needs_write&&handle.mode&WRITE==0){
                    return Err(FsError::InvalidName);
                }
                if matches!(action,13|16|31){
                    let mut target=[0u8;24];
                    self.write_destination(owner,path,&mut target)?;
                }
            },
            _=>return Err(FsError::InvalidName),
        }
        // Check after authentication, but before capacity, GPT or payload I/O.
        // Fsync does not publish a generation and remains legal with readers.
        if self.selected_volume.is_some()&&Self::descriptor_action_mutates(action)&&
           self.has_foreign_descriptors(owner){
            return Err(FsError::InvalidName);
        }
        Ok(())
    }
    pub fn open<D:BlockDevice>(&mut self,pid:u32,path:&str,mode:u64,
                               fs:&mut HierVfs<D>)->Result<u64,FsError>{
        self.open_for(Identity{pid,generation:0},path,mode,fs)
    }
    fn open_for<D:BlockDevice>(&mut self,owner:Identity,path:&str,mode:u64,
                               fs:&mut HierVfs<D>)->Result<u64,FsError>{
        // Authorize and retain the same bounded canonical spelling that the
        // VFS resolves. Reject traversal rather than resolving it, even when
        // its destination would otherwise be authorized.
        let mut bytes=[0u8;24];
        let len=normalize_absolute(path,&mut bytes[..23])
            .map_err(|_|FsError::InvalidName)?;
        let path=core::str::from_utf8(&bytes[..len])
            .map_err(|_|FsError::InvalidName)?;
        if !Self::policy(owner.pid,path,mode)||
           !self.root_allows(owner,path){
            return Err(FsError::InvalidName);
        }
        // S0: even trusted PID1's ordinary application handle cannot
        // write protected OS/boot/recovery paths. Existing read-only
        // access remains available for recovery and inspection.
        if mode&WRITE!=0&&WriteGuard::allow_path(path).is_err(){
            self.write_guard.deny();
            return Err(FsError::InvalidName);
        }
        let slot=self.entries.iter().position(|e|e.token==0)
            .ok_or(FsError::Full)?;
        // A zero-byte range read validates path, parent directories and the
        // complete data checksum before creating a usable handle.
        fs.read_at(path,0,&mut [])?;
        let token=self.next;
        self.next=self.next.checked_add(1).ok_or(FsError::Full)?;
        self.entries[slot]=Handle{token,pid:owner.pid,generation:owner.generation,
            mode,offset:0,filesystem_generation:fs.generation(),path:bytes,len,
            lock_kind:0,lock_start:0,lock_end:0};
        Ok(token)
    }
    pub fn read<D:BlockDevice>(&mut self,pid:u32,token:u64,
                              out:&mut [u8],fs:&mut HierVfs<D>)
                              ->Result<usize,FsError>{
        self.read_for(Identity{pid,generation:0},token,out,fs)
    }
    fn read_for<D:BlockDevice>(&mut self,owner:Identity,token:u64,
                              out:&mut [u8],fs:&mut HierVfs<D>)
                              ->Result<usize,FsError>{
        if out.is_empty()||out.len()>72{return Err(FsError::TooLarge);}
        let slot=self.index(owner,token)?;
        let h=self.entries[slot];
        if h.mode&READ==0 || h.filesystem_generation!=fs.generation(){
            // Never let a stale path handle acquire a newly created object.
            // In the bounded COW implementation this is a conservative
            // volume generation check: an unrelated mutation also requires
            // closing and reopening the handle.
            return Err(FsError::InvalidName);
        }
        let end=h.offset.checked_add(out.len() as u64).ok_or(FsError::TooLarge)?;
        if self.conflicting_lock(owner,h.pathname(),h.offset,end,false){return Err(FsError::Full);}
        // Stage the read into a bounded private buffer. The underlying
        // filesystem may return an I/O error after touching its destination:
        // no prefix or corrupt device bytes may leak to the caller.
        let mut scratch=[0u8;72];
        let n=match fs.read_at(h.pathname(),h.offset,&mut scratch[..out.len()]){
            Ok(n)=>n,
            Err(error)=>{
                // An I/O or damaged-volume error makes the on-disk identity
                // uncertain. The caller must explicitly reopen on verified
                // storage; a subsequent read cannot replay a stale token.
                if matches!(error,FsError::Io|FsError::InvalidVolume){
                    self.entries[slot]=Handle::EMPTY;
                }
                return Err(error);
            }
        };
        let next=h.offset.checked_add(n as u64)
            .ok_or(FsError::TooLarge)?;
        out[..n].copy_from_slice(&scratch[..n]);
        self.entries[slot].offset=next;
        Ok(n)
    }
    pub fn write<D:BlockDevice>(&mut self,pid:u32,token:u64,
                               input:&[u8],fs:&mut HierVfs<D>)
                               ->Result<usize,FsError>{
        self.write_for(Identity{pid,generation:0},token,input,fs)
    }
    fn write_for<D:BlockDevice>(&mut self,owner:Identity,token:u64,
                               input:&[u8],fs:&mut HierVfs<D>)
                               ->Result<usize,FsError>{
        if input.is_empty()||input.len()>256{return Err(FsError::TooLarge);}
        let slot=self.index(owner,token)?;
        let h=self.entries[slot];
        if h.mode&WRITE==0 || h.filesystem_generation!=fs.generation(){
            // No write, quota charge or cursor advancement on an old
            // volume snapshot; require explicit fresh object selection.
            return Err(FsError::InvalidName);
        }
        let end=h.offset.checked_add(input.len() as u64).ok_or(FsError::TooLarge)?;
        if self.conflicting_lock(owner,h.pathname(),h.offset,end,true){return Err(FsError::Full);}
        // Authorize immediately before mutation, not only on open.
        // The budget is shared by all handles and survives close/reopen.
        if self.write_guard.preflight(h.pathname(),input.len()).is_err(){
            return Err(FsError::InvalidName);
        }
        // Charge *only* after a successful bounded, durable write.
        let n=match fs.write_at(h.pathname(),h.offset,input){
            Ok(n)=>n,
            Err(error)=>{
                // An interrupted payload/metadata write may have changed
                // durable state even when no success was reported. Retire
                // the handle before a retry can reuse stale assumptions.
                if matches!(error,FsError::Io|FsError::InvalidVolume){
                    self.entries[slot]=Handle::EMPTY;
                }
                return Err(error);
            }
        };
        self.write_guard.completed(n);
        self.entries[slot].offset=h.offset.checked_add(n as u64)
            .ok_or(FsError::TooLarge)?;
        self.entries[slot].filesystem_generation=fs.generation();
        Ok(n)
    }
    /// Resize through an existing write capability. The cursor is preserved,
    /// including when it lies beyond the new EOF; subsequent reads return EOF.
    fn truncate_for<D:BlockDevice>(&mut self,owner:Identity,token:u64,
        length:u64,fs:&mut HierVfs<D>)->Result<(),FsError>{
        if length>MAX_FILE_BYTES as u64{return Err(FsError::TooLarge);}
        let slot=self.index(owner,token)?;
        let h=self.entries[slot];
        if h.mode&WRITE==0||h.filesystem_generation!=fs.generation(){
            return Err(FsError::InvalidName);
        }
        // Read/CRC verification must succeed before quota admission or writes.
        let old=match fs.file_len(h.pathname()){
            Ok(n)=>n,
            Err(error)=>{
                if matches!(error,FsError::Io|FsError::InvalidVolume){
                    self.entries[slot]=Handle::EMPTY;
                }
                return Err(error);
            }
        };
        let changed=old.abs_diff(length as usize);
        if changed==0{return Ok(());}
        // Account for removed bytes as well as zero-extension, so shrinking
        // cannot evade the same service-wide mutation budget as writes.
        if self.write_guard.preflight(h.pathname(),changed).is_err(){
            return Err(FsError::InvalidName);
        }
        if let Err(error)=fs.truncate(h.pathname(),length){
            if matches!(error,FsError::Io|FsError::InvalidVolume){
                self.entries[slot]=Handle::EMPTY;
            }
            return Err(error);
        }
        self.write_guard.completed(changed);
        self.entries[slot].filesystem_generation=fs.generation();
        Ok(())
    }
    pub fn seek(&mut self,pid:u32,token:u64,offset:u64)->Result<u64,FsError>{
        self.seek_for(Identity{pid,generation:0},token,offset)
    }
    fn seek_for(&mut self,owner:Identity,token:u64,offset:u64)->Result<u64,FsError>{
        if offset>MAX_FILE_BYTES as u64{return Err(FsError::TooLarge);}
        let slot=self.index(owner,token)?;
        self.entries[slot].offset=offset;
        Ok(offset)
    }
    pub fn close(&mut self,pid:u32,token:u64)->Result<(),FsError>{
        self.close_for(Identity{pid,generation:0},token)
    }
    fn close_for(&mut self,owner:Identity,token:u64)->Result<(),FsError>{
        let slot=self.index(owner,token)?;
        self.entries[slot]=Handle::EMPTY;
        Ok(())
    }
    /// Cursor changes and capability release do not access the file's data.
    /// In particular a reader must be able to release its selected-volume
    /// lease while the device is offline; CLOSE is not an implicit fsync.
    fn cached_selected_descriptor_operation(&mut self,action:u64,owner:Identity,
        token:u64,argument:u64,path:&str,buffer:&[u8])->Result<u64,FsError>{
        self.preflight_selected(action,owner,token,argument,path,buffer)?;
        let guard=self.selected_operation_guard(owner)?;
        if guard.selected_volume.is_none(){return Err(FsError::InvalidVolume);}
        if !path.is_empty()||!buffer.is_empty()||
           (action==5&&argument!=0){return Err(FsError::InvalidName);}
        match action{
            4=>self.seek_for(owner,token,argument),
            5=>{self.close_for(owner,token)?;Ok(0)},
            _=>Err(FsError::InvalidName),
        }
    }
    pub fn terminate(&mut self,pid:u32){
        for h in &mut self.entries{
            if h.pid==pid&&h.generation==0{*h=Handle::EMPTY;}
        }
    }
    /// Legacy PID entrypoints use generation zero and cannot access handles
    /// created by the authenticated runtime route.
    pub fn dispatch<D:BlockDevice>(&mut self,action:u64,pid:u32,
                                   token:u64,argument:u64,path:&str,
                                   buffer:&mut [u8],fs:&mut HierVfs<D>)
                                   ->Result<u64,FsError>{
        self.dispatch_for(action,Identity{pid,generation:0},token,argument,path,buffer,fs)
    }
    pub fn dispatch_authenticated<D:BlockDevice>(&mut self,action:u64,owner:Identity,
        token:u64,argument:u64,path:&str,buffer:&mut [u8],fs:&mut HierVfs<D>)
        ->Result<u64,FsError>{
        if !self.owns(owner){return Err(FsError::InvalidName);}
        self.dispatch_for(action,owner,token,argument,path,buffer,fs)
    }

    /// R094 protected, kernel-issued handle mutations; this is reachable only
    /// through an authenticated owner or a legacy generation-zero handle.
    /// A failure that makes file identity uncertain permanently retires token.
    fn r094_edit_for<D:BlockDevice>(&mut self,action:u64,owner:Identity,
        token:u64,argument:u64,target:&str,buffer:&[u8],
        fs:&mut HierVfs<D>)->Result<u64,FsError>{
        let slot=self.index(owner,token)?;
        let h=self.entries[slot];
        if h.mode&WRITE==0 || h.filesystem_generation!=fs.generation(){
            return Err(FsError::InvalidName);
        }
        // Validate all argument lengths before accounting or disk access.
        let amount=match action{
            7|8=>{if buffer.is_empty()||buffer.len()>256{
                return Err(FsError::TooLarge);
            }buffer.len()},
            9=>{if !buffer.is_empty(){return Err(FsError::InvalidName);}
                let n=usize::try_from(argument).map_err(|_|FsError::TooLarge)?;
                if n==0||n>256{return Err(FsError::TooLarge);}n},
            10=>{if buffer.len()!=1{return Err(FsError::InvalidName);}
                let n=usize::try_from(argument).map_err(|_|FsError::TooLarge)?;
                if n==0||n>256{return Err(FsError::TooLarge);}n},
            11=>{let n=usize::try_from(argument).map_err(|_|FsError::TooLarge)?;
                if n>buffer.len()||buffer.len()>256{return Err(FsError::TooLarge);}
                buffer.len().max(1)},
            12|13=>{if buffer.len()>256{return Err(FsError::TooLarge);}
                buffer.len().max(1)},
            16=>{if !buffer.is_empty(){return Err(FsError::InvalidName);}
                1},
            _=>return Err(FsError::InvalidName),
        };
        let mut canonical_target=[0u8;24];
        let target=if matches!(action,13|16){
            // A privileged source handle must not be used to bypass S0
            // protection OR the current process's narrower namespace root.
            match self.write_destination(owner,target,&mut canonical_target){
                Ok(target)=>target,
                Err(error)=>{self.write_guard.deny();return Err(error);}
            }
        }else{target};
        if self.write_guard.preflight(h.pathname(),amount).is_err(){
            return Err(FsError::InvalidName);
        }
        let offset=usize::try_from(h.offset).map_err(|_|FsError::TooLarge)?;
        let old_len=if action==7 {
            match fs.file_len(h.pathname()){
                Ok(len)=>len,
                Err(error)=>{
                    if matches!(error,FsError::Io|FsError::InvalidVolume){
                        self.entries[slot]=Handle::EMPTY;
                    }
                    return Err(error);
                }
            }
        }else{0};
        // All mutations pass through HierVfs, which independently verifies
        // the parent, file/directory alias, CRC, and COW generation.
        let result:Result<u64,FsError>=match action{
            7=>fs.append_file(h.pathname(),buffer).map(|n|n as u64),
            8=>fs.insert_file_range(h.pathname(),offset,buffer).map(|n|n as u64),
            9=>fs.remove_file_range(h.pathname(),offset,amount).map(|n|n as u64),
            10=>fs.fill_file_range(h.pathname(),offset,amount,buffer[0]).map(|n|n as u64),
            11=>{
                let split=argument as usize;
                fs.compare_exchange_file(h.pathname(),&buffer[..split],&buffer[split..])
                  .map(|changed|u64::from(changed))
            },
            12=>fs.unlink_if_equal(h.pathname(),buffer).map(|changed|u64::from(changed)),
            13=>fs.rename_file_if_equal(h.pathname(),target,buffer)
                  .map(|changed|u64::from(changed)),
            16=>fs.swap_file_names(h.pathname(),target).map(|_|1),
            _=>Err(FsError::InvalidName),
        };
        let value=match result{
            Ok(value)=>value,
            Err(error)=>{
                if matches!(error,FsError::Io|FsError::InvalidVolume){
                    self.entries[slot]=Handle::EMPTY;
                }
                return Err(error);
            }
        };
        // Conditional mismatches are read-only (no charge, no cursor move).
        if matches!(action,11|12|13)&&value==0{return Ok(0);}
        if action==11 {
            let split=argument as usize;
            if buffer[..split]==buffer[split..]{return Ok(value);}
        }
        self.write_guard.completed(amount);
        if matches!(action,12|13|16){
            // A removed or renamed object invalidates its original token.
            self.entries[slot]=Handle::EMPTY;
        }else{
            let next=match action{
                7=>old_len.checked_add(value as usize),
                8|10=>offset.checked_add(value as usize),
                _=>Some(offset),
            }.ok_or(FsError::TooLarge)?;
            self.entries[slot].filesystem_generation=fs.generation();
            self.entries[slot].offset=next as u64;
        }
        Ok(value)
    }
    /// R094 read-only checks, bound to one authenticated live READ handle.
    fn r094_read_check_for<D:BlockDevice>(&mut self,action:u64,
        owner:Identity,token:u64,buffer:&[u8],
        fs:&mut HierVfs<D>)->Result<u64,FsError>{
        let slot=self.index(owner,token)?;
        let h=self.entries[slot];
        if h.mode&READ==0||h.filesystem_generation!=fs.generation(){
            return Err(FsError::InvalidName);
        }
        if buffer.len()>256{return Err(FsError::TooLarge);}
        let result=match action{
            14=>{if !buffer.is_empty(){return Err(FsError::InvalidName);}
                fs.crc32_file(h.pathname()).map(u64::from)},
            15=>fs.file_equals_bytes(h.pathname(),buffer).map(|b|u64::from(b)),
            _=>Err(FsError::InvalidName),
        };
        if matches!(result,Err(FsError::Io|FsError::InvalidVolume)){
            self.entries[slot]=Handle::EMPTY;
        }
        result
    }

    /// R099: read-only search/peek operations are bound to the exact live
    /// process incarnation and unchanged COW generation. No mutations or
    /// cursor changes occur in these paths. Missing match uses u64::MAX.
    fn r099_verified_read_for<D:BlockDevice>(&mut self,action:u64,owner:Identity,
        token:u64,buffer:&mut[u8],fs:&mut HierVfs<D>)->Result<u64,FsError>{
        let slot=self.index(owner,token)?;
        let h=self.entries[slot];
        if h.mode&READ==0||h.filesystem_generation!=fs.generation(){
            return Err(FsError::InvalidName);
        }
        let len=buffer.len();
        if len>256{return Err(FsError::TooLarge);}
        let result=match action{
            17|18|19=>{
                if len==0{return Err(FsError::InvalidName);}
                match action{
                    17=>fs.find_bytes_file(h.pathname(),buffer)
                       .map(|hit|hit.map(|i|i as u64).unwrap_or(u64::MAX)),
                    18=>fs.rfind_bytes_file(h.pathname(),buffer)
                       .map(|hit|hit.map(|i|i as u64).unwrap_or(u64::MAX)),
                    _=>fs.count_bytes_file(h.pathname(),buffer).map(|n|n as u64),
                }
            },
            20=>fs.has_prefix_file(h.pathname(),buffer).map(u64::from),
            21=>fs.has_suffix_file(h.pathname(),buffer).map(u64::from),
            22|23=>{
                if len==0||len>72{return Err(FsError::TooLarge);}
                // Keep caller's mutable buffer untouched if a physical
                // read, checksum, or removable-media identity fails.
                let mut staged=[0u8;72];
                let read=if action==22{
                    fs.read_head_file(h.pathname(),&mut staged[..len])
                }else{
                    fs.read_tail_file(h.pathname(),&mut staged[..len])
                };
                read.map(|n|{
                    buffer[..n].copy_from_slice(&staged[..n]);
                    n as u64
                })
            },
            _=>Err(FsError::InvalidName),
        };
        if matches!(result,Err(FsError::Io|FsError::InvalidVolume)){
            self.entries[slot]=Handle::EMPTY;
        }
        result
    }
    /// R099-08: append-at-front through an authenticated WRITE handle.
    /// Successful COW publication consumes one service-wide write budget
    /// and refreshes the handle generation. Cursor points after the prefix.
    fn r099_prepend_for<D:BlockDevice>(&mut self,owner:Identity,token:u64,
        buffer:&[u8],fs:&mut HierVfs<D>)->Result<u64,FsError>{
        if buffer.is_empty()||buffer.len()>256{return Err(FsError::TooLarge);}
        let slot=self.index(owner,token)?;
        let h=self.entries[slot];
        if h.mode&WRITE==0||h.filesystem_generation!=fs.generation(){
            return Err(FsError::InvalidName);
        }
        if self.write_guard.preflight(h.pathname(),buffer.len()).is_err(){
            return Err(FsError::InvalidName);
        }
        let n=match fs.prepend_file(h.pathname(),buffer){
            Ok(n)=>n,
            Err(error)=>{
                if matches!(error,FsError::Io|FsError::InvalidVolume){
                    self.entries[slot]=Handle::EMPTY;
                }
                return Err(error);
            }
        };
        self.write_guard.completed(n);
        self.entries[slot].filesystem_generation=fs.generation();
        self.entries[slot].offset=n as u64;
        Ok(n as u64)
    }

    /// Positioned read through the authenticated handle, never advancing
    /// cursor. Data is privately staged; errors cannot reveal partial bytes.
    fn pread_for<D:BlockDevice>(&mut self,owner:Identity,token:u64,
        offset:u64,out:&mut[u8],fs:&mut HierVfs<D>)->Result<u64,FsError>{
        if out.is_empty()||out.len()>72{return Err(FsError::TooLarge);}
        if offset>MAX_FILE_BYTES as u64{return Err(FsError::TooLarge);}
        let slot=self.index(owner,token)?;
        let h=self.entries[slot];
        if h.mode&READ==0||h.filesystem_generation!=fs.generation(){
            return Err(FsError::InvalidName);
        }
        let mut staged=[0u8;72];
        let count=match fs.read_at(h.pathname(),offset,&mut staged[..out.len()]){
            Ok(n)=>n,
            Err(error)=>{
                if matches!(error,FsError::Io|FsError::InvalidVolume){
                    self.entries[slot]=Handle::EMPTY;
                }
                return Err(error);
            }
        };
        out[..count].copy_from_slice(&staged[..count]);
        Ok(count as u64)
    }
    /// Positioned write through the authenticated handle. The shared S0
    /// quota and protected-path check are enforced at the mutation boundary.
    /// No cursor change occurs; fresh metadata generation is retained.
    fn pwrite_for<D:BlockDevice>(&mut self,owner:Identity,token:u64,
        offset:u64,input:&[u8],fs:&mut HierVfs<D>)->Result<u64,FsError>{
        if input.is_empty()||input.len()>256||offset>MAX_FILE_BYTES as u64{
            return Err(FsError::TooLarge);
        }
        let slot=self.index(owner,token)?;
        let h=self.entries[slot];
        if h.mode&WRITE==0||h.filesystem_generation!=fs.generation(){
            return Err(FsError::InvalidName);
        }
        if self.write_guard.preflight(h.pathname(),input.len()).is_err(){
            return Err(FsError::InvalidName);
        }
        let count=match fs.write_at(h.pathname(),offset,input){
            Ok(n)=>n,
            Err(error)=>{
                if matches!(error,FsError::Io|FsError::InvalidVolume){
                    self.entries[slot]=Handle::EMPTY;
                }
                return Err(error);
            }
        };
        self.write_guard.completed(count);
        self.entries[slot].filesystem_generation=fs.generation();
        Ok(count as u64)
    }
    /// Authenticated durable flush. A successful fsync verifies both
    /// checksummed metadata heads and requires an actual driver cache flush.
    /// Unsupported/failed sync retires the handle and the mounted filesystem.
    fn fsync_for<D:BlockDevice>(&mut self,owner:Identity,token:u64,
        fs:&mut HierVfs<D>)->Result<u64,FsError>{
        let slot=self.index(owner,token)?;
        let h=self.entries[slot];
        if h.mode&WRITE==0||h.filesystem_generation!=fs.generation(){
            return Err(FsError::InvalidName);
        }
        match fs.sync(){
            Ok(())=>Ok(fs.generation()),
            Err(error)=>{
                if matches!(error,FsError::Io|FsError::InvalidVolume){
                    self.entries[slot]=Handle::EMPTY;
                }
                Err(error)
            }
        }
    }
    /// Conditional write using the exact currently verified on-disk COW
    /// generation. If another writer publishes a new generation, reject
    /// without consuming budget or changing the cursor.
    fn write_if_generation_for<D:BlockDevice>(&mut self,owner:Identity,
       token:u64,expected_generation:u64,data:&[u8],
       fs:&mut HierVfs<D>)->Result<u64,FsError>{
        if expected_generation==0||fs.generation()!=expected_generation{
            return Err(FsError::InvalidVolume);
        }
        self.write_for(owner,token,data,fs).map(|n|n as u64)
    }
    /// Kernel-only authenticated file creation. Caller cannot forge PID,
    /// traversal path or protected boot namespace; quota is held across
    /// handle reopen and process rebinding. The file remains unopened until
    /// the caller explicitly requests a new read/write token.
    fn create_authenticated_for<D:BlockDevice>(&mut self,owner:Identity,
       path:&str,data:&[u8],fs:&mut HierVfs<D>)->Result<u64,FsError>{
        if data.len()>256{return Err(FsError::TooLarge);}
        let mut normalized=[0u8;24];
        let n=normalize_absolute(path,&mut normalized[..23])
            .map_err(|_|FsError::InvalidName)?;
        let canonical=core::str::from_utf8(&normalized[..n])
            .map_err(|_|FsError::InvalidName)?;
        if !Self::policy(owner.pid,canonical,WRITE)||
           !self.root_allows(owner,canonical){
            return Err(FsError::InvalidName);
        }
        let amount=data.len().max(1);
        if self.write_guard.preflight(canonical,amount).is_err(){
            return Err(FsError::InvalidName);
        }
        if owner.generation!=0{fs.create_as(self.credentials_for(owner)?,canonical,data,0o644)?;}
        else{fs.create(canonical,data)?;}
        self.write_guard.completed(amount);
        Ok(data.len() as u64)
    }
    /// Unlink only the file selected by a current authenticated WRITE
    /// handle. Once deleted, its token is retired so it cannot later access
    /// another object that reuses the same pathname.
    fn unlink_authenticated_for<D:BlockDevice>(&mut self,owner:Identity,
       token:u64,fs:&mut HierVfs<D>)->Result<u64,FsError>{
        let slot=self.index(owner,token)?;
        let h=self.entries[slot];
        if h.mode&WRITE==0||h.filesystem_generation!=fs.generation(){
            return Err(FsError::InvalidName);
        }
        if self.write_guard.preflight(h.pathname(),1).is_err(){
            return Err(FsError::InvalidName);
        }
        match fs.unlink(h.pathname()){
            Ok(())=>{
                self.entries[slot]=Handle::EMPTY;
                self.write_guard.completed(1);
                Ok(1)
            },
            Err(error)=>{
                if matches!(error,FsError::Io|FsError::InvalidVolume){
                    self.entries[slot]=Handle::EMPTY;
                }
                Err(error)
            }
        }
    }
    /// Metadata-atomic file rename through the current authenticated
    /// WRITE handle. Destination is independently checked against S0
    /// protected paths; retire the original pathname token after success.
    fn rename_authenticated_for<D:BlockDevice>(&mut self,owner:Identity,
       token:u64,target:&str,fs:&mut HierVfs<D>)->Result<u64,FsError>{
        let slot=self.index(owner,token)?;
        let h=self.entries[slot];
        if h.mode&WRITE==0||h.filesystem_generation!=fs.generation(){
            return Err(FsError::InvalidName);
        }
        let mut normalized=[0u8;24];
        let n=normalize_absolute(target,&mut normalized[..23])
            .map_err(|_|FsError::InvalidName)?;
        let to=core::str::from_utf8(&normalized[..n])
            .map_err(|_|FsError::InvalidName)?;
        if !Self::policy(owner.pid,to,WRITE)||!self.root_allows(owner,to)||
           WriteGuard::allow_path(to).is_err(){
            self.write_guard.deny();
            return Err(FsError::InvalidName);
        }
        if self.write_guard.preflight(h.pathname(),1).is_err(){
            return Err(FsError::InvalidName);
        }
        match fs.rename_file(h.pathname(),to){
            Ok(())=>{
                self.entries[slot]=Handle::EMPTY;
                self.write_guard.completed(1);
                Ok(1)
            },
            Err(error)=>{
                if matches!(error,FsError::Io|FsError::InvalidVolume){
                    self.entries[slot]=Handle::EMPTY;
                }
                Err(error)
            }
        }
    }
    /// Authenticated directory-tree rename. This is one COW metadata
    /// transaction for the parent marker and every descendant entry,
    /// never a partial sequence of per-file renames. The trusted caller
    /// must supply a fresh kernel-issued PID1 incarnation and a verified
    /// destination path; new writes outside /shared are denied by S0.
    fn rename_tree_authenticated_for<D:BlockDevice>(&mut self,
       owner:Identity,source:&str,target:&[u8],fs:&mut HierVfs<D>)
       ->Result<u64,FsError>{
        if owner.pid!=1||owner.generation==0||
           target.is_empty()||target.len()>23 {
            return Err(FsError::InvalidName);
        }
        let target_str=core::str::from_utf8(target)
            .map_err(|_|FsError::InvalidName)?;
        let mut src_buf=[0u8;24];
        let src_len=normalize_absolute(source,&mut src_buf[..23])
            .map_err(|_|FsError::InvalidName)?;
        let src=core::str::from_utf8(&src_buf[..src_len])
            .map_err(|_|FsError::InvalidName)?;
        let mut dst_buf=[0u8;24];
        let dst_len=normalize_absolute(target_str,&mut dst_buf[..23])
            .map_err(|_|FsError::InvalidName)?;
        let dst=core::str::from_utf8(&dst_buf[..dst_len])
            .map_err(|_|FsError::InvalidName)?;
        if src==dst||!self.root_allows(owner,src)||!self.root_allows(owner,dst)||
           !src.starts_with("/shared/")||
           !dst.starts_with("/shared/")||
           !Self::policy(owner.pid,src,WRITE)||
           !Self::policy(owner.pid,dst,WRITE)||
           WriteGuard::allow_path(src).is_err()||
           WriteGuard::allow_path(dst).is_err(){
            self.write_guard.deny();
            return Err(FsError::InvalidName);
        }
        if self.write_guard.preflight(src,1).is_err(){
            return Err(FsError::InvalidName);
        }
        fs.rename_dir(src,dst)?;
        self.write_guard.completed(1);
        // A child file opened under the previous tree must not follow a
        // replacement created at its former pathname. Revoke subtree
        // capabilities immediately, not only at their next generation check.
        for entry in &mut self.entries {
            if entry.token==0 {continue;}
            let pathname=entry.pathname();
            if pathname==src ||
               (pathname.starts_with(src) &&
                pathname.as_bytes().get(src.len())==Some(&b'/')){
                *entry=Handle::EMPTY;
            }
        }
        Ok(1)
    }
    fn dispatch_for<D:BlockDevice>(&mut self,action:u64,owner:Identity,
                                   token:u64,argument:u64,path:&str,
                                   buffer:&mut [u8],fs:&mut HierVfs<D>)
                                   ->Result<u64,FsError>{
        if matches!(action,34..=37){
            return self.descriptor_control(action,owner,token,argument,path,buffer);
        }
        if owner.generation!=0{
            let credentials=self.credentials_for(owner)?;
            // Check immutable namespace and S0 destination policy before
            // probing the target's durable parent. Otherwise /boot/... and
            // an out-of-root destination may report NotFound instead of
            // the mandated authorization denial, leaking lookup results.
            // This validation is read-only and never charges write quota.
            if matches!(action,13|16|29|31){
                let mut destination=[0u8;24];
                self.write_destination(owner,path,&mut destination)?;
            }
            if action==1{
                let access=(if argument&WRITE!=0{2}else{0})|(if argument&READ!=0{4}else{0});
                fs.authorize_access(credentials,path,access)?;
            }else if matches!(action,29|32){
                fs.authorize_parent(credentials,path)?;
            }else if !matches!(action,4|5){
                let handle=self.entries[self.index(owner,token)?];
                let access=if Self::descriptor_action_mutates(action){2}else{4};
                fs.authorize_access(credentials,handle.pathname(),if matches!(action,27|38){0}else{access})?;
                if matches!(action,13|16|30|31){
                    fs.authorize_parent(credentials,handle.pathname())?;
                    if matches!(action,13|16|31){fs.authorize_parent(credentials,path)?;}
                }
            }
        }
        if action==38{
            self.preflight_selected(action,owner,token,argument,path,buffer)?;
            let handle=self.entries[self.index(owner,token)?];
            if handle.filesystem_generation!=fs.generation(){return Err(FsError::InvalidName);}
            self.write_guard.preflight(handle.pathname(),1).map_err(|_|FsError::InvalidName)?;
            fs.chmod_as(self.credentials_for(owner)?,handle.pathname(),argument as u16)?;
            fs.sync()?;
            self.write_guard.completed(1);
            self.entries.fill(Handle::EMPTY);
            return Ok(0);
        }
        if !matches!(action,1..=5){
            if let Ok(slot)=self.index(owner,token){
                let h=self.entries[slot];
                let read=matches!(action,2|14|15|17..=23|25);
                if (read||Self::descriptor_action_mutates(action))&&
                    self.conflicting_lock(owner,h.pathname(),0,MAX_FILE_BYTES as u64,!read){
                    return Err(FsError::Full);
                }
            }
        }
        if self.selected_volume.is_some()&&matches!(action,4|5){
            return self.cached_selected_descriptor_operation(
                action,owner,token,argument,path,buffer);
        }
        // Repeat admission at execution as well: kernel callers using an
        // already mounted selected volume cannot bypass the physical adapter.
        // Unselected legacy volumes retain their established behavior.
        if self.selected_volume.is_some()&&Self::descriptor_action_mutates(action)&&
           self.has_foreign_descriptors(owner){return Err(FsError::InvalidName);}
        match action{
            1=>self.open_for(owner,path,argument,fs),
            2=>self.read_for(owner,token,buffer,fs).map(|n|n as u64),
            3=>self.write_for(owner,token,buffer,fs).map(|n|n as u64),
            4=>self.seek_for(owner,token,argument),
            5=>{self.close_for(owner,token)?;Ok(0)},
            6=>{self.truncate_for(owner,token,argument,fs)?;Ok(0)},
            7|8|9|10|11|12|13|16=>self.r094_edit_for(
                action,owner,token,argument,path,buffer,fs),
            14|15=>self.r094_read_check_for(action,owner,token,buffer,fs),
            17..=23=>self.r099_verified_read_for(action,owner,token,buffer,fs),
            24=>self.r099_prepend_for(owner,token,buffer,fs),
            25=>self.pread_for(owner,token,argument,buffer,fs),
            26=>self.pwrite_for(owner,token,argument,buffer,fs),
            27=>{if !buffer.is_empty()||argument!=0||!path.is_empty(){
                Err(FsError::InvalidName)
              }else{self.fsync_for(owner,token,fs)}},
            28=>{if !path.is_empty(){Err(FsError::InvalidName)}
                else{self.write_if_generation_for(owner,token,argument,buffer,fs)}},
            29=>{if token!=0||argument!=0{Err(FsError::InvalidName)}
                else{self.create_authenticated_for(owner,path,buffer,fs)}},
            30=>{if !buffer.is_empty()||argument!=0||!path.is_empty(){
                Err(FsError::InvalidName)
              }else{self.unlink_authenticated_for(owner,token,fs)}},
            31=>{if !buffer.is_empty()||argument!=0{
                Err(FsError::InvalidName)
              }else{self.rename_authenticated_for(owner,token,path,fs)}},
            32=>{if token!=0||argument!=0{
                Err(FsError::InvalidName)
              }else{self.rename_tree_authenticated_for(owner,path,buffer,fs)}},
            _=>Err(FsError::InvalidName)
        }
    }
}
pub fn operation(table:&mut FileHandles,action:u64,pid:u32,
                 token:u64,argument:u64,path:&str,buffer:&mut [u8])
                 ->Result<u64,FsError>{
    operation_for(table,action,Identity{pid,generation:0},token,argument,path,buffer)
}
/// Alias-scoped create/open cannot fall back to raw path authorization.
/// A stale or foreign mount alias is rejected before ATA probing and I/O.
pub fn operation_authenticated_mount_alias(
    table:&mut FileHandles, alias:MountAlias, action:u64, owner:Identity,
    token:u64,argument:u64,path:&str,buffer:&mut [u8]
)->Result<u64,FsError>{
    if !matches!(action,1|29)||token!=0||
       !table.mount_alias_allows(alias,owner,path){
        return Err(FsError::InvalidName);
    }
    // Independently calculate the bounded relative suffix using the R115
    // all-or-nothing canonicalization. An exact-root OPEN is never a file.
    let mut relative=[0u8;24];
    let Some(length)=crate::vfs_path::copy_relative_under_root(
        path,alias.root(),&mut relative).map_err(|_|FsError::InvalidName)?
    else{return Err(FsError::InvalidName)};
    if action==1 && length==0{return Err(FsError::InvalidName);}
    operation_authenticated(table,action,owner,token,argument,path,buffer)
}
/// Authenticate before ATA probing or filesystem reads, including open.
pub fn operation_authenticated(table:&mut FileHandles,action:u64,owner:Identity,
                 token:u64,argument:u64,path:&str,buffer:&mut [u8])
                 ->Result<u64,FsError>{
    let namespace=table.namespace_receipt(owner)?;
    table.preflight_selected(action,owner,token,argument,path,buffer)?;
    if !table.namespace_receipt_is_current(namespace){
        return Err(FsError::InvalidName);
    }
    if table.selected_volume.is_none(){
        table.last_syscall_namespace=Some(namespace);
    }
    operation_for(table,action,owner,token,argument,path,buffer)
}
fn operation_for(table:&mut FileHandles,action:u64,owner:Identity,
                 token:u64,argument:u64,path:&str,buffer:&mut [u8])
                 ->Result<u64,FsError>{
    if matches!(action,34..=37){
        return table.descriptor_control(action,owner,token,argument,path,buffer);
    }
    if let Some(receipt)=table.selected_volume{
        table.selected_operation_guard(owner)?;
        if matches!(action,4|5){
            return table.cached_selected_descriptor_operation(
                action,owner,token,argument,path,buffer);
        }
        let mut disk=probe_selected_ata(table)?;
        return operation_on_selected_volume(table,action,owner,token,argument,
            path,buffer,&mut disk,receipt);
    }
    // Ordinary desktop file calls must never inherit the scratch-volume
    // compatibility route used by the old opt-in acceptance profiles.
    #[cfg(feature="normal-desktop")]
    return Err(FsError::InvalidVolume);
    #[cfg(not(feature="normal-desktop"))]
    {
    let mut disk=unsafe{AtaPio::probe()}.map_err(|_|FsError::Io)?;
    if disk.sector_count()!=128||crate::r030_ondevice::COUNT<MIN_SECTORS{
        return Err(FsError::InvalidVolume);
    }
    let window=BlockWindow::new(&mut disk,crate::r030_ondevice::START,
                                crate::r030_ondevice::COUNT)?;
    let mut fs=HierVfs::mount(window)?;
    table.dispatch_for(action,owner,token,argument,path,buffer,&mut fs)
    }
}

fn probe_selected_ata(table:&mut FileHandles)->Result<AtaPio,FsError>{
    match unsafe{AtaPio::probe()}{
        Ok(disk)=>Ok(disk),
        Err(_)=>{
            table.selected_quarantined=true;table.entries.fill(Handle::EMPTY);
            Err(FsError::Io)
        }
    }
}

/// Admit exactly one boot-selected, already formatted healthy volume before
/// any process incarnation exists. This kernel-only boot call performs no
/// formatting, and cannot replace a running service's volume or reset quotas.
pub fn prepare_selected_volume_kernel<D:BlockDevice>(
    table:&mut FileHandles,disk:&mut D,
    receipt:crate::m1_partition::PartitionReceipt)->Result<(),FsError>{
    if table.next_generation!=1||table.owners.iter().any(|generation|*generation!=0)||
       table.entries.iter().any(|handle|handle.token!=0)||
       table.selected_volume.is_some()||table.selected_quarantined{
        return Err(FsError::InvalidName);
    }
    let revision=table.volume_revision.checked_add(1).ok_or(FsError::Full)?;
    drop(crate::m1_partition::mount_partition_receipt_healthy(disk,receipt)
        .map_err(|_|FsError::InvalidVolume)?);
    table.selected_volume=Some(receipt);
    table.volume_revision=revision;
    Ok(())
}

fn selected_path_preflight(table:&FileHandles,owner:Identity,action:u64,
    path:&str,target:&str,offset:u64,buffer_len:usize,used:usize)
    ->Result<SelectedOperationGuard,FsError>{
    let guard=table.selected_operation_guard(owner)?;
    if guard.selected_volume.is_none(){return Err(FsError::InvalidVolume);}
    if buffer_len>crate::hier_vfs::MAX_TRANSFER||used>buffer_len||
       offset>MAX_FILE_BYTES as u64||!matches!(action,1..=11){
        return Err(FsError::TooLarge);
    }
    let mut canonical=[0u8;24];
    let len=normalize_absolute(path,&mut canonical[..23])
        .map_err(|_|FsError::InvalidName)?;
    let canonical=core::str::from_utf8(&canonical[..len])
        .map_err(|_|FsError::InvalidName)?;
    if !table.root_allows(owner,canonical)||
       (owner.pid==2 && (action!=4&&action!=8&&action!=10))||
       (owner.pid==2 && crate::vfs_path::is_within_tree(canonical,"/shared")!=Ok(true)){
        return Err(FsError::InvalidName);
    }
    let mutating=!matches!(action,4|8|10);
    if mutating&&(owner.pid!=1||WriteGuard::allow_path(canonical).is_err()){
        return Err(FsError::InvalidName);
    }
    // This COW service versions the entire volume, not individual inodes.
    // A successful path mutation revokes cached descriptors, so another
    // process's active descriptor is a read lease on that volume. Reject
    // before probing media rather than silently revoke its authority. A
    // close, root revocation or process retirement releases the lease.
    if mutating && table.has_foreign_descriptors(owner) {
        return Err(FsError::InvalidName);
    }
    if matches!(action,7|9){
        let mut second=[0u8;24];
        let len=normalize_absolute(target,&mut second[..23])
            .map_err(|_|FsError::InvalidName)?;
        let second=core::str::from_utf8(&second[..len]).map_err(|_|FsError::InvalidName)?;
        if !table.root_allows(owner,second)||WriteGuard::allow_path(second).is_err(){
            return Err(FsError::InvalidName);
        }
    }else if !target.is_empty(){return Err(FsError::InvalidName);}
    if !matches!(action,10|11)&&offset!=0{return Err(FsError::InvalidName);}
    if !matches!(action,3|5|11)&&used!=0{return Err(FsError::InvalidName);}
    if action==11&&used==0{return Err(FsError::TooLarge);}
    Ok(guard)
}

/// Existing syscall32 path ABI on the admitted volume. Authentication and
/// namespace checks precede even device-capacity inspection. All output is
/// privately staged, and all successful mutations share the handle budget.
pub fn selected_path_operation_on_device<D:BlockDevice>(
    table:&mut FileHandles,owner:Identity,action:u64,path:&str,target:&str,
    offset:u64,buffer:&mut[u8],used:usize,disk:&mut D)->Result<usize,FsError>{
    let guard=selected_path_preflight(table,owner,action,path,target,offset,buffer.len(),used)?;
    let receipt=guard.selected_volume.ok_or(FsError::InvalidVolume)?;
    let mutating=!matches!(action,4|8|10);
    let charge=if matches!(action,3|5|11){used.max(1)}else{1};
    if mutating&&table.write_guard.preflight(path,charge).is_err(){
        return Err(FsError::InvalidName);
    }
    let mut volume=match crate::m1_partition::mount_partition_receipt_healthy(disk,receipt){
        Ok(volume)=>volume,
        Err(_)=>{
            table.selected_quarantined=true;table.entries.fill(Handle::EMPTY);
            return Err(FsError::InvalidVolume);
        }
    };
    if !table.selected_operation_guard_is_current(guard){return Err(FsError::InvalidName);}
    let credentials=table.credentials_for(owner)?;
    // Authorization can itself perform metadata and directory reads on the
    // selected device. A transport failure or corrupt mount at this stage
    // is NOT a normal permission denial: quarantine the entire selected
    // volume and revoke cached handles before returning to ring 3.
    let authorization=(|| -> Result<(),FsError>{
        match action{
            1|3=>volume.authorize_parent(credentials,path)?,
            2|6=>{volume.authorize_parent(credentials,path)?;volume.authorize_access(credentials,path,0)?;},
            7|9=>{volume.authorize_parent(credentials,path)?;volume.authorize_parent(credentials,target)?;
                   volume.authorize_access(credentials,path,0)?;},
            4|10=>volume.authorize_access(credentials,path,crate::fs_permissions::READ)?,
            8=>volume.authorize_access(credentials,path,crate::fs_permissions::READ|crate::fs_permissions::SEARCH)?,
            5|11=>volume.authorize_access(credentials,path,crate::fs_permissions::WRITE)?,
            _=>return Err(FsError::InvalidName),
        }
        Ok(())
    })();
    if let Err(error)=authorization{
        if matches!(error,FsError::Io|FsError::InvalidVolume){
            table.selected_quarantined=true;
            table.entries.fill(Handle::EMPTY);
        }
        return Err(error);
    }
    let mut scratch=[0u8;crate::hier_vfs::MAX_TRANSFER];
    if matches!(action,3|5|11){scratch[..used].copy_from_slice(&buffer[..used]);}
    let outcome=match action{
        1=>volume.mkdir_as(credentials,path,0o755).map(|_|0),
        3=>volume.create_as(credentials,path,&scratch[..used],0o644).map(|_|0),
        10=>volume.read_at(path,offset,&mut scratch[..buffer.len()]),
        11=>volume.write_at(path,offset,&scratch[..used]),
        _=>volume.dispatch(action,path,target,&mut scratch[..buffer.len()],used),
    };
    match outcome{
        Ok(size)=>{
            if !mutating{
                if size>buffer.len(){return Err(FsError::TooLarge);}
                buffer[..size].copy_from_slice(&scratch[..size]);
            }else{
                table.write_guard.completed(charge);
                // A path mutation may rename/unlink an open file. Do not
                // allow an old descriptor to follow a replacement pathname.
                table.entries.fill(Handle::EMPTY);
            }
            table.last_syscall_namespace=Some(guard.namespace);
            Ok(size)
        },
        Err(error)=>{
            if matches!(error,FsError::Io|FsError::InvalidVolume){
                table.selected_quarantined=true;table.entries.fill(Handle::EMPTY);
            }
            Err(error)
        }
    }
}

pub fn selected_path_operation_authenticated(
    table:&mut FileHandles,owner:Identity,action:u64,path:&str,target:&str,
    offset:u64,buffer:&mut[u8],used:usize)->Result<usize,FsError>{
    selected_path_preflight(table,owner,action,path,target,offset,buffer.len(),used)?;
    let mut disk=probe_selected_ata(table)?;
    selected_path_operation_on_device(table,owner,action,path,target,offset,buffer,used,&mut disk)
}

pub fn selected_sync_on_device<D:BlockDevice>(table:&mut FileHandles,
    owner:Identity,disk:&mut D)->Result<(),FsError>{
    let guard=table.selected_operation_guard(owner)?;
    if owner.pid!=1{return Err(FsError::InvalidName);}
    let receipt=guard.selected_volume.ok_or(FsError::InvalidVolume)?;
    let result=(||{
        let mut volume=crate::m1_partition::mount_partition_receipt_healthy(disk,receipt)
            .map_err(|_|FsError::InvalidVolume)?;
        if !table.selected_operation_guard_is_current(guard){return Err(FsError::InvalidName);}
        volume.sync()
    })();
    if result.is_err(){table.selected_quarantined=true;table.entries.fill(Handle::EMPTY);}
    result
}

pub fn selected_sync_authenticated(table:&mut FileHandles,owner:Identity)
    ->Result<(),FsError>{
    let guard=table.selected_operation_guard(owner)?;
    if owner.pid!=1{return Err(FsError::InvalidName);}
    if guard.selected_volume.is_none(){return Err(FsError::InvalidVolume);}
    let mut disk=probe_selected_ata(table)?;
    selected_sync_on_device(table,owner,&mut disk)
}
/// Verify the pinned, healthy volume after every process has been retired.
/// Used by bounded whole-session restart: this never re-admits quarantined
/// storage, changes authority revisions, resets quotas, formats or writes.
/// All process/handle checks precede even device-capacity inspection.
pub fn revalidate_retired_selected_volume<D:BlockDevice>(
    table:&FileHandles,disk:&mut D,
    receipt:crate::m1_partition::PartitionReceipt)->Result<(),FsError>{
    if table.selected_volume!=Some(receipt)||table.selected_quarantined||
       table.owners.iter().any(|generation|*generation!=0)||
       table.entries.iter().any(|handle|handle.token!=0){
        return Err(FsError::InvalidName);
    }
    if disk.sector_count()!=receipt.device_sectors{
        return Err(FsError::InvalidVolume);
    }
    drop(crate::m1_partition::mount_partition_receipt_healthy(disk,receipt)
        .map_err(|_|FsError::InvalidVolume)?);
    Ok(())
}

/// Privileged, explicit recovery from an uncertain selected-volume
/// mutation. PID1 must own its *current* kernel-issued generation, no handle
/// is allowed to survive quarantine, and the same full GPT/MBR receipt
/// (including physical device capacity) must pass a strict healthy remount.
/// The per-service write budget and monotonic token IDs are not reset.
pub fn revalidate_quarantined_selected_volume<D:BlockDevice>(
 table:&mut FileHandles,owner:Identity,disk:&mut D,
 receipt:crate::m1_partition::PartitionReceipt)->Result<(),FsError>{
 if !table.owns(owner)||owner.pid!=1||!table.selected_quarantined||
    table.selected_volume!=Some(receipt)||
    table.entries.iter().any(|handle|handle.token!=0){
    return Err(FsError::InvalidName);
 }
 // A capacity mismatch is an immediate device-identity failure, before
 // attempting any partition-sector I/O during privileged recovery.
 if disk.sector_count()!=receipt.device_sectors{
    return Err(FsError::InvalidVolume);
 }
 let next=table.volume_revision.checked_add(1).ok_or(FsError::Full)?;
 // The revision must be mintable before even reading a quarantined device.
 let _mounted=crate::m1_partition::mount_partition_receipt_healthy(
     disk,receipt).map_err(|_|FsError::InvalidVolume)?;
 table.volume_revision=next;
 table.selected_quarantined=false;
 Ok(())
}

/// Production-facing, explicitly selected volume service. File data operations
/// revalidate the original full MBR/GPT receipt, mount health and identity
/// before touching a file. Cached SEEK/CLOSE authenticate the pinned receipt
/// without media access. Authentication is checked BEFORE ANY disk I/O.
/// A new process epoch must be issued after reboot; a stale handle token
/// cannot grant authority in a different process incarnation.
pub fn operation_on_selected_volume<D:BlockDevice>(
    table:&mut FileHandles,action:u64,owner:Identity,
    token:u64,argument:u64,path:&str,buffer:&mut[u8],
    disk:&mut D,receipt:crate::m1_partition::PartitionReceipt
)->Result<u64,FsError>{
    // Preserve ABI: stale/revoked handle tokens are InvalidName even while
    // the physical volume is quarantined. A fresh open remains InvalidVolume.
    // This early preflight does no device I/O and cannot resurrect handles.
    table.namespace_receipt(owner)?;
    table.preflight_selected(action,owner,token,argument,path,buffer)?;
    let guard=table.selected_operation_guard(owner)?;
    if !table.selected_operation_guard_is_current(guard){
        return Err(FsError::InvalidName);
    }
    if matches!(action,34..=37){
        if guard.selected_volume!=Some(receipt){return Err(FsError::InvalidVolume);}
        return table.descriptor_control(action,owner,token,argument,path,buffer);
    }
    if matches!(action,4|5){
        // This route accepts an explicit receipt, unlike the runtime adapter.
        // It must name the already admitted volume even though no physical
        // media inspection is necessary to seek/release a cached capability.
        if guard.selected_volume!=Some(receipt){return Err(FsError::InvalidVolume);}
        return table.cached_selected_descriptor_operation(
            action,owner,token,argument,path,buffer);
    }
    // An already authenticated mount receipt pins parent-disk capacity.
    // Reject a device swap before any sector read (and before publication of
    // a new last-admitted namespace). The full GPT/MBR proof is still
    // checked by mount_partition_receipt_healthy after this quick preflight.
    if disk.sector_count()!=receipt.device_sectors ||
       receipt.partition.start.checked_add(receipt.partition.sectors)
         .is_none_or(|end|end>receipt.device_sectors) ||
       receipt.partition.sectors<MIN_SECTORS ||
       receipt.mbr_sector[510..512]!=[0x55,0xaa] {
        return Err(FsError::InvalidVolume);
    }
    let prior=guard.selected_volume;
    // Only a new file open/create may bind the service to a volume. Existing
    // handle actions on an unfamiliar partition fail BEFORE any disk reads.
    // Also reject exhausted mount-revision numbers before touching media:
    // a failed remount must not silently reset or reuse old alias authority.
    let next_revision=if prior!=Some(receipt){
        if !matches!(action,1|29)||
           table.entries.iter().any(|entry|entry.token!=0){
            return Err(FsError::InvalidVolume);
        }
        Some(table.volume_revision.checked_add(1).ok_or(FsError::Full)?)
    }else{None};
    let mut volume=match crate::m1_partition::mount_partition_receipt_healthy(
       disk,receipt){
       Ok(volume)=>volume,
       Err(_)=>{
         // Existing selected volume failed verification on a write: media
         // may be uncertain; revoke cached handles before the next syscall.
         if prior==Some(receipt)&&
            matches!(action,3|6|7..=13|16|24|26|27|28..=32|38){
             table.selected_quarantined=true;
             table.entries.fill(Handle::EMPTY);
         }
         return Err(FsError::InvalidVolume);
       }
    };
    // Revalidate the complete namespace, selected receipt, mount revision
    // and quarantine state after the storage probe, not only the pathname.
    if !table.selected_operation_guard_is_current(guard){
        return Err(FsError::InvalidName);
    }
    // A newly mounted physical volume rotates every outstanding namespace
    // alias, even when it presents familiar pathnames. Do not risk reusing
    // a token from the prior selected partition or stale mount generation.
    // Check exhaustion before changing any cached service authority.
    if let Some(next)=next_revision{
        table.volume_revision=next;
        table.selected_volume=Some(receipt);
    }
    // A namespace is admitted only after the complete selected disk receipt
    // and health probe succeeded; a failed probe cannot publish admission.
    table.last_syscall_namespace=Some(guard.namespace);
    let outcome=table.dispatch_for(action,owner,token,argument,path,buffer,&mut volume);
    if outcome.is_ok() && matches!(action,1|29){
        table.selected_volume=Some(receipt);
    }
    // A failed mutating operation can have torn COW storage. Refuse ALL
    // subsequent file calls, including reads and opens, until the kernel
    // explicitly reprobes the exact same receipt with two healthy heads.
    // Closing a handle, process rebinding, or reopening never resets this.
    if matches!(outcome,Err(FsError::Io|FsError::InvalidVolume)) &&
       matches!(action,3|6|7..=13|16|24|26|27|28..=32|38){
        table.selected_quarantined=true;
        table.entries.fill(Handle::EMPTY);
    }
    outcome
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::block::{BlockError,SECTOR_BYTES};
    const COUNT:usize=100;
    struct Disk{sectors:[[u8;SECTOR_BYTES];COUNT]}
    impl BlockDevice for Disk{
        fn sector_count(&self)->u64{COUNT as u64}
        fn read_sector(&mut self,lba:u64,out:&mut [u8;SECTOR_BYTES])->Result<(),BlockError>{
            *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*data;
            Ok(())
        }
    }
    fn fresh()->HierVfs<Disk>{
        HierVfs::format_blank(Disk{sectors:[[0u8;SECTOR_BYTES];COUNT]}).unwrap()
    }
    #[test] fn m1_truncate_authenticated_handle_refreshes_generation_preserves_cursor(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"original").unwrap();
        let mut table=FileHandles::new();let owner=table.bind_process(1).unwrap();
        let peer=table.bind_process(2).unwrap();
        let fd=table.dispatch_authenticated(1,owner,0,READ|WRITE,"/shared/note",&mut [],&mut fs).unwrap();
        let reader=table.dispatch_authenticated(1,peer,0,READ,"/shared/note",&mut [],&mut fs).unwrap();
        table.dispatch_authenticated(4,owner,fd,8,"",&mut [],&mut fs).unwrap();
        assert_eq!(table.dispatch_authenticated(6,owner,fd,3,"",&mut [],&mut fs),Ok(0));
        let mut out=[0xcc;16];
        assert_eq!(table.dispatch_authenticated(2,owner,fd,0,"",&mut out,&mut fs),Ok(0));
        assert_eq!(table.dispatch_authenticated(2,peer,reader,0,"",&mut out,&mut fs),Err(FsError::InvalidName));
        table.dispatch_authenticated(4,owner,fd,0,"",&mut [],&mut fs).unwrap();
        assert_eq!(table.dispatch_authenticated(2,owner,fd,0,"",&mut out,&mut fs),Ok(3));
        assert_eq!(&out[..3],b"ori");
        assert_eq!(table.dispatch_authenticated(6,owner,fd,7,"",&mut [],&mut fs),Ok(0));
        assert_eq!(table.security_statistics().completed_writes,2);
        assert_eq!(table.security_statistics().changed_bytes,9);
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(fs.read("/shared/note",&mut out),Ok(7));assert_eq!(&out[..7],b"ori\0\0\0\0");
    }
    #[test] fn m1_truncate_denies_readonly_stale_forged_oversize_without_mutation(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();fs.create("/shared/note",b"safe").unwrap();
        let mut table=FileHandles::new();let owner=table.bind_process(1).unwrap();let peer=table.bind_process(2).unwrap();
        let fd=table.dispatch_authenticated(1,owner,0,WRITE,"/shared/note",&mut [],&mut fs).unwrap();
        let reader=table.dispatch_authenticated(1,peer,0,READ,"/shared/note",&mut [],&mut fs).unwrap();
        let generation=fs.generation();
        for (identity,token,length) in [(peer,reader,0),(peer,fd,0),
            (Identity{pid:1,generation:owner.generation+1},fd,0)]{
            assert_eq!(table.dispatch_authenticated(6,identity,token,length,"",&mut [],&mut fs),Err(FsError::InvalidName));
        }
        assert_eq!(table.dispatch_authenticated(6,owner,fd,MAX_FILE_BYTES as u64+1,"",&mut [],&mut fs),Err(FsError::TooLarge));
        assert_eq!(table.dispatch_authenticated(6,owner,fd,4,"",&mut [],&mut fs),Ok(0));
        assert_eq!(fs.generation(),generation);assert_eq!(table.security_statistics().completed_writes,0);
        fs.overwrite("/shared/note",b"replacement").unwrap();
        assert_eq!(table.dispatch_authenticated(6,owner,fd,0,"",&mut [],&mut fs),Err(FsError::InvalidName));
        assert_eq!(fs.file_len("/shared/note"),Ok(11));
        table.retire_process(owner);
        assert_eq!(table.dispatch_authenticated(6,owner,fd,0,"",&mut [],&mut fs),Err(FsError::InvalidName));
    }
    #[test] fn m1_truncate_shrink_and_extend_share_monotonic_write_budget(){
        let mut fs=fresh();fs.create("/note",b"x").unwrap();let mut table=FileHandles::new();
        let fd=table.open(1,"/note",WRITE,&mut fs).unwrap();
        for index in 0..crate::fs_write_guard::MAX_WRITE_OPERATIONS{
            assert_eq!(table.dispatch(6,1,fd,(index%2) as u64,"",&mut [],&mut fs),Ok(0));
        }
        assert_eq!(table.dispatch(6,1,fd,0,"",&mut [],&mut fs),Err(FsError::InvalidName));
        assert_eq!(fs.file_len("/note"),Ok(1));
        assert_eq!(table.security_statistics().completed_writes,32);
        assert_eq!(table.security_statistics().changed_bytes,32);
    }
    #[test] fn m1_truncate_uncertain_io_retires_capability_without_success_charge(){
        use std::{cell::Cell,rc::Rc};
        struct FaultDisk{disk:Disk,fail:Rc<Cell<bool>>}
        impl BlockDevice for FaultDisk{
            fn sector_count(&self)->u64{self.disk.sector_count()}
            fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{self.disk.read_sector(lba,out)}
            fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
                if self.fail.get(){return Err(BlockError::Io);}self.disk.write_sector(lba,data)
            }
        }
        let failed=Rc::new(Cell::new(false));
        let mut fs=HierVfs::format_blank(FaultDisk{disk:Disk{sectors:[[0;SECTOR_BYTES];COUNT]},fail:failed.clone()}).unwrap();
        fs.create("/note",b"original").unwrap();let mut table=FileHandles::new();
        let owner=table.bind_process(1).unwrap();
        let fd=table.dispatch_authenticated(1,owner,0,WRITE,"/note",&mut [],&mut fs).unwrap();
        failed.set(true);
        assert_eq!(table.dispatch_authenticated(6,owner,fd,1,"",&mut [],&mut fs),Err(FsError::Io));
        failed.set(false);
        assert_eq!(table.dispatch_authenticated(6,owner,fd,1,"",&mut [],&mut fs),Err(FsError::InvalidName));
        assert_eq!(table.security_statistics().completed_writes,0);
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(fs.file_len("/note"),Ok(8));
    }
    #[test] fn handle_seek_write_read_close_and_reuse(){
        let mut fs=fresh();
        fs.mkdir("/etc").unwrap();fs.create("/etc/note",b"hello").unwrap();
        let mut table=FileHandles::new();
        let fd=table.open(1,"/etc/note",READ|WRITE,&mut fs).unwrap();
        let mut buf=[0u8;72];
        assert_eq!(table.read(1,fd,&mut buf[..2],&mut fs),Ok(2));
        assert_eq!(&buf[..2],b"he");
        assert_eq!(table.seek(1,fd,2),Ok(2));
        assert_eq!(table.write(1,fd,b"LLO",&mut fs),Ok(3));
        assert_eq!(table.seek(1,fd,0),Ok(0));
        assert_eq!(table.read(1,fd,&mut buf[..8],&mut fs),Ok(5));
        assert_eq!(&buf[..5],b"heLLO");
        assert_eq!(table.read(1,fd,&mut buf[..8],&mut fs),Ok(0));
        table.close(1,fd).unwrap();
        assert_eq!(table.read(1,fd,&mut buf[..8],&mut fs),Err(FsError::InvalidName));
        let other=table.open(1,"/etc/note",READ,&mut fs).unwrap();
        assert_ne!(fd,other);
        assert_eq!(table.write(1,other,b"XX",&mut fs),Err(FsError::InvalidName));
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(fs.read_at("/etc/note",0,&mut buf),Ok(5));
        assert_eq!(&buf[..5],b"heLLO");
    }
    #[test] fn rights_owner_public_only_slots_and_teardown(){
        let mut fs=fresh();fs.mkdir("/etc").unwrap();fs.mkdir("/shared").unwrap();
        fs.create("/etc/private",b"secret").unwrap();
        fs.create("/shared/guide",b"public").unwrap();
        let mut table=FileHandles::new();let mut buf=[0u8;72];
        assert_eq!(table.open(2,"/etc/private",READ,&mut fs),Err(FsError::InvalidName));
        assert_eq!(table.open(2,"/shared/guide",WRITE,&mut fs),Err(FsError::InvalidName));
        assert_eq!(table.open(3,"/shared/guide",READ,&mut fs),Err(FsError::InvalidName));
        let shared=table.open(2,"/shared/guide",READ,&mut fs).unwrap();
        assert_eq!(table.read(1,shared,&mut buf,&mut fs),Err(FsError::InvalidName));
        assert_eq!(table.write(2,shared,b"x",&mut fs),Err(FsError::InvalidName));
        assert_eq!(table.read(2,shared,&mut buf,&mut fs),Ok(6));
        let owner=table.open(1,"/etc/private",READ,&mut fs).unwrap();
        let third=table.open(1,"/shared/guide",READ,&mut fs).unwrap();
        let fourth=table.open(1,"/etc/private",READ,&mut fs).unwrap();
        assert_eq!(table.open(1,"/etc/private",READ,&mut fs),Err(FsError::Full));
        assert_eq!(table.seek(2,owner,5),Err(FsError::InvalidName));
        assert_eq!(table.seek(2,shared,2049),Err(FsError::TooLarge));
        table.terminate(2);
        assert_eq!(table.read(2,shared,&mut buf,&mut fs),Err(FsError::InvalidName));
        assert!(table.open(1,"/etc/private",READ,&mut fs).is_ok());
        assert!(third>0&&fourth>0);
    }
    #[test] fn read_only_and_invalid_offsets_never_mutate_data(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"stored").unwrap();
        let mut handles=FileHandles::new();
        let fd=handles.open(1,"/shared/note",READ,&mut fs).unwrap();
        assert_eq!(handles.write(1,fd,b"x",&mut fs),Err(FsError::InvalidName));
        assert_eq!(handles.seek(1,fd,u64::MAX),Err(FsError::TooLarge));
        assert_eq!(handles.read(1,fd,&mut [0u8;73],&mut fs),Err(FsError::TooLarge));
        handles.close(1,fd).unwrap();
        let fd=handles.open(1,"/shared/note",WRITE,&mut fs).unwrap();
        assert_eq!(handles.read(1,fd,&mut [0u8;6],&mut fs),Err(FsError::InvalidName));
        assert_eq!(handles.seek(1,fd,8),Ok(8));
        assert_eq!(handles.write(1,fd,b"X",&mut fs),Err(FsError::TooLarge));
        let mut data=[0u8;16];
        assert_eq!(fs.read_at("/shared/note",0,&mut data),Ok(6));
        assert_eq!(&data[..6],b"stored");
    }
    #[test] fn security_protected_boot_recovery_and_system_files_never_change(){
        let mut fs=fresh();
        fs.mkdir("/boot").unwrap();fs.mkdir("/system").unwrap();
        fs.mkdir("/recovery").unwrap();
        fs.create("/boot/init",b"trusted").unwrap();
        fs.create("/system/config",b"kernel").unwrap();
        fs.create("/recovery/state",b"backup").unwrap();
        let mut handles=FileHandles::new();
        for path in ["/boot/init","/system/config","/recovery/state",
                     "//boot/./init"]{
            assert_eq!(handles.open(1,path,WRITE,&mut fs),
                Err(FsError::InvalidName),"{path}");
            assert_eq!(handles.open(1,path,READ|WRITE,&mut fs),
                Err(FsError::InvalidName),"{path}");
        }
        let read=handles.open(1,"/boot/init",READ,&mut fs).unwrap();
        let mut data=[0u8;24];
        assert_eq!(handles.read(1,read,&mut data,&mut fs),Ok(7));
        assert_eq!(&data[..7],b"trusted");
        handles.close(1,read).unwrap();
        for (path,expected) in [("/boot/init",b"trusted".as_slice()),
            ("/system/config",b"kernel".as_slice()),
            ("/recovery/state",b"backup".as_slice())]{
            let mut output=[0u8;24];
            let n=fs.read_at(path,0,&mut output).unwrap();
            assert_eq!(&output[..n],expected);
        }
        assert_eq!(handles.security_statistics().denials,8);
    }
    #[test] fn security_burst_write_limit_applies_across_handle_reopen_and_pid(){
        let mut fs=fresh();
        fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"original").unwrap();
        let mut handles=FileHandles::new();
        let mut h=handles.open(1,"/shared/note",WRITE,&mut fs).unwrap();
        for index in 0..crate::fs_write_guard::MAX_WRITE_OPERATIONS{
            if index%7==0&&index!=0{
                handles.close(1,h).unwrap();
                h=handles.open(1,"/shared/note",WRITE,&mut fs).unwrap();
            }
            handles.seek(1,h,0).unwrap();
            assert_eq!(handles.write(1,h,b"X",&mut fs),Ok(1));
        }
        handles.close(1,h).unwrap();
        let another=handles.open(1,"/shared/note",WRITE,&mut fs).unwrap();
        assert_eq!(handles.write(1,another,b"Y",&mut fs),
            Err(FsError::InvalidName));
        assert_eq!(handles.security_statistics().completed_writes,
            crate::fs_write_guard::MAX_WRITE_OPERATIONS);
        assert_eq!(handles.security_statistics().denials,1);
        let mut output=[0u8;24];
        let n=fs.read_at("/shared/note",0,&mut output).unwrap();
        assert_eq!(&output[..n],b"Xriginal");
        assert_eq!(handles.write(2,another,b"Z",&mut fs),
            Err(FsError::InvalidName));
    }
    #[test] fn security_failed_mutation_does_not_consume_write_budget(){
        let mut fs=fresh();
        fs.mkdir("/etc").unwrap();fs.create("/etc/a",b"one").unwrap();
        let mut handles=FileHandles::new();
        let fd=handles.open(1,"/etc/a",WRITE,&mut fs).unwrap();
        handles.seek(1,fd,99).unwrap();
        assert_eq!(handles.write(1,fd,b"X",&mut fs),Err(FsError::TooLarge));
        assert_eq!(handles.security_statistics().completed_writes,0);
        assert_eq!(handles.security_statistics().changed_bytes,0);
        handles.seek(1,fd,0).unwrap();
        assert_eq!(handles.write(1,fd,b"T",&mut fs),Ok(1));
        assert_eq!(handles.security_statistics().completed_writes,1);
        assert_eq!(handles.security_statistics().changed_bytes,1);
    }

    #[test] fn canonical_shared_aliases_store_the_authorized_path(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"durable").unwrap();
        let mut handles=FileHandles::new();
        for alias in ["//shared/./note", "/./shared//note/", "/shared/./././././././note"]{
            let token=handles.open(2,alias,READ,&mut fs).unwrap();
            let index=handles.index(Identity{pid:2,generation:0},token).unwrap();
            assert_eq!(handles.entries[index].pathname(),"/shared/note");
            let mut data=[0u8;8];
            assert_eq!(handles.read(2,token,&mut data,&mut fs),Ok(7));
            assert_eq!(&data[..7],b"durable");
            handles.close(2,token).unwrap();
        }
        let token=handles.open(1,"//shared/./note/",WRITE,&mut fs).unwrap();
        assert_eq!(handles.write(1,token,b"D",&mut fs),Ok(1));
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        let mut data=[0u8;8];
        assert_eq!(fs.read_at("/shared/note",0,&mut data),Ok(7));
        assert_eq!(&data[..7],b"Durable");
    }
    #[test] fn failed_opens_preserve_slots_tokens_and_exact_shared_boundary(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();fs.mkdir("/sharedx").unwrap();
        fs.create("/shared/note",b"ok").unwrap();
        fs.create("/sharedx/note",b"private").unwrap();
        let mut handles=FileHandles::new();
        for (pid,path,mode) in [
            (2,"/shared/../shared/note",READ),(2,"/../shared/note",READ),
            (2,"/shared/note\0",READ),(2,"shared/note",READ),
            (2,"/shared/abcdefghijklmnop",READ),
            (2,"//sharedx/./note",READ),(2,"/shared",READ),
            (2,"/shared/missing",READ),(2,"/shared/note",WRITE),
            (2,"/shared/note",READ|WRITE),(1,"/shared/note",0),
            (1,"/shared/note",4),(3,"/shared/note",READ),
        ]{
            assert!(handles.open(pid,path,mode,&mut fs).is_err(),"{path}");
            assert_eq!(handles.next,1);
            assert!(handles.entries.iter().all(|h|h.token==0));
        }
        for expected in 1..=4{
            assert_eq!(handles.open(2,"//shared/./note",READ,&mut fs),Ok(expected));
        }
        assert_eq!(handles.open(2,"/shared/note",READ,&mut fs),Err(FsError::Full));
        assert_eq!(handles.next,5);
        assert_eq!(handles.close(1,1),Err(FsError::InvalidName));
        assert_eq!(handles.seek(1,1,0),Err(FsError::InvalidName));
        assert_eq!(handles.read(1,1,&mut [0u8;2],&mut fs),Err(FsError::InvalidName));
        handles.close(2,1).unwrap();
        assert_eq!(handles.open(2,"/shared/note",READ,&mut fs),Ok(5));
        assert_eq!(handles.seek(2,1,0),Err(FsError::InvalidName));
    }
    #[test] fn protected_alias_denials_and_token_exhaustion_are_atomic(){
        let mut fs=fresh();fs.mkdir("/boot").unwrap();
        fs.create("/boot/init",b"trusted").unwrap();
        let mut handles=FileHandles::new();
        for alias in ["/./boot/init", "////boot/././././././init/", "/boot//init/"]{
            assert_eq!(handles.open(1,alias,READ|WRITE,&mut fs),Err(FsError::InvalidName));
            assert_eq!(handles.next,1);
            assert!(handles.entries.iter().all(|h|h.token==0));
        }
        assert_eq!(handles.security_statistics().denials,3);
        assert_eq!(handles.security_statistics().completed_writes,0);
        handles.next=u64::MAX;
        assert_eq!(handles.open(1,"/./boot/init",READ,&mut fs),Err(FsError::Full));
        assert_eq!(handles.next,u64::MAX);
        assert!(handles.entries.iter().all(|h|h.token==0));
        let mut data=[0u8;7];
        assert_eq!(fs.read_at("/boot/init",0,&mut data),Ok(7));
        assert_eq!(&data,b"trusted");
    }

    #[test] fn incarnation_rebind_denies_every_stale_operation_and_preserves_peer(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"original").unwrap();
        let mut table=FileHandles::new();
        let old=table.bind_process(1).unwrap();
        let peer=table.bind_process(2).unwrap();
        let token=table.dispatch_authenticated(1,old,0,READ|WRITE,
            "/shared/note",&mut [],&mut fs).unwrap();
        let peer_token=table.dispatch_authenticated(1,peer,0,READ,
            "/shared/note",&mut [],&mut fs).unwrap();
        // Simulate an aborted runtime: no terminate/close for the old PID1.
        let current=table.bind_process(1).unwrap();
        assert_ne!(old.generation,current.generation);
        let generation=fs.generation();let next=table.next;
        for action in 1..=5{
            let mut bytes=[0x5a;8];
            assert_eq!(table.dispatch_authenticated(action,old,token,READ|WRITE,
                "/shared/note",&mut bytes,&mut fs),Err(FsError::InvalidName));
            assert_eq!(bytes,[0x5a;8]);
        }
        assert_eq!(table.next,next);
        assert_eq!(fs.generation(),generation);
        assert!(!table.retire_process(old)); // delayed cleanup is harmless
        assert!(table.owns(current));assert!(table.owns(peer));
        let fresh=table.dispatch_authenticated(1,current,0,READ|WRITE,
            "/shared/note",&mut [],&mut fs).unwrap();
        assert_ne!(fresh,token);
        assert_eq!(table.dispatch_authenticated(2,current,token,0,"",
            &mut [0;8],&mut fs),Err(FsError::InvalidName));
        let mut bytes=[0;8];
        assert_eq!(table.dispatch_authenticated(2,peer,peer_token,0,"",
            &mut bytes,&mut fs),Ok(8));assert_eq!(&bytes,b"original");
        assert!(table.retire_process(current));assert!(!table.retire_process(current));
        assert_eq!(table.dispatch_authenticated(2,current,fresh,0,"",
            &mut bytes,&mut fs),Err(FsError::InvalidName));
        assert!(table.owns(peer));
    }
    #[test] fn legacy_and_authenticated_handles_are_separate_authority_domains(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"content").unwrap();
        let mut table=FileHandles::new();let owner=table.bind_process(1).unwrap();
        let auth=table.dispatch_authenticated(1,owner,0,READ|WRITE,
            "/shared/note",&mut [],&mut fs).unwrap();
        let legacy=table.open(1,"/shared/note",READ|WRITE,&mut fs).unwrap();
        assert_eq!(table.read(1,auth,&mut [0;7],&mut fs),Err(FsError::InvalidName));
        assert_eq!(table.write(1,auth,b"x",&mut fs),Err(FsError::InvalidName));
        assert_eq!(table.seek(1,auth,0),Err(FsError::InvalidName));
        assert_eq!(table.close(1,auth),Err(FsError::InvalidName));
        for action in 2..=5{
            assert_eq!(table.dispatch_authenticated(action,owner,legacy,0,"",
                &mut [0;7],&mut fs),Err(FsError::InvalidName));
        }
        table.terminate(1); // PID-only compatibility teardown cannot revoke owner
        assert!(table.owns(owner));
        assert_eq!(table.dispatch_authenticated(2,owner,auth,0,"",
            &mut [0;7],&mut fs),Ok(7));
        assert_eq!(table.read(1,legacy,&mut [0;7],&mut fs),Err(FsError::InvalidName));
        // The real hardware adapter rejects forged owners before ATA probing.
        for forged in [Identity{pid:1,generation:0},Identity{pid:0,generation:1},
            Identity{pid:3,generation:owner.generation},
            Identity{pid:1,generation:owner.generation+1}]{
            assert_eq!(operation_authenticated(&mut table,1,forged,0,READ,
                "/shared/note",&mut []),Err(FsError::InvalidName));
        }
    }
    #[test] fn generation_exhaustion_and_invalid_bind_preserve_live_authority(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"kept").unwrap();
        let mut table=FileHandles::new();let owner=table.bind_process(1).unwrap();
        let token=table.dispatch_authenticated(1,owner,0,READ,
            "/shared/note",&mut [],&mut fs).unwrap();
        for pid in [0,3,u32::MAX]{
            assert_eq!(table.bind_process(pid),Err(FsError::InvalidName));
            assert!(table.owns(owner));
        }
        table.next_generation=u64::MAX;
        assert_eq!(table.bind_process(1),Err(FsError::Full));
        assert_eq!(table.next_generation,u64::MAX);assert!(table.owns(owner));
        assert_eq!(table.dispatch_authenticated(2,owner,token,0,"",
            &mut [0;4],&mut fs),Ok(4));
    }

    #[test] fn m1_r051_old_handle_cannot_read_or_overwrite_unlinked_recreated_file(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"original").unwrap();
        let mut table=FileHandles::new();
        let owner=table.bind_process(1).unwrap();
        let token=table.dispatch_authenticated(1,owner,0,READ|WRITE,
            "/shared/note",&mut [],&mut fs).unwrap();
        let generation=fs.generation();
        fs.unlink("/shared/note").unwrap();
        fs.create("/shared/note",b"replacement").unwrap();
        assert!(fs.generation()>generation);
        let count=table.security_statistics().completed_writes;
        let mut out=[0x55;32];
        assert_eq!(table.dispatch_authenticated(2,owner,token,0,"",&mut out,&mut fs),
            Err(FsError::InvalidName));
        assert_eq!(out,[0x55;32]);
        assert_eq!(table.dispatch_authenticated(3,owner,token,0,"",&mut [b'X'],&mut fs),
            Err(FsError::InvalidName));
        assert_eq!(table.security_statistics().completed_writes,count);
        let mut exact=[0;32];
        assert_eq!(fs.read("/shared/note",&mut exact),Ok(11));
        assert_eq!(&exact[..11],b"replacement");
    }
    #[test] fn m1_r051_old_handle_cannot_follow_rename_and_new_path_occupant(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"old").unwrap();
        let mut table=FileHandles::new();
        let token=table.open(1,"/shared/note",READ|WRITE,&mut fs).unwrap();
        fs.rename_file("/shared/note","/shared/archive").unwrap();
        fs.create("/shared/note",b"new").unwrap();
        let before=fs.generation();
        let mut out=[0x9a;8];
        assert_eq!(table.read(1,token,&mut out,&mut fs),Err(FsError::InvalidName));
        assert_eq!(table.write(1,token,b"x",&mut fs),Err(FsError::InvalidName));
        assert_eq!(out,[0x9a;8]);assert_eq!(fs.generation(),before);
        assert_eq!(fs.read("/shared/archive",&mut out),Ok(3));
        assert_eq!(&out[..3],b"old");
        let recent=table.open(1,"/shared/note",READ,&mut fs).unwrap();
        assert_eq!(table.read(1,recent,&mut out,&mut fs),Ok(3));
        assert_eq!(&out[..3],b"new");
    }
    #[test] fn m1_r051_same_handle_refreshes_after_successful_cow_writes(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"0000").unwrap();
        let mut table=FileHandles::new();
        let token=table.open(1,"/shared/note",READ|WRITE,&mut fs).unwrap();
        assert_eq!(table.write(1,token,b"A",&mut fs),Ok(1));
        assert_eq!(table.write(1,token,b"B",&mut fs),Ok(1));
        assert_eq!(table.seek(1,token,0),Ok(0));
        let mut out=[0;4];
        assert_eq!(table.read(1,token,&mut out,&mut fs),Ok(4));
        assert_eq!(&out,b"AB00");
        let mut remounted=HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(remounted.read("/shared/note",&mut out),Ok(4));
        assert_eq!(&out,b"AB00");
    }
    #[test] fn m1_r051_peer_handle_requires_reopen_after_writer_commits(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"before").unwrap();
        let mut table=FileHandles::new();
        let init=table.bind_process(1).unwrap();
        let files=table.bind_process(2).unwrap();
        let reader=table.dispatch_authenticated(1,files,0,READ,
            "/shared/note",&mut [],&mut fs).unwrap();
        let writer=table.dispatch_authenticated(1,init,0,WRITE,
            "/shared/note",&mut [],&mut fs).unwrap();
        assert_eq!(table.dispatch_authenticated(3,init,writer,0,"",
            &mut [b'X'],&mut fs),Ok(1));
        let mut out=[0x3d;8];
        assert_eq!(table.dispatch_authenticated(2,files,reader,0,"",
            &mut out,&mut fs),Err(FsError::InvalidName));
        assert_eq!(out,[0x3d;8]);
        table.dispatch_authenticated(5,files,reader,0,"",&mut [],&mut fs).unwrap();
        let fresh_reader=table.dispatch_authenticated(1,files,0,READ,
            "/shared/note",&mut [],&mut fs).unwrap();
        assert_eq!(table.dispatch_authenticated(2,files,fresh_reader,0,"",
            &mut out,&mut fs),Ok(6));
        assert_eq!(&out[..6],b"Xefore");
    }
    #[test] fn m1_r051_unrelated_cow_update_conservatively_retires_old_handle(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"safe").unwrap();
        fs.create("/shared/other",b"one").unwrap();
        let mut table=FileHandles::new();
        let token=table.open(1,"/shared/note",READ,&mut fs).unwrap();
        fs.overwrite("/shared/other",b"two").unwrap();
        let mut out=[0x61;8];
        assert_eq!(table.read(1,token,&mut out,&mut fs),Err(FsError::InvalidName));
        assert_eq!(out,[0x61;8]);
        table.close(1,token).unwrap();
        let current=table.open(1,"/shared/note",READ,&mut fs).unwrap();
        assert_eq!(table.read(1,current,&mut out,&mut fs),Ok(4));
        assert_eq!(&out[..4],b"safe");
    }
    #[test] fn m1_r051_read_failure_preserves_offset_and_volume_generation_token(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"four").unwrap();
        let mut table=FileHandles::new();
        let token=table.open(1,"/shared/note",READ,&mut fs).unwrap();
        let idx=table.index(Identity{pid:1,generation:0},token).unwrap();
        let saved=table.entries[idx];
        assert_eq!(table.read(1,token,&mut [0u8;73],&mut fs),Err(FsError::TooLarge));
        assert_eq!(table.entries[idx].offset,saved.offset);
        assert_eq!(table.entries[idx].filesystem_generation,saved.filesystem_generation);
        let mut out=[0;4];
        assert_eq!(table.read(1,token,&mut out,&mut fs),Ok(4));
        assert_eq!(&out,b"four");
    }
    #[test]fn m1_r052_io_failure_retires_handle_and_does_not_charge_budget(){
        struct FaultDisk{disk:Disk,writes:usize,fail_at:Option<usize>}
        impl BlockDevice for FaultDisk{
            fn sector_count(&self)->u64{self.disk.sector_count()}
            fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
                ->Result<(),BlockError>{self.disk.read_sector(lba,out)}
            fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
                ->Result<(),BlockError>{
                if self.fail_at==Some(self.writes){
                    self.fail_at=None;return Err(BlockError::Io);
                }
                self.writes+=1;self.disk.write_sector(lba,data)
            }
        }
        let disk=FaultDisk{disk:Disk{sectors:[[0;SECTOR_BYTES];COUNT]},
                           writes:0,fail_at:None};
        let mut fs=HierVfs::format_blank(disk).unwrap();
        fs.mkdir("/shared").unwrap();fs.create("/shared/note",b"stable").unwrap();
        let mut dev=fs.into_device();dev.fail_at=Some(dev.writes);
        let mut fs=HierVfs::mount(dev).unwrap();
        let mut table=FileHandles::new();
        let fd=table.open(1,"/shared/note",WRITE,&mut fs).unwrap();
        assert_eq!(table.write(1,fd,b"bad",&mut fs),Err(FsError::Io));
        assert_eq!(table.security_statistics().completed_writes,0);
        assert_eq!(table.write(1,fd,b"retry",&mut fs),Err(FsError::InvalidName));
        assert_eq!(table.seek(1,fd,0),Err(FsError::InvalidName));
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        let mut data=[0u8;12];
        assert_eq!(fs.read("/shared/note",&mut data),Ok(6));
        assert_eq!(&data[..6],b"stable");
        let reopened=table.open(1,"/shared/note",WRITE,&mut fs).unwrap();
        assert_ne!(fd,reopened);
        assert_eq!(table.write(1,reopened,b"S",&mut fs),Ok(1));
    }
    #[test]fn m1_r052_benign_out_of_bounds_write_does_not_retire_handle(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"safe").unwrap();
        let mut t=FileHandles::new();
        let fd=t.open(1,"/shared/note",WRITE,&mut fs).unwrap();
        assert_eq!(t.seek(1,fd,8),Ok(8));
        assert_eq!(t.write(1,fd,b"X",&mut fs),Err(FsError::TooLarge));
        assert_eq!(t.seek(1,fd,0),Ok(0));
        assert_eq!(t.write(1,fd,b"Y",&mut fs),Ok(1));
    }
    #[test]fn m1_r053_failed_block_read_retires_handle_before_retry(){
        use std::{cell::Cell,rc::Rc};
        struct FailingDisk{disk:Disk,fail:Rc<Cell<bool>>}
        impl BlockDevice for FailingDisk{
            fn sector_count(&self)->u64{self.disk.sector_count()}
            fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
                ->Result<(),BlockError>{
                if self.fail.get(){return Err(BlockError::Io);}
                self.disk.read_sector(lba,out)
            }
            fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
                ->Result<(),BlockError>{self.disk.write_sector(lba,data)}
        }
        let failed=Rc::new(Cell::new(false));
        let mut fs=HierVfs::format_blank(FailingDisk{
            disk:Disk{sectors:[[0;SECTOR_BYTES];COUNT]},fail:failed.clone()
        }).unwrap();
        fs.mkdir("/shared").unwrap();fs.create("/shared/note",b"persist").unwrap();
        let mut table=FileHandles::new();
        let fd=table.open(1,"/shared/note",READ,&mut fs).unwrap();
        let before=table.security_statistics().completed_writes;
        failed.set(true);
        let mut out=[0xcc;32];
        assert_eq!(table.read(1,fd,&mut out,&mut fs),Err(FsError::Io));
        assert_eq!(table.seek(1,fd,0),Err(FsError::InvalidName));
        assert_eq!(table.security_statistics().completed_writes,before);
        failed.set(false);
        assert_eq!(table.read(1,fd,&mut out,&mut fs),Err(FsError::InvalidName));
        let new_fd=table.open(1,"/shared/note",READ,&mut fs).unwrap();
        assert_ne!(new_fd,fd);
        assert_eq!(table.read(1,new_fd,&mut out,&mut fs),Ok(7));
        assert_eq!(&out[..7],b"persist");
    }
    #[test]fn m1_r056_faulted_read_does_not_publish_device_dirtied_bytes(){
        use std::{cell::Cell,rc::Rc};
        struct DirtyDisk{disk:Disk,fail:Rc<Cell<bool>>}
        impl BlockDevice for DirtyDisk{
            fn sector_count(&self)->u64{self.disk.sector_count()}
            fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
                ->Result<(),BlockError>{
                if self.fail.get(){
                    out.fill(0x1f);
                    return Err(BlockError::Io);
                }
                self.disk.read_sector(lba,out)
            }
            fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])
                ->Result<(),BlockError>{self.disk.write_sector(lba,bytes)}
        }
        let fail=Rc::new(Cell::new(false));
        let mut fs=HierVfs::format_blank(DirtyDisk{
            disk:Disk{sectors:[[0;SECTOR_BYTES];COUNT]},fail:fail.clone()
        }).unwrap();
        fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"durable").unwrap();
        let mut handles=FileHandles::new();
        let token=handles.open(1,"/shared/note",READ,&mut fs).unwrap();
        let mut output=[0xa7;24];
        fail.set(true);
        assert_eq!(handles.read(1,token,&mut output,&mut fs),Err(FsError::Io));
        assert_eq!(output,[0xa7;24]);
        fail.set(false);
        assert_eq!(handles.read(1,token,&mut output,&mut fs),
                   Err(FsError::InvalidName));
        let reopened=handles.open(1,"/shared/note",READ,&mut fs).unwrap();
        assert_eq!(handles.read(1,reopened,&mut output,&mut fs),Ok(7));
        assert_eq!(&output[..7],b"durable");
        assert_eq!(&output[7..],&[0xa7;17]);
    }
    #[test]fn m1_r056_successful_short_read_preserves_output_tail(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"five!").unwrap();
        let mut handles=FileHandles::new();
        let fd=handles.open(1,"/shared/note",READ,&mut fs).unwrap();
        let mut out=[0xa9;16];
        assert_eq!(handles.read(1,fd,&mut out,&mut fs),Ok(5));
        assert_eq!(&out[..5],b"five!");
        assert_eq!(&out[5..],&[0xa9;11]);
        assert_eq!(handles.read(1,fd,&mut out,&mut fs),Ok(0));
        assert_eq!(&out[5..],&[0xa9;11]);
    }
    #[test]fn m1_r053_invalid_read_count_preserves_valid_handle(){
        let mut fs=fresh();fs.mkdir("/shared").unwrap();
        fs.create("/shared/note",b"readable").unwrap();
        let mut table=FileHandles::new();
        let token=table.open(1,"/shared/note",READ,&mut fs).unwrap();
        let mut too_big=[0;73];
        assert_eq!(table.read(1,token,&mut too_big,&mut fs),Err(FsError::TooLarge));
        let mut out=[0;16];
        assert_eq!(table.read(1,token,&mut out,&mut fs),Ok(8));
        assert_eq!(&out[..8],b"readable");
    }
    #[test] fn authenticated_restarts_keep_rights_and_global_write_budget(){
        use crate::fs_write_guard::MAX_WRITE_OPERATIONS;
        let mut fs=fresh();fs.mkdir("/shared").unwrap();fs.mkdir("/boot").unwrap();
        fs.create("/shared/note",b"data").unwrap();fs.create("/boot/init",b"safe").unwrap();
        let mut table=FileHandles::new();
        for _ in 0..MAX_WRITE_OPERATIONS{
            let owner=table.bind_process(1).unwrap();
            let token=table.dispatch_authenticated(1,owner,0,WRITE,
                "/shared/note",&mut [],&mut fs).unwrap();
            assert_eq!(table.dispatch_authenticated(3,owner,token,0,"",
                &mut [b'x'],&mut fs),Ok(1));
            assert!(table.retire_process(owner));
        }
        let owner=table.bind_process(1).unwrap();
        let token=table.dispatch_authenticated(1,owner,0,WRITE,
            "/shared/note",&mut [],&mut fs).unwrap();
        assert_eq!(table.dispatch_authenticated(3,owner,token,0,"",
            &mut [b'y'],&mut fs),Err(FsError::InvalidName));
        assert_eq!(table.security_statistics().completed_writes,MAX_WRITE_OPERATIONS);
        assert_eq!(table.dispatch_authenticated(1,owner,0,WRITE,
            "/boot/init",&mut [],&mut fs),Err(FsError::InvalidName));
        let peer=table.bind_process(2).unwrap();
        assert_eq!(table.dispatch_authenticated(1,peer,0,WRITE,
            "/shared/note",&mut [],&mut fs),Err(FsError::InvalidName));
        assert_eq!(table.dispatch_authenticated(1,peer,0,READ,
            "/boot/init",&mut [],&mut fs),Err(FsError::InvalidName));
        let mut data=[0;4];assert_eq!(fs.read("/boot/init",&mut data),Ok(4));
        assert_eq!(&data,b"safe");
    }

}


#[cfg(test)]
mod r094_authenticated_cow_ops {
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct Disk([[u8;SECTOR_BYTES];100]);
 impl BlockDevice for Disk {
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,l:u64,b:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *b=*self.0.get(l as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,l:u64,b:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.0.get_mut(l as usize).ok_or(BlockError::OutOfRange)?=*b;Ok(())
  }
 }
 fn volume()->HierVfs<Disk>{
  let mut f=HierVfs::format_blank(Disk([[0;SECTOR_BYTES];100])).unwrap();
  f.mkdir("/shared").unwrap();f.create("/shared/note",b"abc").unwrap();
  f.create("/shared/other",b"xyz").unwrap();f
 }
 fn open(t:&mut FileHandles,fs:&mut HierVfs<Disk>,owner:Identity,mode:u64)->u64{
  t.dispatch_authenticated(1,owner,0,mode,"/shared/note",&mut [],fs).unwrap()
 }
 fn read(fs:&mut HierVfs<Disk>,p:&str)->Vec<u8>{
  let mut b=[0;MAX_FILE_BYTES];let n=fs.read(p,&mut b).unwrap();b[..n].to_vec()
 }
 #[test]fn r094_append_authenticated_persists_and_tracks_cursor(){
  let mut fs=volume();let mut t=FileHandles::new();let owner=t.bind_process(1).unwrap();
  let fd=open(&mut t,&mut fs,owner,READ|WRITE);
  assert_eq!(t.dispatch_authenticated(7,owner,fd,0,"",&mut b"DE".to_vec(),&mut fs),Ok(2));
  assert_eq!(read(&mut fs,"/shared/note"),b"abcDE");
  assert_eq!(t.seek_for(owner,fd,MAX_FILE_BYTES as u64),Ok(MAX_FILE_BYTES as u64));
  assert_eq!(t.dispatch_authenticated(14,owner,fd,0,"",&mut [],&mut fs).is_ok(),true);
 }
 #[test]fn r094_authenticated_insert_and_remove_range(){
  let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(1).unwrap();
  let fd=open(&mut t,&mut fs,o,READ|WRITE);
  t.seek_for(o,fd,1).unwrap();
  assert_eq!(t.dispatch_authenticated(8,o,fd,0,"",&mut b"ZZ".to_vec(),&mut fs),Ok(2));
  assert_eq!(read(&mut fs,"/shared/note"),b"aZZbc");
  t.seek_for(o,fd,1).unwrap();
  assert_eq!(t.dispatch_authenticated(9,o,fd,2,"",&mut [],&mut fs),Ok(2));
  assert_eq!(read(&mut fs,"/shared/note"),b"abc");
 }
 #[test]fn r094_authenticated_fill_uses_one_byte_value_and_bounded_length(){
  let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(1).unwrap();
  let fd=open(&mut t,&mut fs,o,WRITE);
  t.seek_for(o,fd,1).unwrap();
  assert_eq!(t.dispatch_authenticated(10,o,fd,2,"",&mut b"X".to_vec(),&mut fs),Ok(2));
  assert_eq!(read(&mut fs,"/shared/note"),b"aXX");
 }
 #[test]fn r094_conditional_replace_denies_mismatch_without_quota_charge(){
  let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(1).unwrap();
  let fd=open(&mut t,&mut fs,o,WRITE);let g=fs.generation();
  assert_eq!(t.dispatch_authenticated(11,o,fd,3,"",&mut b"badNEW".to_vec(),&mut fs),Ok(0));
  assert_eq!(fs.generation(),g);
  assert_eq!(t.security_statistics().completed_writes,0);
  assert_eq!(t.dispatch_authenticated(11,o,fd,3,"",&mut b"abcNEW".to_vec(),&mut fs),Ok(1));
  assert_eq!(read(&mut fs,"/shared/note"),b"NEW");
  assert_eq!(t.security_statistics().completed_writes,1);
 }
 #[test]fn r094_conditional_remove_retires_token(){
  let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(1).unwrap();
  let fd=open(&mut t,&mut fs,o,WRITE);
  assert_eq!(t.dispatch_authenticated(12,o,fd,0,"",&mut b"bad".to_vec(),&mut fs),Ok(0));
  assert_eq!(t.dispatch_authenticated(12,o,fd,0,"",&mut b"abc".to_vec(),&mut fs),Ok(1));
  assert_eq!(t.dispatch_authenticated(12,o,fd,0,"",&mut b"abc".to_vec(),&mut fs),Err(FsError::InvalidName));
  assert_eq!(fs.read("/shared/note",&mut[0;16]),Err(FsError::NotFound));
 }
 #[test]fn r094_conditional_rename_requires_target_path_permission(){
  let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(1).unwrap();
  let fd=open(&mut t,&mut fs,o,WRITE);
  assert_eq!(t.dispatch_authenticated(13,o,fd,0,"/boot/kernel",&mut b"abc".to_vec(),&mut fs),Err(FsError::InvalidName));
  assert_eq!(t.dispatch_authenticated(13,o,fd,0,"/shared/new",&mut b"abc".to_vec(),&mut fs),Ok(1));
  assert_eq!(read(&mut fs,"/shared/new"),b"abc");
  assert_eq!(t.dispatch_authenticated(8,o,fd,0,"",&mut b"x".to_vec(),&mut fs),Err(FsError::InvalidName));
 }
 #[test]fn r094_crc_and_exact_match_are_readonly_for_pid_two(){
  let mut fs=volume();let mut t=FileHandles::new();let peer=t.bind_process(2).unwrap();
  let fd=open(&mut t,&mut fs,peer,READ);let g=fs.generation();
  assert!(t.dispatch_authenticated(14,peer,fd,0,"",&mut [],&mut fs).is_ok());
  assert_eq!(t.dispatch_authenticated(15,peer,fd,0,"",&mut b"abc".to_vec(),&mut fs),Ok(1));
  assert_eq!(t.dispatch_authenticated(15,peer,fd,0,"",&mut b"bad".to_vec(),&mut fs),Ok(0));
  assert_eq!(t.dispatch_authenticated(7,peer,fd,0,"",&mut b"x".to_vec(),&mut fs),Err(FsError::InvalidName));
  assert_eq!(fs.generation(),g);
 }
 #[test]fn r094_swap_names_retires_source_handle(){
  let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(1).unwrap();
  let fd=open(&mut t,&mut fs,o,WRITE);
  assert_eq!(t.dispatch_authenticated(16,o,fd,0,"/shared/other",&mut [],&mut fs),Ok(1));
  assert_eq!(read(&mut fs,"/shared/note"),b"xyz");
  assert_eq!(read(&mut fs,"/shared/other"),b"abc");
  assert_eq!(t.dispatch_authenticated(16,o,fd,0,"/shared/other",&mut [],&mut fs),Err(FsError::InvalidName));
 }
 #[test]fn narrowed_root_confines_conditional_rename_and_swap_destinations(){
  for action in [13,16]{
   let mut fs=volume();
   fs.mkdir("/shared/docs").unwrap();fs.create("/shared/docs/src",b"abc").unwrap();
   fs.mkdir("/shared/docs2").unwrap();fs.create("/shared/docs2/out",b"foreign").unwrap();
   if action==16{fs.create("/shared/docs/dst",b"inside").unwrap();}
   let mut table=FileHandles::new();let owner=table.bind_process(1).unwrap();
   table.narrow_root_kernel(owner,"/shared/docs").unwrap();
   let token=table.dispatch_authenticated(1,owner,0,WRITE,"/shared/docs/src",
     &mut[],&mut fs).unwrap();
   let generation=fs.generation();let stats=table.security_statistics();
   for target in ["/shared/docs2/out","/shared/other","/boot/kernel",
                  "/shared/docs/../out","relative",""]{
    let mut data=if action==13{b"abc".to_vec()}else{Vec::new()};
    assert_eq!(table.dispatch_authenticated(action,owner,token,0,target,
      &mut data,&mut fs),Err(FsError::InvalidName),"action {action}: {target}");
    assert_eq!(fs.generation(),generation);
    assert_eq!(read(&mut fs,"/shared/docs/src"),b"abc");
    assert_eq!(read(&mut fs,"/shared/docs2/out"),b"foreign");
    assert!(table.index(owner,token).is_ok());
    assert_eq!(table.security_statistics().completed_writes,stats.completed_writes);
    assert_eq!(table.security_statistics().changed_bytes,stats.changed_bytes);
   }
   if action==13{
    assert_eq!(table.dispatch_authenticated(13,owner,token,0,"/shared/docs/dst",
      &mut b"mismatch".to_vec(),&mut fs),Ok(0));
    assert_eq!(fs.generation(),generation);
   }
   let mut data=if action==13{b"abc".to_vec()}else{Vec::new()};
   assert_eq!(table.dispatch_authenticated(action,owner,token,0,"/shared/docs/./dst",
     &mut data,&mut fs),Ok(1));
   assert_eq!(read(&mut fs,"/shared/docs/dst"),b"abc");
   assert!(table.index(owner,token).is_err());
   assert_eq!(table.security_statistics().completed_writes,stats.completed_writes+1);
   if action==16{assert_eq!(read(&mut fs,"/shared/docs/src"),b"inside");}
  }
 }
 #[test]fn range_locks_preserve_disjoint_reads_conflicts_and_retirement(){
  let mut fs=volume();let mut table=FileHandles::new();
  let owner=table.bind_process(1).unwrap();let peer=table.bind_process(2).unwrap();
  let fd=table.dispatch_authenticated(1,owner,0,READ|WRITE,"/shared/note",&mut[],&mut fs).unwrap();
  let other=table.dispatch_authenticated(1,peer,0,READ,"/shared/note",&mut[],&mut fs).unwrap();
  let range=|start:u64,len:u64|start|(len<<32);
  assert_eq!(table.dispatch_authenticated(35,owner,fd,range(0,1),"",&mut[],&mut fs),Ok(0));
  let mut byte=[0xa5];
  assert_eq!(table.dispatch_authenticated(2,peer,other,0,"",&mut byte,&mut fs),Err(FsError::Full));
  assert_eq!(byte,[0xa5]);
  table.dispatch_authenticated(4,peer,other,1,"",&mut[],&mut fs).unwrap();
  assert_eq!(table.dispatch_authenticated(2,peer,other,0,"",&mut byte,&mut fs),Ok(1));
  assert_eq!(table.dispatch_authenticated(34,peer,other,range(1,1),"",&mut[],&mut fs),Ok(0));
  assert_eq!(table.dispatch_authenticated(35,owner,fd,range(1,1),"",&mut[],&mut fs),Ok(1));
  assert_eq!(table.entries[table.index(owner,fd).unwrap()].lock_start,0);
  assert_eq!(table.dispatch_authenticated(34,peer,other,range(0,1),"",&mut[],&mut fs),Ok(1));
  assert!(table.retire_process(owner));
  assert_eq!(table.dispatch_authenticated(34,peer,other,range(0,1),"",&mut[],&mut fs),Ok(0));
  assert_eq!(table.dispatch_authenticated(36,owner,fd,0,"",&mut[],&mut fs),Err(FsError::InvalidName));
  assert_eq!(table.dispatch_authenticated(36,peer,other,0,"",&mut[],&mut fs),Ok(0));
 }
 #[test]fn filesystem_abi_and_lock_control_authenticate_without_native_device_probe(){
  let mut table=FileHandles::new();let owner=table.bind_process(1).unwrap();
  assert_eq!(operation_authenticated(&mut table,37,owner,0,0,"",&mut[]),Ok(FS_ABI_INFO));
  assert_eq!(FS_ABI_INFO&0xffff,1);assert_eq!((FS_ABI_INFO>>16)&0xffff,64);
  assert_eq!(operation_authenticated(&mut table,37,owner,1,0,"",&mut[]),Err(FsError::InvalidName));
  assert_eq!(operation_authenticated(&mut table,37,owner,0,1,"",&mut[]),Err(FsError::InvalidName));
  assert_eq!(FileHandles::lock_range(u64::MAX),Err(FsError::TooLarge));
  assert_eq!(FileHandles::lock_range(MAX_FILE_BYTES as u64),Err(FsError::TooLarge));
  let replacement=table.bind_process(1).unwrap();assert_ne!(owner,replacement);
  assert_eq!(operation_authenticated(&mut table,37,owner,0,0,"",&mut[]),Err(FsError::InvalidName));
 }
 #[test]fn persistent_permissions_enforce_kernel_credentials_and_native_chmod(){
  struct FlushDisk(Disk);
  impl BlockDevice for FlushDisk{
   fn sector_count(&self)->u64{self.0.sector_count()}
   fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{self.0.read_sector(lba,out)}
   fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{self.0.write_sector(lba,data)}
   fn sync(&mut self)->Result<(),BlockError>{Ok(())}
  }
  let mut fs=HierVfs::mount_healthy(FlushDisk(volume().into_device())).unwrap();
  let mut table=FileHandles::new();
  let owner=table.bind_process(1).unwrap();let peer=table.bind_process(2).unwrap();
  table.set_credentials_kernel(owner,1000,1000).unwrap();
  table.set_credentials_kernel(peer,2000,2000).unwrap();
  let creds=table.credentials_for(owner).unwrap();
  fs.create_as(creds,"/shared/private",b"secret",0o600).unwrap();
  let fd=table.dispatch_authenticated(1,owner,0,READ|WRITE,"/shared/private",&mut[],&mut fs).unwrap();
  assert_eq!(table.dispatch_authenticated(1,peer,0,READ,"/shared/private",&mut[],&mut fs),Err(FsError::InvalidName));
  assert_eq!(table.dispatch_authenticated(38,owner,fd,0o644,"",&mut[],&mut fs),Ok(0));
  assert!(table.index(owner,fd).is_err());
  let mut fs=HierVfs::mount_healthy(fs.into_device()).unwrap();
  let mode=fs.permissions("/shared/private").unwrap();
  assert_eq!((mode.owner,mode.group,mode.mode),(1000,1000,0o644));
  let readable=table.dispatch_authenticated(1,peer,0,READ,"/shared/private",&mut[],&mut fs).unwrap();
  let mut output=[0xa5;6];
  assert_eq!(table.dispatch_authenticated(2,peer,readable,0,"",&mut output,&mut fs),Ok(6));
  assert_eq!(&output,b"secret");
  assert_eq!(table.dispatch_authenticated(38,peer,readable,0o777,"",&mut[],&mut fs),Err(FsError::InvalidName));
  table.set_credentials_kernel(peer,3000,3000).unwrap();
  assert!(table.index(peer,readable).is_err());
  assert_eq!(table.credentials_for(peer).unwrap().uid(),3000);
  let fresh=table.bind_process(2).unwrap();
  assert_eq!(table.credentials_for(fresh).unwrap().uid(),1000);
  assert_eq!(table.set_credentials_kernel(peer,0,0),Err(FsError::InvalidName));
 }
 #[test]fn directory_search_and_parent_write_permissions_survive_remount(){
  let mut fs=volume();
  let owner=crate::fs_permissions::Credentials::kernel(1000,1000);
  let other=crate::fs_permissions::Credentials::kernel(2000,2000);
  fs.mkdir_as(owner,"/shared/private",0o700).unwrap();
  fs.create_as(owner,"/shared/private/n",b"hidden",0o644).unwrap();
  let mut fs=HierVfs::mount_healthy(fs.into_device()).unwrap();
  let mut output=[0xa5;6];
  assert_eq!(fs.read_as(other,"/shared/private/n",&mut output),Err(FsError::InvalidName));
  assert_eq!(output,[0xa5;6]);
  assert_eq!(fs.create_as(other,"/shared/private/x",b"bad",0o777),Err(FsError::InvalidName));
  assert_eq!(fs.chmod_as(other,"/shared/private",0o777),Err(FsError::InvalidName));
  fs.chmod_as(owner,"/shared/private",0o755).unwrap();
  assert_eq!(fs.read_as(other,"/shared/private/n",&mut output),Ok(6));
  assert_eq!(&output,b"hidden");
 }
 #[test]fn r094_forged_and_replaced_pid_cannot_mutate(){
  let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(1).unwrap();
  let fd=open(&mut t,&mut fs,o,WRITE);let false_owner=Identity{pid:1,generation:o.generation+42};
  assert_eq!(t.dispatch_authenticated(7,false_owner,fd,0,"",&mut b"x".to_vec(),&mut fs),Err(FsError::InvalidName));
  let replacement=t.bind_process(1).unwrap();
  assert_eq!(t.dispatch_authenticated(7,o,fd,0,"",&mut b"x".to_vec(),&mut fs),Err(FsError::InvalidName));
  assert!(t.owns(replacement));
  assert_eq!(read(&mut fs,"/shared/note"),b"abc");
 }
 #[test]fn r094_stale_handle_generation_stops_new_mutations(){
  let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(1).unwrap();
  let fd=open(&mut t,&mut fs,o,WRITE);fs.overwrite("/shared/note",b"new").unwrap();
  assert_eq!(t.dispatch_authenticated(8,o,fd,0,"",&mut b"x".to_vec(),&mut fs),Err(FsError::InvalidName));
  assert_eq!(read(&mut fs,"/shared/note"),b"new");
 }
}

#[cfg(test)]
mod r099_authenticated_search {
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct Disk([[u8;SECTOR_BYTES];100]);
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,l:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.0.get(l as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,l:u64,buf:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.0.get_mut(l as usize).ok_or(BlockError::OutOfRange)?=*buf;Ok(())
  }
 }
 fn volume()->HierVfs<Disk>{
  let mut f=HierVfs::format_blank(Disk([[0;SECTOR_BYTES];100])).unwrap();
  f.mkdir("/shared").unwrap();f.create("/shared/note",b"abc").unwrap();f
 }
 fn open(t:&mut FileHandles,fs:&mut HierVfs<Disk>,o:Identity,mode:u64)->u64{
  t.dispatch_authenticated(1,o,0,mode,"/shared/note",&mut [],fs).unwrap()
 }
 fn read(fs:&mut HierVfs<Disk>,path:&str)->Vec<u8>{
  let mut data=[0;MAX_FILE_BYTES];let n=fs.read(path,&mut data).unwrap();data[..n].to_vec()
 }
 #[test]fn r099_01_first_occurrence(){let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(2).unwrap();let fd=open(&mut t,&mut fs,o,READ);let mut needle=b"bc".to_vec();assert_eq!(t.dispatch_authenticated(17,o,fd,0,"",&mut needle,&mut fs),Ok(1));}
 #[test]fn r099_02_last_occurrence(){let mut fs=volume();fs.overwrite("/shared/note",b"abcabc").unwrap();let mut t=FileHandles::new();let o=t.bind_process(2).unwrap();let fd=open(&mut t,&mut fs,o,READ);let mut needle=b"bc".to_vec();assert_eq!(t.dispatch_authenticated(18,o,fd,0,"",&mut needle,&mut fs),Ok(4));}
 #[test]fn r099_03_disjoint_occurrence_count(){let mut fs=volume();fs.overwrite("/shared/note",b"aaaaa").unwrap();let mut t=FileHandles::new();let o=t.bind_process(2).unwrap();let fd=open(&mut t,&mut fs,o,READ);let mut needle=b"aa".to_vec();assert_eq!(t.dispatch_authenticated(19,o,fd,0,"",&mut needle,&mut fs),Ok(2));}
 #[test]fn r099_04_prefix_check(){let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(2).unwrap();let fd=open(&mut t,&mut fs,o,READ);assert_eq!(t.dispatch_authenticated(20,o,fd,0,"",&mut b"ab".to_vec(),&mut fs),Ok(1));assert_eq!(t.dispatch_authenticated(20,o,fd,0,"",&mut b"bc".to_vec(),&mut fs),Ok(0));}
 #[test]fn r099_05_suffix_check(){let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(2).unwrap();let fd=open(&mut t,&mut fs,o,READ);assert_eq!(t.dispatch_authenticated(21,o,fd,0,"",&mut b"bc".to_vec(),&mut fs),Ok(1));}
 #[test]fn r099_06_verified_head_peek(){let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(2).unwrap();let fd=open(&mut t,&mut fs,o,READ);let mut out=[0xee;2];assert_eq!(t.dispatch_authenticated(22,o,fd,0,"",&mut out,&mut fs),Ok(2));assert_eq!(&out,b"ab");}
 #[test]fn r099_07_verified_tail_peek(){let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(2).unwrap();let fd=open(&mut t,&mut fs,o,READ);let mut out=[0xee;2];assert_eq!(t.dispatch_authenticated(23,o,fd,0,"",&mut out,&mut fs),Ok(2));assert_eq!(&out,b"bc");}
 #[test]fn r099_08_authenticated_prepend(){let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(1).unwrap();let fd=open(&mut t,&mut fs,o,WRITE);assert_eq!(t.dispatch_authenticated(24,o,fd,0,"",&mut b"X".to_vec(),&mut fs),Ok(1));assert_eq!(read(&mut fs,"/shared/note"),b"Xabc");assert_eq!(t.security_statistics().completed_writes,1);}
 #[test]fn r099_09_readonly_peer_cannot_prepend(){let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(2).unwrap();let fd=open(&mut t,&mut fs,o,READ);let g=fs.generation();assert_eq!(t.dispatch_authenticated(24,o,fd,0,"",&mut b"x".to_vec(),&mut fs),Err(FsError::InvalidName));assert_eq!(fs.generation(),g);}
 #[test]fn r099_10_stale_process_cannot_query(){let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(2).unwrap();let fd=open(&mut t,&mut fs,o,READ);let n=t.bind_process(2).unwrap();assert_eq!(t.dispatch_authenticated(17,o,fd,0,"",&mut b"a".to_vec(),&mut fs),Err(FsError::InvalidName));assert!(t.owns(n));}
 #[test]fn r099_11_verified_peek_refuses_too_large(){let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(2).unwrap();let fd=open(&mut t,&mut fs,o,READ);let mut out=[0x55;73];assert_eq!(t.dispatch_authenticated(22,o,fd,0,"",&mut out,&mut fs),Err(FsError::TooLarge));assert_eq!(out,[0x55;73]);}
 #[test]fn r099_12_missing_match_sentinel(){let mut fs=volume();let mut t=FileHandles::new();let o=t.bind_process(2).unwrap();let fd=open(&mut t,&mut fs,o,READ);assert_eq!(t.dispatch_authenticated(17,o,fd,0,"",&mut b"not-found".to_vec(),&mut fs),Ok(u64::MAX));}
}


#[cfg(test)]
mod r100_positioned_handle_tests {
 use super::*;
 use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};
 struct Disk([[u8;SECTOR_BYTES];100]);
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,l:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.0.get(l as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,l:u64,input:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.0.get_mut(l as usize).ok_or(BlockError::OutOfRange)?=*input;Ok(())
  }
 }
 fn fixture()->(HierVfs<Disk>,FileHandles,Identity,Identity,u64,u64){
  let mut fs=HierVfs::format_blank(Disk([[0;SECTOR_BYTES];100])).unwrap();
  fs.mkdir("/shared").unwrap();fs.create("/shared/note",b"abcdef").unwrap();
  let mut handles=FileHandles::new();
  let admin=handles.bind_process(1).unwrap();
  let guest=handles.bind_process(2).unwrap();
  let writer=handles.dispatch_authenticated(1,admin,0,READ|WRITE,
     "/shared/note",&mut[],&mut fs).unwrap();
  let reader=handles.dispatch_authenticated(1,guest,0,READ,
     "/shared/note",&mut[],&mut fs).unwrap();
  (fs,handles,admin,guest,writer,reader)
 }
 #[test]fn positioned_read_does_not_move_cursor(){
  let (mut fs,mut h,admin,_,fd,_)=fixture();
  h.dispatch_authenticated(4,admin,fd,3,"",&mut[],&mut fs).unwrap();
  let mut out=[0x55;2];
  assert_eq!(h.dispatch_authenticated(25,admin,fd,1,"",&mut out,&mut fs),Ok(2));
  assert_eq!(&out,b"bc");
  assert_eq!(h.dispatch_authenticated(2,admin,fd,0,"",&mut out,&mut fs),Ok(2));
  assert_eq!(&out,b"de");
 }
 #[test]fn positioned_write_persists_without_moving_cursor(){
  let (mut fs,mut h,admin,_,fd,_)=fixture();
  h.dispatch_authenticated(4,admin,fd,2,"",&mut[],&mut fs).unwrap();
  assert_eq!(h.dispatch_authenticated(26,admin,fd,4,"",
                &mut b"XY".to_vec(),&mut fs),Ok(2));
  let mut b=[0;8];
  assert_eq!(fs.read("/shared/note",&mut b),Ok(6));
  assert_eq!(&b[..6],b"abcdXY");
  assert_eq!(h.dispatch_authenticated(2,admin,fd,0,"",&mut b,&mut fs),Ok(4));
  assert_eq!(&b[..4],b"cdXY");
  assert_eq!(h.security_statistics().completed_writes,1);
 }
 #[test]fn read_only_peer_can_pread_but_cannot_pwrite(){
  let (mut fs,mut h,_,peer,_,fd)=fixture();
  let mut b=[0xaa;3];
  assert_eq!(h.dispatch_authenticated(25,peer,fd,0,"",&mut b,&mut fs),Ok(3));
  assert_eq!(&b,b"abc");
  assert_eq!(h.dispatch_authenticated(26,peer,fd,0,"",
                  &mut b"x".to_vec(),&mut fs),Err(FsError::InvalidName));
  assert_eq!(h.security_statistics().completed_writes,0);
 }
 #[test]fn stale_process_and_stale_volume_are_not_authorized(){
  let (mut fs,mut h,owner,_,fd,_)=fixture();
  let token=Identity{pid:owner.pid,generation:owner.generation+5};
  assert_eq!(h.dispatch_authenticated(25,token,fd,0,"",
                &mut[0;3],&mut fs),Err(FsError::InvalidName));
  fs.overwrite("/shared/note",b"new").unwrap();
  assert_eq!(h.dispatch_authenticated(26,owner,fd,0,"",
                &mut b"x".to_vec(),&mut fs),Err(FsError::InvalidName));
 }
 #[test]fn read_out_of_range_does_not_publish_any_output(){
  let (mut fs,mut h,owner,_,fd,_)=fixture();
  let mut b=[0x5a;3];
  assert_eq!(h.dispatch_authenticated(25,owner,fd,u64::MAX,"",
                &mut b,&mut fs),Err(FsError::TooLarge));
  assert_eq!(b,[0x5a;3]);
 }
 #[test]fn positioned_write_overflow_does_not_charge_or_modify_disk(){
  let (mut fs,mut h,owner,_,fd,_)=fixture();
  assert_eq!(h.dispatch_authenticated(26,owner,fd,MAX_FILE_BYTES as u64,"",
               &mut b"x".to_vec(),&mut fs),Err(FsError::TooLarge));
  assert_eq!(h.security_statistics().completed_writes,0);
  let mut b=[0;8];fs.read("/shared/note",&mut b).unwrap();
  assert_eq!(&b[..6],b"abcdef");
 }
}


#[cfg(test)]
mod r102_authenticated_fsync_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct Disk{sectors:[[u8;SECTOR_BYTES];100],flushes:u32,fail:bool}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,l:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.sectors.get(l as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,l:u64,v:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.sectors.get_mut(l as usize).ok_or(BlockError::OutOfRange)?=*v;Ok(())
  }
  fn sync(&mut self)->Result<(),BlockError>{
   if self.fail{return Err(BlockError::Io);}
   self.flushes+=1;Ok(())
  }
 }
 fn fixture()->(HierVfs<Disk>,FileHandles,Identity,Identity,u64,u64){
  let disk=Disk{sectors:[[0;SECTOR_BYTES];100],flushes:0,fail:false};
  let mut fs=HierVfs::format_blank(disk).unwrap();
  fs.mkdir("/shared").unwrap();fs.create("/shared/note",b"hello").unwrap();
  let mut table=FileHandles::new();
  let admin=table.bind_process(1).unwrap();
  let guest=table.bind_process(2).unwrap();
  let writer=table.dispatch_authenticated(1,admin,0,WRITE,
       "/shared/note",&mut[],&mut fs).unwrap();
  let reader=table.dispatch_authenticated(1,guest,0,READ,
       "/shared/note",&mut[],&mut fs).unwrap();
  (fs,table,admin,guest,writer,reader)
 }
 #[test]fn authenticated_writer_can_durably_flush_without_charging_mutation(){
  let (mut fs,mut h,admin,_,writer,_)=fixture();
  let before=fs.generation();
  assert_eq!(h.dispatch_authenticated(27,admin,writer,0,"",&mut[],&mut fs),Ok(before));
  assert_eq!(fs.generation(),before);
  assert_eq!(h.security_statistics().completed_writes,0);
 }
 #[test]fn read_only_peer_cannot_issue_storage_flush(){
  let (mut fs,mut h,_,guest,_,reader)=fixture();
  assert_eq!(h.dispatch_authenticated(27,guest,reader,0,"",&mut[],&mut fs),
            Err(FsError::InvalidName));
 }
 #[test]fn invalid_fsync_arguments_cannot_touch_driver(){
  let (mut fs,mut h,admin,_,writer,_)=fixture();
  assert_eq!(h.dispatch_authenticated(27,admin,writer,1,"",&mut[],&mut fs),
            Err(FsError::InvalidName));
  assert_eq!(h.dispatch_authenticated(27,admin,writer,0,"",&mut[1],&mut fs),
            Err(FsError::InvalidName));
  assert_eq!(h.dispatch_authenticated(27,admin,writer,0,"/other",&mut[],&mut fs),
            Err(FsError::InvalidName));
 }
}


#[cfg(test)]
mod r103_authenticated_cow_create_and_delete {
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct Disk([[u8;SECTOR_BYTES];100]);
 impl BlockDevice for Disk {
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,l:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.0.get(l as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,l:u64,buf:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.0.get_mut(l as usize).ok_or(BlockError::OutOfRange)?=*buf;Ok(())
  }
 }
 fn fixture()->(HierVfs<Disk>,FileHandles,Identity,Identity,u64){
  let mut fs=HierVfs::format_blank(Disk([[0;SECTOR_BYTES];100])).unwrap();
  fs.mkdir("/shared").unwrap();
  fs.create("/shared/old",b"old").unwrap();
  let mut h=FileHandles::new();
  let admin=h.bind_process(1).unwrap();
  let guest=h.bind_process(2).unwrap();
  let token=h.dispatch_authenticated(1,admin,0,WRITE,
     "/shared/old",&mut[],&mut fs).unwrap();
  (fs,h,admin,guest,token)
 }
 #[test]fn guarded_write_rejects_stale_generation_without_quota_or_cursor_move(){
  let (mut fs,mut h,admin,_,token)=fixture();
  let g=fs.generation();
  assert_eq!(h.dispatch_authenticated(28,admin,token,g+1,"",
      &mut b"X".to_vec(),&mut fs),Err(FsError::InvalidVolume));
  assert_eq!(h.security_statistics().completed_writes,0);
  assert_eq!(h.dispatch_authenticated(28,admin,token,g,"",
      &mut b"X".to_vec(),&mut fs),Ok(1));
  let mut out=[0;4];
  assert_eq!(fs.read("/shared/old",&mut out),Ok(3));
  assert_eq!(&out[..3],b"Xld");
  assert_eq!(h.security_statistics().completed_writes,1);
 }
 #[test]fn authenticated_create_persists_and_reopens_only_explicitly(){
  let (mut fs,mut h,admin,_,_)=fixture();
  assert_eq!(h.dispatch_authenticated(29,admin,0,0,"/shared/new",
      &mut b"hello".to_vec(),&mut fs),Ok(5));
  let new=h.dispatch_authenticated(1,admin,0,READ,
      "/shared/new",&mut[],&mut fs).unwrap();
  let mut out=[0;8];
  assert_eq!(h.dispatch_authenticated(2,admin,new,0,"",
      &mut out,&mut fs),Ok(5));
  assert_eq!(&out[..5],b"hello");
  let mut rebooted=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(rebooted.read("/shared/new",&mut out),Ok(5));
  assert_eq!(&out[..5],b"hello");
 }
 #[test]fn guest_cannot_create_or_modify_protected_paths(){
  let (mut fs,mut h,admin,guest,_)=fixture();let g=fs.generation();
  assert_eq!(h.dispatch_authenticated(29,guest,0,0,"/shared/evil",
      &mut b"x".to_vec(),&mut fs),Err(FsError::InvalidName));
  assert_eq!(h.dispatch_authenticated(29,admin,0,0,"/boot/init",
      &mut b"x".to_vec(),&mut fs),Err(FsError::InvalidName));
  assert_eq!(h.dispatch_authenticated(29,admin,0,0,"/shared/../boot/init",
      &mut b"x".to_vec(),&mut fs),Err(FsError::InvalidName));
  assert_eq!(fs.generation(),g);
  assert_eq!(fs.read("/shared/evil",&mut[0;8]),Err(FsError::NotFound));
 }
 #[test]fn unlink_retires_handle_and_persists_namespace_delete(){
  let (mut fs,mut h,admin,_,token)=fixture();
  assert_eq!(h.dispatch_authenticated(30,admin,token,0,"",&mut[],&mut fs),Ok(1));
  assert_eq!(h.dispatch_authenticated(30,admin,token,0,"",&mut[],&mut fs),
             Err(FsError::InvalidName));
  assert_eq!(fs.read("/shared/old",&mut[0;8]),Err(FsError::NotFound));
  let mut recovered=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(recovered.read("/shared/old",&mut[0;8]),Err(FsError::NotFound));
 }
 #[test]fn guest_and_retired_process_cannot_delete_existing_file(){
  let (mut fs,mut h,admin,guest,token)=fixture();
  let read=h.dispatch_authenticated(1,guest,0,READ,
      "/shared/old",&mut[],&mut fs).unwrap();
  assert_eq!(h.dispatch_authenticated(30,guest,read,0,"",&mut[],&mut fs),
             Err(FsError::InvalidName));
  let next=h.bind_process(1).unwrap();
  assert_eq!(h.dispatch_authenticated(30,admin,token,0,"",&mut[],&mut fs),
             Err(FsError::InvalidName));
  assert!(h.owns(next));
  let mut out=[0;8];assert_eq!(fs.read("/shared/old",&mut out),Ok(3));
  assert_eq!(&out[..3],b"old");
 }
}


#[cfg(test)]
mod r104_authenticated_atomic_rename_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct Disk([[u8;SECTOR_BYTES];100]);
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,l:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.0.get(l as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,l:u64,buf:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.0.get_mut(l as usize).ok_or(BlockError::OutOfRange)?=*buf;Ok(())
  }
 }
 fn fixture()->(HierVfs<Disk>,FileHandles,Identity,Identity,u64){
  let mut fs=HierVfs::format_blank(Disk([[0;SECTOR_BYTES];100])).unwrap();
  fs.mkdir("/shared").unwrap();fs.create("/shared/note",b"abc").unwrap();
  let mut h=FileHandles::new();
  let owner=h.bind_process(1).unwrap();let guest=h.bind_process(2).unwrap();
  let fd=h.dispatch_authenticated(1,owner,0,WRITE,
      "/shared/note",&mut[],&mut fs).unwrap();
  (fs,h,owner,guest,fd)
 }
 #[test]fn rename_is_atomic_persistent_and_retires_original_handle(){
  let (mut fs,mut h,owner,_,fd)=fixture();let gen=fs.generation();
  assert_eq!(h.dispatch_authenticated(31,owner,fd,0,"/shared/new",&mut[],&mut fs),Ok(1));
  assert_eq!(fs.generation(),gen+1);
  assert_eq!(h.dispatch_authenticated(31,owner,fd,0,"/shared/other",&mut[],&mut fs),
             Err(FsError::InvalidName));
  let mut recovered=HierVfs::mount(fs.into_device()).unwrap();
  let mut b=[0u8;4];
  assert_eq!(recovered.read("/shared/note",&mut b),Err(FsError::NotFound));
  assert_eq!(recovered.read("/shared/new",&mut b),Ok(3));
  assert_eq!(&b[..3],b"abc");
  assert_eq!(h.security_statistics().completed_writes,1);
 }
 #[test]fn foreign_owner_protected_target_and_traversal_are_denied(){
  let (mut fs,mut h,owner,guest,fd)=fixture();let gen=fs.generation();
  assert_eq!(h.dispatch_authenticated(31,guest,fd,0,"/shared/evil",&mut[],&mut fs),
             Err(FsError::InvalidName));
  assert_eq!(h.dispatch_authenticated(31,owner,fd,0,"/boot/init",&mut[],&mut fs),
             Err(FsError::InvalidName));
  assert_eq!(h.dispatch_authenticated(31,owner,fd,0,
             "/shared/../boot/init",&mut[],&mut fs),Err(FsError::InvalidName));
  assert_eq!(fs.generation(),gen);
  assert_eq!(h.security_statistics().completed_writes,0);
 }
 #[test]fn existing_target_collision_is_read_only(){
  let (mut fs,mut h,owner,_,fd)=fixture();
  fs.create("/shared/occupied",b"other").unwrap();
  let fresh=h.dispatch_authenticated(1,owner,0,WRITE,
      "/shared/note",&mut[],&mut fs).unwrap();
  let gen=fs.generation();
  assert_eq!(h.dispatch_authenticated(31,owner,fresh,0,
      "/shared/occupied",&mut[],&mut fs),Err(FsError::Exists));
  assert_eq!(fs.generation(),gen);
  assert_eq!(h.security_statistics().completed_writes,0);
  let mut b=[0u8;8];
  assert_eq!(fs.read("/shared/note",&mut b),Ok(3));
  assert_eq!(&b[..3],b"abc");
 }
}


#[cfg(test)]
mod r106_checked_partition_authenticated_file_api_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 use crate::m1_partition::{inspect_partition_receipt,PartitionReceipt};
 const COUNT:usize=256;
 struct Disk{sectors:[[u8;SECTOR_BYTES];COUNT],reads:usize,writes:usize}
 struct OfflineDescriptorDevice;
 impl BlockDevice for OfflineDescriptorDevice{
  fn sector_count(&self)->u64{panic!("cached descriptor inspected offline capacity")}
  fn read_sector(&mut self,_:u64,_:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   panic!("cached descriptor read offline media")
  }
  fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   panic!("cached descriptor wrote offline media")
  }
 }
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{COUNT as u64}
  fn read_sector(&mut self,l:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   self.reads+=1;
   *out=*self.sectors.get(l as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,l:u64,buf:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.sectors.get_mut(l as usize).ok_or(BlockError::OutOfRange)?=*buf;
   self.writes+=1;Ok(())
  }
 }

 #[test]fn unselected_authenticated_preflight_preserves_legacy_mutation_semantics(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  let admin=table.bind_process(1).unwrap();let guest=table.bind_process(2).unwrap();
  let mut volume=crate::m1_partition::mount_partition_receipt_healthy(&mut disk,receipt).unwrap();
  let writer=table.dispatch_authenticated(1,admin,0,WRITE,"/shared/note",&mut[],&mut volume).unwrap();
  let reader=table.dispatch_authenticated(1,guest,0,READ,"/shared/note",&mut[],&mut volume).unwrap();
  assert!(table.selected_volume.is_none());
  // operation_authenticated uses this shared preflight even on the legacy
  // raw-volume route; its semantics must agree with mounted dispatch.
  assert_eq!(table.preflight_selected(3,admin,writer,0,"",b"X"),Ok(()));
  assert_eq!(table.dispatch_authenticated(3,admin,writer,0,"",
    &mut b"X".to_vec(),&mut volume),Ok(1));
  assert_eq!(table.dispatch_authenticated(2,guest,reader,0,"",
    &mut[0;9],&mut volume),Err(FsError::InvalidName));
 }

 #[test]fn selected_cached_seek_and_close_release_foreign_lease_without_media(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let admin=table.bind_process(1).unwrap();let guest=table.bind_process(2).unwrap();
  let writer=operation_on_selected_volume(&mut table,1,admin,0,READ|WRITE,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let reader=operation_on_selected_volume(&mut table,1,guest,0,READ,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let before=(disk.reads,disk.writes);let stats=table.security_statistics();
  assert_eq!(operation_on_selected_volume(&mut table,4,guest,reader,5,"",
    &mut[],&mut OfflineDescriptorDevice,receipt),Ok(5));
  assert_eq!(table.entries[table.index(guest,reader).unwrap()].offset,5);
  assert_eq!(operation_on_selected_volume(&mut table,3,admin,writer,0,"",
    &mut[b'X'],&mut OfflineDescriptorDevice,receipt),Err(FsError::InvalidName));
  // Exercise the actual runtime adapter too: this must return before its
  // native AtaPio::probe, which a host test cannot legitimately execute.
  assert_eq!(operation_authenticated(&mut table,5,guest,reader,0,"",&mut[]),Ok(0));
  assert_eq!((disk.reads,disk.writes),before);
  assert_eq!(table.security_statistics(),stats);
  assert!(!table.has_foreign_descriptors(admin));
  assert_eq!(operation_on_selected_volume(&mut table,2,guest,reader,0,"",
    &mut[0xa5;4],&mut OfflineDescriptorDevice,receipt),Err(FsError::InvalidName));
  assert_eq!(operation_on_selected_volume(&mut table,3,admin,writer,0,"",
    &mut[b'X'],&mut disk,receipt),Ok(1));
  assert_eq!(table.security_statistics().completed_writes,stats.completed_writes+1);
 }
 #[test]fn selected_rename_destinations_fail_namespace_preflight_before_media(){
  let (mut disk,receipt)=disk();
  {
   let mut volume=crate::m1_partition::mount_partition_receipt_healthy(&mut disk,receipt).unwrap();
   volume.mkdir("/shared/docs").unwrap();volume.create("/shared/docs/src",b"abc").unwrap();
  }
  let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let owner=table.bind_process(1).unwrap();
  table.narrow_root_kernel(owner,"/shared/docs").unwrap();
  let token=operation_on_selected_volume(&mut table,1,owner,0,WRITE,
    "/shared/docs/src",&mut[],&mut disk,receipt).unwrap();
  let slot=table.index(owner,token).unwrap();let handle=table.entries[slot];
  let stats=table.security_statistics();let namespace=table.last_syscall_namespace;
  for action in [13,16,31]{
   for target in ["/shared/note","/shared/docs2/item","/boot/kernel",
                  "/shared/docs/../out","relative",""]{
    let mut data=if action==13{b"abc".to_vec()}else{Vec::new()};
    assert_eq!(operation_on_selected_volume(&mut table,action,owner,token,0,
      target,&mut data,&mut OfflineDescriptorDevice,receipt),Err(FsError::InvalidName));
    // The native runtime adapter must also reject before AtaPio::probe.
    assert_eq!(operation_authenticated(&mut table,action,owner,token,0,
      target,&mut data),Err(FsError::InvalidName));
   }
   let data=if action==13{b"abc".as_slice()}else{b"".as_slice()};
   assert_eq!(table.preflight_selected(action,owner,token,0,"/shared/docs/dst",data),Ok(()));
  }
  assert_eq!(table.security_statistics(),stats);
  assert_eq!(table.last_syscall_namespace,namespace);
  assert_eq!((table.entries[slot].token,table.entries[slot].offset,
    table.entries[slot].filesystem_generation),
    (handle.token,handle.offset,handle.filesystem_generation));
 }

 #[test]fn selected_cached_descriptor_denies_foreign_stale_and_malformed_requests(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  let guest=table.bind_process(2).unwrap();let admin=table.bind_process(1).unwrap();
  let token=operation_on_selected_volume(&mut table,1,guest,0,READ,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let slot=table.index(guest,token).unwrap();let before=table.entries[slot];
  for action in [4,5]{
   assert_eq!(operation_on_selected_volume(&mut table,action,admin,token,0,"",
     &mut[],&mut OfflineDescriptorDevice,receipt),Err(FsError::InvalidName));
   assert_eq!(operation_on_selected_volume(&mut table,action,guest,token,0,"/shared/note",
     &mut[],&mut OfflineDescriptorDevice,receipt),Err(FsError::InvalidName));
   let mut bytes=[0xa5];
   assert_eq!(operation_on_selected_volume(&mut table,action,guest,token,0,"",
     &mut bytes,&mut OfflineDescriptorDevice,receipt),Err(FsError::InvalidName));
   assert_eq!(bytes,[0xa5]);
  }
  assert_eq!(operation_on_selected_volume(&mut table,5,guest,token,1,"",
    &mut[],&mut OfflineDescriptorDevice,receipt),Err(FsError::InvalidName));
  assert_eq!(operation_on_selected_volume(&mut table,4,guest,token,MAX_FILE_BYTES as u64+1,"",
    &mut[],&mut OfflineDescriptorDevice,receipt),Err(FsError::TooLarge));
  let mut foreign=receipt;foreign.mbr_sector[440]^=1;
  assert_eq!(operation_on_selected_volume(&mut table,5,guest,token,0,"",
    &mut[],&mut OfflineDescriptorDevice,foreign),Err(FsError::InvalidVolume));
  assert_eq!((table.entries[slot].token,table.entries[slot].offset,
    table.entries[slot].filesystem_generation),
    (before.token,before.offset,before.filesystem_generation));
  let fresh=table.bind_process(2).unwrap();
  for owner in [guest,fresh]{
   assert_eq!(operation_on_selected_volume(&mut table,5,owner,token,0,"",
     &mut[],&mut OfflineDescriptorDevice,receipt),Err(FsError::InvalidName));
  }
 }

 #[test]fn selected_cached_seek_never_refreshes_a_stale_filesystem_identity(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  let owner=table.bind_process(1).unwrap();
  let token=operation_on_selected_volume(&mut table,1,owner,0,READ,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let slot=table.index(owner,token).unwrap();
  let generation=table.entries[slot].filesystem_generation;
  {
   let mut volume=crate::m1_partition::mount_partition_receipt_healthy(&mut disk,receipt).unwrap();
   volume.write_at("/shared/note",0,b"changed").unwrap();
   assert_ne!(volume.generation(),generation);
  }
  assert_eq!(operation_on_selected_volume(&mut table,4,owner,token,1,"",
    &mut[],&mut OfflineDescriptorDevice,receipt),Ok(1));
  assert_eq!(table.entries[slot].filesystem_generation,generation);
  let mut output=[0xa5;8];
  assert_eq!(operation_on_selected_volume(&mut table,2,owner,token,0,"",
    &mut output,&mut disk,receipt),Err(FsError::InvalidName));
  assert_eq!(output,[0xa5;8]);
  assert_eq!(operation_on_selected_volume(&mut table,5,owner,token,0,"",
    &mut[],&mut OfflineDescriptorDevice,receipt),Ok(0));
 }

 #[test]fn selected_descriptor_mutations_preserve_foreign_leases_before_any_device_access(){
  struct NeverIo;
  impl BlockDevice for NeverIo{
   fn sector_count(&self)->u64{panic!("lease denial inspected device")}
   fn read_sector(&mut self,_:u64,_:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{panic!("read")}
   fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])->Result<(),BlockError>{panic!("write")}
  }
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let admin=table.bind_process(1).unwrap();let guest=table.bind_process(2).unwrap();
  let writer=operation_on_selected_volume(&mut table,1,admin,0,READ|WRITE,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let reader=operation_on_selected_volume(&mut table,1,guest,0,READ,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let stats=table.security_statistics();
  let original=table.entries;
  // Exercise every selected descriptor mutation, including pathless write,
  // truncate, edit, positioned/generation write and namespace operations.
  for action in [3,6,7,8,9,10,11,12,13,16,24,26,28,29,30,31,32]{
   let (token,path)=match action{29=>(0,"/shared/new"),32=>(0,"/shared/a"),
      31=>(writer,"/shared/new"),_=>(writer,"")};
   let mut bytes=if action==32{b"/shared/b".to_vec()}else{b"X".to_vec()};
   let previous=bytes.clone();
   assert_eq!(operation_on_selected_volume(&mut table,action,admin,token,0,
     path,&mut bytes,&mut NeverIo,receipt),Err(FsError::InvalidName),"action {action}");
   assert_eq!(bytes,previous);
  }
  assert_eq!(table.security_statistics(),stats);
  for (a,b) in table.entries.iter().zip(original.iter()){
   assert_eq!((a.token,a.offset,a.filesystem_generation),(b.token,b.offset,b.filesystem_generation));
  }
  let mut bytes=[0;9];
  assert_eq!(operation_on_selected_volume(&mut table,2,guest,reader,0,"",
    &mut bytes,&mut disk,receipt),Ok(9));
  assert_eq!(&bytes,b"persisted");
 }

 #[test]fn selected_descriptor_write_resumes_after_authenticated_reader_close(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let admin=table.bind_process(1).unwrap();let guest=table.bind_process(2).unwrap();
  let writer=operation_on_selected_volume(&mut table,1,admin,0,READ|WRITE,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let reader=operation_on_selected_volume(&mut table,1,guest,0,READ,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  assert_eq!(operation_on_selected_volume(&mut table,3,admin,writer,0,"",
    &mut b"X".to_vec(),&mut disk,receipt),Err(FsError::InvalidName));
  assert_eq!(operation_on_selected_volume(&mut table,5,admin,reader,0,"",
    &mut[],&mut disk,receipt),Err(FsError::InvalidName));
  assert_eq!(operation_on_selected_volume(&mut table,5,guest,reader,0,"",
    &mut[],&mut disk,receipt),Ok(0));
  assert_eq!(operation_on_selected_volume(&mut table,3,admin,writer,0,"",
    &mut b"X".to_vec(),&mut disk,receipt),Ok(1));
  assert_eq!(table.security_statistics().completed_writes,1);
  let fresh=operation_on_selected_volume(&mut table,1,guest,0,READ,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let mut bytes=[0;9];
  assert_eq!(operation_on_selected_volume(&mut table,2,guest,fresh,0,"",
    &mut bytes,&mut disk,receipt),Ok(9));
  assert_eq!(&bytes,b"Xersisted");
  assert_eq!(operation_on_selected_volume(&mut table,2,guest,reader,0,"",
    &mut bytes,&mut disk,receipt),Err(FsError::InvalidName));
 }

 #[test]fn selected_descriptor_lease_covers_unrelated_file_generation_changes(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let admin=table.bind_process(1).unwrap();let guest=table.bind_process(2).unwrap();
  selected_path_operation_on_device(&mut table,admin,3,"/shared/other","",0,
    &mut b"other".to_vec(),5,&mut disk).unwrap();
  let writer=operation_on_selected_volume(&mut table,1,admin,0,WRITE,
    "/shared/other",&mut[],&mut disk,receipt).unwrap();
  let reader=operation_on_selected_volume(&mut table,1,guest,0,READ,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let before=(disk.reads,disk.writes);
  assert_eq!(operation_on_selected_volume(&mut table,6,admin,writer,1,"",
    &mut[],&mut disk,receipt),Err(FsError::InvalidName));
  assert_eq!((disk.reads,disk.writes),before);
  let mut bytes=[0;9];
  assert_eq!(operation_on_selected_volume(&mut table,2,guest,reader,0,"",
    &mut bytes,&mut disk,receipt),Ok(9));
  assert_eq!(&bytes,b"persisted");
 }

 #[test]fn selected_fsync_with_shared_readers_preserves_their_generation_and_cursor(){
  struct FlushDisk<'a>{disk:&'a mut Disk,flushes:u32}
  impl BlockDevice for FlushDisk<'_>{
   fn sector_count(&self)->u64{COUNT as u64}
   fn read_sector(&mut self,l:u64,b:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{self.disk.read_sector(l,b)}
   fn write_sector(&mut self,l:u64,b:&[u8;SECTOR_BYTES])->Result<(),BlockError>{self.disk.write_sector(l,b)}
   fn sync(&mut self)->Result<(),BlockError>{self.flushes+=1;Ok(())}
  }
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let admin=table.bind_process(1).unwrap();let guest=table.bind_process(2).unwrap();
  let writer=operation_on_selected_volume(&mut table,1,admin,0,WRITE,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let reader=operation_on_selected_volume(&mut table,1,guest,0,READ,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let generation=table.entries[table.index(guest,reader).unwrap()].filesystem_generation;
  let writes=disk.writes;
  let mut device=FlushDisk{disk:&mut disk,flushes:0};
  assert_eq!(operation_on_selected_volume(&mut table,27,admin,writer,0,"",
    &mut[],&mut device,receipt),Ok(generation));
  assert_eq!(device.flushes,1);
  assert_eq!(device.disk.writes,writes);
  assert_eq!(table.security_statistics().completed_writes,0);
  let mut bytes=[0;9];
  assert_eq!(operation_on_selected_volume(&mut table,2,guest,reader,0,"",
    &mut bytes,&mut device,receipt),Ok(9));
  assert_eq!(&bytes,b"persisted");
 }

 #[test]fn selected_mounted_dispatch_cannot_bypass_descriptor_leases(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let admin=table.bind_process(1).unwrap();let guest=table.bind_process(2).unwrap();
  let writer=operation_on_selected_volume(&mut table,1,admin,0,WRITE,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  operation_on_selected_volume(&mut table,1,guest,0,READ,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let writes=disk.writes;
  let mut volume=crate::m1_partition::mount_partition_receipt_healthy(&mut disk,receipt).unwrap();
  let before=volume.generation();
  assert_eq!(table.dispatch_authenticated(3,admin,writer,0,"",
    &mut b"X".to_vec(),&mut volume),Err(FsError::InvalidName));
  assert_eq!(volume.generation(),before);
  drop(volume);assert_eq!(disk.writes,writes);
 }

 #[test]fn selected_descriptor_lease_is_released_by_process_and_namespace_retirement(){
  for mode in 0..3{
   let (mut disk,receipt)=disk();let mut table=FileHandles::new();
   prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
   let admin=table.bind_process(1).unwrap();let guest=table.bind_process(2).unwrap();
   let writer=operation_on_selected_volume(&mut table,1,admin,0,WRITE,
     "/shared/note",&mut[],&mut disk,receipt).unwrap();
   operation_on_selected_volume(&mut table,1,guest,0,READ,
     "/shared/note",&mut[],&mut disk,receipt).unwrap();
   match mode{
    0=>assert!(table.retire_process(guest)),
    1=>{table.narrow_root_kernel(guest,"/shared").unwrap();},
    _=>{assert_ne!(table.bind_process(2).unwrap(),guest);},
   }
   assert_eq!(operation_on_selected_volume(&mut table,6,admin,writer,3,"",
     &mut[],&mut disk,receipt),Ok(0));
  }
 }

 #[test]fn desktop_selected_boot_admission_is_before_process_creation_and_never_formats(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  let writes=disk.writes;
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  assert_eq!(table.selected_volume,Some(receipt));
  assert_eq!(disk.writes,writes);
  let owner=table.bind_process(1).unwrap();let reads=disk.reads;
  assert_eq!(prepare_selected_volume_kernel(&mut table,&mut disk,receipt),
    Err(FsError::InvalidName));
  assert_eq!((disk.reads,disk.writes),(reads,writes));
  assert!(table.owns(owner));
 }

 #[test]fn desktop_selected_boot_admission_failed_receipt_cannot_publish_volume(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  let writes=disk.writes;disk.sectors[0][440]^=1;
  assert_eq!(prepare_selected_volume_kernel(&mut table,&mut disk,receipt),
    Err(FsError::InvalidVolume));
  assert!(table.selected_volume.is_none());assert_eq!(table.volume_revision,1);
  assert_eq!(disk.writes,writes);
  disk.sectors[0][440]^=1;
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  assert_eq!(table.selected_volume,Some(receipt));
 }

 #[test]fn desktop_selected_boot_admission_cannot_reuse_retired_process_table(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  let retired=table.bind_process(1).unwrap();assert!(table.retire_process(retired));
  let reads=disk.reads;let writes=disk.writes;
  assert_eq!(prepare_selected_volume_kernel(&mut table,&mut disk,receipt),
    Err(FsError::InvalidName));
  assert_eq!((disk.reads,disk.writes),(reads,writes));
  assert!(table.selected_volume.is_none());
 }

 #[test]fn desktop_selected_path_crud_range_and_reboot_share_one_real_volume(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let admin=table.bind_process(1).unwrap();let guest=table.bind_process(2).unwrap();
  let fd=operation_on_selected_volume(&mut table,1,guest,0,READ,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let before=(disk.reads,disk.writes);
  assert_eq!(selected_path_operation_on_device(&mut table,admin,5,"/shared/note",
    "",0,&mut b"hello".to_vec(),5,&mut disk),Err(FsError::InvalidName));
  assert_eq!((disk.reads,disk.writes),before);
  assert_eq!(operation_on_selected_volume(&mut table,5,guest,fd,0,"",
    &mut[],&mut disk,receipt),Ok(0));
  assert_eq!(selected_path_operation_on_device(&mut table,admin,5,"/shared/note",
    "",0,&mut b"hello".to_vec(),5,&mut disk),Ok(0));
  assert_eq!(operation_on_selected_volume(&mut table,2,guest,fd,0,"",
    &mut[0;16],&mut disk,receipt),Err(FsError::InvalidName));
  assert_eq!(selected_path_operation_on_device(&mut table,admin,11,"/shared/note",
    "",1,&mut b"A".to_vec(),1,&mut disk),Ok(1));
  let mut bytes=[0u8;32];
  assert_eq!(selected_path_operation_on_device(&mut table,guest,10,"/shared/note",
    "",1,&mut bytes[..3],0,&mut disk),Ok(3));
  assert_eq!(&bytes[..3],b"All");
  assert_eq!(selected_path_operation_on_device(&mut table,admin,7,"/shared/note",
    "/shared/saved",0,&mut[],0,&mut disk),Ok(0));
  assert_eq!(selected_path_operation_on_device(&mut table,admin,1,"/shared/dir",
    "",0,&mut[],0,&mut disk),Ok(0));
  assert_eq!(selected_path_operation_on_device(&mut table,admin,3,"/shared/dir/file",
    "",0,&mut b"x".to_vec(),1,&mut disk),Ok(0));
  assert_eq!(selected_path_operation_on_device(&mut table,admin,6,"/shared/dir/file",
    "",0,&mut[],0,&mut disk),Ok(0));
  assert_eq!(selected_path_operation_on_device(&mut table,admin,2,"/shared/dir",
    "",0,&mut[],0,&mut disk),Ok(0));
  let mut reboot=FileHandles::new();
  prepare_selected_volume_kernel(&mut reboot,&mut disk,receipt).unwrap();
  let reader=reboot.bind_process(2).unwrap();
  assert_eq!(selected_path_operation_on_device(&mut reboot,reader,4,"/shared/saved",
    "",0,&mut bytes,0,&mut disk),Ok(5));
  assert_eq!(&bytes[..5],b"hAllo");
  let n=selected_path_operation_on_device(&mut reboot,reader,8,"/shared",
    "",0,&mut bytes,0,&mut disk).unwrap();
  assert_eq!(&bytes[..n],b"f:saved\n");
 }


 #[test]fn g7_selected_authentication_io_failure_revokes_handles_and_quarantines_volume(){
  // First measure only the healthy mount's reads. Trigger a disk error on
  // the very next metadata read, which is performed by VFS authorization.
  // It must not escape through an early '?' without quarantining the mount.
  struct DelayedRead<'a>{disk:&'a mut Disk,reads:usize,fail_at:Option<usize>}
  impl BlockDevice for DelayedRead<'_>{
   fn sector_count(&self)->u64{COUNT as u64}
   fn read_sector(&mut self,l:u64,b:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
    let index=self.reads;self.reads+=1;
    if self.fail_at==Some(index){return Err(BlockError::Io);}
    self.disk.read_sector(l,b)
   }
   fn write_sector(&mut self,l:u64,b:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
    self.disk.write_sector(l,b)
   }
  }
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let guest=table.bind_process(2).unwrap();
  let token=operation_on_selected_volume(&mut table,1,guest,0,READ,
      "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let mount_reads={
   let mut measured=DelayedRead{disk:&mut disk,reads:0,fail_at:None};
   let mounted=crate::m1_partition::mount_partition_receipt_healthy(&mut measured,receipt).unwrap();
   drop(mounted);
   measured.reads
  };
  assert!(mount_reads>0);
  let writes=disk.writes;
  let mut failing=DelayedRead{disk:&mut disk,reads:0,fail_at:Some(mount_reads)};
  let mut output=[0xa5u8;16];
  assert_eq!(selected_path_operation_on_device(&mut table,guest,4,
      "/shared/note","",0,&mut output,0,&mut failing),Err(FsError::Io));
  assert_eq!(output,[0xa5;16]);
  assert!(table.selected_quarantined);
  assert!(table.entries.iter().all(|h|h.token==0));
  assert_eq!(table.index(guest,token),Err(FsError::InvalidName));
  assert_eq!(failing.disk.writes,writes);
  let reads=failing.reads;
  assert_eq!(selected_path_operation_on_device(&mut table,guest,4,
      "/shared/note","",0,&mut output,0,&mut failing),Err(FsError::InvalidVolume));
  assert_eq!(failing.reads,reads); // second attempt cannot even probe media
  assert_eq!(output,[0xa5;16]);
 }

 #[test]fn desktop_selected_path_read_lease_blocks_mutations_before_io(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let admin=table.bind_process(1).unwrap();let guest=table.bind_process(2).unwrap();
  let own=operation_on_selected_volume(&mut table,1,admin,0,READ,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let foreign=operation_on_selected_volume(&mut table,1,guest,0,READ,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let before=(disk.reads,disk.writes);
  let stats=table.security_statistics();
  for (action,path,target,used) in [
    (1,"/shared/new","",0),(2,"/shared/dir","",0),
    (3,"/shared/new","",1),(5,"/shared/note","",1),
    (6,"/shared/note","",0),(7,"/shared/note","/shared/new",0),
    (9,"/shared/dir","/shared/renamed",0),(11,"/shared/note","",1)]{
    let mut data=[b'X';16];
    assert_eq!(selected_path_operation_on_device(&mut table,admin,action,path,
      target,0,&mut data,used,&mut disk),Err(FsError::InvalidName));
    assert_eq!(data,[b'X';16]);
  }
  assert_eq!((disk.reads,disk.writes),before);
  assert_eq!(table.security_statistics(),stats);
  // Read sharing is legal and the refused mutations preserve both cursors.
  for (owner,token) in [(admin,own),(guest,foreign)]{
    let mut out=[0;9];
    assert_eq!(operation_on_selected_volume(&mut table,2,owner,token,0,"",
      &mut out,&mut disk,receipt),Ok(9));
    assert_eq!(&out,b"persisted");
  }
  // The foreign owner's actual close releases the lease. A forged close
  // from PID1 cannot release somebody else's authority.
  assert_eq!(operation_on_selected_volume(&mut table,5,admin,foreign,0,"",
    &mut[],&mut disk,receipt),Err(FsError::InvalidName));
  assert_eq!(operation_on_selected_volume(&mut table,5,guest,foreign,0,"",
    &mut[],&mut disk,receipt),Ok(0));
  assert_eq!(selected_path_operation_on_device(&mut table,admin,7,
    "/shared/note","/shared/new",0,&mut[],0,&mut disk),Ok(0));
 }

 #[test]fn desktop_selected_path_revocation_and_pid_reuse_release_lease(){
  for revoke in 0..3{
   let (mut disk,receipt)=disk();let mut table=FileHandles::new();
   prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
   let admin=table.bind_process(1).unwrap();let guest=table.bind_process(2).unwrap();
   let old=operation_on_selected_volume(&mut table,1,guest,0,READ,
     "/shared/note",&mut[],&mut disk,receipt).unwrap();
   match revoke{
    0=>assert!(table.retire_process(guest)),
    1=>{table.narrow_root_kernel(guest,"/shared").unwrap();},
    _=>{assert_ne!(table.bind_process(2).unwrap(),guest);},
   }
   assert_eq!(operation_on_selected_volume(&mut table,2,guest,old,0,"",
     &mut[0;9],&mut disk,receipt),Err(FsError::InvalidName));
   assert_eq!(selected_path_operation_on_device(&mut table,admin,6,
     "/shared/note","",0,&mut[],0,&mut disk),Ok(0));
  }
 }

 #[test]fn desktop_retired_volume_revalidation_requires_complete_retirement(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let admin=table.bind_process(1).unwrap();let guest=table.bind_process(2).unwrap();
  let fd=operation_on_selected_volume(&mut table,1,guest,0,READ,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let before=(disk.reads,disk.writes);
  assert_eq!(revalidate_retired_selected_volume(&table,&mut disk,receipt),
    Err(FsError::InvalidName));
  assert_eq!((disk.reads,disk.writes),before);
  assert!(table.retire_process(admin));assert!(table.retire_process(guest));
  let snapshot=table.staged_copy_kernel();
  let writes=disk.writes;
  assert_eq!(revalidate_retired_selected_volume(&table,&mut disk,receipt),Ok(()));
  assert!(disk.reads>before.0);assert_eq!(disk.writes,writes);
  assert_eq!(table.next,snapshot.next);assert_eq!(table.next_generation,snapshot.next_generation);
  assert_eq!(table.volume_revision,snapshot.volume_revision);
  assert_eq!(table.security_statistics(),snapshot.security_statistics());
  let fresh=table.bind_process(2).unwrap();
  assert_eq!(operation_on_selected_volume(&mut table,2,fresh,fd,0,"",
    &mut[0;9],&mut disk,receipt),Err(FsError::InvalidName));
 }

 #[test]fn desktop_retired_volume_denials_precede_capacity_inspection(){
  struct NeverIo;
  impl BlockDevice for NeverIo{
   fn sector_count(&self)->u64{panic!("rejected recovery inspected capacity")}
   fn read_sector(&mut self,_:u64,_:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{panic!("read")}
   fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])->Result<(),BlockError>{panic!("write")}
  }
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  assert_eq!(revalidate_retired_selected_volume(&table,&mut NeverIo,receipt),
    Err(FsError::InvalidName));
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let owner=table.bind_process(1).unwrap();
  assert_eq!(revalidate_retired_selected_volume(&table,&mut NeverIo,receipt),
    Err(FsError::InvalidName));
  assert!(table.retire_process(owner));
  table.selected_quarantined=true;
  assert_eq!(revalidate_retired_selected_volume(&table,&mut NeverIo,receipt),
    Err(FsError::InvalidName));
  table.selected_quarantined=false;
  let mut other=receipt;other.device_sectors+=1;
  assert_eq!(revalidate_retired_selected_volume(&table,&mut NeverIo,other),
    Err(FsError::InvalidName));
 }

 #[test]fn desktop_retired_volume_revalidation_rejects_receipt_quarantine_and_corruption(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let before=(disk.reads,disk.writes);
  let mut other=receipt;other.device_sectors+=1;
  assert_eq!(revalidate_retired_selected_volume(&table,&mut disk,other),
    Err(FsError::InvalidName));
  table.selected_quarantined=true;
  assert_eq!(revalidate_retired_selected_volume(&table,&mut disk,receipt),
    Err(FsError::InvalidName));
  assert_eq!((disk.reads,disk.writes),before);
  table.selected_quarantined=false;
  disk.sectors[0][440]^=1;
  assert_eq!(revalidate_retired_selected_volume(&table,&mut disk,receipt),
    Err(FsError::InvalidVolume));
  assert_eq!(disk.writes,before.1);
 }

 #[test]fn desktop_selected_paths_reject_stale_owners_escape_and_guest_writes_before_io(){
  struct NeverIo;
  impl BlockDevice for NeverIo{
   fn sector_count(&self)->u64{panic!("denied request queried device")}
   fn read_sector(&mut self,_:u64,_:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{panic!("read")}
   fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])->Result<(),BlockError>{panic!("write")}
  }
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let admin=table.bind_process(1).unwrap();let stale=table.bind_process(2).unwrap();
  let guest=table.bind_process(2).unwrap();
  for (owner,action,path,target) in [
   (stale,4,"/shared/note",""),(guest,5,"/shared/note",""),
   (guest,8,"/",""),(guest,4,"/sharedness/note",""),
   (admin,5,"/system/kernel",""),(admin,7,"/shared/note","/keys/stolen"),
   (admin,4,"/shared/../system/kernel","")]{
    assert_eq!(selected_path_operation_on_device(&mut table,owner,action,path,
      target,0,&mut[0;32],0,&mut NeverIo),Err(FsError::InvalidName));
  }
  assert_eq!(selected_sync_on_device(&mut table,guest,&mut NeverIo),Err(FsError::InvalidName));
 }

 #[test]fn desktop_selected_path_budget_survives_calls_and_rejects_before_mount(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let admin=table.bind_process(1).unwrap();
  assert!(table.narrow_service_write_budget(admin,1,1));
  assert_eq!(selected_path_operation_on_device(&mut table,admin,5,"/shared/note",
    "",0,&mut b"x".to_vec(),1,&mut disk),Ok(0));
  let reads=disk.reads;let writes=disk.writes;
  assert_eq!(selected_path_operation_on_device(&mut table,admin,5,"/shared/note",
    "",0,&mut b"y".to_vec(),1,&mut disk),Err(FsError::InvalidName));
  assert_eq!((disk.reads,disk.writes),(reads,writes));
  assert_eq!(table.security_statistics().completed_writes,1);
 }

 #[test]fn desktop_selected_path_changed_partition_fails_without_output_or_writes(){
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let guest=table.bind_process(2).unwrap();let writes=disk.writes;
  disk.sectors[0][440]^=1;
  let mut output=[0x55;16];
  assert_eq!(selected_path_operation_on_device(&mut table,guest,4,"/shared/note",
    "",0,&mut output,0,&mut disk),Err(FsError::InvalidVolume));
  assert_eq!(output,[0x55;16]);assert_eq!(disk.writes,writes);
  assert!(table.selected_quarantined);
 }

 #[test]fn desktop_selected_sync_requires_real_barrier_and_quarantines_failure(){
  struct SyncDisk<'a>{disk:&'a mut Disk,flushes:usize,fail:bool}
  impl BlockDevice for SyncDisk<'_>{
   fn sector_count(&self)->u64{COUNT as u64}
   fn read_sector(&mut self,l:u64,b:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{self.disk.read_sector(l,b)}
   fn write_sector(&mut self,l:u64,b:&[u8;SECTOR_BYTES])->Result<(),BlockError>{self.disk.write_sector(l,b)}
   fn sync(&mut self)->Result<(),BlockError>{self.flushes+=1;if self.fail{Err(BlockError::Io)}else{Ok(())}}
  }
  let (mut disk,receipt)=disk();let mut table=FileHandles::new();
  prepare_selected_volume_kernel(&mut table,&mut disk,receipt).unwrap();
  let admin=table.bind_process(1).unwrap();
  let fd=operation_on_selected_volume(&mut table,1,admin,0,READ,
    "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let mut durable=SyncDisk{disk:&mut disk,flushes:0,fail:false};
  selected_sync_on_device(&mut table,admin,&mut durable).unwrap();
  assert_eq!(durable.flushes,1);durable.fail=true;
  assert_eq!(selected_sync_on_device(&mut table,admin,&mut durable),Err(FsError::Io));
  assert_eq!(durable.flushes,2);assert!(table.selected_quarantined);
  assert!(table.index(admin,fd).is_err());
  assert_eq!(selected_sync_on_device(&mut table,admin,&mut durable),Err(FsError::InvalidVolume));
  assert_eq!(durable.flushes,2);
 }
 

 #[test]fn r122_selected_token_cannot_probe_disk_after_namespace_root_drift(){
  let (mut disk,receipt)=disk();
  let mut handles=FileHandles::new();
  let owner=handles.bind_process(1).unwrap();
  let token=operation_on_selected_volume(&mut handles,1,owner,0,READ,
      "/shared/note",&mut [],&mut disk,receipt).unwrap();
  let old=handles.selected_operation_guard(owner).unwrap();
  assert!(handles.owns(owner));
  // Simulate a faulty concurrent root publication that missed the ordinary
  // narrow_root_kernel handle revocation. The independent selected-volume
  // preflight must still reject before any ATA sector read or write.
  let mut new_root=RootGrant::EMPTY;
  new_root.epoch=owner.generation;
  new_root.path[..8].copy_from_slice(b"/private");
  new_root.len=8;
  handles.roots[0]=new_root;
  handles.root_revisions[0]+=1;
  assert!(!handles.selected_operation_guard_is_current(old));
  let before=(disk.reads,disk.writes);
  let mut output=[0xa5u8;32];
  assert_eq!(operation_on_selected_volume(
      &mut handles,2,owner,token,0,"",&mut output,&mut disk,receipt),
      Err(FsError::InvalidName));
  assert_eq!((disk.reads,disk.writes),before);
  assert_eq!(output,[0xa5;32]);
  assert!(handles.owns(owner));
 }

 #[test]fn r122_selected_volume_mutation_fuzz_denies_stale_alias_before_disk(){
  // R122-031 source-level negative mutation matrix using the ACTUAL
  // selected-volume API, not a policy-only mock. Each round opens a real
  // disk-backed file and invalidates the namespace through two distinct
  // supported kernel paths. Neither old handle nor old mount alias may
  // touch the selected block device after revocation.
  let (mut disk,receipt)=disk();
  let mut files=FileHandles::new();
  let admin=files.bind_process(1).unwrap();
  let mut guest=files.bind_process(2).unwrap();
  let mut stale=[0u64;16];
  for iteration in 0..128usize {
    let token=operation_on_selected_volume(&mut files,1,guest,0,READ,
        "/shared/note",&mut [],&mut disk,receipt).unwrap();
    let guard=files.selected_operation_guard(guest).unwrap();
    let alias=files.mount_alias_for_current_root_kernel(guest).unwrap();
    // Use the fixed bounded bank size instead of borrowing the array
    // immutably while it is mutably indexed.
    stale[iteration%16]=token;
    if iteration%2==0 {
      // Narrows/reissues the root revision even with the same spelling;
      // descriptor revocation must rely on the receipt, not the string.
      files.narrow_root_kernel(guest,"/shared").unwrap();
    }else{
      // Recycle the exact PID, changing its trusted generation.
      guest=files.bind_process(2).unwrap();
    }
    assert!(!files.selected_operation_guard_is_current(guard));
    assert!(!files.mount_alias_allows(alias,guest,"/shared/note"));
    let media_before=(disk.reads,disk.writes);
    for &old in &stale{
      if old==0{continue;}
      let mut output=[0xa5u8;24];
      for action in [2u64,3,4,5,6,26,27,28,30,31]{
        assert_eq!(operation_on_selected_volume(
            &mut files,action,guest,old,0,"",&mut output,&mut disk,receipt),
            Err(FsError::InvalidName));
        assert_eq!(output,[0xa5;24]);
      }
    }
    assert_eq!(operation_on_selected_volume_alias(&mut files,alias,1,
        guest,0,READ,"/shared/note",&mut [],&mut disk,receipt),
        Err(FsError::InvalidName));
    assert_eq!((disk.reads,disk.writes),media_before);
    assert!(files.owns(admin)&&files.owns(guest));
  }
 }

 #[test]fn r118_selected_volume_stale_handle_fuzz_across_128_pid_recycles(){
  // The *real* selected-volume API must deny stale tokens before any ATA
  // read, including when a recycled PID happens to reuse the same path.
  let (mut media,receipt)=disk();
  let mut files=FileHandles::new();
  let admin=files.bind_process(1).unwrap();
  let mut guest=files.bind_process(2).unwrap();
  for _ in 0..128{
   let token=operation_on_selected_volume(&mut files,1,guest,0,READ,
       "/shared/note",&mut [],&mut media,receipt).unwrap();
   let guard=files.selected_operation_guard(guest).unwrap();
   let alias=files.mount_alias_for_current_root_kernel(guest).unwrap();
   let old=guest;
   guest=files.bind_process(2).unwrap();
   assert_ne!(old.generation,guest.generation);
   assert!(!files.selected_operation_guard_is_current(guard));
   let before=media.reads;
   for caller in [old,guest]{
    for action in [2u64,5u64]{
     assert_eq!(operation_on_selected_volume(&mut files,action,caller,token,0,
       "",&mut [0u8;16],&mut media,receipt),Err(FsError::InvalidName));
    }
    assert!(!files.mount_alias_allows(alias,caller,"/shared/note"));
   }
   assert_eq!(operation_on_selected_volume_alias(&mut files,alias,1,guest,
      0,READ,"/shared/note",&mut [],&mut media,receipt),
      Err(FsError::InvalidName));
   assert_eq!(media.reads,before);
   assert!(files.owns(admin));
  }
 }

 #[test]fn r118_selected_mount_revision_swap_rejects_old_alias_before_disk_probe(){
  let (mut media,receipt)=disk();
  let mut files=FileHandles::new();
  let admin=files.bind_process(1).unwrap();
  let token=operation_on_selected_volume(&mut files,1,admin,0,READ,
      "/shared/note",&mut [],&mut media,receipt).unwrap();
  let alias=files.mount_alias_for_current_root_kernel(admin).unwrap();
  let guard=files.selected_operation_guard(admin).unwrap();
  assert!(files.mount_alias_allows(alias,admin,"/shared/note"));
  // Model a serialized trusted remount revision transition. No claim of
  // physical multicore remount contention is made by this host test.
  files.volume_revision=files.volume_revision.checked_add(1).unwrap();
  let before=(media.reads,media.writes);
  assert!(!files.selected_operation_guard_is_current(guard));
  assert!(!files.mount_alias_allows(alias,admin,"/shared/note"));
  assert_eq!(operation_on_selected_volume_alias(&mut files,alias,1,admin,
       0,READ,"/shared/note",&mut [],&mut media,receipt),
       Err(FsError::InvalidName));
  assert_eq!((media.reads,media.writes),before);
  // An already-open token remains owned by the same PID generation; its
  // lifetime is governed by the separately authenticated descriptor path.
  assert!(token!=0 && files.owns(admin));
 }

 #[test]fn r118_capacity_swap_rejected_before_any_selected_disk_reads(){
  struct Changed<'a>{inner:&'a mut Disk,reads:usize,writes:usize}
  impl BlockDevice for Changed<'_>{
   fn sector_count(&self)->u64{(COUNT as u64)+1}
   fn read_sector(&mut self,l:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
    self.reads+=1;self.inner.read_sector(l,out)
   }
   fn write_sector(&mut self,l:u64,input:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
    self.writes+=1;self.inner.write_sector(l,input)
   }
  }
  let (mut disk,receipt)=disk();
  let mut table=FileHandles::new();
  let owner=table.bind_process(1).unwrap();
  let mut changed=Changed{inner:&mut disk,reads:0,writes:0};
  assert_eq!(operation_on_selected_volume(&mut table,1,owner,0,READ,
    "/shared/note",&mut[],&mut changed,receipt),Err(FsError::InvalidVolume));
  assert_eq!((changed.reads,changed.writes),(0,0));
  assert!(table.owns(owner));
  assert_eq!(table.last_syscall_namespace(),None);
 }


 #[test]fn r118_initial_mount_revision_overflow_rejects_before_sector_io(){
   let (mut disk,receipt)=disk();
   let mut table=FileHandles::new();
   let owner=table.bind_process(1).unwrap();
   table.volume_revision=u64::MAX;
   let reads=disk.reads;let writes=disk.writes;
   assert_eq!(operation_on_selected_volume(&mut table,1,owner,0,READ,
       "/shared/note",&mut[],&mut disk,receipt),Err(FsError::Full));
   assert_eq!((disk.reads,disk.writes),(reads,writes));
   assert_eq!(table.selected_volume,None);
   assert_eq!(table.last_syscall_namespace(),None);
   table.volume_revision=1;
   let token=operation_on_selected_volume(&mut table,1,owner,0,READ,
       "/shared/note",&mut[],&mut disk,receipt).unwrap();
   assert!(token!=0);
   assert_eq!(table.volume_revision,2);
 }
 #[test]fn r118_media_probe_failure_does_not_publish_namespace_authority(){
   struct Failing<'a>{inner:&'a mut Disk,reads:usize}
   impl BlockDevice for Failing<'_>{
      fn sector_count(&self)->u64{COUNT as u64}
      fn read_sector(&mut self,_:u64,_:&mut[u8;SECTOR_BYTES])
         ->Result<(),BlockError>{self.reads+=1;Err(BlockError::Io)}
      fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])
         ->Result<(),BlockError>{Err(BlockError::Io)}
   }
   let (mut media,receipt)=disk();
   let mut handles=FileHandles::new();
   let owner=handles.bind_process(1).unwrap();
   let old=handles.mount_alias_for_current_root_kernel(owner).unwrap();
   let mut failing=Failing{inner:&mut media,reads:0};
   assert_eq!(operation_on_selected_volume(&mut handles,1,owner,0,READ,
       "/shared/note",&mut[],&mut failing,receipt),Err(FsError::InvalidVolume));
   assert!(failing.reads>0);
   assert!(handles.last_syscall_namespace().is_none());
   assert_eq!(handles.selected_volume,None);
   assert!(handles.mount_alias_allows(old,owner,"/shared/note"));
 }

 #[test]fn r118_failed_batch_preserves_file_handles_and_delegated_alias(){
  let (mut disk,receipt)=disk();
  let mut handles=FileHandles::new();
  let admin=handles.bind_process(1).unwrap();
  let guest=handles.bind_process(2).unwrap();
  let token1=operation_on_selected_volume(&mut handles,1,admin,0,READ,
     "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let token2=operation_on_selected_volume(&mut handles,1,guest,0,READ,
     "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let alias=handles.mount_alias_for_current_root_kernel(guest).unwrap();
  assert!(handles.mount_alias_allows(alias,guest,"/shared/note"));
  assert_eq!(handles.retire_process_batch_kernel(&[
     admin,Identity{pid:guest.pid,generation:guest.generation+1}]),
     Err(FsError::InvalidName));
  assert!(handles.owns(admin)&&handles.owns(guest));
  assert!(handles.mount_alias_allows(alias,guest,"/shared/note"));
  let mut data=[0u8;16];
  assert_eq!(operation_on_selected_volume(&mut handles,2,admin,token1,0,
      "",&mut data,&mut disk,receipt),Ok(9));
  assert_eq!(operation_on_selected_volume(&mut handles,2,guest,token2,0,
      "",&mut data,&mut disk,receipt),Ok(9));
  assert_eq!(&data[..9],b"persisted");
 }
 #[test]fn r118_kernel_batch_retirement_drops_handles_and_recycled_pid_aliases(){
  let (mut disk,receipt)=disk();
  let mut handles=FileHandles::new();
  let admin=handles.bind_process(1).unwrap();
  let guest=handles.bind_process(2).unwrap();
  let fd=operation_on_selected_volume(&mut handles,1,guest,0,READ,
      "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let alias=handles.mount_alias_for_current_root_kernel(guest).unwrap();
  assert_eq!(handles.retire_process_batch_kernel(&[admin,guest]),Ok(2));
  assert!(!handles.mount_alias_allows(alias,guest,"/shared/note"));
  let replacement=handles.bind_process(2).unwrap();
  let before=disk.reads;
  assert_eq!(operation_on_selected_volume(&mut handles,2,replacement,fd,0,
      "",&mut[0u8;16],&mut disk,receipt),Err(FsError::InvalidName));
  assert_eq!(operation_on_selected_volume(&mut handles,2,guest,fd,0,
      "",&mut[0u8;16],&mut disk,receipt),Err(FsError::InvalidName));
  assert_eq!(disk.reads,before);
  assert!(!handles.mount_alias_allows(alias,replacement,"/shared/note"));
  let fresh=operation_on_selected_volume(&mut handles,1,replacement,0,READ,
      "/shared/note",&mut[],&mut disk,receipt).unwrap();
  assert_ne!(fresh,fd);
 }

 #[test]fn r119_failed_write_mount_verification_quarantines_selected_session(){
  struct Broken<'a>(&'a mut Disk);
  impl BlockDevice for Broken<'_>{
   fn sector_count(&self)->u64{COUNT as u64}
   fn read_sector(&mut self,_:u64,_:&mut[u8;SECTOR_BYTES])
     ->Result<(),BlockError>{Err(BlockError::Io)}
   fn write_sector(&mut self,l:u64,b:&[u8;SECTOR_BYTES])
     ->Result<(),BlockError>{self.0.write_sector(l,b)}
  }
  let (mut d,receipt)=disk();let mut t=FileHandles::new();
  let id=t.bind_process(1).unwrap();
  let fd=operation_on_selected_volume(&mut t,1,id,0,READ|WRITE,
      "/shared/note",&mut[],&mut d,receipt).unwrap();
  assert_eq!(operation_on_selected_volume(&mut t,3,id,fd,0,"",
      &mut b"x".to_vec(),&mut Broken(&mut d),receipt),Err(FsError::InvalidVolume));
  assert!(t.selected_quarantined);
  assert_eq!(operation_on_selected_volume(&mut t,2,id,fd,0,"",
      &mut[0u8;8],&mut d,receipt),Err(FsError::InvalidName));
  revalidate_quarantined_selected_volume(&mut t,id,&mut d,receipt).unwrap();
  assert_eq!(operation_on_selected_volume(&mut t,2,id,fd,0,"",
      &mut[0u8;8],&mut d,receipt),Err(FsError::InvalidName));
 }
 #[test]fn r119_full_revision_recovery_has_zero_disk_io(){
  struct ReadCounter<'a>{disk:&'a mut Disk,reads:usize}
  impl BlockDevice for ReadCounter<'_>{
   fn sector_count(&self)->u64{COUNT as u64}
   fn read_sector(&mut self,l:u64,out:&mut[u8;SECTOR_BYTES])
      ->Result<(),BlockError>{self.reads+=1;self.disk.read_sector(l,out)}
   fn write_sector(&mut self,l:u64,b:&[u8;SECTOR_BYTES])
      ->Result<(),BlockError>{self.disk.write_sector(l,b)}
  }
  let (mut d,receipt)=disk();let mut t=FileHandles::new();
  let id=t.bind_process(1).unwrap();
  operation_on_selected_volume(&mut t,1,id,0,READ,
      "/shared/note",&mut[],&mut d,receipt).unwrap();
  t.selected_quarantined=true;t.entries.fill(Handle::EMPTY);
  t.volume_revision=u64::MAX;
  let mut watch=ReadCounter{disk:&mut d,reads:0};
  assert_eq!(revalidate_quarantined_selected_volume(&mut t,id,
      &mut watch,receipt),Err(FsError::Full));
  assert_eq!(watch.reads,0);assert!(t.selected_quarantined);
 }

 #[test]fn r120_partition_receipt_shape_denied_without_any_media_read(){
  let (mut d,original)=disk();
  let mut t=FileHandles::new();let owner=t.bind_process(1).unwrap();
  let mut invalid=original;
  invalid.partition.sectors=u64::MAX;
  let bad=[invalid,
    crate::m1_partition::PartitionReceipt{
       mbr_sector:[0u8;SECTOR_BYTES],..original},
    crate::m1_partition::PartitionReceipt{
       device_sectors:original.device_sectors-1,..original}
  ];
  let reads=d.reads;let writes=d.writes;
  for receipt in bad {
    assert_eq!(operation_on_selected_volume(&mut t,1,owner,0,READ,
      "/shared/note",&mut[],&mut d,receipt),Err(FsError::InvalidVolume));
  }
  assert_eq!((d.reads,d.writes),(reads,writes));
  assert!(t.last_syscall_namespace().is_none());
 }
 #[test]fn r120_recovery_capacity_mismatch_rejects_before_sector_io(){
  struct WrongCapacity<'a>{inner:&'a mut Disk,reads:usize}
  impl BlockDevice for WrongCapacity<'_>{
    fn sector_count(&self)->u64{COUNT as u64+1}
    fn read_sector(&mut self,l:u64,out:&mut[u8;SECTOR_BYTES])
       ->Result<(),BlockError>{self.reads+=1;self.inner.read_sector(l,out)}
    fn write_sector(&mut self,l:u64,b:&[u8;SECTOR_BYTES])
       ->Result<(),BlockError>{self.inner.write_sector(l,b)}
  }
  let (mut d,receipt)=disk();
  let mut t=FileHandles::new();let id=t.bind_process(1).unwrap();
  operation_on_selected_volume(&mut t,1,id,0,READ,
    "/shared/note",&mut[],&mut d,receipt).unwrap();
  t.selected_quarantined=true;t.entries.fill(Handle::EMPTY);
  let mut wrong=WrongCapacity{inner:&mut d,reads:0};
  assert_eq!(revalidate_quarantined_selected_volume(&mut t,id,
    &mut wrong,receipt),Err(FsError::InvalidVolume));
  assert_eq!(wrong.reads,0);assert!(t.selected_quarantined);
 }
 fn disk()->(Disk,PartitionReceipt){
  let mut d=Disk{sectors:[[0;SECTOR_BYTES];COUNT],reads:0,writes:0};
  d.sectors[0][510]=0x55;d.sectors[0][511]=0xaa;
  let entry=&mut d.sectors[0][446..462];
  entry[4]=0x83;entry[8..12].copy_from_slice(&10u32.to_le_bytes());
  entry[12..16].copy_from_slice(&100u32.to_le_bytes());
  {
   let win=BlockWindow::new(&mut d,10,100).unwrap();
   let mut fs=HierVfs::format_blank(win).unwrap();
   fs.mkdir("/shared").unwrap();
   fs.create("/shared/note",b"persisted").unwrap();
  }
  let receipt=inspect_partition_receipt(&mut d,0).unwrap();
  (d,receipt)
 }
 #[test]fn r109_pid2_and_protected_namespace_reject_before_device_io(){
  let (mut d,receipt)=disk();let mut table=FileHandles::new();
  let guest=table.bind_process(2).unwrap();
  let admin=table.bind_process(1).unwrap();
  let reads=d.reads;let writes=d.writes;
  for (owner,action,mode,path,data) in [
    (guest,1,READ,"/boot/init",b"".as_slice()),
    (guest,1,WRITE,"/shared/note",b""),
    (guest,29,0,"/shared/new",b"create"),
    (admin,1,WRITE,"/boot/init",b""),
    (admin,29,0,"/system/unsafe",b"create"),
    (admin,1,WRITE,"/shared/../boot/init",b""),
  ]{
    let mut content=data.to_vec();
    assert_eq!(operation_on_selected_volume(&mut table,action,owner,0,mode,
         path,&mut content,&mut d,receipt),Err(FsError::InvalidName));
  }
  assert_eq!(d.reads,reads);assert_eq!(d.writes,writes);
  assert_eq!(table.security_statistics().completed_writes,0);
 }

 #[test]fn r115_selected_mount_rotates_alias_after_real_verified_volume_open(){
  let (mut media,receipt)=disk();
  let mut handles=FileHandles::new();
  let guest=handles.bind_process(2).unwrap();
  let old=handles.mount_alias_for_current_root_kernel(guest).unwrap();
  assert!(handles.mount_alias_allows(old,guest,"/shared/note"));
  let fd=operation_on_selected_volume(&mut handles,1,guest,0,READ,
      "/shared/note",&mut[],&mut media,receipt).unwrap();
  assert_ne!(fd,0);
  assert!(!handles.mount_alias_allows(old,guest,"/shared/note"));
  assert_eq!(operation_authenticated_mount_alias(&mut handles,old,1,guest,
      0,READ,"/shared/note",&mut[]),Err(FsError::InvalidName));
  let current=handles.mount_alias_for_current_root_kernel(guest).unwrap();
  assert!(handles.mount_alias_allows(current,guest,"/shared/note"));
  assert!(!handles.mount_alias_allows(current,guest,"/sharedness/private"));
 }
 #[test]fn r115_healthy_remount_and_quarantine_revoke_old_mount_alias(){
  let (mut media,receipt)=disk();
  let mut handles=FileHandles::new();
  let guest=handles.bind_process(2).unwrap();
  let admin=handles.bind_process(1).unwrap();
  let fd=operation_on_selected_volume(&mut handles,1,guest,0,READ,
      "/shared/note",&mut[],&mut media,receipt).unwrap();
  operation_on_selected_volume(&mut handles,5,guest,fd,0,"",
      &mut[],&mut media,receipt).unwrap();
  let old=handles.mount_alias_for_current_root_kernel(guest).unwrap();
  assert!(handles.mount_alias_allows(old,guest,"/shared/note"));
  // Simulate a failed mutation that made the actual selected mount unsafe.
  handles.selected_quarantined=true;
  assert!(!handles.mount_alias_allows(old,guest,"/shared/note"));
  revalidate_quarantined_selected_volume(&mut handles,admin,&mut media,receipt)
      .unwrap();
  assert!(!handles.mount_alias_allows(old,guest,"/shared/note"));
  let fresh=handles.mount_alias_for_current_root_kernel(guest).unwrap();
  assert!(handles.mount_alias_allows(fresh,guest,"/shared/note"));
 }

 #[test]fn r116_selected_alias_open_requires_authenticated_current_volume(){
  let (mut disk,receipt)=disk();
  let mut table=FileHandles::new();
  let guest=table.bind_process(2).unwrap();
  let before=disk.reads;
  let unmounted=table.mount_alias_for_current_root_kernel(guest).unwrap();
  assert_eq!(operation_on_selected_volume_alias(&mut table,unmounted,1,
      guest,0,READ,"/shared/note",&mut[],&mut disk,receipt),
      Err(FsError::InvalidName));
  assert_eq!(disk.reads,before);
  let init=table.bind_process(1).unwrap();
  let first=operation_on_selected_volume(&mut table,1,init,0,READ,
      "/shared/note",&mut[],&mut disk,receipt).unwrap();
  operation_on_selected_volume(&mut table,5,init,first,0,"",
      &mut[],&mut disk,receipt).unwrap();
  let alias=table.mount_alias_for_current_root_kernel(guest).unwrap();
  let fd=operation_on_selected_volume_alias(&mut table,alias,1,
      guest,0,READ,"/shared/note",&mut[],&mut disk,receipt).unwrap();
  assert!(fd>0);
  let bytes=disk.reads;
  for forbidden in ["/sharedness/file","/boot/keys",
                    "/shared/../boot/keys","/shared","relative/file"]{
    assert_eq!(operation_on_selected_volume_alias(&mut table,alias,1,
       guest,0,READ,forbidden,&mut[],&mut disk,receipt),
       Err(FsError::InvalidName));
  }
  assert_eq!(disk.reads,bytes);
 }
 #[test]fn r116_selected_mount_alias_rejects_retired_pid_and_quarantine_without_io(){
  let (mut disk,receipt)=disk();
  let mut table=FileHandles::new();
  let admin=table.bind_process(1).unwrap();
  let fd=operation_on_selected_volume(&mut table,1,admin,0,READ,
      "/shared/note",&mut[],&mut disk,receipt).unwrap();
  operation_on_selected_volume(&mut table,5,admin,fd,0,"",
      &mut[],&mut disk,receipt).unwrap();
  let guest=table.bind_process(2).unwrap();
  let alias=table.mount_alias_for_current_root_kernel(guest).unwrap();
  let newer=table.bind_process(2).unwrap();
  let reads=disk.reads;
  assert_eq!(operation_on_selected_volume_alias(&mut table,alias,1,
      newer,0,READ,"/shared/note",&mut[],&mut disk,receipt),
      Err(FsError::InvalidName));
  assert_eq!(operation_on_selected_volume_alias(&mut table,alias,1,
      guest,0,READ,"/shared/note",&mut[],&mut disk,receipt),
      Err(FsError::InvalidName));
  assert_eq!(disk.reads,reads);
  let fresh=table.mount_alias_for_current_root_kernel(newer).unwrap();
  table.selected_quarantined=true;
  assert_eq!(operation_on_selected_volume_alias(&mut table,fresh,1,
      newer,0,READ,"/shared/note",&mut[],&mut disk,receipt),
      Err(FsError::InvalidName));
  assert_eq!(disk.reads,reads);
 }
 #[test]fn r116_selected_mount_alias_denies_cross_identity_and_mutating_guest(){
  let (mut disk,receipt)=disk();
  let mut table=FileHandles::new();
  let admin=table.bind_process(1).unwrap();
  let fd=operation_on_selected_volume(&mut table,1,admin,0,READ,
      "/shared/note",&mut[],&mut disk,receipt).unwrap();
  operation_on_selected_volume(&mut table,5,admin,fd,0,"",
      &mut[],&mut disk,receipt).unwrap();
  let guest=table.bind_process(2).unwrap();
  let alias=table.mount_alias_for_current_root_kernel(guest).unwrap();
  let reads=disk.reads;let writes=disk.writes;
  assert_eq!(operation_on_selected_volume_alias(&mut table,alias,1,
      admin,0,READ,"/shared/note",&mut[],&mut disk,receipt),
      Err(FsError::InvalidName));
  assert_eq!(operation_on_selected_volume_alias(&mut table,alias,29,
      guest,0,0,"/shared/new",&mut b"bad".to_vec(),&mut disk,receipt),
      Err(FsError::InvalidName));
  assert_eq!(operation_on_selected_volume_alias(&mut table,alias,1,
      guest,0,WRITE,"/shared/note",&mut[],&mut disk,receipt),
      Err(FsError::InvalidName));
  assert_eq!((disk.reads,disk.writes),(reads,writes));
 }
 #[test]fn r109_foreign_stale_and_unknown_tokens_never_probe_selected_media(){
  let (mut d,receipt)=disk();let mut table=FileHandles::new();
  let owner=table.bind_process(2).unwrap();
  let fd=operation_on_selected_volume(&mut table,1,owner,0,READ,
      "/shared/note",&mut[],&mut d,receipt).unwrap();
  let another=table.bind_process(1).unwrap();
  let reads=d.reads;
  let mut untouched=[0x55;16];
  assert_eq!(operation_on_selected_volume(&mut table,2,another,fd,0,"",
      &mut untouched,&mut d,receipt),Err(FsError::InvalidName));
  assert_eq!(operation_on_selected_volume(&mut table,3,owner,fd,0,"",
      &mut b"X".to_vec(),&mut d,receipt),Err(FsError::InvalidName));
  assert_eq!(operation_on_selected_volume(&mut table,2,owner,fd+900,0,"",
      &mut untouched,&mut d,receipt),Err(FsError::InvalidName));
  assert_eq!(d.reads,reads);assert_eq!(untouched,[0x55;16]);
  let replacement=table.bind_process(2).unwrap();
  assert!(!table.owns(owner));assert!(table.owns(replacement));
  assert_eq!(operation_on_selected_volume(&mut table,2,owner,fd,0,"",
      &mut untouched,&mut d,receipt),Err(FsError::InvalidName));
  assert_eq!(d.reads,reads);
 }
 #[test]fn r109_rebind_new_owner_can_open_only_after_epoch_rotation(){
  let (mut d,receipt)=disk();let mut table=FileHandles::new();
  let old=table.bind_process(2).unwrap();
  let first=operation_on_selected_volume(&mut table,1,old,0,READ,
       "/shared/note",&mut[],&mut d,receipt).unwrap();
  let replacement=table.bind_process(2).unwrap();
  let before=d.reads;
  assert_eq!(operation_on_selected_volume(&mut table,2,old,first,0,"",
       &mut[0;9],&mut d,receipt),Err(FsError::InvalidName));
  assert_eq!(d.reads,before);
  let second=operation_on_selected_volume(&mut table,1,replacement,0,READ,
       "/shared/note",&mut[],&mut d,receipt).unwrap();
  assert_ne!(first,second);
  let mut out=[0;16];
  assert_eq!(operation_on_selected_volume(&mut table,2,replacement,second,0,"",
       &mut out,&mut d,receipt),Ok(9));
  assert_eq!(&out[..9],b"persisted");
 }
 #[test]fn r110_authenticated_selected_unlink_survives_reboot_and_retires_token(){
  let (mut disk,receipt)=disk();
  let mut handles=FileHandles::new();
  let owner=handles.bind_process(1).unwrap();
  let token=operation_on_selected_volume(&mut handles,1,owner,0,WRITE,
      "/shared/note",&mut[],&mut disk,receipt).unwrap();
  assert_eq!(operation_on_selected_volume(&mut handles,30,owner,token,0,"",
       &mut[],&mut disk,receipt),Ok(1));
  let writes=disk.writes;
  assert_eq!(operation_on_selected_volume(&mut handles,30,owner,token,0,"",
       &mut[],&mut disk,receipt),Err(FsError::InvalidName));
  assert_eq!(disk.writes,writes);
  let mut next_boot=FileHandles::new();
  let fresh=next_boot.bind_process(2).unwrap();
  assert_eq!(operation_on_selected_volume(&mut next_boot,1,fresh,0,READ,
       "/shared/note",&mut[],&mut disk,receipt),Err(FsError::NotFound));
  assert_eq!(handles.security_statistics().completed_writes,1);
 }
 #[test]fn r110_selected_volume_failed_read_never_publishes_partial_output(){
  struct FailDisk{inner:Disk,fault:Option<u64>}
  impl BlockDevice for FailDisk{
    fn sector_count(&self)->u64{self.inner.sector_count()}
    fn read_sector(&mut self,l:u64,out:&mut[u8;SECTOR_BYTES])
      ->Result<(),BlockError>{
      if self.fault==Some(l){
        out.fill(0x3c); // simulate driver dirtied destination
        return Err(BlockError::Io);
      }
      self.inner.read_sector(l,out)
    }
    fn write_sector(&mut self,l:u64,data:&[u8;SECTOR_BYTES])
      ->Result<(),BlockError>{self.inner.write_sector(l,data)}
  }
  let (raw,receipt)=disk();let mut d=FailDisk{inner:raw,fault:None};
  let mut handles=FileHandles::new();let owner=handles.bind_process(2).unwrap();
  let token=operation_on_selected_volume(&mut handles,1,owner,0,READ,
       "/shared/note",&mut[],&mut d,receipt).unwrap();
  let old_writes=d.inner.writes;
  d.fault=Some(10); // selected COW volume metadata head
  let mut out=[0xa8;16];
  assert_eq!(operation_on_selected_volume(&mut handles,2,owner,token,0,"",
       &mut out,&mut d,receipt),Err(FsError::InvalidVolume));
  assert_eq!(out,[0xa8;16]);assert_eq!(d.inner.writes,old_writes);
  d.fault=None;
  assert_eq!(operation_on_selected_volume(&mut handles,2,owner,token,0,"",
       &mut out,&mut d,receipt),Ok(9));
  assert_eq!(&out[..9],b"persisted");
 }
 #[test]fn r112_torn_selected_write_quarantines_all_tokens_until_verified_remount(){
  struct TornDisk{inner:Disk,fail_next_write:bool}
  impl BlockDevice for TornDisk{
   fn sector_count(&self)->u64{self.inner.sector_count()}
   fn read_sector(&mut self,l:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
    self.inner.read_sector(l,out)
   }
   fn write_sector(&mut self,l:u64,buf:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
    if self.fail_next_write{
     self.fail_next_write=false;
     // Torn write is a real media mutation despite the I/O error.
     let dst=self.inner.sectors.get_mut(l as usize).ok_or(BlockError::OutOfRange)?;
     dst[..13].copy_from_slice(&buf[..13]);
     self.inner.writes+=1;
     return Err(BlockError::Io);
    }
    self.inner.write_sector(l,buf)
   }
  }
  let (d,receipt)=disk();let mut disk=TornDisk{inner:d,fail_next_write:false};
  let mut handles=FileHandles::new();
  let admin=handles.bind_process(1).unwrap();
  let guest=handles.bind_process(2).unwrap();
  let writer=operation_on_selected_volume(&mut handles,1,admin,0,WRITE,
      "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let reader=operation_on_selected_volume(&mut handles,1,guest,0,READ,
      "/shared/note",&mut[],&mut disk,receipt).unwrap();
  disk.fail_next_write=true;
  let budget=handles.security_statistics().completed_writes;
  // Admission must protect the foreign reader before fault injection reaches
  // the device. Close its lease, then retain a same-owner token to prove that
  // the admitted torn write still quarantines every remaining descriptor.
  let before=(disk.inner.reads,disk.inner.writes);
  assert_eq!(operation_on_selected_volume(&mut handles,3,admin,writer,0,
       "",&mut b"new data".to_vec(),&mut disk,receipt),Err(FsError::InvalidName));
  assert_eq!((disk.inner.reads,disk.inner.writes),before);
  assert!(disk.fail_next_write);
  operation_on_selected_volume(&mut handles,5,guest,reader,0,"",
       &mut[],&mut disk,receipt).unwrap();
  let peer=operation_on_selected_volume(&mut handles,1,admin,0,READ,
       "/shared/note",&mut[],&mut disk,receipt).unwrap();
  assert_eq!(operation_on_selected_volume(&mut handles,3,admin,writer,0,
       "",&mut b"new data".to_vec(),&mut disk,receipt),Err(FsError::Io));
  assert!(handles.selected_quarantined);
  assert!(handles.entries.iter().all(|handle|handle.token==0));
  assert!(handles.index(admin,peer).is_err());
  assert_eq!(handles.security_statistics().completed_writes,budget);
  let reads=disk.inner.reads;let writes=disk.inner.writes;
  let mut untouched=[0xaa;16];
  assert_eq!(operation_on_selected_volume(&mut handles,2,guest,reader,0,
       "",&mut untouched,&mut disk,receipt),Err(FsError::InvalidName));
  assert_eq!(operation_on_selected_volume(&mut handles,1,guest,0,READ,
       "/shared/note",&mut[],&mut disk,receipt),Err(FsError::InvalidVolume));
  assert_eq!(untouched,[0xaa;16]);
  assert_eq!(disk.inner.reads,reads);assert_eq!(disk.inner.writes,writes);
  assert_eq!(revalidate_quarantined_selected_volume(
       &mut handles,guest,&mut disk,receipt),Err(FsError::InvalidName));
  assert!(handles.selected_quarantined);
  assert_eq!(revalidate_quarantined_selected_volume(
       &mut handles,admin,&mut disk,receipt),Ok(()));
  assert!(!handles.selected_quarantined);
  let new_token=operation_on_selected_volume(&mut handles,1,guest,0,READ,
       "/shared/note",&mut[],&mut disk,receipt).unwrap();
  assert_ne!(new_token,reader);
  assert_eq!(operation_on_selected_volume(&mut handles,2,guest,new_token,0,
       "",&mut untouched,&mut disk,receipt),Ok(9));
  assert_eq!(&untouched[..9],b"persisted");
 }
 #[test]fn r112_recovery_refuses_changed_physical_receipt_and_replayed_owner(){
  let (mut disk,receipt)=disk();
  let mut handles=FileHandles::new();let owner=handles.bind_process(1).unwrap();
  handles.selected_volume=Some(receipt);
  handles.selected_quarantined=true;
  let replacement=handles.bind_process(1).unwrap();
  assert_eq!(revalidate_quarantined_selected_volume(
        &mut handles,owner,&mut disk,receipt),Err(FsError::InvalidName));
  assert!(handles.selected_quarantined);
  let reads=disk.reads;
  let mut changed=receipt;changed.device_sectors+=1;
  assert_eq!(revalidate_quarantined_selected_volume(
        &mut handles,replacement,&mut disk,changed),Err(FsError::InvalidName));
  assert_eq!(disk.reads,reads);
  disk.sectors[0][440]^=0x50;
  assert_eq!(revalidate_quarantined_selected_volume(
        &mut handles,replacement,&mut disk,receipt),Err(FsError::InvalidVolume));
  assert!(handles.selected_quarantined);
 }
 #[test]fn r113_write_quota_survives_selected_volume_reopen_and_process_epoch(){
  let (mut disk,receipt)=disk();
  let mut handles=FileHandles::new();
  let owner=handles.bind_process(1).unwrap();
  let guest=handles.bind_process(2).unwrap();
  assert!(!handles.narrow_service_write_budget(guest,1,1));
  assert!(handles.narrow_service_write_budget(owner,2,8));
  assert!(!handles.narrow_service_write_budget(owner,3,8));
  for (round,byte) in [b'X',b'Y'].into_iter().enumerate(){
    let admin=if round==0{owner}else{handles.bind_process(1).unwrap()};
    let fd=operation_on_selected_volume(&mut handles,1,admin,0,WRITE,
        "/shared/note",&mut[],&mut disk,receipt).unwrap();
    assert_eq!(operation_on_selected_volume(&mut handles,3,admin,fd,0,"",
        &mut[byte],&mut disk,receipt),Ok(1));
    assert_eq!(operation_on_selected_volume(&mut handles,5,admin,fd,0,"",
        &mut[],&mut disk,receipt),Ok(0));
    if round==0{
      assert_eq!(handles.narrow_service_write_budget(owner,2,8),true);
    }
  }
  let current=handles.bind_process(1).unwrap();
  let fd=operation_on_selected_volume(&mut handles,1,current,0,WRITE,
       "/shared/note",&mut[],&mut disk,receipt).unwrap();
  let before_writes=disk.writes;
  assert_eq!(operation_on_selected_volume(&mut handles,3,current,fd,0,"",
       &mut b"Z".to_vec(),&mut disk,receipt),Err(FsError::InvalidName));
  assert_eq!(disk.writes,before_writes);
  assert_eq!(handles.security_statistics().completed_writes,2);
  assert_eq!(handles.security_statistics().changed_bytes,2);
  assert_eq!(handles.narrow_service_write_budget(owner,8,80),false);
 }
 #[test]fn checked_selection_reads_file_after_fresh_kernel_mount(){
  let (mut d,receipt)=disk();let mut table=FileHandles::new();
  let owner=table.bind_process(2).unwrap();
  let token=operation_on_selected_volume(&mut table,1,owner,0,READ,
             "/shared/note",&mut[],&mut d,receipt).unwrap();
  let mut out=[0u8;16];
  assert_eq!(operation_on_selected_volume(&mut table,2,owner,token,0,
             "",&mut out,&mut d,receipt),Ok(9));
  assert_eq!(&out[..9],b"persisted");
 }
 #[test]fn expired_or_foreign_generation_is_denied_before_disk_reads(){
  let (mut d,receipt)=disk();let mut table=FileHandles::new();
  let owner=table.bind_process(2).unwrap();
  let forged=Identity{pid:owner.pid,generation:owner.generation+1};
  let reads=d.reads;
  assert_eq!(operation_on_selected_volume(&mut table,1,forged,0,READ,
       "/shared/note",&mut[],&mut d,receipt),Err(FsError::InvalidName));
  assert_eq!(d.reads,reads);
  let next=table.bind_process(2).unwrap();
  assert_eq!(operation_on_selected_volume(&mut table,1,owner,0,READ,
       "/shared/note",&mut[],&mut d,receipt),Err(FsError::InvalidName));
  assert!(table.owns(next));assert_eq!(d.reads,reads);
 }
 #[test]fn changed_mbr_identity_rejects_stale_volume_before_file_mutation(){
  let (mut d,receipt)=disk();let mut table=FileHandles::new();
  let owner=table.bind_process(1).unwrap();
  let writes=d.writes;
  d.sectors[0][440]^=0x9a; // valid MBR with changed partition identity
  assert_eq!(operation_on_selected_volume(&mut table,29,owner,0,0,
       "/shared/rejected",&mut b"bad".to_vec(),&mut d,receipt),
       Err(FsError::InvalidVolume));
  assert_eq!(d.writes,writes);
 }
 #[test]fn never_reuse_a_live_descriptor_on_another_partition_identity(){
  let (mut first,receipt1)=disk();
  let (mut second,_)=disk();
  second.sectors[0][440]^=0x7e;
  let receipt2=inspect_partition_receipt(&mut second,0).unwrap();
  assert_ne!(receipt1,receipt2);
  let mut table=FileHandles::new();let owner=table.bind_process(2).unwrap();
  let token=operation_on_selected_volume(&mut table,1,owner,0,READ,
       "/shared/note",&mut[],&mut first,receipt1).unwrap();
  let reads=second.reads;
  let mut out=[0x55;12];
  assert_eq!(operation_on_selected_volume(&mut table,2,owner,token,0,
       "",&mut out,&mut second,receipt2),Err(FsError::InvalidVolume));
  assert_eq!(out,[0x55;12]);
  assert_eq!(second.reads,reads); // reject wrong mount before touching it
  assert_eq!(operation_on_selected_volume(&mut table,1,owner,0,READ,
       "/shared/note",&mut[],&mut second,receipt2),Err(FsError::InvalidVolume));
  assert_eq!(second.reads,reads);
  // Explicitly retire old token on the previously authenticated volume.
  assert_eq!(operation_on_selected_volume(&mut table,5,owner,token,0,
       "",&mut[],&mut first,receipt1),Ok(0));
  let fresh=operation_on_selected_volume(&mut table,1,owner,0,READ,
       "/shared/note",&mut[],&mut second,receipt2).unwrap();
  assert_ne!(fresh,token);
  assert_eq!(operation_on_selected_volume(&mut table,2,owner,fresh,0,
       "",&mut out,&mut second,receipt2),Ok(9));
  assert_eq!(&out[..9],b"persisted");
 }
}


#[cfg(test)]
mod r108_auth_directory_tree_recovery {
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct Disk([[u8;SECTOR_BYTES];100]);
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,l:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.0.get(l as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,l:u64,buf:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.0.get_mut(l as usize).ok_or(BlockError::OutOfRange)?=*buf;Ok(())
  }
 }
 fn fixture()->(HierVfs<Disk>,FileHandles,Identity,Identity,u64){
  let mut fs=HierVfs::format_blank(Disk([[0;SECTOR_BYTES];100])).unwrap();
  fs.mkdir("/shared").unwrap();fs.mkdir("/shared/a").unwrap();
  fs.create("/shared/a/note",b"persist").unwrap();
  let mut handles=FileHandles::new();
  let admin=handles.bind_process(1).unwrap();
  let guest=handles.bind_process(2).unwrap();
  let fd=handles.dispatch_authenticated(1,admin,0,READ|WRITE,
             "/shared/a/note",&mut[],&mut fs).unwrap();
  (fs,handles,admin,guest,fd)
 }
 #[test]fn authorized_directory_tree_rename_is_atomic_and_survives_remount(){
  let (mut fs,mut handles,admin,_,fd)=fixture();
  let original=fs.generation();
  assert_eq!(handles.dispatch_authenticated(32,admin,0,0,"/shared/a",
       &mut b"/shared/b".to_vec(),&mut fs),Ok(1));
  assert_eq!(fs.generation(),original+1);
  assert_eq!(handles.dispatch_authenticated(2,admin,fd,0,"",&mut[0;8],&mut fs),
             Err(FsError::InvalidName));
  let mut remounted=HierVfs::mount(fs.into_device()).unwrap();
  let mut output=[0u8;16];
  assert_eq!(remounted.read("/shared/a/note",&mut output),Err(FsError::NotFound));
  assert_eq!(remounted.read("/shared/b/note",&mut output),Ok(7));
  assert_eq!(&output[..7],b"persist");
  assert_eq!(handles.security_statistics().completed_writes,1);
 }
 #[test]fn protected_target_guest_and_stale_generation_never_change_tree(){
  let (mut fs,mut handles,admin,guest,_)=fixture();let g=fs.generation();
  assert_eq!(handles.dispatch_authenticated(32,guest,0,0,"/shared/a",
        &mut b"/shared/b".to_vec(),&mut fs),Err(FsError::InvalidName));
  assert_eq!(handles.dispatch_authenticated(32,admin,0,0,"/shared/a",
        &mut b"/boot/hidden".to_vec(),&mut fs),Err(FsError::InvalidName));
  assert_eq!(handles.dispatch_authenticated(32,admin,0,0,"/shared/a",
        &mut b"/shared/../boot".to_vec(),&mut fs),Err(FsError::InvalidName));
  assert_eq!(handles.dispatch_authenticated(32,Identity{pid:1,generation:admin.generation+1},
        0,0,"/shared/a",&mut b"/shared/b".to_vec(),&mut fs),Err(FsError::InvalidName));
  assert_eq!(fs.generation(),g);
  let mut b=[0u8;16];
  assert_eq!(fs.read("/shared/a/note",&mut b),Ok(7));
  assert_eq!(&b[..7],b"persist");
  assert_eq!(handles.security_statistics().completed_writes,0);
 }
}


// R114 follow-up: this is the real authenticated FileHandles policy path,
// not a disconnected path utility or a replacement ACL.
#[cfg(test)]
mod r114_live_files_policy_tests {
    use super::*;
    #[test]
    fn pid2_is_confined_to_canonical_shared_tree() {
        for name in ["/shared/note", "/shared/sub/file"] {
            assert!(FileHandles::policy(2,name,READ));
        }
        for name in ["/shared", "/sharedness/note", "/boot/note",
                     "/shared/../boot", "/../shared/note", "/shared/note\0evil"] {
            assert!(!FileHandles::policy(2,name,READ),"{name}");
        }
        assert!(!FileHandles::policy(2,"/shared/note",WRITE));
        assert!(!FileHandles::policy(3,"/shared/note",READ));
        assert!(FileHandles::policy(1,"/etc/note",READ|WRITE));
    }
}


#[cfg(test)]
mod r114_root_grant_epoch_tests {
 use super::*;
 #[test]
 fn roots_are_minted_per_incarnation_and_only_narrow(){
  let mut t=FileHandles::new();
  let parent=t.bind_process(1).unwrap();
  let files=t.bind_process(2).unwrap();
  assert!(t.root_allows(parent,"/etc/config"));
  assert!(t.root_allows(files,"/shared/docs/file"));
  assert!(!t.root_allows(files,"/sharedelse/file"));
  assert!(!t.root_allows(files,"/boot/file"));
  assert_eq!(t.narrow_root_kernel(files,"/shared/docs"),Ok(()));
  assert!(!t.root_allows(files,"/shared/file"));
  assert!(t.root_allows(files,"/shared/docs/file"));
  assert_eq!(t.narrow_root_kernel(files,"/shared"),Err(FsError::InvalidName));
  assert_eq!(t.narrow_root_kernel(files,"/shared/../boot"),Err(FsError::InvalidName));
  assert_eq!(t.narrow_root_kernel(files,"/shared/docs/../"),Err(FsError::InvalidName));
  assert_eq!(t.narrow_root_kernel(parent,"/etc"),Ok(()));
  assert!(!t.root_allows(parent,"/boot/file"));
  assert!(t.root_allows(parent,"/etc/config"));
 }
 #[test]
 fn recycled_pid_cannot_reuse_root_or_narrow_replacement(){
  let mut t=FileHandles::new();
  let old=t.bind_process(2).unwrap();
  t.narrow_root_kernel(old,"/shared/private").unwrap();
  let current=t.bind_process(2).unwrap();
  assert!(!t.root_allows(old,"/shared/private/a"));
  assert_eq!(t.narrow_root_kernel(old,"/shared/private"),
      Err(FsError::InvalidName));
  assert!(t.root_allows(current,"/shared/other"));
  assert!(!t.retire_process(old));
  assert!(t.root_allows(current,"/shared/other"));
  assert!(t.retire_process(current));
  assert!(!t.root_allows(current,"/shared/a"));
 }
 #[test]
 fn failed_narrow_does_not_change_authorized_root(){
  let mut t=FileHandles::new();
  let who=t.bind_process(2).unwrap();
  for candidate in ["shared","/sharedness","/shared/../../system","/a\0b"]{
   assert_eq!(t.narrow_root_kernel(who,candidate),Err(FsError::InvalidName));
   assert!(t.root_allows(who,"/shared/ok"));
  }
 }
}


#[cfg(test)]
mod r114_namespace_receipt_replay_tests{
 use super::*;
 #[test]
 fn root_narrowing_invalidates_old_syscall_receipt(){
  let mut handles=FileHandles::new();
  let owner=handles.bind_process(2).unwrap();
  let before=handles.namespace_receipt(owner).unwrap();
  assert!(handles.namespace_receipt_is_current(before));
  handles.narrow_root_kernel(owner,"/shared/docs").unwrap();
  let after=handles.namespace_receipt(owner).unwrap();
  assert_ne!(before,after);
  assert!(!handles.namespace_receipt_is_current(before));
  assert!(handles.namespace_receipt_is_current(after));
  assert!(handles.root_allows(owner,"/shared/docs/a"));
  assert!(!handles.root_allows(owner,"/shared/sibling"));
 }
 #[test]
 fn pid_reuse_or_foreign_pid_cannot_replay_namespace_token(){
  let mut handles=FileHandles::new();
  let owner=handles.bind_process(1).unwrap();
  let receipt=handles.namespace_receipt(owner).unwrap();
  let other=handles.bind_process(2).unwrap();
  assert!(!handles.namespace_receipt_is_current(NamespaceReceipt{
      owner:other,namespace_id:receipt.namespace_id,revision:receipt.revision}));
  let replacement=handles.bind_process(1).unwrap();
  assert_ne!(replacement,owner);
  assert!(!handles.namespace_receipt_is_current(receipt));
  assert!(handles.namespace_receipt_is_current(
      handles.namespace_receipt(replacement).unwrap()));
 }
 #[test]
 fn failed_root_narrow_does_not_rotate_namespace_generation(){
  let mut handles=FileHandles::new();
  let owner=handles.bind_process(2).unwrap();
  let prior=handles.namespace_receipt(owner).unwrap();
  assert_eq!(handles.narrow_root_kernel(owner,"/etc"),
      Err(FsError::InvalidName));
  assert_eq!(handles.namespace_receipt(owner),Ok(prior));
  assert!(!handles.retire_process(Identity{pid:2,generation:owner.generation+1}));
  assert_eq!(handles.namespace_receipt(owner),Ok(prior));
 }
}

#[cfg(test)]
mod r114_mount_alias_security_tests {
    use super::*;
    #[test]
    fn aliases_reject_sibling_prefix_traversal_and_foreign_identity() {
        let mut handles=FileHandles::new();
        let p2=handles.bind_process(2).unwrap();
        let p1=handles.bind_process(1).unwrap();
        let alias=handles.mount_alias_kernel(p2,"//shared/./docs").unwrap();
        assert!(handles.mount_alias_allows(alias,p2,"/shared/docs/file"));
        for path in ["/shared/docs2/file","/shared/../etc/keys",
                     "/shared/private","shared/docs/file"]{
            assert!(!handles.mount_alias_allows(alias,p2,path));
        }
        assert!(!handles.mount_alias_allows(alias,p1,"/shared/docs/file"));
        assert!(handles.mount_alias_kernel(p2,"/etc").is_err());
        assert!(handles.mount_alias_kernel(p2,"/shared/../etc").is_err());
        // Rejected before any disk access, including a forged PID1 use.
        assert_eq!(operation_authenticated_mount_alias(
           &mut handles,alias,1,p1,0,READ,"/shared/docs/file",&mut []),
           Err(FsError::InvalidName));
    }
    #[test]
    fn root_narrowing_and_pid_reuse_withdraw_alias_before_io() {
        let mut handles=FileHandles::new();
        let first=handles.bind_process(2).unwrap();
        let old=handles.mount_alias_kernel(first,"/shared/docs").unwrap();
        handles.narrow_root_kernel(first,"/shared/docs").unwrap();
        assert!(!handles.mount_alias_allows(old,first,"/shared/docs/file"));
        assert_eq!(operation_authenticated_mount_alias(
            &mut handles,old,1,first,0,READ,"/shared/docs/file",&mut []),
            Err(FsError::InvalidName));
        let fresh=handles.mount_alias_kernel(first,"/shared/docs").unwrap();
        assert!(handles.mount_alias_allows(fresh,first,"/shared/docs/file"));
        let replacement=handles.bind_process(2).unwrap();
        assert!(!handles.mount_alias_allows(fresh,replacement,"/shared/docs/file"));
        assert!(!handles.mount_alias_allows(fresh,first,"/shared/docs/file"));
        assert_eq!(operation_authenticated_mount_alias(
            &mut handles,fresh,1,replacement,0,READ,"/shared/docs/file",&mut []),
            Err(FsError::InvalidName));
    }
}



#[cfg(test)]
mod r115_live_namespace_open_security_tests {
    use super::*;
    #[test]
    fn pid2_root_comes_from_kernel_and_cannot_be_widened() {
        let mut files=FileHandles::new();
        let pid2=files.bind_process(2).unwrap();
        let alias=files.mount_alias_for_current_root_kernel(pid2).unwrap();
        assert!(files.mount_alias_allows(alias,pid2,"/shared/docs"));
        for target in ["/sharedness/keys","/etc/passwd","/shared/../etc",
                       "/private", "shared/docs", "/shared\0/private"]{
            assert_eq!(operation_authenticated_mount_alias(
                &mut files,alias,1,pid2,0,READ,target,&mut []),
                Err(FsError::InvalidName));
        }
        // Unauthorized file opens are denied before the ATA probe.
        assert_eq!(operation_authenticated_mount_alias(
            &mut files,alias,1,pid2,0,READ,"/shared",&mut []),
            Err(FsError::InvalidName));
    }
    #[test]
    fn narrowing_and_process_reuse_revoke_kernel_minted_alias() {
        let mut files=FileHandles::new();
        let first=files.bind_process(2).unwrap();
        let old=files.mount_alias_for_current_root_kernel(first).unwrap();
        files.narrow_root_kernel(first,"/shared/docs").unwrap();
        assert!(!files.mount_alias_allows(old,first,"/shared/docs/note"));
        assert_eq!(operation_authenticated_mount_alias(
            &mut files,old,1,first,0,READ,"/shared/docs/note",&mut []),
            Err(FsError::InvalidName));
        let narrow=files.mount_alias_for_current_root_kernel(first).unwrap();
        assert!(files.mount_alias_allows(narrow,first,"/shared/docs/note"));
        assert!(!files.mount_alias_allows(narrow,first,"/shared2/note"));
        let replacement=files.bind_process(2).unwrap();
        assert!(!files.mount_alias_allows(narrow,replacement,"/shared/docs/note"));
        assert_eq!(operation_authenticated_mount_alias(
            &mut files,narrow,1,replacement,0,READ,"/shared/docs/note",&mut []),
            Err(FsError::InvalidName));
    }
}


/// R116: authenticated alias open/create on a previously selected physical
/// partition. The full disk/partition receipt and namespace mount epoch are
/// checked BEFORE probing the disk. A first mount must be performed by the
/// privileged volume service, never by an untrusted alias.
pub fn operation_on_selected_volume_alias<D:BlockDevice>(
    table:&mut FileHandles,alias:MountAlias,action:u64,owner:Identity,
    token:u64,argument:u64,path:&str,buffer:&mut[u8],
    disk:&mut D,receipt:crate::m1_partition::PartitionReceipt,
)->Result<u64,FsError>{
    if !matches!(action,1|29)||token!=0||
       table.selected_quarantined||
       table.selected_volume!=Some(receipt)||
       alias.selected_volume!=Some(receipt)||
       !table.mount_alias_allows(alias,owner,path){
        return Err(FsError::InvalidName);
    }
    // Independent bounds and component validation; reject an exact root
    // as a file open and never accept a sibling-prefix or traversal bypass.
    let mut relative=[0u8;24];
    let n=crate::vfs_path::copy_relative_under_root(path,alias.root(),&mut relative)
        .map_err(|_|FsError::InvalidName)?
        .ok_or(FsError::InvalidName)?;
    if action==1 && n==0{return Err(FsError::InvalidName);}
    operation_on_selected_volume(table,action,owner,token,argument,path,buffer,
                                 disk,receipt)
}


/// R117: a serialized supervisor may retire several exact process epochs as
/// one operation. Validate every owner, duplicate and capacity first; a bad
/// batch must never evict even one live peer or its file descriptors.
impl FileHandles {
    pub fn retire_process_batch_kernel(
        &mut self, owners:&[Identity],
    )->Result<usize,FsError>{
        if owners.len()>self.owners.len(){return Err(FsError::Full);}
        for (index,owner) in owners.iter().enumerate(){
            if !self.owns(*owner)||
               owners[..index].iter().any(|prior|prior.pid==owner.pid){
                return Err(FsError::InvalidName);
            }
        }
        // Canonical PID order is independent of caller batching order.
        // Today this runs under the single-BSP exclusive supervisor; keep the
        // policy order explicit before adding per-table SMP/IRQ locks.
        for pid in 1..=2u32 {
            if let Some(owner)=owners.iter().find(|owner|owner.pid==pid) {
                if !self.retire_process(*owner) {
                    return Err(FsError::InvalidName);
                }
            }
        }
        Ok(owners.len())
    }
}
#[cfg(test)]
mod r117_atomic_file_teardown_tests {
 use super::*;
 #[test]fn both_live_generations_are_revoked_together(){
  let mut t=FileHandles::new();
  let a=t.bind_process(1).unwrap();let b=t.bind_process(2).unwrap();
  let a_alias=t.mount_alias_for_current_root_kernel(a).unwrap();
  let b_alias=t.mount_alias_for_current_root_kernel(b).unwrap();
  assert_eq!(t.retire_process_batch_kernel(&[a,b]),Ok(2));
  assert!(!t.owns(a));assert!(!t.owns(b));
  assert!(!t.mount_alias_allows(a_alias,a,"/boot/init"));
  assert!(!t.mount_alias_allows(b_alias,b,"/shared/note"));
  let replacement=t.bind_process(2).unwrap();
  assert!(t.owns(replacement));
  assert!(!t.mount_alias_allows(b_alias,replacement,"/shared/note"));
 }
 #[test]fn stale_or_wrong_pid_batch_is_atomic(){
  let mut t=FileHandles::new();
  let a=t.bind_process(1).unwrap();let old=t.bind_process(2).unwrap();
  let current=t.bind_process(2).unwrap();
  let fresh=t.mount_alias_for_current_root_kernel(current).unwrap();
  for denied in [
   &[a,old][..],
   &[a,a][..],
   &[a,Identity{pid:3,generation:1}][..],
   &[a,Identity{pid:2,generation:0}][..],
  ]{
   assert_eq!(t.retire_process_batch_kernel(denied),Err(FsError::InvalidName));
   assert!(t.owns(a));assert!(t.owns(current));
   assert!(t.mount_alias_allows(fresh,current,"/shared/note"));
  }
 }
 #[test]fn oversized_batch_and_empty_batch_never_mutate(){
  let mut t=FileHandles::new();let a=t.bind_process(1).unwrap();
  let b=t.bind_process(2).unwrap();
  assert_eq!(t.retire_process_batch_kernel(&[]),Ok(0));
  assert_eq!(t.retire_process_batch_kernel(&[a,b,a]),Err(FsError::Full));
  assert!(t.owns(a)&&t.owns(b));
  assert_eq!(t.retire_process_batch_kernel(&[a]),Ok(1));
  assert!(!t.owns(a));assert!(t.owns(b));
  assert_eq!(t.retire_process_batch_kernel(&[a]),Err(FsError::InvalidName));
  assert!(t.owns(b));
 }
}


#[cfg(test)]
mod r117_selected_volume_guard_tests {
    use super::*;
    #[test]
    fn initial_guard_rejects_root_narrowing_and_pid_reincarnation(){
        let mut table=FileHandles::new();
        let p2=table.bind_process(2).unwrap();
        let initial=table.selected_operation_guard(p2).unwrap();
        assert!(table.selected_operation_guard_is_current(initial));
        table.narrow_root_kernel(p2,"/shared/docs").unwrap();
        assert!(!table.selected_operation_guard_is_current(initial));
        let newer=table.selected_operation_guard(p2).unwrap();
        let recycled=table.bind_process(2).unwrap();
        assert_ne!(recycled,p2);
        assert!(!table.selected_operation_guard_is_current(newer));
        assert!(table.selected_operation_guard_is_current(
            table.selected_operation_guard(recycled).unwrap()));
    }
    #[test]
    fn remount_revision_and_volume_identity_invalidate_old_guard(){
        let mut table=FileHandles::new();
        let p1=table.bind_process(1).unwrap();
        let previous=table.selected_operation_guard(p1).unwrap();
        table.volume_revision=table.volume_revision.checked_add(1).unwrap();
        assert!(!table.selected_operation_guard_is_current(previous));
        let current=table.selected_operation_guard(p1).unwrap();
        assert!(table.selected_operation_guard_is_current(current));
        table.selected_quarantined=true;
        assert!(!table.selected_operation_guard_is_current(current));
        assert!(matches!(table.selected_operation_guard(p1),
                        Err(FsError::InvalidVolume)));
    }
    #[test]
    fn failed_guard_request_does_not_modify_peer_process(){
        let mut table=FileHandles::new();
        let current=table.bind_process(2).unwrap();
        let p1=table.bind_process(1).unwrap();
        assert!(matches!(table.selected_operation_guard(
            Identity{pid:2,generation:current.generation+1}),
            Err(FsError::InvalidName)));
        assert!(table.owns(current));assert!(table.owns(p1));
        assert!(table.selected_operation_guard_is_current(
            table.selected_operation_guard(current).unwrap()));
    }
}

#[cfg(test)]
mod r119_inherited_mount_authority_regressions{
 use super::*;
 #[test]
 fn mount_revision_replacement_revokes_old_alias_and_operation_guard(){
  let mut t=FileHandles::new();
  let init=t.bind_process(1).unwrap();
  let guest=t.bind_process(2).unwrap();
  let old_guard=t.selected_operation_guard(guest).unwrap();
  let old_alias=t.mount_alias_for_current_root_kernel(guest).unwrap();
  assert!(t.mount_alias_allows(old_alias,guest,"/shared/note"));
  t.volume_revision=t.volume_revision.checked_add(1).unwrap();
  assert!(!t.selected_operation_guard_is_current(old_guard));
  assert!(!t.mount_alias_allows(old_alias,guest,"/shared/note"));
  let new_guard=t.selected_operation_guard(guest).unwrap();
  let new_alias=t.mount_alias_for_current_root_kernel(guest).unwrap();
  assert!(t.selected_operation_guard_is_current(new_guard));
  assert!(t.mount_alias_allows(new_alias,guest,"/shared/note"));
  assert!(t.owns(init));
 }
 #[test]
 fn root_revision_and_pid_incarnation_reject_old_lookup_receipts(){
  let mut t=FileHandles::new();
  let original=t.bind_process(2).unwrap();
  let lookup=t.selected_operation_guard(original).unwrap();
  let alias=t.mount_alias_for_current_root_kernel(original).unwrap();
  t.narrow_root_kernel(original,"/shared/docs").unwrap();
  assert!(!t.selected_operation_guard_is_current(lookup));
  assert!(!t.mount_alias_allows(alias,original,"/shared/docs/a"));
  let after=t.selected_operation_guard(original).unwrap();
  let replacement=t.bind_process(2).unwrap();
  assert!(!t.selected_operation_guard_is_current(after));
  assert!(!t.mount_alias_allows(alias,replacement,"/shared/docs/a"));
 }
 #[test]
 fn selected_volume_quarantine_blocks_authority_before_probe(){
  let mut t=FileHandles::new();
  let p=t.bind_process(1).unwrap();
  let guard=t.selected_operation_guard(p).unwrap();
  t.selected_quarantined=true;
  assert!(!t.selected_operation_guard_is_current(guard));
  assert_eq!(t.selected_operation_guard(p),Err(FsError::InvalidVolume));
 }
}


#[cfg(test)]
mod r120_inherited_r118_file_retirement_regressions {
    use super::*;

    #[test]
    fn reversed_batch_order_revokes_exact_owners_and_preserves_generation_fence() {
        for reversed in [false, true] {
            let mut files=FileHandles::new();
            let admin=files.bind_process(1).unwrap();
            let guest=files.bind_process(2).unwrap();
            let old_alias=files.mount_alias_for_current_root_kernel(guest).unwrap();
            let owners=if reversed {[guest,admin]} else {[admin,guest]};
            assert_eq!(files.retire_process_batch_kernel(&owners),Ok(2));
            assert!(!files.owns(admin));
            assert!(!files.owns(guest));
            assert!(!files.mount_alias_allows(old_alias,guest,"/shared/note"));
            let replacement=files.bind_process(2).unwrap();
            assert!(replacement.generation>guest.generation);
            assert!(!files.mount_alias_allows(old_alias,replacement,"/shared/note"));
        }
    }

    #[test]
    fn repeated_stale_pid_reuse_never_partially_retires_a_peer() {
        let mut files=FileHandles::new();
        let admin=files.bind_process(1).unwrap();
        let admin_alias=files.mount_alias_for_current_root_kernel(admin).unwrap();
        let mut previous=files.bind_process(2).unwrap();
        for _ in 0..64 {
            let current=files.bind_process(2).unwrap();
            let current_alias=files.mount_alias_for_current_root_kernel(current).unwrap();
            assert_eq!(
                files.retire_process_batch_kernel(&[admin,previous]),
                Err(FsError::InvalidName)
            );
            assert!(files.owns(admin)&&files.owns(current));
            assert!(files.mount_alias_allows(admin_alias,admin,"/shared/readme"));
            assert!(files.mount_alias_allows(current_alias,current,"/shared/note"));
            previous=current;
        }
    }

    #[test]
    fn stale_mount_namespace_receipt_cannot_authorize_batch_replacement() {
        let mut files=FileHandles::new();
        let admin=files.bind_process(1).unwrap();
        let guest=files.bind_process(2).unwrap();
        let previous=files.selected_operation_guard(guest).unwrap();
        let previous_alias=files.mount_alias_for_current_root_kernel(guest).unwrap();
        files.narrow_root_kernel(guest,"/shared/docs").unwrap();
        assert!(!files.selected_operation_guard_is_current(previous));
        assert!(!files.mount_alias_allows(previous_alias,guest,"/shared/docs/a"));
        let fresh=files.selected_operation_guard(guest).unwrap();
        assert!(files.selected_operation_guard_is_current(fresh));
        assert_eq!(files.retire_process_batch_kernel(&[admin,guest]),Ok(2));
        assert!(!files.selected_operation_guard_is_current(fresh));
    }
}

#[cfg(test)]
mod r118_bsp_local_mount_proof_regressions {
    use super::*;
    #[test]
    fn mount_proofs_reject_recycled_pid_even_with_same_root_spelling() {
        let mut files=FileHandles::new();
        let first=files.bind_process(1).unwrap();
        let guard=files.selected_operation_guard(first).unwrap();
        let alias=files.mount_alias_for_current_root_kernel(first).unwrap();
        // Copyable within the BSP, but !Send and !Sync to other CPU threads.
        let copy=guard;
        assert!(files.selected_operation_guard_is_current(copy));
        assert!(files.mount_alias_allows(alias,first,"/shared/note"));
        assert_eq!(files.retire_process_batch_kernel(&[first]),Ok(1));
        let next=files.bind_process(1).unwrap();
        assert_ne!(next.generation,first.generation);
        assert!(!files.selected_operation_guard_is_current(guard));
        assert!(!files.mount_alias_allows(alias,next,"/shared/note"));
        assert!(!files.mount_alias_allows(alias,first,"/shared/note"));
        let fresh=files.selected_operation_guard(next).unwrap();
        assert_ne!(fresh,guard);
        assert!(files.selected_operation_guard_is_current(fresh));
    }
    #[test]
    fn narrowing_a_root_revokes_old_mount_capability_and_guard() {
        let mut files=FileHandles::new();
        let owner=files.bind_process(1).unwrap();
        let guard=files.selected_operation_guard(owner).unwrap();
        let old=files.mount_alias_kernel(owner,"/shared").unwrap();
        assert!(files.mount_alias_allows(old,owner,"/shared/note"));
        files.narrow_root_kernel(owner,"/shared").unwrap();
        assert!(!files.selected_operation_guard_is_current(guard));
        assert!(!files.mount_alias_allows(old,owner,"/shared/note"));
        let current=files.mount_alias_for_current_root_kernel(owner).unwrap();
        assert!(files.mount_alias_allows(current,owner,"/shared/note"));
    }
}

#[cfg(test)]
mod r118_staged_file_retirement_tests {
    use super::*;
    #[test]
    fn prepared_file_copy_never_alters_live_pid_or_monotonic_policy() {
        let mut live=FileHandles::new();
        let first=live.bind_process(1).unwrap();
        let second=live.bind_process(2).unwrap();
        let guard=live.selected_operation_guard(second).unwrap();
        let mut draft=live.staged_copy_kernel();
        assert_eq!(draft.retire_process_batch_kernel(&[first]),Ok(1));
        assert!(!draft.owns(first));
        // A failed transaction simply drops the draft. The live owners and
        // their exact namespace capability remain unchanged.
        assert!(live.owns(first));
        assert!(live.owns(second));
        assert!(live.selected_operation_guard_is_current(guard));
        let next=live.bind_process(1).unwrap();
        assert_ne!(next.generation,first.generation);
        assert!(!draft.owns(next));
    }
}

#[cfg(test)]
mod r121_pending_mount_and_owner_stress {
    use super::*;

    #[test]
    fn sixty_four_mount_revisions_and_pid_recycles_reject_every_old_proof() {
        let mut files=FileHandles::new();
        let init=files.bind_process(1).unwrap();
        let mut guest=files.bind_process(2).unwrap();
        for _ in 0..64 {
            let old_guard=files.selected_operation_guard(guest).unwrap();
            let old_alias=files.mount_alias_for_current_root_kernel(guest).unwrap();
            assert!(files.selected_operation_guard_is_current(old_guard));
            assert!(files.mount_alias_allows(old_alias,guest,"/shared/note"));

            // Emulate a trusted successful remount's commit-point revision
            // change. This is NOT concurrent physical CPU or disk evidence.
            files.volume_revision=files.volume_revision.checked_add(1).unwrap();
            assert!(!files.selected_operation_guard_is_current(old_guard));
            assert!(!files.mount_alias_allows(old_alias,guest,"/shared/note"));

            let fresh_guard=files.selected_operation_guard(guest).unwrap();
            let fresh_alias=files.mount_alias_for_current_root_kernel(guest).unwrap();
            assert!(files.selected_operation_guard_is_current(fresh_guard));
            assert!(files.mount_alias_allows(fresh_alias,guest,"/shared/note"));

            let replaced=files.bind_process(2).unwrap();
            assert!(!files.selected_operation_guard_is_current(fresh_guard));
            assert!(!files.mount_alias_allows(fresh_alias,replaced,"/shared/note"));
            assert!(!files.mount_alias_allows(old_alias,guest,"/shared/note"));
            assert!(files.owns(init));
            assert!(files.owns(replaced));
            guest=replaced;
        }
    }

    #[test]
    fn late_wrong_owner_retirement_never_revokes_current_peer() {
        let mut files=FileHandles::new();
        let init=files.bind_process(1).unwrap();
        let old=files.bind_process(2).unwrap();
        let current=files.bind_process(2).unwrap();
        let live=files.selected_operation_guard(current).unwrap();
        assert_eq!(files.retire_process_batch_kernel(&[init,old]),
                   Err(FsError::InvalidName));
        assert!(files.owns(init) && files.owns(current));
        assert!(files.selected_operation_guard_is_current(live));
        assert_eq!(files.retire_process_batch_kernel(&[current,init]),Ok(2));
        assert!(!files.selected_operation_guard_is_current(live));
    }
}
