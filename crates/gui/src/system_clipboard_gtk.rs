//! Native GTK clipboard exchange, shared by X11 and Wayland. Reads are async:
//! waiting for another application must never re-enter a borrowed CanvasState.
use super::*;
use std::{ffi::{c_char,c_void,CStr},ptr,sync::mpsc};
type Clipboard=*mut c_void;
type Atom=*mut c_void;
#[repr(C)] struct Target {name:*mut c_char,flags:u32,info:u32}
#[link(name="gtk-3")]
unsafe extern "C" {
    fn gtk_clipboard_get(selection:Atom)->Clipboard;
    fn gtk_clipboard_request_targets(c:Clipboard,callback:unsafe extern "C" fn(Clipboard,*mut Atom,i32,*mut c_void),data:*mut c_void);
    fn gtk_clipboard_request_contents(c:Clipboard,target:Atom,callback:unsafe extern "C" fn(Clipboard,*mut c_void,*mut c_void),data:*mut c_void);
    fn gtk_clipboard_request_text(c:Clipboard,callback:unsafe extern "C" fn(Clipboard,*const c_char,*mut c_void),data:*mut c_void);
    fn gtk_clipboard_set_with_data(c:Clipboard,targets:*const Target,count:u32,
        get:unsafe extern "C" fn(Clipboard,*mut c_void,u32,*mut c_void),clear:unsafe extern "C" fn(Clipboard,*mut c_void),data:*mut c_void)->i32;
    fn gtk_clipboard_set_can_store(c:Clipboard,targets:*const Target,count:i32);
    fn gtk_clipboard_store(c:Clipboard);
    fn gtk_selection_data_get_target(selection:*const c_void)->Atom;
    fn gtk_selection_data_get_length(selection:*const c_void)->i32;
    fn gtk_selection_data_get_data(selection:*const c_void)->*const u8;
    fn gtk_selection_data_set(selection:*mut c_void,target:Atom,format:i32,data:*const u8,length:i32);
    fn gtk_selection_data_set_text(selection:*mut c_void,text:*const c_char,length:i32)->i32;
}
#[link(name="gdk-3")] unsafe extern "C" {fn gdk_atom_intern_static_string(name:*const c_char)->Atom;}
#[link(name="glib-2.0")] unsafe extern "C" {fn g_idle_add(callback:unsafe extern "C" fn(*mut c_void)->i32,data:*mut c_void)->u32;}
const PRIVATE:&CStr=c"application/x-impress-fragment-v3";
const HTML:&CStr=c"text/html";
const RTF:&CStr=c"text/rtf";
const PNG:&CStr=c"image/png";
fn clipboard()->Result<Clipboard,String>{
    let c=unsafe{gtk_clipboard_get(gdk_atom_intern_static_string(c"CLIPBOARD".as_ptr()))};
    if c.is_null(){Err("The system clipboard is unavailable.".into())}else{Ok(c)}
}

struct Request {sender:mpsc::Sender<Result<Captured,String>>,captured:Captured,format:u32,plain:bool}
pub fn capture(plain_only:bool)->Result<Captured,String>{
    let c=clipboard()?;let(sender,receiver)=mpsc::channel();
    let request=Box::new(Request{sender,captured:Captured::default(),format:0,plain:plain_only});
    unsafe{gtk_clipboard_request_targets(c,targets,Box::into_raw(request).cast());}
    Ok(Captured{pending:Some(receiver),..Default::default()})
}
unsafe extern "C" fn targets(c:Clipboard,atoms:*mut Atom,count:i32,data:*mut c_void){unsafe{
    let mut request=Box::from_raw(data.cast::<Request>());
    let available=if atoms.is_null()||count<=0{&[][..]}else{std::slice::from_raw_parts(atoms,count as usize)};
    if !request.plain {
        for (name,format) in [(PRIVATE,0),(HTML,1),(RTF,2),(c"application/rtf",2),(PNG,3)] {
            let atom=gdk_atom_intern_static_string(name.as_ptr());
            if available.contains(&atom){
                request.format=format;
                gtk_clipboard_request_contents(c,atom,contents,Box::into_raw(request).cast());return;
            }
        }
    }
    gtk_clipboard_request_text(c,text,Box::into_raw(request).cast());
}}
unsafe extern "C" fn contents(c:Clipboard,selection:*mut c_void,data:*mut c_void){unsafe{
    let mut request=Box::from_raw(data.cast::<Request>());
    let length=gtk_selection_data_get_length(selection);
    let bytes=gtk_selection_data_get_data(selection);
    if length<0 || bytes.is_null(){
        if request.format==0 {let _=request.sender.send(Err("Could not read the Impress clipboard fragment. Try Paste again.".into()));return;}
    }else{
        if length as usize>MAX_TRANSFER {let _=request.sender.send(Err("Clipboard data exceeds 128 MiB.".into()));return;}
        let bytes=std::slice::from_raw_parts(bytes,length as usize).to_vec();
        match request.format {0=>request.captured.private=Some(bytes),1=>request.captured.html=Some(bytes),2=>request.captured.rtf=Some(bytes),_=>request.captured.png=Some(bytes)}
        if request.format==0 {let _=request.sender.send(Ok(request.captured));return;}
    }
    gtk_clipboard_request_text(c,text,Box::into_raw(request).cast());
}}
unsafe extern "C" fn text(_c:Clipboard,value:*const c_char,data:*mut c_void){unsafe{
    let mut request=Box::from_raw(data.cast::<Request>());
    if !value.is_null(){
        let value=CStr::from_ptr(value).to_bytes();
        let existing=request.captured.html.as_ref().map_or(0,Vec::len)+request.captured.rtf.as_ref().map_or(0,Vec::len)+request.captured.png.as_ref().map_or(0,Vec::len);
        if existing+value.len().saturating_mul(2)>MAX_TRANSFER {let _=request.sender.send(Err("Combined clipboard data exceeds 128 MiB.".into()));return;}
        request.captured.text=Some(String::from_utf8_lossy(value).encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect());
    }
    let _=request.sender.send(Ok(request.captured));
}}

struct Owner {private:Vec<u8>,html:Vec<u8>,rtf:Option<Vec<u8>>,png:Option<Vec<u8>>,text:String}
pub fn publish(p:&Prepared,_expected:u32)->Result<(),String>{
    let c=clipboard()?;
    let units=p.text.chunks_exact(2).map(|c|u16::from_le_bytes([c[0],c[1]])).take_while(|u|*u!=0).collect::<Vec<_>>();
    let owner=Box::new(Owner{private:p.private.clone(),html:clipboard::html::unwrap_cf_html(&p.html)?.as_bytes().to_vec(),
        rtf:p.rtf.clone(),png:p.png.clone(),text:String::from_utf16_lossy(&units).replace("\r\n","\n")});
    let mut formats=vec![(PRIVATE,0),(HTML,1),(c"UTF8_STRING",4),(c"text/plain;charset=utf-8",4),(c"text/plain",4),(c"TEXT",4),(c"STRING",4)];
    if owner.rtf.is_some(){formats.push((RTF,2));formats.push((c"application/rtf",2));}
    if owner.png.is_some(){formats.push((PNG,3));}
    let targets=formats.into_iter().map(|(name,info)|Target{name:name.as_ptr().cast_mut(),flags:0,info}).collect::<Vec<_>>();
    let owner=Box::into_raw(owner);
    if unsafe{gtk_clipboard_set_with_data(c,targets.as_ptr(),targets.len() as u32,provide,clear,owner.cast())}==0 {
        unsafe{drop(Box::from_raw(owner));}return Err("Could not publish to the system clipboard.".into());
    }
    // A clipboard manager can retain all formats after Impress closes. Store
    // from the next idle event, outside the editor's active RefCell borrow.
    unsafe{gtk_clipboard_set_can_store(c,ptr::null(),0);g_idle_add(store,c);}
    Ok(())
}
unsafe extern "C" fn provide(_c:Clipboard,selection:*mut c_void,info:u32,data:*mut c_void){unsafe{
    let owner=&*data.cast::<Owner>();
    if info==4 {gtk_selection_data_set_text(selection,owner.text.as_ptr().cast(),owner.text.len() as i32);return;}
    let bytes=match info {0=>Some(owner.private.as_slice()),1=>Some(owner.html.as_slice()),2=>owner.rtf.as_deref(),3=>owner.png.as_deref(),_=>None};
    if let Some(bytes)=bytes{gtk_selection_data_set(selection,gtk_selection_data_get_target(selection),8,bytes.as_ptr(),bytes.len() as i32);}
}}
unsafe extern "C" fn clear(_c:Clipboard,data:*mut c_void){unsafe{drop(Box::from_raw(data.cast::<Owner>()));}}
unsafe extern "C" fn store(data:*mut c_void)->i32{unsafe{gtk_clipboard_store(data);}0}
