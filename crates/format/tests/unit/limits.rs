use super::*;use std::io::{Cursor,Write};
#[test]fn rejects_unsafe_paths(){let mut c=Cursor::new(Vec::new());{let mut z=zip::ZipWriter::new(&mut c);z.start_file("../outside.xml",zip::write::SimpleFileOptions::default()).unwrap();z.write_all(b"x").unwrap();z.finish().unwrap();}c.set_position(0);let mut z=zip::ZipArchive::new(c).unwrap();assert!(validate_zip_directory(&mut z,"Test").is_err());}
