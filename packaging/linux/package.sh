#!/bin/sh
# Packs the release build as an AppImage, .deb, .rpm and tar.gz, plus a tar.gz of bawkbitd alone, each with a checksum. Usage: package.sh <tag such as v0.2.0> [output dir]
set -eu

tag="$1"
mkdir -p "${2:-dist}"
out="$(cd "${2:-dist}" && pwd)"
cd "$(dirname "$0")/../.."
name="bawkbit-$tag-linux-x64"
daemon="bawkbitd-$tag-linux-x64"
release="${CARGO_TARGET_DIR:-target}/release"
desktop=packaging/linux/bawkbit.desktop
icon=assets/brand/bawkbit.png
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

mkdir "$work/$name" "$work/$daemon"
cp "$release/bawkbit" "$release/bawkbitd" "$desktop" "$icon" LICENSE README.md "$work/$name/"
tar -czf "$out/$name.tar.gz" --owner=0 --group=0 -C "$work" "$name"
cp "$release/bawkbitd" LICENSE README.md "$work/$daemon/"
tar -czf "$out/$daemon.tar.gz" --owner=0 --group=0 -C "$work" "$daemon"

cargo deb --no-build --no-strip --output "$out"
# the builtin scan lists libraries of libraries and a glibc symbol no system provides, so rpm's own scanner does it
cargo generate-rpm --auto-req find-requires --output "$out"

# fetch <file> <sha256> <url>: the tools are pinned, so a changed upstream file fails the build instead of shipping
fetch() {
  curl -fsSL -o "$work/$1" "$3"
  echo "$2  $work/$1" | sha256sum -c -
}
fetch linuxdeploy c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d \
  https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage
fetch runtime 2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d \
  https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64
chmod +x "$work/linuxdeploy"
# containers have no FUSE to mount an AppImage with, so the tool unpacks itself
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 LDAI_OUTPUT="$out/$name.AppImage" LDAI_RUNTIME_FILE="$work/runtime" \
  "$work/linuxdeploy" --appdir "$work/AppDir" --executable "$release/bawkbit" --desktop-file "$desktop" --icon-file "$icon" \
  --output appimage

cd "$out"
for file in "$name.AppImage" "$name.tar.gz" "$daemon.tar.gz" bawkbit_*.deb bawkbit-*.rpm; do
  sha256sum "$file" > "$file.sha256"
done
