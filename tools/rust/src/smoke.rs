//! Bounded, unprivileged QEMU/OVMF serial smoke runner in Rust.
//! Replaces the Python host orchestration only after independent parity
//! checks. Never attaches physical disks: only an ISO and OVMF pflash.

use std::{
    fs,
    io::{Read,Write},
    path::{Path,PathBuf},
    process::{Command,Stdio},
    sync::{mpsc,Arc,Mutex},
    thread,
    time::{Duration,Instant,SystemTime,UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::net::UnixStream;

const MILESTONES:[&str;15]=[
    "IndianOS: kernel entered",
    "IndianOS: boot information validated",
    "IndianOS: milestone 001 complete",
    "IndianOS: exception handlers ready",
    "IndianOS: physical memory manager ready",
    "IndianOS: milestone 002 complete",
    "IndianOS: detached virtual memory manager ready",
    "IndianOS: milestone 003 complete",
    "IndianOS: paging capabilities inspected",
    "IndianOS: milestone 004 complete",
    "IndianOS: ELF64 user-image validation ready",
    "IndianOS: syscall ABI metadata validation ready",
    "IndianOS: milestone 005 foundation complete",
    "IndianOS: read-only initramfs /bin/init validated",
    "IndianOS: initramfs /bin/init staged and released safely",
];
fn in_order(text:&str,required:&[&str])->Result<(),String>{
    let mut at=0;
    for marker in required{
        let Some(pos)=text[at..].find(marker) else{
            return Err(format!("missing or out-of-order marker: {marker}"));
        };
        at+=pos+marker.len();
    }
    Ok(())
}
fn occurrences(text:&str,value:&str)->usize{text.match_indices(value).count()}
pub fn verify(mode:&str,text:&str)->Result<(),String>{
    if mode!="exception" && mode!="pagefault" && text.contains("IndianOS: FATAL"){
        return Err("kernel reported a fatal boot error".into())
    }
    let mut checks:Vec<&str>=Vec::new();
    match mode {
        "normal"=>checks.extend(MILESTONES),
        "exception"=>{
            checks.extend(&MILESTONES[..5]);
            checks.extend(["IndianOS: triggering intentional UD2",
                "IndianOS: FATAL CPU exception vector=0x0000000000000006"]);
        }
        "pagefault"=>{
            checks.extend(MILESTONES);
            checks.extend(["IndianOS: triggering intentional page fault",
                "IndianOS: FATAL CPU exception vector=0x000000000000000e",
                "IndianOS: page fault cr2=0x00007fff00000000"]);
        }
        "shadow"=>{
            checks.extend(&MILESTONES[..9]);
            checks.push("IndianOS: shadow CR3 switched and restored");
            checks.extend(&MILESTONES[9..]);
        }
        "usermode"=>{
            checks.extend(MILESTONES);
            checks.extend(["IndianOS: entering ring-3 /bin/init (QEMU experimental)",
                "IndianOS user: Namaste from IndianOS user mode!",
                "IndianOS: /bin/init exited through int 0x80",
                "IndianOS: ring-3 user program experiment complete"]);
        }
        "fault-guard" | "fault-supervisor"=>{
            checks.extend(MILESTONES);
            checks.extend(["IndianOS: entering ring-3 /bin/init (QEMU experimental)",
                "IndianOS: isolated ring-3 page fault",
                "IndianOS: user page fault contained; kernel resumed",
                "IndianOS: ring-3 borrowed user tables detached",
                "IndianOS: ring-3 supervisor clone released",
                "IndianOS: ring-3 user image released",
                "IndianOS: ring-3 PMM frame count restored",
                "IndianOS: user fault isolation experiment complete"]);
        }
        "processes"=>{
            checks.extend(MILESTONES);
            checks.extend(["IndianOS: M008 sequential process launch begin",
                "IndianOS: PID 1 READY","IndianOS: PID 1 RUNNING",
                "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
                "IndianOS user: Namaste from IndianOS user mode!",
                "IndianOS: PID 1 EXITED",
                "IndianOS: ring-3 PMM frame count restored",
                "IndianOS: PID 1 REAPED","IndianOS: PID 2 READY",
                "IndianOS: PID 2 RUNNING",
                "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
                "IndianOS user: IndianOS worker process 2 completed",
                "IndianOS: PID 2 EXITED",
                "IndianOS: ring-3 PMM frame count restored",
                "IndianOS: PID 2 REAPED",
                "IndianOS: M008 two independent processes exited and reaped"]);
        }
        "shell"=>{
            checks.extend(MILESTONES);
            checks.extend([
                "IndianOS: R013 init shell begin",
                "IndianOS: PID 1 READY",
                "IndianOS: PID 1 RUNNING",
                "IndianOS user: R013 shell: IndianOS init started",
                "IndianOS user: R013 shell: commands: help ps run /bin/worker exit",
                "IndianOS user: R013 shell: ps PID 1 RUNNING",
                "IndianOS: R013 PID 1 requested /bin/worker",
                "IndianOS user: R013 shell: run /bin/worker accepted",
                "IndianOS user: R013 shell: exit",
                "IndianOS: PID 1 EXITED",
                "IndianOS: PID 1 REAPED",
                "IndianOS: R013 launching /bin/worker",
                "IndianOS: PID 2 READY",
                "IndianOS: PID 2 RUNNING",
                "IndianOS user: IndianOS worker process 2 completed",
                "IndianOS: PID 2 EXITED",
                "IndianOS: PID 2 REAPED",
                "IndianOS: R013 worker exited and reaped",
                "IndianOS: R013 shell session complete",
            ]);
        }
        "keyboard"=>{
            checks.extend(&MILESTONES[..2]);
            checks.push("IndianOS: R014.2 framebuffer text console ready");
            checks.extend(&MILESTONES[2..]);
            checks.extend([
                "IndianOS: R013 init shell begin",
                "IndianOS: R014.2 IRQ1 keyboard controller armed",
                "IndianOS: PID 1 RUNNING",
                "IndianOS user: R014 interactive shell ready",
                "IndianOS user: R014 shell: commands help ps run /bin/worker exit",
                "IndianOS user: R014 shell: ps PID 1 RUNNING",
                "IndianOS: R013 PID 1 requested /bin/worker",
                "IndianOS user: R014 shell: worker queued for after exit",
                "IndianOS user: R014 shell: exit",
                "IndianOS: PID 1 REAPED",
                "IndianOS: R014.2 PS/2 IRQ1 delivery verified",
                "IndianOS: R013 launching /bin/worker",
                "IndianOS: PID 2 REAPED",
                "IndianOS: R013 shell session complete",
            ]);
        }
        "r026"=>{
            checks.extend(MILESTONES);
            checks.extend([
                "IndianOS: R026 persistent init shell and on-demand worker begin",
                "IndianOS: PID 1 READY", "IndianOS: PID 2 READY",
                "IndianOS: R026 worker prepared but not yet runnable",
                "IndianOS: PID 1 DISPATCHED",
                "IndianOS user: R026 persistent interactive shell ready",
                "IndianOS user: indianos> ",
                "IndianOS user: R014 shell: commands help ps run /bin/worker exit",
                "IndianOS user: indianos> ",
                "IndianOS user: R026 shell: ps PID1 RUNNING PID2 NOT_STARTED",
                "IndianOS user: indianos> ",
                "IndianOS: R026 /bin/worker launch granted to PID1",
                "IndianOS user: R026 shell: worker started; yielding to scheduler",
                "IndianOS: PID 1 YIELDED",
                "IndianOS: PID 2 DISPATCHED",
                "IndianOS user: R026 /bin/worker ran in isolated ring-3 process",
                "IndianOS: PID 2 EXITED", "IndianOS: PID 2 REAPED",
                "IndianOS: R026 worker exited and reaped while shell remains alive",
                "IndianOS: PID 1 DISPATCHED",
                "IndianOS user: R026 shell: resumed after worker completion",
                "IndianOS user: indianos> ",
                "IndianOS user: R026 shell: ps PID1 RUNNING PID2 REAPED",
                "IndianOS user: indianos> ",
                "IndianOS user: R026 shell: wait /bin/worker completed",
                "IndianOS user: indianos> ",
                "IndianOS user: R026 shell: duplicate worker launch rejected",
                "IndianOS user: indianos> ",
                "IndianOS user: R026 shell: user requested shutdown",
                "IndianOS: PID 1 EXITED", "IndianOS: PID 1 REAPED",
                "IndianOS: R026 persistent interactive worker lifecycle PASSED",
                "IndianOS: R026 persistent interactive shell session complete"
            ]);
        }
        "interactive"=>{
            checks.extend(MILESTONES);
            checks.extend([
                "IndianOS: R013 init shell begin",
                "IndianOS: PID 1 READY",
                "IndianOS: PID 1 RUNNING",
                "IndianOS user: R014 interactive shell ready",
                "IndianOS user: indianos> ",
                "IndianOS user: R014 shell: commands help ps run /bin/worker exit",
                "IndianOS user: R014 shell: ps PID 1 RUNNING",
                "IndianOS: R013 PID 1 requested /bin/worker",
                "IndianOS user: R014 shell: worker queued for after exit",
                "IndianOS user: R014 shell: exit",
                "IndianOS: PID 1 EXITED",
                "IndianOS: PID 1 REAPED",
                "IndianOS: R013 launching /bin/worker",
                "IndianOS: PID 2 READY",
                "IndianOS: PID 2 RUNNING",
                "IndianOS user: IndianOS worker process 2 completed",
                "IndianOS: PID 2 EXITED",
                "IndianOS: PID 2 REAPED",
                "IndianOS: R013 shell session complete",
            ]);
        }
        "scheduler"=>{
            checks.extend(MILESTONES);
            checks.extend(["IndianOS: M009 cooperative scheduler experiment begin",
                "IndianOS: PID 1 READY","IndianOS: PID 2 READY",
                "IndianOS: M009 two simultaneous user address spaces ready",
                "IndianOS: PID 1 DISPATCHED","IndianOS user: M009 task A step 1",
                "IndianOS: PID 1 YIELDED","IndianOS: PID 2 DISPATCHED",
                "IndianOS user: M009 task B step 1",
                "IndianOS: PID 2 YIELDED","IndianOS: PID 1 DISPATCHED",
                "IndianOS user: M009 task A step 2","IndianOS: PID 1 YIELDED",
                "IndianOS: PID 2 DISPATCHED","IndianOS user: M009 task B step 2",
                "IndianOS: PID 2 YIELDED","IndianOS: PID 1 DISPATCHED",
                "IndianOS user: M009 task A step 3","IndianOS: PID 1 EXITED",
                "IndianOS: PID 1 REAPED","IndianOS: PID 2 DISPATCHED",
                "IndianOS user: M009 task B step 3","IndianOS: PID 2 EXITED",
                "IndianOS: PID 2 REAPED",
                "IndianOS: M009 round-robin cooperative switching PASSED",
                "IndianOS: M009 scheduler experiment complete"]);
        }
        "r025"=>{
            checks.extend(MILESTONES);
            checks.extend([
                "IndianOS: R025 complete syscall services test begin",
                "IndianOS: PID 1 READY",
                "IndianOS: PID 2 READY",
                "IndianOS: R025 authenticated IPC mailboxes ready",
                "IndianOS: R025 capability and isolated file services ready",
                "IndianOS: M009 two simultaneous user address spaces ready",
                "IndianOS: PID 1 DISPATCHED",
                "IndianOS user: R025 PID1 process service validated",
                "IndianOS user: R025 PID1 isolated filesystem create/read verified",
                "IndianOS user: R025 PID1 capability IPC ping sent",
                "IndianOS: PID 1 YIELDED",
                "IndianOS: PID 2 DISPATCHED",
                "IndianOS user: R025 PID2 process service validated",
                "IndianOS user: R025 PID2 authenticated IPC ping received",
                "IndianOS user: R025 PID2 foreign filesystem and token access denied",
                "IndianOS user: R025 PID2 capability IPC reply sent",
                "IndianOS: PID 2 YIELDED",
                "IndianOS: PID 1 DISPATCHED",
                "IndianOS user: R025 PID1 authenticated IPC reply received",
                "IndianOS user: R025 PID1 invalid token and foreign resource rejected",
                "IndianOS user: R025 PID1 file deletion and capability revocation verified",
                "IndianOS: PID 1 YIELDED",
                "IndianOS: PID 2 DISPATCHED",
                "IndianOS user: R025 PID2 file deletion and rights teardown verified",
                "IndianOS: PID 2 YIELDED",
                "IndianOS: PID 1 DISPATCHED",
                "IndianOS user: R025 PID1 exited cleanly",
                "IndianOS: PID 1 CAPABILITIES_REVOKED",
                "IndianOS: PID 1 EXITED",
                "IndianOS: PID 1 REAPED",
                "IndianOS: PID 2 DISPATCHED",
                "IndianOS user: R025 PID2 exited cleanly",
                "IndianOS: PID 2 CAPABILITIES_REVOKED",
                "IndianOS: PID 2 EXITED",
                "IndianOS: PID 2 REAPED",
                "IndianOS: M009 round-robin cooperative switching PASSED",
                "IndianOS: R025 secure IPC two-process handshake PASSED",
                "IndianOS: R025 capability IPC filesystem process services PASSED",
                "IndianOS: M009 scheduler experiment complete",
            ]);
        }
        "ipc"=>{
            checks.extend(MILESTONES);
            checks.extend([
                "IndianOS: R025 secure syscall IPC test begin",
                "IndianOS: PID 1 READY", "IndianOS: PID 2 READY",
                "IndianOS: R025 authenticated IPC mailboxes ready",
                "IndianOS: PID 1 DISPATCHED",
                "IndianOS user: R025 PID1 sent authenticated ping",
                "IndianOS: PID 1 YIELDED",
                "IndianOS: PID 2 DISPATCHED",
                "IndianOS user: R025 PID2 received authenticated ping",
                "IndianOS user: R025 PID2 sent authenticated reply",
                "IndianOS: PID 2 YIELDED",
                "IndianOS: PID 1 DISPATCHED",
                "IndianOS user: R025 PID1 received authenticated reply",
                "IndianOS user: R025 PID1 invalid destination and pointers denied",
                "IndianOS: PID 1 YIELDED",
                "IndianOS: PID 2 DISPATCHED",
                "IndianOS user: R025 PID2 empty queue is isolated",
                "IndianOS: PID 2 YIELDED",
                "IndianOS: PID 1 DISPATCHED",
                "IndianOS user: R025 PID1 exited cleanly",
                "IndianOS: PID 1 EXITED", "IndianOS: PID 1 REAPED",
                "IndianOS: PID 2 DISPATCHED",
                "IndianOS user: R025 PID2 exited cleanly",
                "IndianOS: PID 2 EXITED", "IndianOS: PID 2 REAPED",
                "IndianOS: M009 round-robin cooperative switching PASSED",
                "IndianOS: R025 secure IPC two-process handshake PASSED",
                "IndianOS: M009 scheduler experiment complete",
            ]);
        }
        "preempt"=>{
            checks.extend(MILESTONES);
            checks.extend(["IndianOS: R024 APIC preemptive scheduler experiment begin",
                "IndianOS: R024 xAPIC supervisor UC mapping installed",
                "IndianOS: M009 two simultaneous user address spaces ready",
                "IndianOS: R024 calibrated periodic xAPIC IRQ at 100 Hz"]);
            // Preemption events are asynchronous; the original harness
            // checks presence of markers for both tasks rather than a
            // specific ordering.
            for (pid,name) in [(1,"A"),(2,"B")] {
                for marker in [
                    format!("IndianOS: PID {pid} READY"),
                    format!("IndianOS: PID {pid} DISPATCHED"),
                    format!("IndianOS: PID {pid} APIC_TIMER_PREEMPTED"),
                    format!("IndianOS user: M009 timer task {name} started"),
                    format!("IndianOS user: M009 timer task {name} finished"),
                    format!("IndianOS: PID {pid} EXITED"),
                    format!("IndianOS: PID {pid} REAPED"),
                    format!("IndianOS: PID {pid} YIELDED"),
                ]{
                    if !text.contains(&marker){return Err(format!("missing {marker}"))}
                }
            }
            for marker in ["IndianOS: R024 APIC timer-preemptive round-robin PASSED",
                            "IndianOS: M009 scheduler experiment complete"] {
                if !text.contains(marker){return Err(format!("missing {marker}"))}
            }
        }
        _=>return Err(format!("unknown mode: {mode}")),
    }
    in_order(text,&checks)?;
    match mode{
        "exception" if text.contains("IndianOS: milestone 002 complete")=>
            Err("UD2 test incorrectly returned from exception".into()),
        "fault-guard"|"fault-supervisor"
            if text.contains("IndianOS: /bin/init exited through int 0x80")=>
            Err("faulting user unexpectedly exited normally".into()),
        "keyboard" if occurrences(text,"IndianOS user: indianos> ")!=4 ||
            occurrences(text,"IndianOS: ring-3 PMM frame count restored")!=2 ||
            text.contains("R014 shell: unknown command") =>
            Err("R014.2 physical keyboard IRQ1 or process lifecycle failed".into()),
        "interactive" if occurrences(text,"IndianOS user: indianos> ")!=4 ||
            occurrences(text,"IndianOS: ring-3 PMM frame count restored")!=2 ||
            occurrences(text,"IndianOS: PID 1 REAPED")!=1 ||
            occurrences(text,"IndianOS: PID 2 REAPED")!=1 ||
            text.contains("R014 shell: unknown command") =>
            Err("R014 interactive I/O or two-process lifecycle mismatch".into()),
        "shell" if occurrences(text,"IndianOS: ring-3 PMM frame count restored")!=2 ||
            occurrences(text,"IndianOS: PID 1 REAPED")!=1 ||
            occurrences(text,"IndianOS: PID 2 REAPED")!=1 =>
            Err("R013 must launch and reclaim exactly two lifetimes".into()),
        "processes" if occurrences(text,"IndianOS: entering ring-3 /bin/init (QEMU experimental)")!=2 ||
            occurrences(text,"IndianOS: ring-3 PMM frame count restored")!=2=>
            Err("two distinct complete user lifetimes were not observed".into()),
        "r025" if (1..=2).any(|pid|{
            occurrences(text,&format!("IndianOS: PID {pid} DISPATCHED"))!=3 ||
            occurrences(text,&format!("IndianOS: PID {pid} YIELDED"))!=2 ||
            occurrences(text,&format!("IndianOS: PID {pid} CAPABILITIES_REVOKED"))!=1 ||
            occurrences(text,&format!("IndianOS: PID {pid} REAPED"))!=1
        })=>Err("R025 rights teardown or process lifecycle incomplete".into()),
        "r026" if occurrences(text,"IndianOS: PID 1 REAPED")!=1 ||
            occurrences(text,"IndianOS: PID 2 REAPED")!=1 ||
            occurrences(text,"IndianOS: R026 /bin/worker launch granted to PID1")!=1 ||
            occurrences(text,"IndianOS user: indianos> ")!=7 ||
            text.find("IndianOS: PID 2 REAPED")>text.find("IndianOS: PID 1 EXITED")=>
            Err("R026 process launch/reap or persistent console lifecycle invalid".into()),
        "ipc" if (1..=2).any(|pid|{
            occurrences(text,&format!("IndianOS: PID {pid} DISPATCHED"))!=3 ||
            occurrences(text,&format!("IndianOS: PID {pid} YIELDED"))!=2 ||
            occurrences(text,&format!("IndianOS: PID {pid} REAPED"))!=1
        })=>Err("R025 two-process IPC did not preserve lifecycle".into()),
        "scheduler" if (1..=2).any(|pid|{
            occurrences(text,&format!("IndianOS: PID {pid} DISPATCHED"))!=3 ||
            occurrences(text,&format!("IndianOS: PID {pid} YIELDED"))!=2
        })=>Err("round robin did not dispatch/yield each task 3/2 times".into()),
        "preempt" if {
            let pa=text.find("IndianOS: PID 1 APIC_TIMER_PREEMPTED");
            let pb=text.find("IndianOS: PID 2 APIC_TIMER_PREEMPTED");
            let b=text.find("IndianOS: PID 2 DISPATCHED");
            let ea=text.find("IndianOS: PID 1 EXITED");
            let eb=text.find("IndianOS: PID 2 EXITED");
            match (pa,pb,b,ea,eb){
                (Some(pa),Some(pb),Some(b),Some(ea),Some(eb))=>
                    !(pa<b && b<ea && pb<ea && pa<eb && pb<eb),
                _=>true
            }
        }=>Err("both CPU-bound tasks must be preempted before either exits".into()),
        "processes"|"scheduler"|"preempt"|"ipc"|"r025"|"r026" if text.contains("IndianOS: PID 1 FAULTED") ||
              text.contains("IndianOS: PID 2 FAULTED")=>
            Err("successful task unexpectedly faulted".into()),
        _=>Ok(())
    }
}
fn option(args:&[String],key:&str)->Option<String>{
    args.windows(2).find(|v|v[0]==key).map(|v|v[1].clone())
}
struct TempVars { dir:PathBuf }
impl Drop for TempVars{
    fn drop(&mut self){let _=fs::remove_dir_all(&self.dir);}
}
fn matching_vars(code:&Path)->Option<PathBuf>{
    let name=code.file_name()?.to_str()?;
    let suffix=name.strip_prefix("OVMF_CODE")?;
    let vars=code.with_file_name(format!("OVMF_VARS{suffix}"));
    vars.is_file().then_some(vars)
}
// Kill and reap on every return path, including monitor/console errors.
struct QemuChild(std::process::Child);
impl std::ops::Deref for QemuChild {
    type Target=std::process::Child;
    fn deref(&self)->&Self::Target { &self.0 }
}
impl std::ops::DerefMut for QemuChild {
    fn deref_mut(&mut self)->&mut Self::Target { &mut self.0 }
}
impl Drop for QemuChild {
    fn drop(&mut self) { let _=self.0.kill(); let _=self.0.wait(); }
}
fn machine_argument(machine:&str,mode:&str)->String {
    // HPET legacy replacement can suppress PIT IRQ0 during calibration.
    if mode=="preempt" { format!("{machine},hpet=off") } else { machine.into() }
}
fn drain_stderr(mut source:impl Read,tail:&Mutex<Vec<u8>>) {
    let mut bytes=[0u8;4096];
    while let Ok(n)=source.read(&mut bytes) {
        if n==0 { break }
        let mut saved=tail.lock().unwrap();
        saved.extend_from_slice(&bytes[..n]);
        let excess=saved.len().saturating_sub(8192);
        saved.drain(..excess);
    }
}
pub fn run(args:&[String])->Result<String,String>{
    let iso=PathBuf::from(option(args,"--iso").ok_or("missing --iso")?);
    let ovmf=PathBuf::from(option(args,"--ovmf").ok_or("missing --ovmf")?);
    if !iso.is_file(){return Err(format!("ISO not found: {}",iso.display()))}
    if !ovmf.is_file(){return Err(format!("OVMF not found: {}",ovmf.display()))}
    let mode=option(args,"--mode").unwrap_or_else(||"normal".into());
    let machine=option(args,"--machine").unwrap_or_else(||"q35".into());
    if machine!="q35" && machine!="pc"{return Err("invalid QEMU machine".into())}
    let qemu=option(args,"--qemu").unwrap_or_else(||"qemu-system-x86_64".into());
    let timeout=option(args,"--timeout")
        .map(|s|s.parse::<f64>().map_err(|e|e.to_string()))
        .transpose()?.unwrap_or(25.0);
    if !timeout.is_finite() || timeout<=0.0 || timeout>180.0 {
        return Err("timeout must be between 0 and 180 seconds".into())
    }
    // Reject unknown modes before starting QEMU.
    if !["normal","exception","pagefault","shadow","usermode",
         "fault-guard","fault-supervisor","processes","scheduler","preempt","ipc","r025","shell","interactive","keyboard","r026"].contains(&mode.as_str()){
        return Err("invalid QEMU smoke mode".into())
    }

    let mut command=Command::new(qemu);
    command.args(["-machine",&machine_argument(&machine,&mode),"-m","512M"]);
    command.args(["-drive",&format!("if=pflash,unit=0,format=raw,file={},readonly=on",ovmf.display())]);
    command.args(["-cdrom",iso.to_str().ok_or("non-UTF8 ISO path")?,
                  "-boot","d","-display","none","-serial","stdio",
                  "-no-reboot","-no-shutdown"]);

    let mut tmp:Option<TempVars>=None;
    if let Some(template)=matching_vars(&ovmf){
        let nanos=SystemTime::now().duration_since(UNIX_EPOCH)
            .map_err(|e|e.to_string())?.as_nanos();
        let dir=std::env::temp_dir().join(format!("indianos-rust-qemu-{}-{nanos}",std::process::id()));
        fs::create_dir(&dir).map_err(|e|e.to_string())?;
        tmp=Some(TempVars{dir:dir.clone()});
        let to=dir.join(template.file_name().ok_or("invalid OVMF name")?);
        fs::copy(template,to.clone()).map_err(|e|e.to_string())?;
        command.args(["-drive",&format!("if=pflash,unit=1,format=raw,file={}",to.display())]);
    }
    if mode=="keyboard" && tmp.is_none(){
        let stamp=SystemTime::now().duration_since(UNIX_EPOCH)
            .map_err(|e|e.to_string())?.as_nanos();
        let dir=std::env::temp_dir().join(format!("indianos-r0142-{}-{stamp}",std::process::id()));
        fs::create_dir(&dir).map_err(|e|e.to_string())?;
        tmp=Some(TempVars{dir});
    }
    let monitor_path=if mode=="keyboard"{
        let path=tmp.as_ref().ok_or("missing QEMU monitor workspace")?.dir.join("monitor.sock");
        command.args(["-monitor",&format!("unix:{},server,nowait",path.display())]);
        Some(path)
    }else{
        command.args(["-monitor","none"]);None
    };
    let _cleanup=tmp;
    let mut child=QemuChild(command.stdin(if mode=="interactive"||mode=="r026"{Stdio::piped()}else{Stdio::null()}).stdout(Stdio::piped())
        .stderr(Stdio::piped()).spawn().map_err(|e|format!("cannot start QEMU: {e}"))?);
    let stderr_tail=Arc::new(Mutex::new(Vec::new()));
    let stderr=child.stderr.take().ok_or("no QEMU stderr")?;
    let saved=Arc::clone(&stderr_tail);
    let stderr_reader=thread::spawn(move || drain_stderr(stderr,&saved));
    let stdout=child.stdout.take().ok_or("no QEMU stdout")?;
    let mut stdin=if mode=="interactive"||mode=="r026"{child.stdin.take()}else{None};
    #[cfg(unix)]
    let mut monitor=if let Some(path)=monitor_path.as_ref(){
        let mut found=None;
        for _ in 0..60 {
            if let Ok(stream)=UnixStream::connect(path){found=Some(stream);break;}
            thread::sleep(Duration::from_millis(50));
        }
        Some(found.ok_or_else(||format!("unable to connect to QEMU HMP monitor; QEMU stderr: {}",
            String::from_utf8_lossy(&stderr_tail.lock().unwrap())))?)
    }else{None};
    #[cfg(unix)]
    if let Some(ref hmp)=monitor{
        hmp.set_nonblocking(true).map_err(|e|format!("HMP nonblocking: {e}"))?;
    }
    #[cfg(not(unix))]
    let mut monitor:Option<std::io::Sink>={
        if monitor_path.is_some(){return Err("keyboard mode requires Unix".into());}
        None
    };
    // Each QEMU UART RX FIFO has a finite depth: send one short command
    // only after the previous reply causes another visible shell prompt.
    let legacy_script:[&[u8];4]=[b"helpx\x7f\n",b"ps\n",b"run /bin/worker\n",b"exit\n"];
    let r026_script:[&[u8];7]=[b"help\n",b"ps\n",b"run /bin/worker\n",b"ps\n",b"wait /bin/worker\n",b"run /bin/worker\n",b"exit\n"];
    let script:&[&[u8]]=if mode=="r026"{&r026_script}else{&legacy_script};
    let mut sent=0usize;
    let (tx,rx)=mpsc::channel::<Vec<u8>>();
    let reader=thread::spawn(move || {
        let mut stdout=stdout;
        let mut bytes=[0u8;4096];
        while let Ok(n)=stdout.read(&mut bytes){
            if n==0 || tx.send(bytes[..n].to_vec()).is_err(){break}
        }
    });
    let deadline=Instant::now()+Duration::from_secs_f64(timeout);
    let mut output:Vec<u8>=Vec::new();
    let mut passed=false;
    while Instant::now()<deadline {
        match rx.recv_timeout(Duration::from_millis(200)){
            Ok(part)=>{
                output.extend(part);
                if output.len()>1024*1024 {break}
                if mode=="interactive" || mode=="keyboard" || mode=="r026"{
                    let prompts=occurrences(&String::from_utf8_lossy(&output),
                                            "IndianOS user: indianos> ");
                    while sent<prompts && sent<script.len(){
                        if mode=="interactive"||mode=="r026"{
                            let pipe=stdin.as_mut().ok_or("QEMU console stdin missing")?;
                            pipe.write_all(script[sent]).map_err(|e|format!("QEMU console send: {e}"))?;
                            pipe.flush().map_err(|e|format!("QEMU console flush: {e}"))?;
                        }else{
                            let hmp=monitor.as_mut().ok_or("QEMU monitor missing")?;
                            for &key in script[sent]{
                                let name=match key{
                                    b' '=>"spc",
                                    b'/'=>"slash",
                                    0x7f=>"backspace",
                                    b'\n'=>"ret",
                                    b'a'..=b'z'=>{
                                        // Only ASCII script keys; derived keys require
                                        // no spaces or shell metacharacters.
                                        let key_text=[key];
                                        let txt=core::str::from_utf8(&key_text).map_err(|e|e.to_string())?;
                                        hmp.write_all(format!("sendkey {txt} 30\n").as_bytes())
                                            .map_err(|e|format!("QEMU HMP key: {e}"))?;
                                        thread::sleep(Duration::from_millis(55));
                                        continue
                                    }
                                    _=>return Err("unsupported keyboard test key".into()),
                                };
                                hmp.write_all(format!("sendkey {name} 30\n").as_bytes())
                                    .map_err(|e|format!("QEMU HMP key: {e}"))?;
                                thread::sleep(Duration::from_millis(55));
                            }
                        }
                        if mode=="keyboard"{
                            let hmp=monitor.as_mut().ok_or("missing HMP connection")?;
                            let mut reply=[0u8;2048];
                            match hmp.read(&mut reply){
                                Ok(n) if n>0=>eprintln!("R014.2 HMP after command {sent}: {}",
                                    String::from_utf8_lossy(&reply[..n])),
                                Ok(_)=>{},
                                Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>{},
                                Err(e)=>return Err(format!("HMP monitor read failed: {e}")),
                            }
                        }
                        sent+=1;
                    }
                }
                if verify(&mode,&String::from_utf8_lossy(&output)).is_ok(){
                    passed=true;break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected)=>break,
            Err(mpsc::RecvTimeoutError::Timeout)=>{
                if child.try_wait().map_err(|e|e.to_string())?.is_some(){break}
            }
        }
    }
    let _=child.kill();
    let status=child.wait().ok();
    let _=reader.join();
    let _=stderr_reader.join();
    let text=String::from_utf8_lossy(&output).into_owned();
    if !passed {
        let reason=verify(&mode,&text).err().unwrap_or_else(||"QEMU timed out".into());
        // Never slice at an arbitrary UTF-8 byte boundary when firmware
        // logs contain non-ASCII data.
        let tail: String = text.chars().rev().take(8192).collect::<Vec<_>>()
            .into_iter().rev().collect();
        let diagnostics=if mode=="keyboard"{
            format!("R014.2 keyboard diagnostics: commands_sent={sent}/{}; prompts_seen={}; irq1_seen={}; ps2_pending={}; shell_exited={}\\n",
                script.len(),
                occurrences(&text,"IndianOS user: indianos> "),
                text.contains("IndianOS: R014.2 IRQ1 first interrupt delivered"),
                text.contains("IndianOS: R014.2 PS/2 data pending without IRQ1"),
                text.contains("IndianOS user: R014 shell: exit"))
        }else{String::new()};
        return Err(format!("{reason}\nQEMU exit status: {status:?}\n--- QEMU stderr (last 8192 bytes) ---\n{}\n{diagnostics}--- QEMU serial output ---\n{tail}",
            String::from_utf8_lossy(&stderr_tail.lock().unwrap())))
    }
    Ok(text)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn machine_property_preserves_ordinary_modes() {
        for machine in ["pc","q35"] {
            assert_eq!(machine_argument(machine,"preempt"),format!("{machine},hpet=off"));
            for mode in ["normal","keyboard","scheduler","r026"] {
                assert_eq!(machine_argument(machine,mode),machine);
            }
        }
    }
    #[test]
    #[cfg(unix)]
    fn startup_failure_retains_bounded_stderr_without_blocking() {
        use std::os::unix::fs::PermissionsExt;
        let stamp=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir=std::env::temp_dir().join(format!("indianos-smoke-test-{}-{stamp}",std::process::id()));
        fs::create_dir(&dir).unwrap();
        let _cleanup=TempVars{dir:dir.clone()};
        let firmware=dir.join("firmware");
        fs::write(&firmware,b"fixture").unwrap();
        let fixture=dir.join("fake-qemu");
        fs::write(&fixture,b"#!/usr/bin/python3\nimport os\nos.write(2, b'x' * 262144)\nos.write(2, b'QEMU fixture: invalid option\\n')\nraise SystemExit(23)\n").unwrap();
        fs::set_permissions(&fixture,fs::Permissions::from_mode(0o700)).unwrap();
        let args=vec!["--iso".into(),firmware.display().to_string(),
            "--ovmf".into(),firmware.display().to_string(),
            "--qemu".into(),fixture.display().to_string(),"--timeout".into(),"5".into()];
        let start=Instant::now();
        let error=run(&args).unwrap_err();
        assert!(start.elapsed()<Duration::from_secs(5));
        assert!(error.contains("QEMU fixture: invalid option"),"{error}");
        assert!(error.contains("QEMU exit status:"));
        assert!(error.len()<10000);
    }
    #[test]
    fn rejects_missing_and_out_of_order_markers(){
        assert!(verify("normal","").is_err());
        assert!(verify("processes","IndianOS: PID 1 REAPED").is_err());
        assert!(verify("scheduler","IndianOS: PID 2 DISPATCHED").is_err());
    }
    #[test]
    fn keyboard_mode_requires_real_irq_delivery_and_lifecycle(){
        // A shell transcript alone must never pass the hardware IRQ gate.
        let shell_only=MILESTONES.join("\\n");
        assert!(verify("keyboard",&shell_only).is_err());
        let with_fake_marker=format!("{shell_only}\\nIndianOS: R014.2 PS/2 IRQ1 delivery verified");
        assert!(verify("keyboard",&with_fake_marker).is_err());
    }
    #[test]
    fn milestones_order_and_fatal_detection(){
        let good=MILESTONES.join("\n");
        assert!(verify("normal",&good).is_ok());
        let invalid=format!("{good}\nIndianOS: FATAL late crash");
        assert!(verify("normal",&invalid).is_err());
    }
}
