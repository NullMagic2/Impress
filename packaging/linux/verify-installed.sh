#!/bin/bash
# Run inside the target distribution after apt installs the generated package.
set -euo pipefail
output=${1:?output directory required}
mkdir -p "$output"
ldd /usr/lib/impress/Impress > "$output/ldd-gui.txt"
ldd /usr/lib/impress/impress-convert > "$output/ldd-converter.txt"
if grep -q 'not found' "$output"/ldd-*.txt; then cat "$output"/ldd-*.txt; exit 1; fi
impress-convert --help > "$output/converter-help.txt" 2>&1
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
impress-convert crates/docx/tests/fixtures/mixed.docx "$work/roundtrip.impress"
impress-convert "$work/roundtrip.impress" "$work/roundtrip.docx"
python3 - "$work/roundtrip.docx" <<'PY'
import sys,zipfile,xml.etree.ElementTree as ET
with zipfile.ZipFile(sys.argv[1]) as z:
 assert z.testzip() is None
 root=ET.fromstring(z.read('word/document.xml'))
 assert root.find('.//{http://schemas.openxmlformats.org/wordprocessingml/2006/main}t') is not None
PY
export GDK_BACKEND=x11
unset WAYLAND_DISPLAY
export XDG_CONFIG_HOME="$work/config" XDG_DATA_HOME="$work/data" XDG_CACHE_HOME="$work/cache"
export IMPRESS_SMOKE_OUTPUT=$(realpath "$output")
xvfb-run -a -s '-screen 0 1600x1000x24' bash -c '
 set -euo pipefail
 impress > "$IMPRESS_SMOKE_OUTPUT/gui-startup.log" 2>&1 &
 app_pid=$!
 trap "kill $app_pid 2>/dev/null || true" EXIT
 sleep 8
 kill -0 "$app_pid"
 xdotool search --onlyvisible --class Impress > "$IMPRESS_SMOKE_OUTPUT/windows.txt"
 test -s "$IMPRESS_SMOKE_OUTPUT/windows.txt"
 scrot "$IMPRESS_SMOKE_OUTPUT/empty-document.png"
'
if grep -Ei 'panicked at|error while loading shared libraries|Failed to initialize wxWidgets|assertion failed|invalid itemid' "$output/gui-startup.log"; then exit 1; fi
printf 'Passed: install, dynamic linking, public launchers, DOCX round trip and GUI window startup.\n' > "$output/RESULT.txt"
