#!/bin/sh
# Builds jox in release mode and installs it as an app bundle.
#
#   build-aux/macos/install.sh [/Applications/Jox.app]
#
# The bundle links GTK, libadwaita and mpv from Homebrew, so it runs on Macs
# that have them installed (see the README).
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
app=${1:-/Applications/Jox.app}
contents="$app/Contents"
build="$root/build-app"
stage="$build/stage"
brew=$(brew --prefix)

# Meson reads the version from git when it configures, so configure every time.
if [ -d "$build" ]; then
    meson setup --reconfigure "$build" "$root"
else
    meson setup "$build" "$root" \
        --prefix="$contents" \
        --bindir=MacOS \
        --datadir=Resources/share \
        --localedir=Resources/share/locale \
        -Drust-target=release
fi
meson compile -C "$build"

rm -rf "$stage"
meson install -C "$build" --no-rebuild --destdir "$stage"
staged="$stage$contents"
glib-compile-schemas "$staged/Resources/share/glib-2.0/schemas"

version=$(meson introspect "$build" --projectinfo | sed -n 's/.*"version": "\([^"]*\)".*/\1/p')
sed "s/@VERSION@/$version/" "$root/crates/jox/share/macos/Info.plist" > "$staged/Info.plist"
plist="/usr/libexec/PlistBuddy"
"$plist" -c "Add :LSEnvironment dict" "$staged/Info.plist"
"$plist" -c "Add :LSEnvironment:GSETTINGS_SCHEMA_DIR string $contents/Resources/share/glib-2.0/schemas" "$staged/Info.plist"
"$plist" -c "Add :LSEnvironment:XDG_DATA_DIRS string $contents/Resources/share:$brew/share" "$staged/Info.plist"
cp "$root/crates/jox/share/macos/AppIcon.icns" "$staged/Resources/"

# A signature, even ad hoc, gives macOS a stable identity for permissions.
codesign --force --sign - "$staged/MacOS/jox"
codesign --force --sign - "$stage$app"

rm -rf "$app"
mkdir -p "$(dirname "$app")"
mv "$stage$app" "$app"
rm -rf "$stage"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$app"
echo "Installed $app ($version)"
