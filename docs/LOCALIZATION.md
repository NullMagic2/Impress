# Interface languages

Impress ships English, Greek, Spanish, Brazilian Portuguese and European Portuguese. Open the flag menu on the right of the title toolbar, 20 logical pixels before the collapse arrow. The selected language applies immediately and is saved in `interface-language.json` in the account data directory, independently of the executable directory. Linux and Windows use bundled PNG flag images.

The catalog covers ribbon captions and tooltips, menu commands, native dialogs, status/progress messages, language names and built-in style labels. English remains the source language. Document text, custom style names, filenames, identifiers, numerical values and proofing tags keep their original values. Translated choice controls map back to their stable model values. Switching interface language does not change a selection's spelling language.

Dictionary installation uses **Installed by User** for private installations and **System-wide** for system installations, with equivalent translated labels. The stored receipt value remains `local` for compatibility. The manager still shows only installations owned by Impress and retains the existing removal validation.

## Catalog maintenance

- `crates/gui/assets/locales/glossary.tsv` contains reviewed captions and terminology. Columns are English, Greek, Spanish, Brazilian Portuguese and optionally European Portuguese; the last defaults to the Brazilian column if omitted.
- `messages.json` contains secondary strings. These were drafted locally using Helsinki-NLP OPUS English–Greek and English–Romance models, then filtered and corrected. They have not all received independent professional linguistic review.
- `translations.json` and the four `.mo` catalogs are generated. The native catalogs translate stock wxWidgets labels; Rust translates interface text at the widget/drawing boundary.
- `tools/localization_kit.py` assigns stable names to translatable XRC controls during ribbon/dialog generation. `tools/build_localization.py` generates typed localization hooks and the resource string inventory.
- `tools/build_translations.py` combines the glossary and secondary messages, derives punctuated captions used in resources and runtime-built dialogs, checks nonempty translations and unchanged formatting placeholders, and reports missing resource translations. No network or translation model is used by Impress at runtime.

After changing interface resources, run the relevant resource generator, then:

```sh
python3 tools/build_localization.py
python3 tools/build_translations.py
cargo build -p gui --release
```

Flags are from flag-icons v7.3.2 (MIT), with attribution and license under `crates/gui/assets/flags/`. Draft translation model provenance: `Helsinki-NLP/opus-mt-en-el` at `1653673990e6f1e3baf0a903f38e231730704444` and `Helsinki-NLP/opus-mt-en-ROMANCE` at `f8f3a28e8b6272d0ccc0290b832f699e154ae431` (Apache 2.0). Model weights are development tools and are not shipped.
