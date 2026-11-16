//! Output backends consume final page commands, never a canvas screenshot.
//! Review cards, spelling marks, selection geometry and handles are not output.
pub mod pdf;
pub mod raster;
pub mod links;
use std::collections::BTreeSet;
/// Parse 1-based inclusive ranges. Explicit ranges are sorted and deduplicated.
pub fn page_ranges(input:&str,total:usize)->Result<Vec<usize>,String>{
 if total==0 || total>10_000{return Err("Output requires 1–10,000 pages".into());}
 if input.trim().is_empty(){return Ok((0..total).collect());}
 if input.len()>8192{return Err("Page range is too long".into());}
 let mut set=BTreeSet::new();
 for part in input.split(','){let p=part.trim();let mut pair=p.split('-');
  let first=pair.next().unwrap_or("").trim().parse::<usize>().map_err(|_|"Use page numbers or ranges such as 1,3-5")?;
  let last=match pair.next(){Some(p)=>p.trim().parse::<usize>().map_err(|_|"Invalid page range")?,None=>first};
  if pair.next().is_some()||first==0||last<first||last>total{return Err("Page range is outside this document".into());}
  set.extend((first-1)..last);
 }Ok(set.into_iter().collect())
}
