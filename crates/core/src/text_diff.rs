//! Bounded grapheme diff shared by review UI and DOCX output. Offsets remain
//! UTF-8 bytes; a combining sequence or emoji cluster is never cut in half.
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct TextChange { pub old:Range<usize>, pub new:Range<usize> }
pub fn text_changes(old:&str,new:&str)->Vec<TextChange>{
    if old==new{return Vec::new();}
    let a:Vec<_>=old.grapheme_indices(true).collect();let b:Vec<_>=new.grapheme_indices(true).collect();
    let mut prefix=0;while prefix<a.len().min(b.len())&&a[prefix].1==b[prefix].1{prefix+=1;}
    let mut suffix=0;while suffix<a.len()-prefix&&suffix<b.len()-prefix&&a[a.len()-1-suffix].1==b[b.len()-1-suffix].1{suffix+=1;}
    let ai=|i:usize|a.get(i).map(|v|v.0).unwrap_or(old.len());let bi=|i:usize|b.get(i).map(|v|v.0).unwrap_or(new.len());
    let n=a.len()-prefix-suffix;let m=b.len()-prefix-suffix;
    // Keep worst-case memory/work bounded. A large replacement remains a single
    // accurately bounded change, not a fabricated character history.
    if (n+1).checked_mul(m+1).is_none_or(|size|size>2_000_000){return vec![TextChange{old:ai(prefix)..ai(prefix+n),new:bi(prefix)..bi(prefix+m)}];}
    let stride=m+1;let mut lcs=vec![0u32;(n+1)*stride];
    for i in (0..n).rev(){for j in (0..m).rev(){lcs[i*stride+j]=if a[prefix+i].1==b[prefix+j].1{1+lcs[(i+1)*stride+j+1]}else{lcs[(i+1)*stride+j].max(lcs[i*stride+j+1])};}}
    let(mut i,mut j)=(0,0);let mut active=None;let mut out=Vec::new();
    while i<n||j<m{
        if i<n&&j<m&&a[prefix+i].1==b[prefix+j].1{
            if let Some((start_i,start_j))=active.take(){out.push(TextChange{old:ai(prefix+start_i)..ai(prefix+i),new:bi(prefix+start_j)..bi(prefix+j)});}
            i+=1;j+=1;
        }else{
            active.get_or_insert((i,j));
            if j<m&&(i==n||lcs[i*stride+j+1]>lcs[(i+1)*stride+j]){j+=1;}else{i+=1;}
        }
    }
    if let Some((si,sj))=active{out.push(TextChange{old:ai(prefix+si)..ai(prefix+i),new:bi(prefix+sj)..bi(prefix+j)});}
    out
}
/// UTC timestamp, without depending on the UI's locale or time zone.
pub fn utc_timestamp()->String {
    let secs=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    timestamp_from_seconds(secs)
}
fn timestamp_from_seconds(secs:u64)->String {
    // Gregorian civil date from days since 1970, valid for the practical range
    // of system timestamps. Clamp pathological clocks to year 9999.
    let secs=secs.min(253402300799);let days=(secs/86400)as i64;let z=days+719468;
    let era=if z>=0{z}else{z-146096}/146097;let doe=z-era*146097;
    let yoe=(doe-doe/1460+doe/36524-doe/146096)/365;let mut y=yoe+era*400;
    let doy=doe-(365*yoe+yoe/4-yoe/100);let mp=(5*doy+2)/153;
    let day=doy-(153*mp+2)/5+1;let month=mp+if mp<10{3}else{-9};y+=i64::from(month<=2);
    format!("{y:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",secs/3600%24,secs/60%60,secs%60)
}
#[cfg(test)] #[path = "../tests/unit/text_diff.rs"] mod tests;
