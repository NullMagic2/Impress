//! The OS clipboard is authoritative. No in-process plain-text equality cache:
//! two unrelated selections containing the same words must not share formatting.
use editor::{ClipboardFragment, clipboard};

pub enum Content { Rich(ClipboardFragment), Text(String), Empty }
#[derive(serde::Serialize, serde::Deserialize)]
struct Envelope { version: u32, fragment: ClipboardFragment }

const MAX_TRANSFER:usize=128*1024*1024;
#[derive(Default)]
pub struct Captured {private:Option<Vec<u8>>,html:Option<Vec<u8>>,rtf:Option<Vec<u8>>,png:Option<Vec<u8>>,dib:Option<Vec<u8>>,text:Option<Vec<u8>>}
pub struct Prepared {private:Vec<u8>,html:Vec<u8>,rtf:Option<Vec<u8>>,png:Option<Vec<u8>>,dib:Option<Vec<u8>>,text:Vec<u8>}
/// CPU parsing, image conversion, fragment validation and serialization run on
/// a clipboard worker, outside both the OS clipboard lock and the UI borrow.
pub fn prepare(fragment:&ClipboardFragment)->Result<Prepared,String>{
    fragment.validate()?;
    let private=encode(fragment)?;
    let html=clipboard::html::wrap_cf_html(&clipboard::html::export(fragment));
    let rtf=clipboard::rtf::encode(fragment).ok().map(|mut data|{data.push(0);data});
    let text=fragment.plain_text.replace("\r\n","\n").replace('\n',"\r\n").encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect::<Vec<_>>();
    let bitmap=clipboard::bitmap::single_picture(fragment).map(|r|clipboard::bitmap::decode_image(&r.data)).transpose()?;
    let(png,dib)=if let Some(b)=bitmap{(Some(b.png()?),Some(b.dib_v5()?))}else{(None,None)};
    let size=private.len()+html.len()+text.len()+rtf.as_ref().map_or(0,Vec::len)+png.as_ref().map_or(0,Vec::len)+dib.as_ref().map_or(0,Vec::len);
    if size>MAX_TRANSFER{return Err("Combined clipboard representations exceed 128 MiB".into());}
    Ok(Prepared{private,html,rtf,png,dib,text})
}
pub fn decode_captured(c:Captured)->Result<Content,String>{
    if let Some(bytes)=c.private{return decode(&bytes).map(Content::Rich).ok_or_else(||"Invalid or unsupported Impress clipboard data. Use Text only for an explicit fallback.".into());}
    if let Some(bytes)=c.html{if let Ok(f)=clipboard::html::unwrap_cf_html(&bytes).and_then(clipboard::html::import){if !f.paragraphs.is_empty()||f.table.is_some()||!f.blocks.is_empty(){return Ok(Content::Rich(f));}}}
    if let Some(bytes)=c.rtf{let end=bytes.iter().rposition(|b|*b!=0).map(|i|i+1).unwrap_or(0);return clipboard::rtf::decode(&bytes[..end]).map(Content::Rich);}
    if let Some(bytes)=c.png{return clipboard::bitmap::decode_image(&bytes)?.fragment().map(Content::Rich);}
    if let Some(bytes)=c.dib{return clipboard::bitmap::decode_dib(&bytes)?.fragment().map(Content::Rich);}
    if let Some(bytes)=c.text{let units=bytes.chunks_exact(2).map(|c|u16::from_le_bytes([c[0],c[1]])).take_while(|u|*u!=0).collect::<Vec<_>>();return Ok(Content::Text(String::from_utf16_lossy(&units)));}
    Ok(Content::Empty)
}
#[cfg(windows)]
mod native {
    use super::*;
    use std::{ffi::c_void, ptr};
    use windows_sys::Win32::{
        Foundation::GlobalFree,
        System::{DataExchange::*, Memory::*},
        UI::Input::KeyboardAndMouse::GetActiveWindow,
    };
    const MAX: usize = 64 * 1024 * 1024;
    const UNICODE: u32 = 13;
    const PRIVATE: &str = "Impress.SemanticFragment.v3";
    fn format(name: &str) -> Result<u32, String> {
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let id = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
        if id == 0 { Err("Could not register a clipboard format".into()) } else { Ok(id) }
    }
    struct Open;
    impl Open {
        fn acquire() -> Result<Self, String> {
            // A non-null owner is required when EmptyClipboard is followed by
            // SetClipboardData. Never hold this guard across a modal dialog.
            let owner = unsafe { GetActiveWindow() };
            if owner.is_null() || unsafe { OpenClipboard(owner) } == 0 {
                return Err("The clipboard is busy. Try Copy or Paste again.".into());
            }
            Ok(Self)
        }
    }
    impl Drop for Open { fn drop(&mut self) { unsafe { CloseClipboard(); } } }
    struct Memory(*mut c_void);
    impl Memory {
        fn new(data: &[u8]) -> Result<Self, String> {
            if data.is_empty() || data.len() > MAX { return Err("Clipboard data exceeds the supported size".into()); }
            let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, data.len()) };
            if handle.is_null() { return Err("Not enough memory for the clipboard".into()); }
            let owned = Self(handle);
            let dst = unsafe { GlobalLock(handle) };
            if dst.is_null() { return Err("Could not lock clipboard memory".into()); }
            unsafe { ptr::copy_nonoverlapping(data.as_ptr(), dst.cast(), data.len()); GlobalUnlock(handle); }
            Ok(owned)
        }
        fn give(mut self, format: u32) -> Result<(), String> {
            if unsafe { SetClipboardData(format, self.0) }.is_null() {
                return Err("Windows could not accept the clipboard data".into());
            }
            self.0 = ptr::null_mut(); // ownership transferred to Windows
            Ok(())
        }
    }
    impl Drop for Memory { fn drop(&mut self) { if !self.0.is_null() { unsafe { GlobalFree(self.0); } } } }
    fn read(format: u32) -> Option<Vec<u8>> {
        let handle = unsafe { GetClipboardData(format) };
        if handle.is_null() { return None; }
        let size = unsafe { GlobalSize(handle) };
        if size == 0 || size > MAX { return None; }
        let src = unsafe { GlobalLock(handle) };
        if src.is_null() { return None; }
        let data = unsafe { std::slice::from_raw_parts(src.cast::<u8>(), size) }.to_vec();
        unsafe { GlobalUnlock(handle); }
        Some(data)
    }
    pub fn sequence()->u32 {unsafe{GetClipboardSequenceNumber()}}
    pub fn publish(prepared:&Prepared,expected_sequence:u32)->Result<(),String>{
        // All handle allocation/copies precede EmptyClipboard. These bounded
        // native ownership calls stay on the HWND thread, not the worker.
        let private=format(PRIVATE)?;let html_format=format("HTML Format")?;
        let rich=Memory::new(&prepared.private)?;let html=Memory::new(&prepared.html)?;let text=Memory::new(&prepared.text)?;
        let rtf=prepared.rtf.as_ref().map(|data|Ok::<_,String>((format("Rich Text Format")?,Memory::new(data)?))).transpose()?;
        let png=prepared.png.as_ref().map(|data|Ok::<_,String>((format("PNG")?,Memory::new(data)?))).transpose()?;
        let dib=prepared.dib.as_ref().map(|data|Memory::new(data)).transpose()?;
        let _clipboard=Open::acquire()?;
        if sequence()!=expected_sequence{return Err("Clipboard changed while Copy/Cut was preparing; nothing was published or deleted. Copy again.".into());}
        if unsafe{EmptyClipboard()}==0{return Err("Could not replace clipboard contents".into());}
        text.give(UNICODE)?;html.give(html_format)?;rich.give(private)?;
        if let Some((id,data))=rtf{data.give(id)?;}if let Some((id,data))=png{data.give(id)?;}if let Some(data)=dib{data.give(17)?;}
        Ok(())
    }
    pub fn capture(plain_only:bool)->Result<Captured,String>{
        let _clipboard=Open::acquire()?;
        let mut remaining=MAX_TRANSFER;
        fn take(id:u32,remaining:&mut usize)->Result<Option<Vec<u8>>,String>{
            if unsafe{IsClipboardFormatAvailable(id)}==0{return Ok(None);}
            let bytes=read(id).ok_or("Could not capture a present clipboard format (invalid, locked or over 64 MiB)")?;
            if bytes.len()>*remaining{return Err("Combined captured clipboard data exceeds 128 MiB".into());}
            *remaining-=bytes.len();Ok(Some(bytes))
        }
        let mut c=Captured::default();
        if !plain_only{
            c.private=take(format(PRIVATE)?,&mut remaining)?;
            // Present private data is authoritative. Do not allocate fallback
            // representations and never downgrade malformed current private data.
            if c.private.is_some(){return Ok(c);}
            c.html=take(format("HTML Format")?,&mut remaining)?;
            c.rtf=take(format("Rich Text Format")?,&mut remaining)?;
            c.png=take(format("PNG")?,&mut remaining)?;
            c.dib=take(17,&mut remaining)?;if c.dib.is_none(){c.dib=take(8,&mut remaining)?;}
        }
        c.text=take(UNICODE,&mut remaining)?;Ok(c)
    }

}
#[cfg(windows)] pub use native::{capture,publish,sequence};
#[cfg(not(windows))]pub fn sequence()->u32{0}
#[cfg(not(windows))]pub fn capture(_plain_only:bool)->Result<Captured,String>{
    let text=wxdragon::clipboard::Clipboard::get().get_text().map(|s|s.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect());
    Ok(Captured{text,..Default::default()})
}
#[cfg(not(windows))]pub fn publish(prepared:&Prepared,_expected:u32)->Result<(),String>{
    let units=prepared.text.chunks_exact(2).map(|c|u16::from_le_bytes([c[0],c[1]])).take_while(|u|*u!=0).collect::<Vec<_>>();
    if wxdragon::clipboard::Clipboard::get().set_text(&String::from_utf16_lossy(&units)){Ok(())}else{Err("Could not write clipboard".into())}
}

// GlobalSize may include arbitrary allocator padding. A bounded payload length
// makes semantic decoding independent of that padding and of JSON whitespace.
fn encode(fragment: &ClipboardFragment) -> Result<Vec<u8>, String> {
    let payload = serde_json::to_vec(&Envelope { version: 3, fragment: fragment.clone() }).map_err(|e| e.to_string())?;
    let mut bytes = b"IMPRCLP3".to_vec(); bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes()); bytes.extend(payload); Ok(bytes)
}
fn decode(bytes: &[u8]) -> Option<ClipboardFragment> {
    if bytes.get(..8)? != b"IMPRCLP3" { return None; }
    let len = usize::try_from(u64::from_le_bytes(bytes.get(8..16)?.try_into().ok()?)).ok()?;
    if len > 64 * 1024 * 1024 { return None; }
    let payload = bytes.get(16..16usize.checked_add(len)?)?;
    let envelope: Envelope = serde_json::from_slice(payload).ok()?;
    (envelope.version == 3 && envelope.fragment.validate().is_ok()).then_some(envelope.fragment)
}
#[cfg(test)] #[path = "../tests/unit/system_clipboard.rs"] mod tests;
