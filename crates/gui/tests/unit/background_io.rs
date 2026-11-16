use super::*;
 #[test]fn stale_saves_and_uncommitted_input_never_authorize_close(){
  assert!(close_after_save(12,12,false,false,false));assert!(!close_after_save(12,13,true,false,false));
  assert!(!close_after_save(12,12,true,false,false));assert!(!close_after_save(12,12,false,true,false));assert!(!close_after_save(12,12,false,false,true));
 }
