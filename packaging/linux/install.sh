#!/bin/sh
# Install the Impress launcher and icon for the current user, so the GNOME /
# Ubuntu dock and the app grid show the Impress icon instead of a gear.
#
# The dock matches a running window to a launcher by application ID. Impress's
# ID is its executable name, "Impress", so the launcher must be Impress.desktop.
#
# The binary is copied to one fixed place, $PREFIX/lib/impress/Impress, and
# the launcher always runs that copy. Installing another version replaces the
# copy, never the launcher's path, so apps that keep launchers in memory (an
# Electron app's "Open with") still start the version installed last.
#
# Usage:  packaging/linux/install.sh [path/to/Impress]      install
#         packaging/linux/install.sh --uninstall           remove
# The binary defaults to target/release/Impress. Set PREFIX to install
# elsewhere than ~/.local (for example PREFIX=/usr/local, run as root).
set -eu

APP_ID=Impress
# Launcher and icon names used before the executable was renamed.
LEGACY_ID=impress
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
PREFIX=${PREFIX:-"$HOME/.local"}
APPLICATIONS="$PREFIX/share/applications"
ICONS="$PREFIX/share/icons/hicolor"
DESKTOP_FILE="$APPLICATIONS/$APP_ID.desktop"
ICON_SOURCE="$ROOT/design/icons/color/logo.svg"
ICON_FILE="$ICONS/scalable/apps/$APP_ID.svg"
INSTALLED="$PREFIX/lib/impress/$APP_ID"

refresh_caches() {
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$APPLICATIONS" || true
    command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$ICONS" || true
}

remove_legacy() {
    rm -f "$APPLICATIONS/$LEGACY_ID.desktop" "$ICONS/scalable/apps/$LEGACY_ID.svg"
}

# Dictionary files and ownership receipts live in the user data folder.
# Preserve them on uninstall and upgrade; never remove system dictionaries.
if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$DESKTOP_FILE" "$ICON_FILE" "$INSTALLED"
    rmdir "$(dirname "$INSTALLED")" 2>/dev/null || true
    remove_legacy
    refresh_caches
    echo "Removed the Impress launcher and icon from $PREFIX."
    exit 0
fi

BINARY=${1:-"$ROOT/target/release/Impress"}
if [ ! -x "$BINARY" ]; then
    echo "No executable at $BINARY. Build it first (cargo build -p gui --release) or pass its path." >&2
    exit 1
fi
BINARY=$(cd "$(dirname "$BINARY")" && pwd)/$(basename "$BINARY")

remove_legacy
mkdir -p "$APPLICATIONS" "$(dirname "$ICON_FILE")" "$(dirname "$INSTALLED")"
cp "$ICON_SOURCE" "$ICON_FILE"
# Replace the copy in one step, so an Impress still running keeps its file.
cp "$BINARY" "$INSTALLED.new" && chmod 755 "$INSTALLED.new" && mv -f "$INSTALLED.new" "$INSTALLED"
sed "s|@EXEC@|$INSTALLED|g" "$ROOT/packaging/linux/Impress.desktop.in" > "$DESKTOP_FILE"
chmod 644 "$DESKTOP_FILE" "$ICON_FILE"
refresh_caches
echo "Installed $INSTALLED (from $BINARY), $DESKTOP_FILE and the Impress icon."
echo "Restart Impress so the dock picks up the icon."
