//! R018 developer workstation: bounded, no-heap service-side primitives.
//! Five independent eight-operation tracks (40 operations), not 40 coding
//! processes. These APIs are host-tested policy/state machines; they do NOT
//! provide a graphical IDE, a process executor, disk durability, or TLS.
//! Trusted kernel ingress must supply authenticated caller IDs.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error { Invalid, Denied, Full, Missing, Stale, Overflow, Conflict }

pub mod projects {
    use super::Error;
    pub const NAME_MAX: usize = 32;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Project {
        pub id: u32, pub owner: u32, pub revision: u64, pub tag: u16,
        pub archived: bool, pub pinned: bool, pub readonly: bool,
        name: [u8; NAME_MAX], length: usize,
    }
    impl Project { pub fn name(&self) -> &[u8] { &self.name[..self.length] } }
    pub struct Registry<const N: usize> { entries: [Option<Project>; N] }
    impl<const N: usize> Registry<N> {
        pub const fn new() -> Self { Self { entries: [None; N] } }
        fn index(&self, id: u32) -> Option<usize> {
            self.entries.iter().position(|e| e.is_some_and(|p| p.id == id))
        }
        fn validated_name(name: &[u8]) -> Result<([u8; NAME_MAX], usize), Error> {
            if name.is_empty() || name.len() > NAME_MAX ||
                name.iter().any(|b| !(b.is_ascii_alphanumeric() || b"._-".contains(b))) {
                return Err(Error::Invalid);
            }
            let mut out = [0; NAME_MAX];
            out[..name.len()].copy_from_slice(name);
            Ok((out, name.len()))
        }
        fn owned(&self, caller: u32, id: u32, expected: u64) -> Result<(usize, Project), Error> {
            let index = self.index(id).ok_or(Error::Missing)?;
            let p = self.entries[index].ok_or(Error::Missing)?;
            if caller == 0 || p.owner != caller { return Err(Error::Denied); }
            if p.revision != expected { return Err(Error::Stale); }
            Ok((index, p))
        }
        fn save(&mut self, index: usize, mut p: Project) -> Result<u64, Error> {
            p.revision = p.revision.checked_add(1).ok_or(Error::Overflow)?;
            self.entries[index] = Some(p);
            Ok(p.revision)
        }
        /// 01: create new owner-bound project (not a filesystem directory).
        pub fn create(&mut self, owner: u32, id: u32, name: &[u8]) -> Result<u64, Error> {
            if owner == 0 || id == 0 { return Err(Error::Invalid); }
            let (name, length) = Self::validated_name(name)?;
            if self.index(id).is_some() { return Err(Error::Conflict); }
            let slot = self.entries.iter().position(Option::is_none).ok_or(Error::Full)?;
            self.entries[slot] = Some(Project {
                id, owner, revision: 1, tag: 0, archived: false,
                pinned: false, readonly: false, name, length
            });
            Ok(1)
        }
        /// 02: revision-guarded rename.
        pub fn rename(&mut self, caller: u32, id: u32, rev: u64, name: &[u8]) -> Result<u64, Error> {
            let (i, mut p) = self.owned(caller, id, rev)?;
            if p.readonly || p.archived { return Err(Error::Denied); }
            let (bytes, len) = Self::validated_name(name)?;
            p.name = bytes; p.length = len; self.save(i, p)
        }
        /// 03: classification tag.
        pub fn tag(&mut self, caller: u32, id: u32, rev: u64, tag: u16) -> Result<u64, Error> {
            let (i, mut p) = self.owned(caller, id, rev)?;
            if p.readonly || p.archived { return Err(Error::Denied); }
            p.tag = tag; self.save(i, p)
        }
        /// 04: archive without destroying files.
        pub fn archive(&mut self, caller: u32, id: u32, rev: u64) -> Result<u64, Error> {
            let (i, mut p) = self.owned(caller, id, rev)?;
            if p.archived { return Err(Error::Conflict); }
            p.archived = true; self.save(i, p)
        }
        /// 05: pin or unpin in project navigator.
        pub fn pin(&mut self, caller: u32, id: u32, rev: u64, pinned: bool) -> Result<u64, Error> {
            let (i, mut p) = self.owned(caller, id, rev)?;
            if p.archived { return Err(Error::Denied); }
            p.pinned = pinned; self.save(i, p)
        }
        /// 06: enforce read-only project metadata mode.
        pub fn set_readonly(&mut self, caller: u32, id: u32, rev: u64, yes: bool) -> Result<u64, Error> {
            let (i, mut p) = self.owned(caller, id, rev)?;
            p.readonly = yes; self.save(i, p)
        }
        /// 07: restore archived project metadata.
        pub fn restore(&mut self, caller: u32, id: u32, rev: u64) -> Result<u64, Error> {
            let (i, mut p) = self.owned(caller, id, rev)?;
            if !p.archived { return Err(Error::Conflict); }
            p.archived = false; self.save(i, p)
        }
        /// 08: remove registry metadata (not user files).
        pub fn remove(&mut self, caller: u32, id: u32, rev: u64) -> Result<(), Error> {
            let (i, p) = self.owned(caller, id, rev)?;
            if !p.archived { return Err(Error::Denied); }
            self.entries[i] = None; Ok(())
        }
        pub fn get(&self, caller: u32, id: u32) -> Result<Project, Error> {
            let i = self.index(id).ok_or(Error::Missing)?;
            let p = self.entries[i].ok_or(Error::Missing)?;
            if caller == 0 || p.owner != caller { Err(Error::Denied) } else { Ok(p) }
        }
    }
}
pub mod editor {
    use super::Error;
    #[derive(Clone, Copy)]
    struct Snapshot<const N: usize> { bytes: [u8; N], length: usize }
    pub struct Buffer<const N: usize> {
        owner: u32, bytes: [u8; N], length: usize, revision: u64,
        undo: Option<Snapshot<N>>,
    }
    impl<const N: usize> Buffer<N> {
        pub const fn new(owner: u32) -> Self {
            Self { owner, bytes: [0; N], length: 0, revision: 0, undo: None }
        }
        fn guard(&self, caller: u32, rev: u64) -> Result<(), Error> {
            if caller == 0 || caller != self.owner { return Err(Error::Denied); }
            if rev != self.revision { return Err(Error::Stale); }
            Ok(())
        }
        fn publish(&mut self, next: [u8; N], len: usize) -> Result<u64, Error> {
            let rev = self.revision.checked_add(1).ok_or(Error::Overflow)?;
            self.undo = Some(Snapshot { bytes: self.bytes, length: self.length });
            self.bytes = next; self.length = len; self.revision = rev; Ok(rev)
        }
        pub fn revision(&self) -> u64 { self.revision }
        pub fn len(&self) -> usize { self.length }
        /// 09: load exact editor bytes, preserving undo.
        pub fn load(&mut self, caller: u32, rev: u64, input: &[u8]) -> Result<u64, Error> {
            self.guard(caller, rev)?;
            if input.len() > N { return Err(Error::Full); }
            let mut next = [0; N]; next[..input.len()].copy_from_slice(input);
            self.publish(next, input.len())
        }
        /// 10: atomic insertion.
        pub fn insert(&mut self, caller: u32, rev: u64, at: usize, text: &[u8]) -> Result<u64, Error> {
            self.guard(caller, rev)?;
            if at > self.length { return Err(Error::Invalid); }
            let size = self.length.checked_add(text.len()).ok_or(Error::Overflow)?;
            if size > N { return Err(Error::Full); }
            let mut next = self.bytes;
            next.copy_within(at..self.length, at + text.len());
            next[at..at + text.len()].copy_from_slice(text);
            self.publish(next, size)
        }
        /// 11: atomic byte-range replacement.
        pub fn replace(&mut self, caller: u32, rev: u64, from: usize, to: usize, text: &[u8]) -> Result<u64, Error> {
            self.guard(caller, rev)?;
            if from > to || to > self.length { return Err(Error::Invalid); }
            let new_size = self.length.checked_sub(to - from)
                .and_then(|n| n.checked_add(text.len())).ok_or(Error::Overflow)?;
            if new_size > N { return Err(Error::Full); }
            let mut next = [0; N];
            next[..from].copy_from_slice(&self.bytes[..from]);
            next[from..from + text.len()].copy_from_slice(text);
            next[from + text.len()..new_size].copy_from_slice(&self.bytes[to..self.length]);
            self.publish(next, new_size)
        }
        /// 12: remove range.
        pub fn delete(&mut self, caller: u32, rev: u64, from: usize, to: usize) -> Result<u64, Error> {
            self.replace(caller, rev, from, to, b"")
        }
        /// 13: bounded append.
        pub fn append(&mut self, caller: u32, rev: u64, text: &[u8]) -> Result<u64, Error> {
            self.insert(caller, rev, self.length, text)
        }
        /// 14: one-step undo; consumes saved edit.
        pub fn undo(&mut self, caller: u32, rev: u64) -> Result<u64, Error> {
            self.guard(caller, rev)?;
            let undo = self.undo.ok_or(Error::Missing)?;
            let new_rev = self.revision.checked_add(1).ok_or(Error::Overflow)?;
            self.bytes = undo.bytes; self.length = undo.length;
            self.undo = None; self.revision = new_rev; Ok(new_rev)
        }
        /// 15: find first exact byte pattern; empty matches are invalid.
        pub fn find(&self, caller: u32, needle: &[u8]) -> Result<Option<usize>, Error> {
            if caller == 0 || caller != self.owner { return Err(Error::Denied); }
            if needle.is_empty() { return Err(Error::Invalid); }
            Ok(self.bytes[..self.length].windows(needle.len()).position(|w| w == needle))
        }
        /// 16: copy range without partially touching short destination.
        pub fn copy(&self, caller: u32, from: usize, to: usize, out: &mut [u8]) -> Result<usize, Error> {
            if caller == 0 || caller != self.owner { return Err(Error::Denied); }
            if from > to || to > self.length { return Err(Error::Invalid); }
            let len = to - from;
            if len > out.len() { return Err(Error::Full); }
            out[..len].copy_from_slice(&self.bytes[from..to]); Ok(len)
        }
    }
}
pub mod jobs {
    use super::Error;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum State { Queued, Running, Done, Failed, Cancelled }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Job {
        pub id: u32, pub owner: u32, pub priority: u8, pub state: State,
        pub attempts: u8, pub max_attempts: u8, pub result_code: u16,
    }
    pub struct Queue<const N: usize> { jobs: [Option<Job>; N] }
    impl<const N: usize> Queue<N> {
        pub const fn new() -> Self { Self { jobs: [None; N] } }
        fn index(&self, id: u32) -> Option<usize> {
            self.jobs.iter().position(|x| x.is_some_and(|j| j.id == id))
        }
        fn owned(&self, owner: u32, id: u32) -> Result<(usize, Job), Error> {
            let i = self.index(id).ok_or(Error::Missing)?;
            let job = self.jobs[i].ok_or(Error::Missing)?;
            if owner == 0 || job.owner != owner { Err(Error::Denied) } else { Ok((i, job)) }
        }
        /// 17: enqueue bounded owner-scoped job.
        pub fn submit(&mut self, owner: u32, id: u32, priority: u8, max_attempts: u8) -> Result<(), Error> {
            if owner == 0 || id == 0 || max_attempts == 0 { return Err(Error::Invalid); }
            if self.index(id).is_some() { return Err(Error::Conflict); }
            let i = self.jobs.iter().position(Option::is_none).ok_or(Error::Full)?;
            self.jobs[i] = Some(Job { id, owner, priority, state: State::Queued,
                attempts: 0, max_attempts, result_code: 0 }); Ok(())
        }
        /// 18: reprioritize only queued jobs.
        pub fn prioritize(&mut self, owner: u32, id: u32, priority: u8) -> Result<(), Error> {
            let (i, mut j) = self.owned(owner, id)?;
            if j.state != State::Queued { return Err(Error::Conflict); }
            j.priority = priority; self.jobs[i] = Some(j); Ok(())
        }
        /// 19: schedule highest priority, stable ascending ID on ties.
        pub fn start_next(&mut self, owner: u32) -> Result<Job, Error> {
            if owner == 0 { return Err(Error::Denied); }
            let mut selected: Option<(usize, Job)> = None;
            for (i, entry) in self.jobs.iter().enumerate() {
                if let Some(j) = entry {
                    if j.owner == owner && j.state == State::Queued &&
                        selected.is_none_or(|(_, previous)| j.priority > previous.priority ||
                        (j.priority == previous.priority && j.id < previous.id)) {
                        selected = Some((i, *j));
                    }
                }
            }
            let (i, mut job) = selected.ok_or(Error::Missing)?;
            job.state = State::Running;
            job.attempts = job.attempts.checked_add(1).ok_or(Error::Overflow)?;
            self.jobs[i] = Some(job); Ok(job)
        }
        /// 20: mark running job successful.
        pub fn complete(&mut self, owner: u32, id: u32) -> Result<(), Error> {
            let (i, mut j) = self.owned(owner, id)?;
            if j.state != State::Running { return Err(Error::Conflict); }
            j.state = State::Done; self.jobs[i] = Some(j); Ok(())
        }
        /// 21: record failed attempt without implicit retry.
        pub fn fail(&mut self, owner: u32, id: u32, code: u16) -> Result<(), Error> {
            let (i, mut j) = self.owned(owner, id)?;
            if j.state != State::Running || code == 0 { return Err(Error::Conflict); }
            j.state = State::Failed; j.result_code = code; self.jobs[i] = Some(j); Ok(())
        }
        /// 22: explicitly retry failed job only within attempt budget.
        pub fn retry(&mut self, owner: u32, id: u32) -> Result<(), Error> {
            let (i, mut j) = self.owned(owner, id)?;
            if j.state != State::Failed || j.attempts >= j.max_attempts { return Err(Error::Denied); }
            j.state = State::Queued; j.result_code = 0; self.jobs[i] = Some(j); Ok(())
        }
        /// 23: cancel queued or failed job; running job needs worker termination.
        pub fn cancel(&mut self, owner: u32, id: u32) -> Result<(), Error> {
            let (i, mut j) = self.owned(owner, id)?;
            if j.state != State::Queued && j.state != State::Failed { return Err(Error::Conflict); }
            j.state = State::Cancelled; self.jobs[i] = Some(j); Ok(())
        }
        /// 24: inspect job without exposing another owner's metadata.
        pub fn get(&self, owner: u32, id: u32) -> Result<Job, Error> { self.owned(owner, id).map(|(_, j)| j) }
    }
}
pub mod services {
    use super::Error;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Profile { Desktop, Developer, Server, AllInOne }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Service { pub id: u32, pub enabled: bool, pub running: bool, pub quota: u16 }
    pub struct Plan<const N: usize> { owner: u32, profile: Profile, services: [Option<Service>; N] }
    impl<const N: usize> Plan<N> {
        pub const fn new(owner: u32) -> Self {
            Self { owner, profile: Profile::Desktop, services: [None; N] }
        }
        fn auth(&self, caller: u32) -> Result<(), Error> {
            if caller == 0 || caller != self.owner { Err(Error::Denied) } else { Ok(()) }
        }
        fn index(&self, id: u32) -> Option<usize> {
            self.services.iter().position(|x| x.is_some_and(|s| s.id == id))
        }
        /// 25: select edition without reinstalling or changing service grants.
        pub fn select_profile(&mut self, caller: u32, profile: Profile) -> Result<(), Error> {
            self.auth(caller)?; self.profile = profile; Ok(())
        }
        /// 26: register metadata previously approved by trusted verifier.
        pub fn register_verified(&mut self, caller: u32, id: u32, verified: bool) -> Result<(), Error> {
            self.auth(caller)?;
            if id == 0 || !verified { return Err(Error::Denied); }
            if self.index(id).is_some() { return Err(Error::Conflict); }
            let i = self.services.iter().position(Option::is_none).ok_or(Error::Full)?;
            self.services[i] = Some(Service { id, enabled: false, running: false, quota: 1 }); Ok(())
        }
        /// 27: permissioned, explicit service opt-in.
        pub fn enable(&mut self, caller: u32, id: u32) -> Result<(), Error> {
            self.auth(caller)?;
            let i = self.index(id).ok_or(Error::Missing)?;
            let mut s = self.services[i].ok_or(Error::Missing)?;
            s.enabled = true; self.services[i] = Some(s); Ok(())
        }
        /// 28: disable only after service has been stopped.
        pub fn disable(&mut self, caller: u32, id: u32) -> Result<(), Error> {
            self.auth(caller)?;
            let i = self.index(id).ok_or(Error::Missing)?;
            let mut s = self.services[i].ok_or(Error::Missing)?;
            if s.running { return Err(Error::Conflict); }
            s.enabled = false; self.services[i] = Some(s); Ok(())
        }
        /// 29: explicit on-demand start (metadata state, not process spawn).
        pub fn start(&mut self, caller: u32, id: u32) -> Result<(), Error> {
            self.auth(caller)?;
            let i = self.index(id).ok_or(Error::Missing)?;
            let mut s = self.services[i].ok_or(Error::Missing)?;
            if !s.enabled || s.running { return Err(Error::Denied); }
            s.running = true; self.services[i] = Some(s); Ok(())
        }
        /// 30: explicit stop (metadata state, not process signalling).
        pub fn stop(&mut self, caller: u32, id: u32) -> Result<(), Error> {
            self.auth(caller)?;
            let i = self.index(id).ok_or(Error::Missing)?;
            let mut s = self.services[i].ok_or(Error::Missing)?;
            if !s.running { return Err(Error::Conflict); }
            s.running = false; self.services[i] = Some(s); Ok(())
        }
        /// 31: record bounded target worker quota for external supervisor.
        pub fn set_quota(&mut self, caller: u32, id: u32, quota: u16) -> Result<(), Error> {
            self.auth(caller)?;
            if quota == 0 { return Err(Error::Invalid); }
            let i = self.index(id).ok_or(Error::Missing)?;
            let mut s = self.services[i].ok_or(Error::Missing)?;
            s.quota = quota; self.services[i] = Some(s); Ok(())
        }
        /// 32: query profile and per-service status.
        pub fn status(&self, caller: u32, id: u32) -> Result<(Profile, Service), Error> {
            self.auth(caller)?;
            let i = self.index(id).ok_or(Error::Missing)?;
            Ok((self.profile, self.services[i].ok_or(Error::Missing)?))
        }
    }
}
pub mod diagnostics {
    use super::Error;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Event { pub sequence: u64, pub owner: u32, pub kind: u16, pub code: u16 }
    pub struct Journal<const N: usize> {
        admin: u32, slots: [Option<Event>; N], cursor: usize,
        sequence: u64, frozen: bool,
    }
    impl<const N: usize> Journal<N> {
        pub const fn new(admin: u32) -> Self {
            Self { admin, slots: [None; N], cursor: 0, sequence: 0, frozen: false }
        }
        fn admin(&self, caller: u32) -> Result<(), Error> {
            if caller == 0 || caller != self.admin { Err(Error::Denied) } else { Ok(()) }
        }
        /// 33: bounded, monotonic event record; oldest slot is overwritten.
        pub fn record(&mut self, caller: u32, owner: u32, kind: u16, code: u16) -> Result<u64, Error> {
            self.admin(caller)?;
            if self.frozen { return Err(Error::Denied); }
            if owner == 0 || kind == 0 || N == 0 { return Err(Error::Invalid); }
            let seq = self.sequence.checked_add(1).ok_or(Error::Overflow)?;
            self.slots[self.cursor] = Some(Event { sequence: seq, owner, kind, code });
            self.cursor = (self.cursor + 1) % N;
            self.sequence = seq; Ok(seq)
        }
        /// 34: owner-isolated exact sequence lookup.
        pub fn get(&self, caller: u32, sequence: u64) -> Result<Event, Error> {
            if caller == 0 || sequence == 0 { return Err(Error::Denied); }
            let e = self.slots.iter().flatten().find(|e| e.sequence == sequence).ok_or(Error::Missing)?;
            if e.owner != caller { Err(Error::Denied) } else { Ok(*e) }
        }
        /// 35: retrieve most recent event visible to owner.
        pub fn latest(&self, caller: u32) -> Result<Event, Error> {
            if caller == 0 { return Err(Error::Denied); }
            self.slots.iter().flatten().filter(|e| e.owner == caller)
                .max_by_key(|e| e.sequence).copied().ok_or(Error::Missing)
        }
        /// 36: owner-scoped event count by kind.
        pub fn count_kind(&self, caller: u32, kind: u16) -> Result<usize, Error> {
            if caller == 0 { return Err(Error::Denied); }
            Ok(self.slots.iter().flatten().filter(|e| e.owner == caller && e.kind == kind).count())
        }
        /// 37: count events newer than a caller checkpoint sequence.
        pub fn count_since(&self, caller: u32, sequence: u64) -> Result<usize, Error> {
            if caller == 0 { return Err(Error::Denied); }
            Ok(self.slots.iter().flatten().filter(|e| e.owner == caller && e.sequence > sequence).count())
        }
        /// 38: freeze privileged event publication for triage.
        pub fn freeze(&mut self, caller: u32) -> Result<(), Error> {
            self.admin(caller)?; self.frozen = true; Ok(())
        }
        /// 39: resume privileged event publication.
        pub fn thaw(&mut self, caller: u32) -> Result<(), Error> {
            self.admin(caller)?; self.frozen = false; Ok(())
        }
        /// 40: privacy-preserving owner event purge, with sequence monotonicity.
        pub fn purge_owner(&mut self, caller: u32, owner: u32) -> Result<usize, Error> {
            self.admin(caller)?;
            if self.frozen || owner == 0 { return Err(Error::Denied); }
            let mut removed = 0;
            for slot in &mut self.slots {
                if slot.is_some_and(|e| e.owner == owner) { *slot = None; removed += 1; }
            }
            Ok(removed)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use projects::Registry;
    use editor::Buffer;
    use jobs::{Queue,State};
    use services::{Plan,Profile};
    use diagnostics::Journal;
    #[test]
    fn project_owner_revision_and_metadata_lifecycle() {
        let mut r=Registry::<2>::new();
        assert_eq!(r.create(7,9,b"kernel"),Ok(1));
        assert_eq!(r.rename(7,9,1,b"rust-kernel"),Ok(2));
        assert_eq!(r.tag(7,9,1,3),Err(Error::Stale));
        assert_eq!(r.tag(7,9,2,3),Ok(3));
        assert_eq!(r.pin(7,9,3,true),Ok(4));
        assert_eq!(r.set_readonly(7,9,4,true),Ok(5));
        assert_eq!(r.rename(7,9,5,b"bad"),Err(Error::Denied));
        assert_eq!(r.set_readonly(7,9,5,false),Ok(6));
        assert_eq!(r.archive(7,9,6),Ok(7));
        assert_eq!(r.restore(7,9,7),Ok(8));
        assert_eq!(r.archive(7,9,8),Ok(9));
        assert_eq!(r.remove(7,9,9),Ok(()));
        assert_eq!(r.get(7,9),Err(Error::Missing));
    }
    #[test]
    fn project_invalid_names_and_foreign_owners_are_fail_closed() {
        let mut r=Registry::<1>::new(); r.create(3,1,b"app").unwrap();
        assert_eq!(r.rename(4,1,1,b"x"),Err(Error::Denied));
        assert_eq!(r.rename(3,1,1,b"../escape"),Err(Error::Invalid));
        assert_eq!(r.get(3,1).unwrap().name(),b"app");
        assert_eq!(r.create(3,2,b"new"),Err(Error::Full));
        assert_eq!(r.get(4,1),Err(Error::Denied));
    }
    #[test]
    fn editor_insert_replace_append_find_and_copy() {
        let mut e=Buffer::<16>::new(3);
        assert_eq!(e.load(3,0,b"ab"),Ok(1));
        assert_eq!(e.insert(3,1,1,b"Z"),Ok(2));
        assert_eq!(e.replace(3,2,2,3,b"C"),Ok(3));
        assert_eq!(e.append(3,3,b"D"),Ok(4));
        assert_eq!(e.find(3,b"ZC"),Ok(Some(1)));
        let mut dst=[0u8;4];
        assert_eq!(e.copy(3,0,4,&mut dst),Ok(4));
        assert_eq!(&dst,b"aZCD");
        assert_eq!(e.delete(3,4,1,3),Ok(5));
        assert_eq!(e.undo(3,5),Ok(6));
        assert_eq!(e.copy(3,0,4,&mut dst),Ok(4));
        assert_eq!(&dst,b"aZCD");
    }
    #[test]
    fn editor_overflow_and_stale_edits_cannot_mutate_destination() {
        let mut e=Buffer::<3>::new(1);e.load(1,0,b"abc").unwrap();
        assert_eq!(e.append(1,1,b"d"),Err(Error::Full));
        assert_eq!(e.delete(1,0,0,1),Err(Error::Stale));
        let mut dst=[0xaau8;1];
        assert_eq!(e.copy(1,0,3,&mut dst),Err(Error::Full));
        assert_eq!(dst,[0xaa]);
        assert_eq!(e.copy(2,0,1,&mut dst),Err(Error::Denied));
        assert_eq!(e.revision(),1);
    }
    #[test]
    fn jobs_schedule_high_priority_then_retry_budget() {
        let mut q=Queue::<3>::new();
        q.submit(1,20,1,2).unwrap();q.submit(1,10,1,1).unwrap();
        q.prioritize(1,20,5).unwrap();
        assert_eq!(q.start_next(1).unwrap().id,20);
        q.fail(1,20,400).unwrap();
        q.retry(1,20).unwrap();
        assert_eq!(q.start_next(1).unwrap().attempts,2);
        q.fail(1,20,500).unwrap();
        assert_eq!(q.retry(1,20),Err(Error::Denied));
        assert_eq!(q.start_next(1).unwrap().id,10);
        q.complete(1,10).unwrap();
        assert_eq!(q.get(1,10).unwrap().state,State::Done);
    }
    #[test]
    fn jobs_reject_foreign_access_and_implicit_running_cancel() {
        let mut q=Queue::<1>::new();q.submit(7,9,3,1).unwrap();
        assert_eq!(q.get(2,9),Err(Error::Denied));
        q.start_next(7).unwrap();
        assert_eq!(q.cancel(7,9),Err(Error::Conflict));
        assert_eq!(q.start_next(7),Err(Error::Missing));
        q.complete(7,9).unwrap();
        assert_eq!(q.retry(7,9),Err(Error::Denied));
    }
    #[test]
    fn profiles_do_not_start_optional_services() {
        let mut p=Plan::<2>::new(7);
        assert_eq!(p.register_verified(7,22,false),Err(Error::Denied));
        p.register_verified(7,22,true).unwrap();
        p.select_profile(7,Profile::AllInOne).unwrap();
        assert_eq!(p.status(7,22).unwrap().1.running,false);
        assert_eq!(p.start(7,22),Err(Error::Denied));
        p.enable(7,22).unwrap();p.set_quota(7,22,4).unwrap();
        p.start(7,22).unwrap();
        assert_eq!(p.disable(7,22),Err(Error::Conflict));
        p.stop(7,22).unwrap();p.disable(7,22).unwrap();
        assert_eq!(p.status(7,22).unwrap().1.quota,4);
    }
    #[test]
    fn profile_owner_authentication_prevents_mutation() {
        let mut p=Plan::<1>::new(10);
        assert_eq!(p.select_profile(11,Profile::Server),Err(Error::Denied));
        assert_eq!(p.register_verified(11,1,true),Err(Error::Denied));
        p.register_verified(10,1,true).unwrap();
        assert_eq!(p.enable(11,1),Err(Error::Denied));
        assert_eq!(p.status(11,1),Err(Error::Denied));
        assert_eq!(p.status(10,1).unwrap().0,Profile::Desktop);
    }
    #[test]
    fn diagnostic_ring_is_owner_isolated_and_monotonic() {
        let mut j=Journal::<2>::new(100);
        assert_eq!(j.record(100,1,7,3),Ok(1));
        assert_eq!(j.record(100,2,9,3),Ok(2));
        assert_eq!(j.get(1,2),Err(Error::Denied));
        assert_eq!(j.count_kind(1,7),Ok(1));
        assert_eq!(j.latest(2).unwrap().sequence,2);
        assert_eq!(j.record(100,1,7,5),Ok(3));
        assert_eq!(j.get(1,1),Err(Error::Missing));
        assert_eq!(j.count_since(1,2),Ok(1));
    }
    #[test]
    fn diagnostic_freeze_purge_and_zero_capacity_fail_closed() {
        let mut j=Journal::<2>::new(100);
        j.record(100,1,7,1).unwrap();
        j.freeze(100).unwrap();
        assert_eq!(j.record(100,1,7,2),Err(Error::Denied));
        assert_eq!(j.purge_owner(100,1),Err(Error::Denied));
        assert_eq!(j.thaw(1),Err(Error::Denied));
        j.thaw(100).unwrap();
        assert_eq!(j.purge_owner(100,1),Ok(1));
        assert_eq!(j.latest(1),Err(Error::Missing));
        assert_eq!(Journal::<0>::new(10).record(10,1,1,1),Err(Error::Invalid));
    }
}
