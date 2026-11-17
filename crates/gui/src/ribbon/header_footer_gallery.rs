//! Header/footer template selection, previewing the same content that is inserted.
use super::*;
use editor::{
    HeaderFooterColor, HeaderFooterColorTarget as ColorTarget, HeaderFooterColors,
    HeaderFooterTemplate as Template, HeaderFooterTemplateText,
};
use model::{Block, Document, HeaderFooter, HeaderFooterVariant};
use render::{Affine2D, VectorFragment, VectorPage};
use std::sync::atomic::AtomicBool;
use wxdragon::{
    DeviceContext,
    bitmap::Bitmap,
    dc::{AutoBufferedPaintDC, BrushStyle, PenStyle},
    dialogs::{DialogBuilder, DialogStyle},
    event::WindowEvents,
    id::ID_CANCEL,
    sizers::{BoxSizer, FlexGridSizer, Orientation, SizerFlag},
    widgets::{
        colour_picker_ctrl::ColourPickerCtrl, combobox::ComboBoxStyle, panel::Panel,
        static_text::StaticText,
    },
    window::BackgroundStyle,
};

/// Native dialog dimensions and spacing, in device-independent pixels.
const GAP_DIP: i32 = 12;
/// Two rows, each with two label/picker pairs.
const COLOR_GRID_ROWS: i32 = 2;
const COLOR_GRID_COLUMNS: i32 = 4;
const PREVIEW_WIDTH_DIP: i32 = 520;
const PREVIEW_HEIGHT_DIP: i32 = 96;
const PAPER_MARGIN_DIP: i32 = 12;
/// Keep outer strokes inside the preview raster, in document points.
const PREVIEW_STROKE_PADDING_PT: f32 = 4.0;
const PREVIEW_TEXT_WIDTH_MM: f32 = 160.0;
const BACKGROUND: Colour = Colour::rgb(232, 236, 242);

const TEMPLATES: [(Template, &str); 11] = [
    (Template::AccentRule, "Accent rule"),
    (Template::SimpleRule, "Simple rule"),
    (Template::DoubleRule, "Double rule"),
    (Template::ShadedBand, "Shaded band"),
    (Template::Framed, "Framed"),
    (Template::Blank, "Blank"),
    (Template::Centered, "Centered"),
    (Template::ThreeColumns, "Three columns"),
    (Template::TitlePage, "Title and page number"),
    (Template::PageOfPages, "Page X of Y"),
    (Template::TitleDate, "Title and date"),
];

fn placeholder_text() -> HeaderFooterTemplateText {
    HeaderFooterTemplateText {
        placeholder: crate::i18n::text("[Type here]"),
        title: crate::i18n::text("[Document title]"),
        author: crate::i18n::text("[Author]"),
        date: crate::header_footer_ui::field(2).1.unwrap_or_default(),
        page: crate::i18n::text("Page"),
        of: crate::i18n::text("of"),
    }
}

pub(super) fn show(ribbon: &Ribbon, header: bool) -> Result<(), String> {
    let title = if header { "Header" } else { "Footer" };
    let (variant, existing, preview_document, width_mm) = {
        let state = ribbon.state.borrow();
        let context = state.editor.header_footer_context();
        let variant = context
            .as_ref()
            .filter(|context| context.header == header)
            .map_or(HeaderFooterVariant::Default, |context| context.variant);
        let existing = state.editor.header_footer_at_caret(header, variant);
        let document = Document {
            styles: state.editor.document.styles.clone(),
            resources: state.editor.document.resources.clone(),
            ..Document::default()
        };
        let width_mm = state
            .editor
            .section_properties_at_caret()
            .map_or(PREVIEW_TEXT_WIDTH_MM, |properties| {
                properties.text_width_mm()
            });
        (variant, existing, Rc::new(document), width_mm)
    };
    let window = DialogBuilder::new(&ribbon.ui.main_frame, &crate::i18n::text(title))
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    let dip = |value| window.from_dip_int(value);
    let root = BoxSizer::builder(Orientation::Vertical).build();
    let body = BoxSizer::builder(Orientation::Horizontal).build();
    let label = StaticText::builder(&window)
        .with_label(&crate::i18n::text("Layout:"))
        .build();
    let mut choices: Vec<String> = TEMPLATES
        .iter()
        .map(|(_, label)| crate::i18n::text(label))
        .collect();
    if existing.is_some() {
        choices.insert(
            0,
            crate::i18n::text(if header {
                "Current header"
            } else {
                "Current footer"
            }),
        );
    }
    let existing = Rc::new(existing);
    let list = ComboBox::builder(&window)
        .with_style(ComboBoxStyle::ReadOnly)
        .with_choices(choices)
        .build();
    list.set_selection(0);
    body.add(
        &label,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        dip(GAP_DIP),
    );
    body.add(&list, 1, SizerFlag::Expand, 0);
    root.add_sizer(&body, 0, SizerFlag::All | SizerFlag::Expand, dip(GAP_DIP));
    let preview = Panel::builder(&window)
        .with_size(Size::new(dip(PREVIEW_WIDTH_DIP), dip(PREVIEW_HEIGHT_DIP)))
        .build();
    preview.set_min_size(Size::new(dip(PREVIEW_WIDTH_DIP), dip(PREVIEW_HEIGHT_DIP)));
    preview.set_background_style(BackgroundStyle::Paint);
    preview.set_name("header_footer_template_preview");
    list.set_name("header_footer_template_layout");
    let selected = Rc::new(Cell::new(if existing.is_some() {
        None
    } else {
        Some(Template::AccentRule)
    }));
    let colors = Rc::new(Cell::new(HeaderFooterColors::default()));
    let fragment = Rc::new(RefCell::new(make_preview(
        selected.get(),
        existing.as_ref().as_ref(),
        header,
        width_mm,
        &preview_document,
        colors.get(),
    )));
    let renderer = Rc::new(RefCell::new(output::raster::BandRenderer::default()));
    {
        let fragment = Rc::clone(&fragment);
        let renderer = Rc::clone(&renderer);
        preview.on_paint(move |_| {
            paint_preview(preview, &fragment.borrow(), &mut renderer.borrow_mut())
        });
    }
    preview.on_size(move |event| {
        preview.refresh(false, None);
        event.skip(true);
    });
    let mut pickers = Vec::new();
    let colors_grid = FlexGridSizer::builder(COLOR_GRID_ROWS, COLOR_GRID_COLUMNS)
        .with_gap(Size::new(dip(GAP_DIP), dip(GAP_DIP)))
        .build();
    for row in [
        [
            (ColorTarget::Bracket, "Bracket color:", "bracket"),
            (ColorTarget::Line, "Line color:", "line"),
        ],
        [
            (ColorTarget::Text, "Text color:", "text"),
            (ColorTarget::Background, "Background color:", "background"),
        ],
    ] {
        for (target, label, name) in row {
            let label = StaticText::builder(&window)
                .with_label(&crate::i18n::text(label))
                .build();
            let rgb = target.default_color().0;
            let picker = ColourPickerCtrl::builder(&window)
                .with_initial_colour(Colour::rgb(rgb[0], rgb[1], rgb[2]))
                .build();
            picker.set_name(&format!("header_footer_{name}_color"));
            colors_grid.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
            colors_grid.add(&picker, 0, SizerFlag::empty(), 0);
            pickers.push((target, picker));
        }
    }
    root.add_sizer(
        &colors_grid,
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom | SizerFlag::Expand,
        dip(GAP_DIP),
    );
    let pickers = Rc::new(pickers);
    root.add(
        &preview,
        1,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand,
        dip(GAP_DIP),
    );
    let footer = BoxSizer::builder(Orientation::Horizontal).build();
    let edit = Button::builder(&window)
        .with_label(&crate::i18n::text("Edit"))
        .build();
    edit.show(existing.is_some());
    footer.add(&edit, 0, SizerFlag::Right, dip(GAP_DIP));
    let apply_color = Button::builder(&window)
        .with_label(&crate::i18n::text("Apply colors"))
        .build();
    apply_color.show(existing.is_some());
    footer.add(&apply_color, 0, SizerFlag::Right, dip(GAP_DIP));
    let insert = Button::builder(&window)
        .with_id(ID_OK)
        .with_label(&crate::i18n::text("Insert"))
        .build();
    // A custom ID keeps GTK stock labels from overriding the interface language.
    let cancel = Button::builder(&window)
        .with_label(&crate::i18n::text("Cancel"))
        .build();
    dialog_actions::pair(&footer, insert, cancel);
    dialog_actions::divider(&window, &root, dip(GAP_DIP));
    root.add_sizer(&footer, 0, SizerFlag::All | SizerFlag::Expand, dip(GAP_DIP));
    window.set_sizer_and_fit(root, true);
    window.set_min_size(window.get_size());
    window.set_escape_id(cancel.get_id());
    insert.enable(selected.get().is_some());
    if existing.is_none() {
        insert.set_default();
    } else {
        apply_color.set_default();
    }
    let refresh: Rc<dyn Fn()> = {
        let selected = Rc::clone(&selected);
        let existing = Rc::clone(&existing);
        let fragment = Rc::clone(&fragment);
        let colors = Rc::clone(&colors);
        let pickers = Rc::clone(&pickers);
        Rc::new(move || {
            let stream = make_stream(selected.get(), existing.as_ref().as_ref(), header, width_mm);
            if let Ok(stream) = &stream {
                let palette = HeaderFooterColors::from_stream(stream, &preview_document.styles);
                for &(target, picker) in pickers.iter() {
                    let rgb = colors
                        .get()
                        .get(target)
                        .or(palette.get(target))
                        .unwrap_or_else(|| target.default_color())
                        .0;
                    picker.set_colour(Colour::rgb(rgb[0], rgb[1], rgb[2]));
                    picker.enable(palette.get(target).is_some());
                }
            }
            *fragment.borrow_mut() = make_preview(
                selected.get(),
                existing.as_ref().as_ref(),
                header,
                width_mm,
                &preview_document,
                colors.get(),
            );
            insert.enable(selected.get().is_some());
            apply_color
                .enable(selected.get().is_none() && colors.get() != HeaderFooterColors::default());
            if selected.get().is_some() {
                insert.set_default();
            } else {
                apply_color.set_default();
            }
            preview.refresh(false, None);
        })
    };
    {
        let selected = Rc::clone(&selected);
        let existing = Rc::clone(&existing);
        let refresh = Rc::clone(&refresh);
        list.on_selection_changed(move |_| {
            let index = list.get_selection().unwrap_or(0) as usize;
            selected.set(if existing.is_some() && index == 0 {
                None
            } else {
                TEMPLATES
                    .get(index - usize::from(existing.is_some()))
                    .map(|(template, _)| *template)
            });
            refresh();
        });
    }
    for &(target, picker) in pickers.iter() {
        let colors = Rc::clone(&colors);
        let refresh = Rc::clone(&refresh);
        picker.on_colour_changed(move |_| {
            let chosen = picker.get_colour();
            let mut patch = colors.get();
            patch.set(target, HeaderFooterColor([chosen.r, chosen.g, chosen.b]));
            colors.set(patch);
            refresh();
        });
    }
    refresh();
    let editing = Rc::new(Cell::new(false));
    let recoloring = Rc::new(Cell::new(false));
    {
        let recoloring = Rc::clone(&recoloring);
        apply_color.on_click(move |_| {
            recoloring.set(true);
            window.end_modal(ID_OK);
        });
    }
    {
        let editing = Rc::clone(&editing);
        edit.on_click(move |_| {
            editing.set(true);
            window.end_modal(ID_OK);
        });
    }
    insert.on_click(move |_| window.end_modal(ID_OK));
    cancel.on_click(move |_| window.end_modal(ID_CANCEL));
    crate::placement::centre(&window);
    list.set_focus();
    let accepted = window.show_modal() == ID_OK;
    window.destroy();
    if accepted && editing.get() {
        ribbon.edit(move |editor| {
            if header {
                editor.enter_header(variant)
            } else {
                editor.enter_footer(variant)
            }
        })?;
    } else if accepted && recoloring.get() {
        ribbon.edit(|editor| editor.set_header_footer_colors(header, variant, colors.get()))?;
    } else if accepted && (existing.is_none() || confirm_replace(ribbon, header)) {
        ribbon.edit(|editor| {
            editor.apply_colored_header_footer_template(
                header,
                variant,
                selected.get().ok_or("Select a header or footer layout")?,
                &placeholder_text(),
                colors.get(),
            )
        })?;
    }
    ribbon.focus_document();
    Ok(())
}

fn confirm_replace(ribbon: &Ribbon, header: bool) -> bool {
    let dialog = MessageDialog::builder(
        &ribbon.ui.main_frame,
        &crate::i18n::text(if header {
            "Replace the current header with this template?"
        } else {
            "Replace the current footer with this template?"
        }),
        &crate::i18n::text(if header {
            "Replace header"
        } else {
            "Replace footer"
        }),
    )
    .with_style(crate::cautious_question())
    .build();
    dialog.set_yes_no_labels(&crate::i18n::text("Replace"), &crate::i18n::text("Cancel"));
    crate::placement::centre(&dialog);
    let accepted = dialog.show_modal() == wxdragon::id::ID_YES;
    dialog.destroy();
    accepted
}

fn make_preview(
    template: Option<Template>,
    existing: Option<&HeaderFooter>,
    header: bool,
    width_mm: f32,
    document: &Document,
    colors: HeaderFooterColors,
) -> Result<VectorFragment, String> {
    let mut stream = make_stream(template, existing, header, width_mm)?;
    colors.apply(&mut stream);
    layout::LayoutEngine::default()
        .layout_header_footer_preview(&stream, width_mm, document)
        .map_err(|error| error.to_string())
}

fn make_stream(
    template: Option<Template>,
    existing: Option<&HeaderFooter>,
    header: bool,
    width_mm: f32,
) -> Result<HeaderFooter, String> {
    if let Some(template) = template {
        Ok(HeaderFooter {
            blocks: vec![Block::Paragraph(template.paragraph(
                width_mm,
                &placeholder_text(),
                header,
            ))]
            .into(),
            ..HeaderFooter::default()
        })
    } else {
        existing
            .cloned()
            .ok_or_else(|| "Enter a header or footer first".into())
    }
}

fn paint_preview(
    panel: Panel,
    fragment: &Result<VectorFragment, String>,
    renderer: &mut output::raster::BandRenderer,
) {
    let dc = AutoBufferedPaintDC::new(&panel);
    let size = panel.get_client_size();
    dc.set_pen(BACKGROUND, 1, PenStyle::Solid);
    dc.set_brush(BACKGROUND, BrushStyle::Solid);
    dc.draw_rectangle(0, 0, size.width, size.height);
    let Ok(fragment) = fragment else {
        dc.draw_text(
            &crate::i18n::text("Preview unavailable"),
            panel.from_dip_int(PAPER_MARGIN_DIP),
            panel.from_dip_int(PAPER_MARGIN_DIP),
        );
        return;
    };
    let margin = panel.from_dip_int(PAPER_MARGIN_DIP);
    let padding = PREVIEW_STROKE_PADDING_PT;
    let page_size = render::Size::new(
        fragment.size.width + padding * 2.0,
        fragment.size.height + padding * 2.0,
    );
    let available_width = (size.width - margin * 2).max(1) as f32;
    let available_height = (size.height - margin * 2).max(1) as f32;
    let zoom = (available_width / page_size.width).min(available_height / page_size.height);
    let pixel_scale = crate::display_scale::window_scale(&panel);
    let scale = zoom * pixel_scale;
    let width = (page_size.width * scale).ceil().max(1.0) as u32;
    let height = (page_size.height * scale).ceil().max(1.0) as u32;
    let mut page = VectorPage::new(page_size);
    page.commands = fragment
        .commands
        .iter()
        .map(|command| command.translated(padding, padding))
        .collect();
    let Ok(mut rgba) = renderer.render(
        &page,
        width,
        height,
        Affine2D::scale(scale, scale),
        &AtomicBool::new(false),
    ) else {
        return;
    };
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    if let Some(bitmap) = Bitmap::from_rgba(&rgba, width, height) {
        let left = ((size.width as f32 - page_size.width * zoom) / 2.0).round() as i32;
        let top = ((size.height as f32 - page_size.height * zoom) / 2.0).round() as i32;
        crate::crisp::draw(&dc, &bitmap, pixel_scale, left, top);
    }
}
