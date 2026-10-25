//! R037 Batch 3 cross-module safety assertions. Host-side only.
//! This must not be confused with a real AHCI/NVMe/ACPI/TLS hardware test.
#[cfg(test)]
mod tests{
    use crate::{
        batch3_driver_isolation::Drivers,
        batch3_ahci::{validate_command,Region},
        batch3_nvme::{Queue,Request},
        batch3_virtio::{validate_chain,Descriptor,NEXT,WRITE},
        batch3_acpi::parse_rsdp,
        batch3_network_security::{parse_lease,TcpHandshake,TcpState},
        batch3_updates::{Update,Candidate,Stage},
        batch3_crashlog::CrashLog,
        batch3_hardware_matrix::{qualify,Evidence,Platform,REQUIRED},
        batch3_security_audit::{audit,Finding,Control,ResultState},
    };
    #[test]
    fn bounded_device_io_network_update_and_release_require_consistent_evidence(){
        let mut drivers=Drivers::<2>::new();
        drivers.allocate(10,1,0x1000,0x1000,7).unwrap();
        assert!(drivers.validate(10,1,0x1000,4096).is_ok());
        assert_eq!(validate_command(0,8,128,&[
            Region{dma_address:0x8000,byte_count:4096}]),Ok(4096));
        let mut nvme=Queue::<2>::new(1);
        let request=Request{command_id:2,namespace_id:1,lba:0,blocks:8,dma_address:0x8000};
        nvme.submit(1,request,128).unwrap();
        assert_eq!(nvme.complete(1,2),Ok(request));
        assert_eq!(validate_chain(&[
            Descriptor{address:0x8000,length:512,flags:NEXT|WRITE,next:1},
            Descriptor{address:0x9000,length:512,flags:WRITE,next:0}
        ],0,true),Ok(1024));
        // No RSDP mapping exists in this host test.
        assert!(parse_rsdp(&[0u8;20]).is_err());

        let lease=[99,130,83,99,1,4,255,255,255,0,3,4,10,0,0,1,
            51,4,0,0,0,30,255];
        assert_eq!(parse_lease([10,0,0,4],&lease).unwrap().lease_seconds,30);
        let mut tcp=TcpHandshake::new();
        tcp.syn(100).unwrap();tcp.syn_ack(101,true,true).unwrap();
        assert_eq!(tcp.state(),TcpState::Established);

        let mut update=Update::new(10,10);
        update.stage(Candidate{image_id:1,version:11,verified_signature:true,
            payload_digest_verified:true,target_arch:62},62).unwrap();
        update.prepared(true,2).unwrap();
        update.attempt().unwrap();
        update.confirm(true).unwrap();
        assert_eq!(update.state(),Stage::Confirmed);
        let mut crashes=CrashLog::<2>::new();
        crashes.record(1,1,14,0,123).unwrap();
        assert_eq!(crashes.get(0).unwrap().vector,14);

        let sha=[1u8;20];
        assert_eq!(qualify(Evidence{platform:Platform::QemuX86_64,
            commit:sha,passed:REQUIRED,failed:0,skipped:0},
            Platform::QemuX86_64,sha),Ok(()));
        let controls=[Control::MemorySafety,Control::ProcessIsolation,
            Control::PackageTrust,Control::DriverIsolation,
            Control::UpdateRollback,Control::DependencyPolicy,
            Control::MainQemuBuild];
        let findings=core::array::from_fn::<Finding,7,_>(|i|Finding{
            control:controls[i],revision:sha,state:ResultState::Passed});
        assert_eq!(audit(&findings,sha,true),Ok(()));
        assert!(audit(&findings,sha,false).is_err());
        drivers.quarantine(10,1).unwrap();
        assert!(drivers.validate(10,1,0x1000,512).is_err());
    }
}
