//! Application-wide increment for the Home ribbon's indent controls.
use super::*;
use std::path::Path;
use wxdragon::{
    dialogs::DialogBuilder,
    id::ID_CANCEL,
    sizers::{BoxSizer, Orientation, SizerFlag},
    widgets::{SpinCtrlDouble, StaticText},
};
const DEFAULT_MM: f32 = 5.0;
fn valid(value: f32) -> bool {
    value.is_finite() && (0.1..=100.0).contains(&value)
}
fn read(path: &Path) -> f32 {
    std::fs::read(path)
        .ok()
        .filter(|bytes| bytes.len() <= 128)
        .and_then(|bytes| serde_json::from_slice::<f32>(&bytes).ok())
        .filter(|value| valid(*value))
        .unwrap_or(DEFAULT_MM)
}
fn write(path: &Path, value: f32) -> Result<(), String> {
    use std::io::Write;
    if !valid(value) {
        return Err("Invalid indent.".into());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    format::atomic_write(path, |file| {
        file.write_all(value.to_string().as_bytes())
            .map_err(|e| e.to_string())
    })
}
fn path() -> Result<std::path::PathBuf, String> {
    Ok(crate::data_dir::data_dir()?.join("indent-step.json"))
}
pub(super) fn load() -> f32 {
    path().map(|path| read(&path)).unwrap_or(DEFAULT_MM)
}
pub(super) fn show(ribbon: &Ribbon) -> Result<(), String> {
    let window = DialogBuilder::new(
        &ribbon.ui.main_frame,
        &crate::i18n::text("Paragraph settings"),
    )
    .build();
    let dip = |v| window.from_dip_int(v);
    let root = BoxSizer::builder(Orientation::Vertical).build();
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    let label = StaticText::builder(&window)
        .with_label(&crate::i18n::text("Default indent step:"))
        .build();
    let step = SpinCtrlDouble::builder(&window)
        .with_range(0.1, 100.0)
        .with_initial_value(f64::from(load()))
        .with_increment(0.5)
        .with_size(Size::new(dip(152), -1))
        .build();
    step.set_digits(1);
    step.set_name(&crate::i18n::text("Default indent step:"));
    let unit = StaticText::builder(&window)
        .with_label(&crate::i18n::text("mm"))
        .build();
    row.add(
        &label,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        dip(12),
    );
    row.add(
        &step,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        dip(8),
    );
    row.add(&unit, 0, SizerFlag::AlignCenterVertical, 0);
    root.add_sizer(&row, 0, SizerFlag::All, dip(24));
    dialog_actions::divider(&window, &root, dip(24));
    let footer = BoxSizer::builder(Orientation::Horizontal).build();
    let ok = Button::builder(&window)
        .with_id(ID_OK)
        .with_label(&crate::i18n::text("OK"))
        .build();
    let cancel = Button::builder(&window)
        .with_id(ID_CANCEL)
        .with_label(&crate::i18n::text("Cancel"))
        .build();
    dialog_actions::pair(&footer, ok, cancel);
    root.add_sizer(&footer, 0, SizerFlag::All | SizerFlag::Expand, dip(16));
    window.set_sizer_and_fit(root, true);
    window.set_escape_id(ID_CANCEL);
    ok.set_default();
    ok.on_click(move |_| window.end_modal(ID_OK));
    cancel.on_click(move |_| window.end_modal(ID_CANCEL));
    crate::placement::centre(&window);
    let accepted = window.show_modal() == ID_OK;
    let value = step.get_value() as f32;
    window.destroy();
    let result = if accepted {
        path().and_then(|path| write(&path, value))
    } else {
        Ok(())
    };
    ribbon.focus_document();
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indent_step_persists_and_invalid_preferences_use_default() {
        let root = std::env::temp_dir().join(format!("impress-indent-{}", model::NodeId::new()));
        let path = root.join("step.json");
        assert_eq!(read(&path), DEFAULT_MM);
        write(&path, 7.5).unwrap();
        assert_eq!(read(&path), 7.5);
        for value in [0.0, -5.0, 101.0, f32::NAN, f32::INFINITY] {
            assert!(write(&path, value).is_err());
        }
        assert_eq!(read(&path), 7.5);
        std::fs::write(&path, "broken").unwrap();
        assert_eq!(read(&path), DEFAULT_MM);
        std::fs::write(&path, "-1").unwrap();
        assert_eq!(read(&path), DEFAULT_MM);
        std::fs::remove_dir_all(root).unwrap();
    }
}
