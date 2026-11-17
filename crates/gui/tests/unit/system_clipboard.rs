use super::*;
#[test]
fn external_raw_html_and_unicode_text_are_interoperable() {
    let text="Ελληνικά 🌍\nSecond line";
    let bytes=text.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect::<Vec<_>>();
    let Content::Text(actual)=decode_captured(Captured{text:Some(bytes.clone()),..Default::default()}).unwrap() else{panic!("plain Unicode text expected")};
    assert_eq!(actual,text);
    let html="<p><b>Ελληνικά 🌍</b></p><p>Second line</p>";
    let Content::Rich(fragment)=decode_captured(Captured{html:Some(html.as_bytes().to_vec()),text:Some(bytes),..Default::default()}).unwrap() else{panic!("formatted external HTML expected")};
    assert_eq!(fragment.plain_text,text);
    assert!(fragment.paragraphs[0].content.iter().any(|item|matches!(item,model::Inline::Text(run) if run.properties.bold==Some(true))));
}
#[cfg(target_os="linux")]
#[test]
fn asynchronous_native_capture_is_decoded_on_the_worker() {
    let(sender,receiver)=std::sync::mpsc::channel();
    let bytes="μηχανή".encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect();
    sender.send(Ok(Captured{text:Some(bytes),..Default::default()})).unwrap();
    let Content::Text(text)=decode_captured(Captured{pending:Some(receiver),..Default::default()}).unwrap()else{panic!("text expected")};
    assert_eq!(text,"μηχανή");
}
#[test] fn private_payload_ignores_allocator_padding_and_rejects_truncation() {
    let fragment = clipboard::html::import("<p><b>Ελληνικά</b></p>").unwrap();
    let mut bytes = encode(&fragment).unwrap(); let len = bytes.len();
    bytes.extend_from_slice(&[255, 17, 98, 0]); assert_eq!(decode(&bytes).unwrap().plain_text, fragment.plain_text);
    assert!(decode(&bytes[..len - 1]).is_none()); bytes[8..16].copy_from_slice(&u64::MAX.to_le_bytes()); assert!(decode(&bytes).is_none());
}
