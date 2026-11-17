//! Compact crop gallery using LibreOffice icons and exact preset fallbacks.
use super::*;
use wxdragon::{
    bitmap_bundle::BitmapBundle,
    dialogs::{DialogBuilder, DialogStyle},
    id::ID_CANCEL,
    sizers::{BoxSizer, FlexGridSizer, Orientation, SizerFlag},
    widgets::{
        bitmaptogglebutton::BitmapToggleButton,
        combobox::ComboBoxStyle,
        scrolled_window::{ScrollBarConfig, ScrolledWindow, ScrolledWindowStyle},
    },
    window::WxWidget,
    Bitmap,
};
#[path = "picture_shape_choices.rs"]
mod choices;
#[path = "picture_shape_icons.rs"]
mod icons;

/// Fixed initial gallery dimensions, in logical display pixels.
const WIDTH_DIP: i32 = 430;
const HEIGHT_DIP: i32 = 390;
const GAP_DIP: i32 = 8;
const BUTTON_SIZE_DIP: i32 = 42;
const GRID_COLUMNS: i32 = 8;
const SCROLL_STEP_DIP: i32 = 16;
/// Shape previews reserve space for their outlines.
const PREVIEW_SIZE_DIP: i32 = 30;
/// The first raster defines the logical size; larger rasters retain detail at high DPI.
const PREVIEW_SCALES: [u32; 3] = [1, 2, 3];
const PREVIEW_INSET_DIP: f32 = 2.0;
const PREVIEW_FILL_RGB: (u8, u8, u8) = (255, 255, 255);
const PREVIEW_LINE_RGB: (u8, u8, u8) = (77, 77, 77);
const GROUPS: [&str; 8] = [
    "Rectangles",
    "Basic shapes",
    "Block arrows",
    "Equation shapes",
    "Flowchart",
    "Stars and banners",
    "Callouts",
    "Action buttons",
];
struct Choice {
    preset: &'static str,
    label: &'static str,
    group: &'static str,
}

pub(super) fn show(ribbon: &Ribbon) {
    let (picture, current) = {
        let state = ribbon.state.borrow();
        let Some(objects_ui::SelectedObject::Picture(id)) = &state.selected_object else {
            return;
        };
        (
            id.clone(),
            state.editor.picture_shape(id).cloned().unwrap_or_default(),
        )
    };
    let window = DialogBuilder::new(&ribbon.ui.main_frame, &crate::i18n::text("Crop to Shape"))
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    let dip = |value| window.from_dip_int(value);
    let root = BoxSizer::builder(Orientation::Vertical).build();
    let category = ComboBox::builder(&window)
        .with_style(ComboBoxStyle::ReadOnly)
        .with_choices(
            GROUPS
                .iter()
                .map(|label| crate::i18n::text(label))
                .collect(),
        )
        .build();
    category.set_name("picture_crop_category");
    category.set_accessibility_label(&crate::i18n::text("Crop to Shape"));
    let initial_group = choices::CHOICES
        .iter()
        .find(|choice| choice.preset == current.preset)
        .and_then(|choice| GROUPS.iter().position(|group| *group == choice.group))
        .unwrap_or_default();
    category.set_selection(initial_group as u32);
    root.add(
        &category,
        0,
        SizerFlag::All | SizerFlag::Expand,
        dip(GAP_DIP),
    );
    let selected = Rc::new(Cell::new(None));
    let mut pages = Vec::new();
    for (group_index, group) in GROUPS.iter().enumerate() {
        let page = ScrolledWindow::builder(&window)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        page.set_name(&format!("picture_crop_group_{group_index}"));
        let grid = FlexGridSizer::builder(0, GRID_COLUMNS)
            .with_gap(Size::new(dip(GAP_DIP), dip(GAP_DIP)))
            .build();
        let mut count = 0;
        for (index, choice) in choices::CHOICES
            .iter()
            .enumerate()
            .filter(|(_, choice)| choice.group == *group)
        {
            let Some(bitmap) = preview_for(choice.preset, &page) else {
                continue;
            };
            let button = BitmapToggleButton::builder(&page)
                .with_bitmap(Some(bitmap))
                .with_size(Size::new(dip(BUTTON_SIZE_DIP), dip(BUTTON_SIZE_DIP)))
                .build();
            crate::gtk_native::style_compact_bitmap_button(&button);
            let label = crate::i18n::text(choice.label);
            button.set_name(&format!("picture_crop_{}", choice.preset));
            button.set_accessibility_label(&label);
            button.set_tooltip(&label);
            button.set_value(choice.preset == current.preset);
            let selected = Rc::clone(&selected);
            button.on_toggle(move |_| {
                selected.set(Some(index));
                window.end_modal(ID_OK);
            });
            grid.add(&button, 0, SizerFlag::empty(), 0);
            count += 1;
        }
        page.set_sizer(grid, true);
        let rows = (count + GRID_COLUMNS - 1) / GRID_COLUMNS;
        let height = dip(rows * (BUTTON_SIZE_DIP + GAP_DIP));
        let step = dip(SCROLL_STEP_DIP).max(1);
        page.set_scrollbars(ScrollBarConfig {
            pixels_per_unit_x: 0,
            pixels_per_unit_y: step,
            no_units_x: 0,
            no_units_y: (height + step - 1) / step,
            x_pos: 0,
            y_pos: 0,
            no_refresh: false,
        });
        page.show(group_index == initial_group);
        root.add(
            &page,
            1,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand,
            dip(GAP_DIP),
        );
        pages.push(page);
    }
    let footer = BoxSizer::builder(Orientation::Horizontal).build();
    footer.add_stretch_spacer(1);
    let close = Button::builder(&window)
        .with_label(&crate::i18n::text("Close"))
        .build();
    close.on_click(move |_| window.end_modal(ID_CANCEL));
    dialog_actions::size(close);
    footer.add(&close, 0, SizerFlag::empty(), 0);
    footer.add_stretch_spacer(1);
    dialog_actions::divider(&window, &root, dip(GAP_DIP));
    root.add_sizer(&footer, 0, SizerFlag::All | SizerFlag::Expand, dip(GAP_DIP));
    window.set_sizer(root, true);
    window.set_size(Size::new(dip(WIDTH_DIP), dip(HEIGHT_DIP)));
    window.set_min_size(window.get_size());
    window.set_escape_id(close.get_id());
    category.on_selection_changed(move |_| {
        let selected = category.get_selection().unwrap_or_default() as usize;
        for (index, page) in pages.iter().enumerate() {
            page.show(index == selected);
        }
        window.layout();
    });
    window.centre();
    let accepted = window.show_modal() == ID_OK;
    window.destroy();
    if accepted {
        if let Some(choice) = selected.get().and_then(|index| choices::CHOICES.get(index)) {
            let shape = model::ImageShape {
                preset: choice.preset.into(),
                adjustments: Default::default(),
            };
            if let Err(error) = ribbon.edit(|editor| editor.set_picture_shape(&picture, shape)) {
                ribbon.state.borrow_mut().action_status = Some(error);
            }
        }
    }
}

fn preview_for(preset: &str, window: &impl WxWidget) -> Option<Bitmap> {
    let bitmaps = PREVIEW_SCALES
        .iter()
        .map(|scale| preview(preset, PREVIEW_SIZE_DIP as u32 * scale))
        .collect::<Option<Vec<_>>>()?;
    BitmapBundle::from_bitmaps(&bitmaps).get_bitmap_for(window)
}

fn preview(preset: &str, side: u32) -> Option<Bitmap> {
    if let Some(bitmap) = icons::png(preset).and_then(|png| art::bitmap_from_png(png, side)) {
        return Some(bitmap);
    }
    // Preserve a recognizable icon for Word presets without matching LibreOffice artwork.
    let mut pixmap = tiny_skia::Pixmap::new(side, side)?;
    let inset = PREVIEW_INSET_DIP * side as f32 / PREVIEW_SIZE_DIP as f32;
    let size = side as f32 - 2.0 * inset;
    let geometry = render::picture_shapes::geometry(preset, &Default::default(), size, size)?;
    let mut fill = tiny_skia::Paint::default();
    let (r, g, b) = PREVIEW_FILL_RGB;
    fill.set_color_rgba8(r, g, b, 255);
    fill.anti_alias = true;
    let mut line = tiny_skia::Paint::default();
    let (r, g, b) = PREVIEW_LINE_RGB;
    line.set_color_rgba8(r, g, b, 255);
    line.anti_alias = true;
    let transform = tiny_skia::Transform::from_translate(inset, inset);
    // Fill first: chart-marker presets put a filled hit area after their strokes.
    // Drawing each path's fill/stroke in order would hide those marker icons.
    for geometry in geometry.iter().filter(|geometry| geometry.fill) {
        if let Some(path) = render::picture_shapes::skia_path(&geometry.path) {
            pixmap.fill_path(&path, &fill, tiny_skia::FillRule::Winding, transform, None);
        }
    }
    for geometry in geometry.iter().filter(|geometry| geometry.stroke) {
        if let Some(path) = render::picture_shapes::skia_path(&geometry.path) {
            pixmap.stroke_path(&path, &line, &tiny_skia::Stroke::default(), transform, None);
        }
    }
    // wx expects straight RGBA; Skia stores premultiplied components.
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect::<Vec<_>>();
    Bitmap::from_rgba(&rgba, side, side)
}
