# Spellchecking (build 0.8.6-31)

Open **Spelling** on the bottom status bar, immediately after **Graphical**, **Typst**, and **Split**. Only the Spelling menu button appears here; the language and checking toggle are inside its menu. The old Home ribbon Language button has moved here.

- **Check spelling as you type** toggles red squiggles. It starts enabled, and the choice is remembered for the next launch and new documents.
- **Language** always lists twenty proofing choices on both Linux and Windows, including **Portuguese (Portugal)**, **Portuguese (Brazil)**, and **Greek**. Missing dictionaries are marked **dictionary not installed**; choosing one still tags the selected text or future typing, and offers local or system-wide installation. Choosing one tags only selected text, or future typing at the caret, with that language. Other text retains its language. Background checks use the language of each text run, including inherited character and paragraph styles; a document can mix English, Portuguese, Greek and other installed languages. The menu reflects the language of the current text selection, or Mixed when languages differ.
- Right-click a red-underlined word to choose a suggestion or **Add to dictionary**. Replacements are undoable. A stale suggestion cannot replace text edited since the menu opened.
- **Check selection…** reviews only the selected byte ranges, including a selection across paragraphs. **Check document…** reviews the full document. The Review ribbon command and **F7** review the selection when text is selected and the full document otherwise. The native Spelling dialog stays open as you review each word, showing its surrounding text, proofing language, suggestions, and an editable replacement. Use **Change**, **Change All**, **Ignore Once**, **Ignore All**, or **Add to Dictionary**. Double-click a suggestion to change the word. Change All and Ignore All affect matching words in the same language within the chosen scope. Each correction is undoable. Close or Escape ends the review; completed reviews stay in the same window.
- Choosing a missing dictionary opens **Install spelling dictionary** with **Install locally**, **Install system-wide**, and **Cancel**.
- Installation opens a separate progress window with **Close**. Closing it dismisses the window while installation continues. Reopen it from **Spelling → Dictionary progress…** to see progress, completion, or an error. Only one installation runs at a time. Available dictionaries refresh automatically after success.
- A review with unavailable dictionaries reports that some text could not be checked. Canceling installation does not remove the selected text language.

## Linux

Impress uses Enchant 2 and its installed Hunspell/Aspell providers. The Debian package depends on `libenchant-2-2`. Dictionaries are installed on demand through Impress or the operating system.

On Ubuntu and Debian-based systems, both installation choices use `.deb` packages from the configured APT repositories. **Local** downloads the selected language package, extracts only its dictionary and copyright notice into the account's Impress data folder, validates it, and publishes it atomically. It runs no package maintainer scripts and requires no administrator password. **System-wide** runs APT through PolicyKit (`pkexec`), which opens the operating system's authentication prompt. Impress never reads or stores the password. APT checks repository/package authenticity; insecure/unauthenticated package options are disabled and package removal is prohibited. Other Linux distributions currently report that automatic installation is unsupported. For a source or portable build on Ubuntu/Debian:

```sh
sudo apt install libenchant-2-2 hunspell-en-us
# Optional examples:
sudo apt install hunspell-pt-pt hunspell-pt-br hunspell-el
cargo build -p gui --release --locked
```

Restart Impress after installing dictionaries. Enchant is loaded at runtime, so no Enchant development headers are required. Without Enchant or a requested dictionary, the document remains editable and the language list marks missing dictionaries.

## Windows

Impress uses the built-in Windows Spell Checking API for system dictionaries. **System-wide** launches Windows `Dism.exe` with UAC elevation to install the selected language's **Basic typing** capability through Windows servicing. Windows handles administrator consent or credentials; Impress never collects passwords. A reboot requirement is reported without automatically restarting the computer.

Windows system language capabilities cannot be installed per user. **Local** therefore downloads private Hunspell dictionaries from the official LibreOffice dictionary repository. PowerShell performs the HTTPS downloads; file sizes and SHA-256 checksums are pinned to the upstream revision recorded in `crates/gui/assets/dictionary-catalog.json`. Dictionary licenses and notices are retained. Downloaded files are data; Impress does not execute downloaded installers. The Hunspell engine is compiled into Impress and reads these private dictionaries without requiring Windows language changes. Native Windows installation and UAC interaction still need verification on Windows.

## Language choices

| Language | Tag | Ubuntu/Debian dictionary |
| --- | --- | --- |
| Czech | `cs-CZ` | `hunspell-cs` |
| Danish | `da-DK` | `hunspell-da` |
| Dutch | `nl-NL` | `hunspell-nl` |
| English (United Kingdom) | `en-GB` | `hunspell-en-gb` |
| English (United States) | `en-US` | `hunspell-en-us` |
| French | `fr-FR` | `hunspell-fr` |
| German | `de-DE` | `hunspell-de-de` |
| Greek | `el-GR` | `hunspell-el` |
| Hungarian | `hu-HU` | `hunspell-hu` |
| Italian | `it-IT` | `hunspell-it` |
| Norwegian (Bokmål) | `nb-NO` | `hunspell-no` |
| Polish | `pl-PL` | `hunspell-pl` |
| Portuguese (Brazil) | `pt-BR` | `hunspell-pt-br` |
| Portuguese (Portugal) | `pt-PT` | `hunspell-pt-pt` |
| Romanian | `ro-RO` | `hunspell-ro` |
| Russian | `ru-RU` | `hunspell-ru` |
| Slovak | `sk-SK` | `hunspell-sk` |
| Spanish | `es-ES` | `hunspell-es` |
| Swedish | `sv-SE` | `hunspell-sv` |
| Ukrainian | `uk-UA` | `hunspell-uk` |

Regional Portuguese dictionaries are never substituted for one another. A neutral Greek dictionary (`el`) can serve Greek (`el-GR`). Dictionary availability changes after installing language features; restart Impress to refresh provider discovery.

## Private dictionaries

Local dictionaries use the bundled Hunspell 1.7.2 engine on both platforms. They are stored under `$XDG_DATA_HOME/impress/dictionaries` (or `~/.local/share/impress/dictionaries`) on Linux and `%LOCALAPPDATA%\Impress\dictionaries` on Windows. They are separate from portable document/settings storage. Existing private dictionaries are discovered at startup, take precedence over the matching system provider, and remain separate for Brazilian and European Portuguese. The engine supports each dictionary's declared encoding and Unicode paths. Hunspell and MySpell license notices ship with the application; the full corresponding source is included in the source archive.

## Storage and limits

`spelling-settings.json` stores the default toggle and fallback language. Selected-text languages are document formatting and survive native/DOCX saves. `spelling-dictionary.json` stores accepted words by language. Both use Impress' existing data-folder policy: beside a writable portable executable, otherwise in the user's Impress data folder. Personal dictionary writes create the data folder if necessary and use a lock and atomic replacement.

Checks run on background threads, debounce typing for 700 ms, and discard results from an older document revision, layout generation, language, or invalidated dictionary. Linux word segmentation preserves Unicode byte positions and normalizes typographic apostrophes only for dictionary lookup. Marks use rendered text geometry and are excluded from printed/exported output. The existing bounds remain: 64 KiB per paragraph, 2 MiB per check, and 256 reported issues. A bounded result is reported; further correction sessions can review remaining issues. Paragraph and character styles marked `lng_no_proof` are skipped. Missing dictionaries are reported per language while available languages continue checking. Unmarked Greek words are recognized by their Unicode script and routed to a Greek dictionary even when Normal inherits the built-in English default. Explicit run languages and language-specific named styles take precedence. Mixed Greek/Latin text retains its original byte positions; an unavailable Greek dictionary is reported rather than substituted with English. General statistical language detection and grammar checking are not implemented.

## Manage dictionaries

Open **Spelling → Manage dictionaries…**. The list shows dictionaries installed through Impress, with **Language** and **Location** columns. **Add…** chooses a language and Local or System-wide installation. **Delete…** removes the selected installation after confirmation; **Refresh** reloads the list. Operations share the closable progress window.

Impress records each successful installation under the account data folder's `dictionary-receipts` directory, beside its private dictionaries and separate from the application executable. Application upgrades and uninstall/reinstall preserve both folders. The application uninstaller removes application files, launchers and icons; dictionary removal remains an explicit action in this manager.

Only an installation with a matching record and unchanged identity appears in the manager. Dictionaries supplied by the operating system, another program, or older Impress versions without installation records are omitted. Missing, replaced or externally modified installations are omitted too. They can still be used for spelling if available. Impress never adopts them for deletion. Losing or manually deleting the account data folder also loses the installation records.

Local removal deletes only the verified dictionary directory. Ubuntu/Debian system removal requests PolicyKit authorization and removes only the recorded dictionary package through `dpkg`, refusing dependency-breaking removal and leaving other packages alone. Windows system removal requests UAC authorization and removes the recorded Basic typing capability through DISM. System removal affects all users; the confirmation explains this. Neither platform automatically removes dictionaries when Impress itself is uninstalled.
