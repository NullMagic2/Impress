# Copy followed immediately by Paste

Copy/Cut prepare their clipboard representations on a worker. A Paste or
Paste Text Only requested while that preparation is running now waits for
successful publication, then uses the new clipboard contents at the requested
destination. The destination is checked again before applying the paste.
Changing the document or selection while waiting discards the pending paste;
Escape cancels the operation. A failed or cancelled Copy does not paste stale
clipboard contents, and Cut deletes its source only after publication succeeds.

## Keyboard layouts

Document command shortcuts use key-down events, separately from Unicode text
and input-method composition. This applies to all existing letter shortcuts,
including Save, Find, formatting, clipboard operations and Undo/Redo. Find's
modeless window and the equation editor use the same resolver for their custom
shortcuts.

On Windows, wxWidgets supplies native virtual-key codes. On Linux (X11 and
Wayland), Impress reads the current GDK hardware key and character. A Latin
letter uses its actual letter, preserving AZERTY, QWERTZ and Dvorak shortcuts.
For non-Latin symbols, libxkbcommon translates the hardware key through a US
reference keymap, matching wxWidgets' Windows-compatible convention. There is
no per-language translation table. Shift and Caps Lock do not change the
command identity; Shift still selects commands such as Paste Text Only or Redo.
The reference map is cached, does not alter the desktop's keyboard layout, and
ignores XKB_DEFAULT_* layout environment variables.

AltGr/level-three input is excluded from shortcut dispatch. On Linux the native
level-three modifier is checked; on Windows right Alt is checked. Ordinary
character events and committed IME strings retain their original Unicode text.
Native text fields continue to use their platform's built-in editing bindings.

Linux needs the standard xkb-data keyboard definitions. If native event data or
the reference map is unavailable, existing toolkit Latin/virtual-key shortcuts
remain available. Custom XKB keycode definitions that depart from the standard
evdev keycodes require corresponding desktop/toolkit support.

Regression coverage includes Greek, Russian, Ukrainian, Arabic, Hebrew, Thai,
Armenian, Georgian and Devanagari keymaps, plus French AZERTY, German QWERTZ,
Dvorak and Turkish Latin letters. Full application testing records the layouts
and commands actually exercised; it does not claim every possible input method
has been manually tested.
