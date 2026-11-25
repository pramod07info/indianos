#[path="../src/desktop_window.rs"]mod policy;
use policy::Manager;
fn initial()->[u8;64]{
 let mut b=[0u8;64];b[..4].copy_from_slice(b"IWM1");b[4..8].copy_from_slice(&1u32.to_le_bytes());
 b[8..10].copy_from_slice(&800u16.to_le_bytes());b[10..12].copy_from_slice(&600u16.to_le_bytes());b[12]=1;
 for (at,x,y) in [(16,45u16,75u16),(32,360,260)]{
  for (offset,value) in [(0,x),(2,y),(4,320),(6,240)]{b[at+offset..at+offset+2].copy_from_slice(&value.to_le_bytes());}
  b[at+8]=1;
 }
 b[48..50].copy_from_slice(&400u16.to_le_bytes());b[50..52].copy_from_slice(&300u16.to_le_bytes());b
}
fn pointer(x:u16,y:u16,left:bool)->[u8;6]{[b'M',x as u8,(x>>8) as u8,y as u8,(y>>8) as u8,2|left as u8]}
#[test]fn focus_minimize_restore_and_launcher_are_owned_policy(){
 let mut m=Manager::new(&initial()).unwrap();
 assert_eq!(m.control(b"K2"),Some(2));assert_eq!(m.control(b"Km"),Some(1));
 assert_eq!(m.wire()[40],2);assert_eq!(m.control(b"Kr"),Some(2));assert_eq!(m.wire()[40],1);
 assert_eq!(m.control(b"Ka"),Some(2));assert_eq!(m.wire()[13],1);
 assert_eq!(m.control(b"Ka"),Some(2));assert_eq!(m.wire()[13],0);
}
#[test]fn pointer_hit_test_and_title_drag_are_bounded(){
 let mut m=Manager::new(&initial()).unwrap();assert_eq!(m.control(&pointer(370,270,true)),Some(2));
 assert_eq!(m.control(&pointer(799,599,true)),Some(2));let b=m.wire();
 assert_eq!(u16::from_le_bytes([b[32],b[33]]),480);assert_eq!(u16::from_le_bytes([b[34],b[35]]),318);
 m.control(&pointer(799,599,false)).unwrap();let before=m.wire();
 m.control(&pointer(0,0,false)).unwrap();assert_eq!(&m.wire()[16..48],&before[16..48]);
}
#[test]fn malformed_and_foreign_controls_never_partially_change_state(){
 let mut m=Manager::new(&initial()).unwrap();let before=m.wire();
 for message in [&b"P0"[..],b"P3",b"Kq",b"EXIT",b"K",&pointer(800,0,true),&[b'M',0,0,0,0,4]]{
  assert_eq!(m.control(message),None);assert_eq!(m.wire(),before);
 }
 let mut bad=initial();bad[14]=1;assert!(Manager::new(&bad).is_none());
}
#[test]fn explicit_policy_transitions_cancel_drag_until_a_new_physical_press(){
 for control in [&[b'P',1][..],b"K1",b"K2",b"Km",b"Kr",b"Ka",b"Kh"]{
  let mut m=Manager::new(&initial()).unwrap();
  assert_eq!(m.control(&pointer(370,270,true)),Some(2));
  assert!(m.control(control).is_some());
  let policy=m.wire();
  assert!(m.control(&pointer(710,540,true)).is_some());
  assert_eq!(&m.wire()[16..48],&policy[16..48],"stale drag after {:?}",control);
  assert_eq!(m.front(),policy[12]);
 }
 let mut m=Manager::new(&initial()).unwrap();
 m.control(&pointer(370,270,true)).unwrap();m.control(&[b'P',1]).unwrap();
 m.control(&pointer(370,270,false)).unwrap();
 assert_eq!(m.control(&pointer(370,270,true)),Some(2));
 m.control(&pointer(400,300,true)).unwrap();
 let moved=m.wire();
 assert_eq!(u16::from_le_bytes([moved[32],moved[33]]),390);
 assert_eq!(u16::from_le_bytes([moved[34],moved[35]]),290);
}

#[test]
fn client_rows_clip_and_preserve_rear_pixels_through_transparency() {
    use policy::{compose_client_row, ClientSpan};
    let left = [Some(99), Some(1), Some(2), None];
    let right = [None, Some(0), Some(3), Some(99)];
    let spans = || [ClientSpan { x: -1, pixels: &left }, ClientSpan { x: 0, pixels: &right }];
    let mut row = [7; 3];
    compose_client_row(true, spans(), &mut row);
    assert_eq!(row, [1, 0, 3]);
    row.fill(7);
    compose_client_row(false, spans(), &mut row);
    assert_eq!(row, [1, 2, 3]);
}

#[test]
fn desktop_strip_translation_clips_and_ignores_minimized_client() {
    use policy::RowRead;
    let mut manager = Manager::new(&initial()).unwrap();
    assert_eq!(manager.client_origin(0), Some((54, 119)));
    assert_eq!(manager.client_read_span(0, (52, 119), 5, (302, 186)),
               Some(RowRead { x: 0, y: 0, count: 3, destination: 2 }));
    assert_eq!(manager.client_read_span(0, (354, 304), 7, (302, 186)),
               Some(RowRead { x: 300, y: 185, count: 2, destination: 0 }));
    assert_eq!(manager.client_read_span(0, (54, 305), 2, (302, 186)), None);
    assert_eq!(manager.control(b"Km"), Some(1));
    assert_eq!(manager.client_read_span(1, (369, 304), 3, (302, 186)), None);
}
