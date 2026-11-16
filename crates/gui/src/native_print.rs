//! Native print-dialog and GDI spool ownership. Stable Win32 ABI declarations
//! stay local: no printer handles enter the document or PDF modules.
//! Driver-owned copies/collation avoid multiplying the selected copy count.
//! References: Microsoft PRINTDLGEXW, StartDocW, GetDeviceCaps, StretchDIBits.
use std::{ffi::c_void,sync::atomic::{AtomicBool,Ordering}};
use render::{VectorDocument,Affine2D};
type Handle=*mut c_void;
#[repr(C)]#[derive(Clone,Copy)]struct PageRange{from:u32,to:u32}
#[repr(C)]struct PrintDialog{size:u32,owner:Handle,devmode:Handle,devnames:Handle,dc:Handle,flags:u32,flags2:u32,exclusion:u32,nranges:u32,maxranges:u32,ranges:*mut PageRange,min:u32,max:u32,copies:u32,instance:Handle,template:*const u16,callback:Handle,nproperties:u32,properties:*mut Handle,startpage:u32,result:u32}
#[repr(C)]struct DocInfo{size:i32,name:*const u16,output:*const u16,datatype:*const u16,kind:u32}
#[repr(C)]struct BitmapHeader{size:u32,width:i32,height:i32,planes:u16,bits:u16,compression:u32,imagesize:u32,xppm:i32,yppm:i32,colors:u32,important:u32}
#[repr(C)]struct BitmapInfo{header:BitmapHeader,color:u32}
#[link(name="comdlg32")]unsafe extern "system"{fn PrintDlgExW(dialog:*mut PrintDialog)->i32;}
#[link(name="kernel32")]unsafe extern "system"{fn GlobalFree(memory:Handle)->Handle;fn GetLastError()->u32;}
#[link(name="gdi32")]unsafe extern "system"{
 fn DeleteDC(dc:Handle)->i32;fn GetDeviceCaps(dc:Handle,index:i32)->i32;
 fn StartDocW(dc:Handle,info:*const DocInfo)->i32;fn EndDoc(dc:Handle)->i32;fn AbortDoc(dc:Handle)->i32;fn StartPage(dc:Handle)->i32;fn EndPage(dc:Handle)->i32;
 fn StretchDIBits(dc:Handle,x:i32,y:i32,cx:i32,cy:i32,sx:i32,sy:i32,sw:i32,sh:i32,bits:*const c_void,info:*const BitmapInfo,usage:u32,rop:u32)->i32;
}
fn error(operation:&str)->String{format!("{operation} failed (Windows error {})",unsafe{GetLastError()})}
pub struct Device{dc:usize,pub pages:Vec<usize>}
// A printer DC is exclusively transferred to the spool worker, never concurrently
// accessed on the UI thread. usize is the transport representation, not a bitmap.
impl Drop for Device{fn drop(&mut self){if self.dc!=0{unsafe{DeleteDC(self.dc as Handle);}}}}
pub fn choose(owner:Handle,count:usize)->Result<Option<Device>,String>{
 if count==0||count>10_000{return Err("Printer page count is outside supported limits".into());}
 let mut ranges=vec![PageRange{from:1,to:count as u32};32];let mut d:PrintDialog=unsafe{std::mem::zeroed()};d.size=std::mem::size_of::<PrintDialog>()as u32;d.owner=owner;d.ranges=ranges.as_mut_ptr();d.maxranges=32;d.min=1;d.max=count as u32;d.copies=1;d.startpage=0xffff_ffff;
 // PD_RETURNDC | PD_USEDEVMODECOPIESANDCOLLATE | PD_NOSELECTION |
 // PD_NOCURRENTPAGE | PD_HIDEPRINTTOFILE. All pages initially selected.
 d.flags=0x100|0x40000|0x4|0x800000|0x100000|0x8; // | PD_NOPAGENUMS: Impress's Print dialog chose the pages
 let hr=unsafe{PrintDlgExW(&mut d)};
 if !d.devmode.is_null(){unsafe{GlobalFree(d.devmode);}}if !d.devnames.is_null(){unsafe{GlobalFree(d.devnames);}}
 let mut device=Device{dc:d.dc as usize,pages:Vec::new()};
 if hr<0{return Err(format!("Print dialog failed (HRESULT {hr:#X})"));}if d.result!=1{return Ok(None);}if d.dc.is_null(){return Err("Print dialog did not return a printer device".into());}
 if d.flags&0x2!=0{if d.nranges==0||d.nranges>32{return Err("Invalid printer page ranges".into());}let mut pages=std::collections::BTreeSet::new();for range in &ranges[..d.nranges as usize]{if range.from==0||range.to<range.from||range.to>count as u32{return Err("Printer page range is outside the document".into());}pages.extend((range.from as usize-1)..range.to as usize);}device.pages=pages.into_iter().collect();}else{device.pages=(0..count).collect();}Ok(Some(device))
}
impl Device{pub fn spool(self,document:&VectorDocument,fit:bool,cancel:&AtomicBool)->Result<(),String>{
 let dc=self.dc as Handle;let(w,h,dx,dy,ox,oy)=unsafe{(GetDeviceCaps(dc,8),GetDeviceCaps(dc,10),GetDeviceCaps(dc,88),GetDeviceCaps(dc,90),GetDeviceCaps(dc,112),GetDeviceCaps(dc,113))};
 if w<=0||h<=0||dx<=0||dy<=0||w>40_000||h>80_000{return Err("Printer reported invalid or excessive device dimensions".into());}
 let name="Impress document\0".encode_utf16().collect::<Vec<_>>();let info=DocInfo{size:std::mem::size_of::<DocInfo>()as i32,name:name.as_ptr(),output:std::ptr::null(),datatype:std::ptr::null(),kind:0};
 if unsafe{StartDocW(dc,&info)}<=0{return Err(error("Start print job"));}
 struct Abort(Handle,bool);impl Drop for Abort{fn drop(&mut self){if self.1{unsafe{AbortDoc(self.0);}}}}let mut guard=Abort(dc,true);
 let mut renderer=output::raster::BandRenderer::default();
 for &index in &self.pages{if cancel.load(Ordering::Acquire){return Err("Printing cancelled".into());}let page=document.pages.get(index).ok_or("Print page not available")?;
  let sx=dx as f32/72.0;let sy=dy as f32/72.0;let factor=if fit{(w as f32/(page.size.width*sx)).min(h as f32/(page.size.height*sy))}else{1.0};
  let tx=if fit{(w as f32-page.size.width*sx*factor)/2.0}else{-ox as f32};let ty=if fit{(h as f32-page.size.height*sy*factor)/2.0}else{-oy as f32};
  if unsafe{StartPage(dc)}<=0{return Err(error("Start page"));}
  for y in (0..h).step_by(128){if cancel.load(Ordering::Acquire){return Err("Printing cancelled".into());}let bh=(h-y).min(128);
   let matrix=Affine2D{sx:sx*factor,sy:sy*factor,tx,ty:ty-y as f32,..Affine2D::IDENTITY};let data=renderer.render(page,w as u32,bh as u32,matrix,cancel)?;
   let info=BitmapInfo{header:BitmapHeader{size:40,width:w,height:-bh,planes:1,bits:32,compression:0,imagesize:data.len()as u32,xppm:0,yppm:0,colors:0,important:0},color:0};
   let copied=unsafe{StretchDIBits(dc,0,y,w,bh,0,0,w,bh,data.as_ptr().cast(),&info,0,0x00cc0020)};if copied==0||copied==-1{return Err(error("Transfer printer band"));}
  }
  if unsafe{EndPage(dc)}<=0{return Err(error("End page"));}
 }
 if unsafe{EndDoc(dc)}<=0{return Err(error("Finish print job"));}guard.1=false;Ok(())
}}
