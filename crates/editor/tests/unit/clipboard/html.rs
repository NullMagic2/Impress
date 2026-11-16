use super::*;
#[test]fn scripts_and_remote_images_are_not_loaded(){let f=import("<p>x<script>secret()</script><img src='file:///private' alt='photo'></p>").unwrap();assert_eq!(f.plain_text,"xphoto");assert!(f.resources.is_empty());}
