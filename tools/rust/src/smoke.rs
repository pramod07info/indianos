//! Bounded, unprivileged QEMU/OVMF serial smoke runner in Rust.
//! Replaces the Python host orchestration only after independent parity
//! checks. Never attaches physical disks: only an ISO and OVMF pflash.

use std::{
    fs,
    io::Read,
    path::{Path,PathBuf},
    process::{Command,Stdio},
    sync::mpsc,
    thread,
    time::{Duration,Instant,SystemTime,UNIX_EPOCH},
};

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
        "preempt"=>{
            checks.extend(MILESTONES);
            checks.extend(["IndianOS: M009 cooperative scheduler experiment begin",
                "IndianOS: M009 two simultaneous user address spaces ready",
                "IndianOS: M009 hardware PIT IRQ0 enabled at 100 Hz"]);
            // Preemption events are asynchronous; the original harness
            // checks presence of markers for both tasks rather than a
            // specific ordering.
            for (pid,name) in [(1,"A"),(2,"B")] {
                for marker in [
                    format!("IndianOS: PID {pid} READY"),
                    format!("IndianOS: PID {pid} DISPATCHED"),
                    format!("IndianOS: PID {pid} TIMER_PREEMPTED"),
                    format!("IndianOS user: M009 timer task {name} started"),
                    format!("IndianOS user: M009 timer task {name} finished"),
                    format!("IndianOS: PID {pid} EXITED"),
                    format!("IndianOS: PID {pid} REAPED"),
                    format!("IndianOS: PID {pid} YIELDED"),
                ]{
                    if !text.contains(&marker){return Err(format!("missing {marker}"))}
                }
            }
            for marker in ["IndianOS: M009 timer-preemptive round-robin PASSED",
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
        "processes" if occurrences(text,"IndianOS: entering ring-3 /bin/init (QEMU experimental)")!=2 ||
            occurrences(text,"IndianOS: ring-3 PMM frame count restored")!=2=>
            Err("two distinct complete user lifetimes were not observed".into()),
        "scheduler" if (1..=2).any(|pid|{
            occurrences(text,&format!("IndianOS: PID {pid} DISPATCHED"))!=3 ||
            occurrences(text,&format!("IndianOS: PID {pid} YIELDED"))!=2
        })=>Err("round robin did not dispatch/yield each task 3/2 times".into()),
        "preempt" if occurrences(text,"IndianOS: PID 1 TIMER_PREEMPTED")==0 ||
            occurrences(text,"IndianOS: PID 2 TIMER_PREEMPTED")==0 ||
            text.find("IndianOS: PID 2 DISPATCHED")>text.find("IndianOS: PID 1 EXITED")=>
            Err("CPU-bound tasks were not preempted".into()),
        "processes"|"scheduler"|"preempt" if text.contains("IndianOS: PID 1 FAULTED") ||
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
         "fault-guard","fault-supervisor","processes","scheduler","preempt"].contains(&mode.as_str()){
        return Err("invalid QEMU smoke mode".into())
    }

    let mut command=Command::new(qemu);
    command.args(["-machine",&machine,"-m","512M"]);
    command.args(["-drive",&format!("if=pflash,unit=0,format=raw,file={},readonly=on",ovmf.display())]);
    command.args(["-cdrom",iso.to_str().ok_or("non-UTF8 ISO path")?,
                  "-boot","d","-display","none","-serial","stdio",
                  "-monitor","none","-no-reboot","-no-shutdown"]);
    if mode=="preempt"{command.arg("-no-hpet");}
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
    let _cleanup=tmp;
    let mut child=command.stdin(Stdio::null()).stdout(Stdio::piped())
        .stderr(Stdio::null()).spawn().map_err(|e|format!("cannot start QEMU: {e}"))?;
    let stdout=child.stdout.take().ok_or("no QEMU stdout")?;
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
    let _=child.wait();
    let _=reader.join();
    let text=String::from_utf8_lossy(&output).into_owned();
    if !passed {
        let reason=verify(&mode,&text).err().unwrap_or_else(||"QEMU timed out".into());
        let tail=if text.len()>8192{&text[text.len()-8192..]}else{&text};
        return Err(format!("{reason}\n--- QEMU serial output ---\n{tail}"))
    }
    Ok(text)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn rejects_missing_and_out_of_order_markers(){
        assert!(verify("normal","").is_err());
        assert!(verify("processes","IndianOS: PID 1 REAPED").is_err());
        assert!(verify("scheduler","IndianOS: PID 2 DISPATCHED").is_err());
    }
    #[test]
    fn milestones_order_and_fatal_detection(){
        let good=MILESTONES.join("\n");
        assert!(verify("normal",&good).is_ok());
        let invalid=format!("{good}\nIndianOS: FATAL late crash");
        assert!(verify("normal",&invalid).is_err());
    }
}
