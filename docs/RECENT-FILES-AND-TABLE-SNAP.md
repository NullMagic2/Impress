# Recent files and table margin snapping

File → Recent uses Yaru’s `recentfilelist` document-and-clock command icon
(from LibreOffice’s Yaru theme), with a dropdown arrow below its caption.
It opens a native menu of the most recently opened or saved files,
newest first. Five files are remembered by default. The menu's count setting
accepts 0–20; zero disables history. Clear recent files removes the list without
changing the configured count. The list survives restarts in recent-files.json
in Impress's normal per-user/portable data directory.

Only successful open/save completions add an entry. Reopening moves the file to
the top, and canonical paths avoid duplicate relative/symlink entries. Missing
files remain visible but disabled. Each entry shows its directory to distinguish
identical file names. Recent opens use the same background import and document
stack as File → Open, preserving unsaved documents.

Dragging a table near a page margin attracts its corresponding edge within six
display-independent pixels. The targets are the physical page margin rectangles
used by the rulers. Zoom changes their screen position, but not the tolerance on
the display. Inline table moves snap horizontally; their vertical placement
remains in document flow. Floating table moves can snap on both axes. Shift
keeps moves horizontal or keeps a corner resize proportional. Whole-table
resizes snap the right/bottom edge, and the outer column border also snaps.
Active margin guides and the table preview show the snapped geometry. Escape
cancels normally. Automatic row height can still grow to fit its text.

Regression tests exercise recent-file order, deduplication, persistence, limits,
clearing/disabling, corrupt settings and missing files, plus margin projection
at multiple zoom/display scales, move/resize thresholds, Shift proportions and
the geometry consumed when a table drag is committed.
