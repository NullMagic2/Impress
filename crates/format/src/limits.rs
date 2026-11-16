//! Bounded package parsing. These are document limits, not ZIP extraction rules.
use crate::{FormatError,FormatResult};
use std::{collections::BTreeSet,io::{Read,Seek}};
pub const MAX_PACKAGE_MEMBER:u64=128*1024*1024;
pub const MAX_XML_MEMBER:u64=32*1024*1024;
pub const MAX_PACKAGE_TOTAL:u64=512*1024*1024;
pub fn check_member_len(len:usize,format:&'static str)->FormatResult<()> {
    if len as u64>MAX_PACKAGE_MEMBER {return Err(FormatError::Invalid{format,message:"Package member exceeds 128 MiB".into()});}Ok(())
}
pub fn validate_zip_directory<R:Read+Seek>(zip:&mut zip::ZipArchive<R>,format:&'static str)->FormatResult<()> {
    let invalid=|message:&str|FormatError::Invalid{format,message:message.into()};
    if zip.len()>8192{return Err(invalid("Package has too many entries"));}
    let mut names=BTreeSet::new();let mut total=0u64;
    for i in 0..zip.len(){
        let entry=zip.by_index(i).map_err(|e|invalid(&e.to_string()))?;
        let name=entry.name();
        if name.is_empty()||name.starts_with('/')||name.contains(['\\',':','\0'])||name.split('/').any(|part|part==".."||part==".")||!names.insert(name.to_owned()) {
            return Err(invalid("Package contains duplicate or unsafe member names"));
        }
        if entry.size()>MAX_PACKAGE_MEMBER{return Err(invalid("Package member exceeds 128 MiB"));}
        total=total.checked_add(entry.size()).ok_or_else(||invalid("Package size overflow"))?;
        if total>MAX_PACKAGE_TOTAL{return Err(invalid("Expanded package exceeds 512 MiB"));}
    }Ok(())
}
#[cfg(test)] #[path = "../tests/unit/limits.rs"] mod tests;
