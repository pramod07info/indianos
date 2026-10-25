//! R036 Batch 2: host-only cross-module acceptance for the desktop process path.
//! QEMU screenshot capture, real ring-3 tasks and physical keyboard wiring are
//! still required before this is considered an end-to-end running desktop.
#[cfg(test)]
mod tests {
    use crate::{
        batch2_init::{InitPlan,Service},
        batch2_launcher::{Image,Launch,LaunchQueue},
        batch2_events::Broker,
        batch2_windows::{Windows,Window,Rect},
        batch2_keyboard::{Router,Focus},
        batch2_terminal::{Terminal,Command},
        batch2_files::View,
        batch2_installer::{Transaction,Proposal,Phase},
        batch2_settings::{Store,Settings,Theme},
        batch2_gui_qa::compare,
    };
    #[test]
    fn boot_desktop_and_user_interaction_policy_path() {
        let mut init=InitPlan::<3>::new();
        init.register(Service{id:1,dependency:None,essential:true}).unwrap();
        init.register(Service{id:2,dependency:Some(1),essential:true}).unwrap();
        assert_eq!(init.next().unwrap().unwrap().id,1);
        init.ready(1).unwrap();
        assert_eq!(init.next().unwrap().unwrap().id,2);
        init.ready(2).unwrap();

        let mut launcher=LaunchQueue::<2>::new();
        launcher.stage(Launch{pid:10,generation:1,image:Image{
            image_id:7,entry:0x1000,mapped_end:0x2000,verified:true
        }}).unwrap();
        assert_eq!(launcher.take(10,1).unwrap().image.image_id,7);

        let mut windows=Windows::<2>::new();
        windows.create(Window{pid:10,generation:1,id:5,
            rect:Rect{x:0,y:0,w:160,h:100}}).unwrap();
        let mut router=Router::<4>::new();
        router.set_focus(&windows,Focus{pid:10,generation:1,window:5}).unwrap();
        router.physical_key(&windows,0x26,true).unwrap();
        assert_eq!(router.receive(10,1).unwrap().scan_code,0x26);

        let mut terminal=Terminal::new();
        for &byte in b"ls"{terminal.type_ascii(byte).unwrap();}
        assert_eq!(terminal.submit(),Ok(Command::ListFiles));
        let mut files=View::<2>::new(10,1).unwrap();
        files.add_from_vfs(b"readme",128,false).unwrap();
        let mut page=[None;2];
        assert_eq!(files.page(10,1,0,&mut page),Ok(1));
        assert_eq!(page[0].unwrap().bytes,128);

        let mut messages=Broker::<2>::new();
        messages.send(1,10,1,7,b"ready").unwrap();
        assert_eq!(messages.receive(10,1).unwrap().kind,7);

        let mut settings=Store::<2>::new();
        settings.register(10,1,Settings{
            theme:Theme::Dark,pointer_speed:5,key_repeat:12}).unwrap();
        assert_eq!(settings.read(10,1).unwrap().theme,Theme::Dark);
        let mut install=Transaction::new();
        install.stage(Proposal{app_id:1,manifest_verified:true,
            payload_verified:true,permissions:1,architecture_ok:true}).unwrap();
        install.approve(1,true).unwrap();
        assert_eq!(install.commit_policy(),Ok(1));
        assert_eq!(install.phase(),Phase::Committed);
        assert_eq!(compare(&[2,3,4,5],&[2,3,4,5],2,2,2)
            .unwrap().differing_pixels,0);
    }
    #[test]
    fn faulted_process_does_not_inherit_old_desktop_events(){
        let mut windows=Windows::<1>::new();
        windows.create(Window{pid:10,generation:1,id:5,
            rect:Rect{x:0,y:0,w:10,h:10}}).unwrap();
        let mut router=Router::<2>::new();
        router.set_focus(&windows,Focus{pid:10,generation:1,window:5}).unwrap();
        router.physical_key(&windows,1,true).unwrap();
        let mut broker=Broker::<2>::new();
        broker.send(1,10,1,1,b"old").unwrap();
        router.retire(10,1);
        broker.retire(10,1);
        windows.retire(10,1);
        windows.create(Window{pid:10,generation:2,id:5,
            rect:Rect{x:0,y:0,w:10,h:10}}).unwrap();
        assert!(router.receive(10,2).is_err());
        assert!(broker.receive(10,2).is_err());
        assert!(!windows.owns(10,1,5));
        assert!(windows.owns(10,2,5));
    }
}
