use model::{Document, ResourceId};
use format::{DocumentFormat, FormatCapabilities, FormatError, FormatResult, ReadSeek, WriteSeek};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

pub const NATIVE_FORMAT_VERSION: u32 = 1;

#[derive(Default)]
pub struct NativeImpressFormat;

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    format: String,
    format_version: u32,
    generator: String,
    resources: Vec<ResourceEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResourceEntry {
    id: ResourceId,
    path: String,
}

impl DocumentFormat for NativeImpressFormat {
    fn id(&self) -> &'static str { "impress" }
    fn display_name(&self) -> &'static str { "Impress Document" }
    fn extensions(&self) -> &'static [&'static str] { &["impress"] }
    fn capabilities(&self) -> FormatCapabilities {
        FormatCapabilities { import: true, export: true, preserves_unknown_data: true, native_equations: true }
    }

    fn import(&self, input: &mut dyn ReadSeek) -> FormatResult<Document> {
        let mut zip = ZipArchive::new(input).map_err(zip_err)?;
        format::validate_zip_directory(&mut zip, "Impress")?;
        let manifest: Manifest = {
            let mut member = zip.by_name("manifest.json").map_err(zip_err)?;
            let mut text = String::new();
            member.take(format::MAX_XML_MEMBER+1).read_to_string(&mut text)?;
            if text.len() as u64>format::MAX_XML_MEMBER{return Err(FormatError::Invalid{format:"Impress",message:"JSON member exceeds the size limit".into()});}
            serde_json::from_str(&text).map_err(|e| FormatError::Serialization(e.to_string()))?
        };
        if manifest.format != "impress" || manifest.format_version > NATIVE_FORMAT_VERSION {
            return Err(FormatError::Invalid { format: "Impress", message: format!("unsupported package version {}", manifest.format_version) });
        }
        let mut document: Document = {
            let mut member = zip.by_name("document.json").map_err(zip_err)?;
            let mut text = String::new();
            member.take(format::MAX_XML_MEMBER+1).read_to_string(&mut text)?;
            if text.len() as u64>format::MAX_XML_MEMBER{return Err(FormatError::Invalid{format:"Impress",message:"JSON member exceeds the size limit".into()});}
            serde_json::from_str(&text).map_err(|e| FormatError::Serialization(e.to_string()))?
        };
        if document.schema_version>model::DOCUMENT_SCHEMA_VERSION {
            return Err(FormatError::Invalid{format:"Impress",message:format!("Unsupported document schema {}",document.schema_version)});
        }
        document.schema_version=model::DOCUMENT_SCHEMA_VERSION;
        for entry in manifest.resources {
            let mut member = zip.by_name(&entry.path).map_err(zip_err)?;
            let mut data = Vec::new();
            member.take(format::MAX_PACKAGE_MEMBER+1).read_to_end(&mut data)?;
            format::check_member_len(data.len(),"Impress")?;
            if let Some(resource) = document.resources.get_mut(&entry.id) { resource.data = data.into(); }
        }
        Ok(document)
    }

    fn export(&self, document: &Document, output: &mut dyn WriteSeek) -> FormatResult<()> {
        let resources: Vec<ResourceEntry> = document.resources.values().map(|r| ResourceEntry {
            id: r.id.clone(),
            path: format!("media/{}", r.id),
        }).collect();
        let manifest = Manifest { format: "impress".into(), format_version: NATIVE_FORMAT_VERSION, generator: concat!("Impress ",env!("CARGO_PKG_VERSION")).into(), resources };
        let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(|e| FormatError::Serialization(e.to_string()))?;
        let document_json = serde_json::to_vec_pretty(document).map_err(|e| FormatError::Serialization(e.to_string()))?;

        let mut zip = ZipWriter::new(output);
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("manifest.json", options).map_err(zip_err)?;
        zip.write_all(&manifest_json)?;
        zip.start_file("document.json", options).map_err(zip_err)?;
        zip.write_all(&document_json)?;
        for resource in document.resources.values() {
            zip.start_file(format!("media/{}", resource.id), options).map_err(zip_err)?;
            zip.write_all(&resource.data)?;
        }
        zip.finish().map_err(zip_err)?;
        Ok(())
    }
}

fn zip_err(e: zip::result::ZipError) -> FormatError {
    FormatError::Invalid { format: "Impress", message: e.to_string() }
}
