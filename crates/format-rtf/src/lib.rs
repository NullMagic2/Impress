//! Isolated RTF format adapter. No editor, GUI, clipboard or Windows dependency.
//! Unsupported active/OLE content is never executed or loaded. RTF fidelity is
//! explicitly a bounded subset, not an unrestricted Word/Excel compatibility claim.
mod lexer;
mod reader;
mod writer;
pub use reader::decode;
pub use writer::encode;
use model::Document;
use format::*;
use std::io::Read;
pub const MAX_INPUT:usize=32*1024*1024;
pub struct RtfFormat;
fn invalid(message:String)->FormatError{FormatError::Invalid{format:"RTF",message}}
impl DocumentFormat for RtfFormat {
    fn id(&self)->&'static str{"rtf"}
    fn display_name(&self)->&'static str{"Rich Text Format"}
    fn extensions(&self)->&'static[&'static str]{&["rtf"]}
    fn capabilities(&self)->FormatCapabilities{FormatCapabilities{import:true,export:true,preserves_unknown_data:false,native_equations:false}}
    fn import(&self,input:&mut dyn ReadSeek)->FormatResult<Document>{let mut data=Vec::new();input.take((MAX_INPUT+1) as u64).read_to_end(&mut data)?;decode(&data).map_err(invalid)}
    fn export_warnings(&self,_document:&Document)->Vec<String>{vec!["RTF exports supported text, formatting, simple tables and PNG/JPEG pictures. Review history, comments, fields, floating anchors, notes, style identities and unsupported objects are not a lossless RTF round trip. Keep a native .impress copy.".into()]}
    fn export(&self,document:&Document,output:&mut dyn WriteSeek)->FormatResult<()>{output.write_all(&encode(document).map_err(invalid)?)?;Ok(())}
}
