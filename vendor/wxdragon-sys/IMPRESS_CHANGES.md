# Impress changes to wxdragon-sys 0.9.22

Upstream: https://github.com/AllenDang/wxDragon (MIT OR Apache-2.0).
Every change is marked with an `Impress:` comment in `build.rs`.

- `build.rs` patches the downloaded wxWidgets 3.3.2 source before building
  it: `include/wx/gtk/private/wayland.h` frees `wl_pointer` and `wl_seat`
  with `*_destroy` instead of `wl_pointer_release` / `wl_seat_release`.
  wxWidgets binds `wl_seat` at version 2, where those release requests do
  not exist, so on Wayland the compositor ended the connection (protocol
  error, the window vanished) whenever a pointer device went away.
- `cpp/src/event.cpp` adds `wxd_UpdateUIEvent_SetMode`, so Impress can stop
  wxWidgets from sending a wxUpdateUIEvent to every window (hundreds in the
  Ribbon, hidden pages included) after every batch of events; Impress does not
  use update-UI events.
- `cpp/src/dc.cpp` adds `wxd_DC_GetNativeHandle` (`wxDC::GetHandle`, a
  `cairo_t*` on GTK 3), so the document canvas paints a persistent cairo
  surface instead of converting its whole frame to a bitmap and pixbuf.

- `cpp/src/file_dialog.cpp` and `cpp/include/dialogs/wxd_dialogs.h` add an
  owned wxFileDialogCustomizeHook for Impress Save's Embed fonts checkbox.
  It uses native/custom GTK controls and the modern Windows file dialog;
  native/DOCX filters enable the checkbox, and acceptance transfers the value.

- `cpp/src/event.cpp` routes window `wxEVT_LEFT_DCLICK` through LeftDown
  callbacks, since wxDragon does not expose a window double-click enum.
  The paired binding is removed when the corresponding closure is unbound.
  While translating the event, Impress can query its native double-click flag.


- `cpp/src/panel.cpp` exposes focus for the document panel itself when it
  contains inline review controls.
