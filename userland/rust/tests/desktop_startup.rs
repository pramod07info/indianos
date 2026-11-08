#[path="../src/desktop_startup.rs"]
mod desktop_startup;
use desktop_startup::Startup;
fn packet(sender:u32,payload:&[u8])->Vec<u8>{
 let mut result=sender.to_le_bytes().to_vec();
 result.extend_from_slice(&(payload.len() as u32).to_le_bytes());
 result.extend_from_slice(payload);result
}
#[test]fn ready_requires_all_services_in_any_arrival_order(){
 for order in [[2,3,4],[2,4,3],[3,2,4],[3,4,2],[4,2,3],[4,3,2]]{
  let mut join=Startup::default();
  for (index,pid) in order.into_iter().enumerate(){
   let name:&[u8]=match pid{2=>b"FILES",3=>b"TERMINAL",_=>b"COMPOSITOR"};
   assert_eq!(join.observe(&packet(pid,name)),Ok(index==2));
  }
 }
}
#[test]fn missing_duplicate_forged_and_malformed_handshakes_cannot_complete_join(){
 let mut join=Startup::default();
 assert_eq!(join.observe(&packet(2,b"FILES")),Ok(false));
 assert_eq!(join.observe(&packet(2,b"FILES")),Err(()));
 for bad in [vec![],vec![0;8],packet(1,b"COMPOSITOR"),packet(4,b"FILES"),
             packet(3,b"TERMINAL\0")]{assert_eq!(join.observe(&bad),Err(()));}
 let mut bad=packet(3,b"TERMINAL");bad[4]=255;
 assert_eq!(join.observe(&bad),Err(()));
 assert_eq!(join.observe(&packet(4,b"COMPOSITOR")),Ok(false));
 assert_eq!(join.observe(&packet(3,b"TERMINAL")),Ok(true));
}
