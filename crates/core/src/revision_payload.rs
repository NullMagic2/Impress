//! Immutable shared journal payloads. Serializing the complete older journal on
//! every keystroke was quadratic in history size. Payload byte counts are cached
//! once, and event metadata is counted without allocating another JSON buffer.
use std::{io::Write, sync::{Arc,OnceLock}};
use serde::{Serialize,Serializer,Deserialize,Deserializer};

#[derive(Debug)]
struct Data { value:serde_json::Value, encoded_bytes:OnceLock<usize> }
#[derive(Clone,Debug)]
pub struct RevisionPayload(Arc<Data>);
impl From<serde_json::Value> for RevisionPayload {
    fn from(value:serde_json::Value)->Self { Self(Arc::new(Data{value,encoded_bytes:OnceLock::new()})) }
}
impl std::ops::Deref for RevisionPayload {
    type Target=serde_json::Value;
    fn deref(&self)->&Self::Target{&self.0.value}
}
impl PartialEq for RevisionPayload {
    fn eq(&self,other:&Self)->bool{Arc::ptr_eq(&self.0,&other.0)||self.0.value==other.0.value}
}
impl Serialize for RevisionPayload {
    fn serialize<S:Serializer>(&self,s:S)->Result<S::Ok,S::Error>{self.0.value.serialize(s)}
}
impl<'de> Deserialize<'de> for RevisionPayload {
    fn deserialize<D:Deserializer<'de>>(d:D)->Result<Self,D::Error>{serde_json::Value::deserialize(d).map(Self::from)}
}
#[derive(Default)]
struct Counter(usize);
impl Write for Counter {
    fn write(&mut self,b:&[u8])->std::io::Result<usize>{self.0=self.0.checked_add(b.len()).ok_or_else(||std::io::Error::other("JSON size overflow"))?;Ok(b.len())}
    fn flush(&mut self)->std::io::Result<()>{Ok(())}
}
pub(crate) fn count<T:Serialize>(value:&T)->Result<usize,serde_json::Error>{let mut c=Counter::default();serde_json::to_writer(&mut c,value)?;Ok(c.0)}
impl RevisionPayload {
    pub fn encoded_len(&self)->Result<usize,serde_json::Error>{
        if let Some(n)=self.0.encoded_bytes.get(){return Ok(*n);}
        let bytes=count(&self.0.value)?;let _=self.0.encoded_bytes.set(bytes);Ok(bytes)
    }
    pub fn shares_storage(&self,other:&Self)->bool{Arc::ptr_eq(&self.0,&other.0)}
}
impl crate::RevisionEvent {
    pub fn encoded_len(&self)->Result<usize,serde_json::Error>{
        #[derive(Serialize)]struct Header<'a>{id:&'a crate::NodeId,author:&'a str,date:&'a str,label:&'a str,status:crate::RevisionStatus,payload:()}
        let header=Header{id:&self.id,author:&self.author,date:&self.date,label:&self.label,status:self.status,payload:()};
        // The stand-in payload serializes to the four bytes `null`.
        Ok(count(&header)?-4+self.payload.encoded_len()?)
    }
}
/// Exact compact JSON array length without reserializing historical payloads.
pub fn revision_history_bytes(events:&[crate::RevisionEvent])->Result<usize,serde_json::Error>{
    let mut bytes=2+events.len().saturating_sub(1);
    for event in events{bytes+=event.encoded_len()?;}
    Ok(bytes)
}
