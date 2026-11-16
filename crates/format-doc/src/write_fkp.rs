//! Pack native character/paragraph formatting pages and their bin tables.
use super::properties::long;
use format::{FormatError, FormatResult};

const PAGE_BYTES: usize = 512;
const TEXT_START: u32 = 1024;
#[derive(Clone)]
pub(super) struct Run {
    pub start: u32,
    pub end: u32,
    pub properties: Vec<u8>,
}

pub(super) fn pages(
    word_stream: &mut Vec<u8>,
    runs: &[Run],
    paragraph: bool,
    data: &mut Vec<u8>,
) -> FormatResult<Vec<u8>> {
    let mut ranges = Vec::new();
    let mut position = 0;
    while position < runs.len() {
        let mut page = vec![0u8; PAGE_BYTES];
        let mut data_end = PAGE_BYTES - 1;
        let mut count = 0;
        let first = position;
        let mut pointers = Vec::new();
        while position < runs.len() && count < if paragraph { 29 } else { 101 } {
            let run = &runs[position];
            let mut payload = Vec::new();
            if paragraph {
                let properties = paragraph_payload(&run.properties, data)?;
                if properties.len() % 2 == 0 {
                    payload.push(0);
                    payload.push((properties.len() / 2) as u8);
                } else {
                    payload.push(((properties.len() + 1) / 2) as u8);
                }
                payload.extend(properties);
            } else {
                if run.properties.len() > 255 {
                    return Err(FormatError::Unsupported {
                        format: "DOC",
                        feature: "Character formatting exceeds the native DOC run limit".into(),
                    });
                }
                payload.push(run.properties.len() as u8);
                payload.extend_from_slice(&run.properties);
            }
            let target = (data_end.saturating_sub(payload.len())) & !1;
            let front = (count + 2) * 4 + (count + 1) * if paragraph { 13 } else { 1 };
            if target < front {
                if count == 0 {
                    return Err(FormatError::Unsupported {
                        format: "DOC",
                        feature: "Formatting cannot fit in a native DOC page".into(),
                    });
                }
                break;
            }
            page[target..target + payload.len()].copy_from_slice(&payload);
            data_end = target;
            page[count * 4..count * 4 + 4]
                .copy_from_slice(&(TEXT_START + run.start * 2).to_le_bytes());
            // Pointers move when rgfc grows; keep them separately until page is final.
            pointers.push((target / 2) as u8);
            count += 1;
            position += 1;
        }
        page[count * 4..count * 4 + 4]
            .copy_from_slice(&(TEXT_START + runs[position - 1].end * 2).to_le_bytes());
        let stride = if paragraph { 13 } else { 1 };
        for (offset, pointer) in pointers.into_iter().enumerate() {
            page[(count + 1) * 4 + offset * stride] = pointer;
        }
        page[511] = count as u8;
        while word_stream.len() % PAGE_BYTES != 0 {
            word_stream.push(0);
        }
        word_stream.extend(page);
        ranges.push(first);
    }
    // Page numbers can exceed u8: derive them directly from the contiguous pages.
    let page_count = ranges.len();
    let first_page = word_stream.len() / PAGE_BYTES - page_count;
    let mut table = Vec::new();
    for first in &ranges {
        long(&mut table, TEXT_START + runs[*first].start * 2);
    }
    if let Some(last) = runs.last() {
        long(&mut table, TEXT_START + last.end * 2);
    }
    for index in 0..page_count {
        long(&mut table, (first_page + index) as u32);
    }
    Ok(table)
}

/// PAPX FKPs refer large table/paragraph grpprls to the Data stream.
fn paragraph_payload(properties: &[u8], data: &mut Vec<u8>) -> FormatResult<Vec<u8>> {
    const FKP_PROPERTY_LIMIT: usize = 450;
    const HUGE_PAPX: u16 = 0x6645;
    if properties.len() <= FKP_PROPERTY_LIMIT {
        return Ok(properties.to_vec());
    }
    let Some(grpprl) = properties.get(2..) else {
        return Err(FormatError::Unsupported {
            format: "DOC",
            feature: "A paragraph style index is missing".into(),
        });
    };
    let length = u16::try_from(grpprl.len()).map_err(|_| FormatError::Unsupported {
        format: "DOC",
        feature: "Paragraph formatting exceeds the native DOC Data record limit".into(),
    })?;
    while data.len() % 4 != 0 {
        data.push(0);
    }
    let offset = u32::try_from(data.len()).map_err(|_| FormatError::Unsupported {
        format: "DOC",
        feature: "The native DOC Data stream is too large".into(),
    })?;
    data.extend_from_slice(&length.to_le_bytes());
    data.extend_from_slice(grpprl);
    let mut reference = properties[..2].to_vec();
    reference.extend_from_slice(&HUGE_PAPX.to_le_bytes());
    reference.extend_from_slice(&offset.to_le_bytes());
    Ok(reference)
}
