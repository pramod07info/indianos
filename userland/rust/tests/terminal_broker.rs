#[path="../src/terminal_broker.rs"]mod terminal_broker;
use terminal_broker::{Decision,Exchange};
#[test]fn filesystem_request_requires_enter_and_is_bounded_to_one_per_transaction(){
 for key in 0..=u8::MAX{
  for begin in [false,true]{
   let mut turn=Exchange::new(key,begin);
   let allowed=!begin&&matches!(key,b'\r'|b'\n');
   assert_eq!(turn.observe(b"Wunrequested"),Ok(if allowed{Decision::Dispatch}else{Decision::Denied}));
   assert_eq!(turn.observe(b"L"),Err(()));
   assert_eq!(turn.observe(b"A"),Ok(Decision::Complete(true)));
   assert_eq!(turn.observe(b"A"),Err(()));
   assert_eq!(turn.observe(b"Wreplayed"),Err(()));
  }
 }
}
#[test]fn cancellation_requires_tab_and_failed_request_cannot_be_replayed(){
 for (key,begin,allowed) in [(b'\t',false,true),(b'\t',true,false),
                            (b'\n',false,false),(0,false,false),(b'x',false,false)]{
  assert_eq!(Exchange::new(key,begin).observe(b"C"),
             if allowed{Ok(Decision::Complete(false))}else{Err(())});
 }
 let mut enter=Exchange::new(b'\n',false);
 assert_eq!(enter.observe(b"Fmalformed"),Ok(Decision::Dispatch));
 // The filesystem codec may reject it and reply E. A valid replacement or
 // duplicate cannot then reach production VFS in the same Enter transaction.
 assert_eq!(enter.observe(b"Wreplacement"),Err(()));
 let mut next=Exchange::new(b'\n',false);
 assert_eq!(next.observe(b"R"),Ok(Decision::Dispatch));
 assert_eq!(next.observe(b"A"),Ok(Decision::Complete(true)));
}
