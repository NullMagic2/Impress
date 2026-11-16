//! Serialize recovered native floating placement, wrapping and text distances.
use crate::model::Image;
use crate::write::esc_attr;

pub(super) fn anchor_open(
    image: &Image,
    cx: i64,
    cy: i64,
    id: u32,
    name: u32,
    description: &str,
) -> String {
    let metadata = image.floating_metadata.as_deref();
    let (fallback_x, fallback_y) = image.floating_offset_emu.unwrap_or_default();
    let horizontal = metadata.and_then(|shape| shape.horizontal_position.as_ref());
    let vertical = metadata.and_then(|shape| shape.vertical_position.as_ref());
    let horizontal_from = horizontal
        .and_then(|position| position.relative_from.as_deref())
        .unwrap_or("page");
    let vertical_from = vertical
        .and_then(|position| position.relative_from.as_deref())
        .unwrap_or("page");
    let x = horizontal
        .and_then(|position| position.offset_emu)
        .unwrap_or(fallback_x);
    let y = vertical
        .and_then(|position| position.offset_emu)
        .unwrap_or(fallback_y);
    let distance = metadata.map(|shape| shape.distance).unwrap_or_default();
    let wrap = metadata
        .and_then(|shape| shape.wrapping.as_ref())
        .map(|wrap| wrap.kind.as_str())
        .unwrap_or("square");
    let wrapping=match wrap {
        "none"=>"<wp:wrapNone/>".into(),
        "topAndBottom"=>"<wp:wrapTopAndBottom/>".into(),
        "tight"|"through"=>format!("<wp:wrap{} wrapText=\"bothSides\"><wp:wrapPolygon edited=\"0\"><wp:start x=\"0\" y=\"0\"/><wp:lineTo x=\"21600\" y=\"0\"/><wp:lineTo x=\"21600\" y=\"21600\"/><wp:lineTo x=\"0\" y=\"21600\"/><wp:lineTo x=\"0\" y=\"0\"/></wp:wrapPolygon></wp:wrap{}>",if wrap=="tight"{"Tight"}else{"Through"},if wrap=="tight"{"Tight"}else{"Through"}),
        _=>"<wp:wrapSquare wrapText=\"bothSides\"/>".into(),
    };
    let behind = metadata.and_then(|shape| shape.behind_doc).unwrap_or(false);
    let locked = metadata.and_then(|shape| shape.locked).unwrap_or(false);
    let mut xml = format!(
        r#"<wp:anchor simplePos="0" relativeHeight="251659264" behindDoc="{}" locked="{}" layoutInCell="1" allowOverlap="1" distT="{}" distB="{}" distL="{}" distR="{}"><wp:simplePos x="0" y="0"/>"#,
        u8::from(behind),
        u8::from(locked),
        distance.top_emu.unwrap_or(0),
        distance.bottom_emu.unwrap_or(0),
        distance.left_emu.unwrap_or(0),
        distance.right_emu.unwrap_or(0)
    );
    xml.push_str(&format!(r#"<wp:positionH relativeFrom="{}"><wp:posOffset>{x}</wp:posOffset></wp:positionH><wp:positionV relativeFrom="{}"><wp:posOffset>{y}</wp:posOffset></wp:positionV><wp:extent cx="{cx}" cy="{cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/>{wrapping}<wp:docPr id="{id}" name="Image{name}"{description}/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr>"#,esc_attr(horizontal_from),esc_attr(vertical_from)));
    xml
}
