#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

# Avoid WebKit/Wayland renderer issues during the Linux build environment.
export GDK_BACKEND=x11
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export __NV_DISABLE_EXPLICIT_SYNC=1

if ! command -v makepkg >/dev/null 2>&1; then
  echo "Error: makepkg is required to build a .pkg.tar.zst package." >&2
  exit 1
fi
if ! command -v ar >/dev/null 2>&1; then
  echo "Error: ar is required to extract the Tauri package." >&2
  exit 1
fi

case "$(uname -m)" in
  x86_64) PKG_ARCH=x86_64 ;;
  aarch64|arm64) PKG_ARCH=aarch64 ;;
  *)
    echo "Error: unsupported architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

APP_VERSION="$(node -p "JSON.parse(require('fs').readFileSync('package.json')).version")"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT
ROOTFS="$WORK_DIR/rootfs"
mkdir -p "$ROOTFS"


echo "==> Type-checking frontend..."
pnpm typecheck

echo "==> Building the Linux bundle..."
pnpm tauri build --bundles deb

DEB_FILE="$(find "$PROJECT_ROOT/src-tauri/target/release/bundle/deb" -maxdepth 1 -type f -name '*.deb' -printf '%T@ %p\n' | sort -nr | head -n1 | cut -d' ' -f2-)"
if [[ -z "$DEB_FILE" || ! -f "$DEB_FILE" ]]; then
  echo "Error: Tauri did not produce a .deb package." >&2
  exit 1
fi

DATA_MEMBER="$(ar t "$DEB_FILE" | grep '^data\.tar\.' | head -n1)"
if [[ -z "$DATA_MEMBER" ]]; then
  echo "Error: could not find the data archive in $DEB_FILE." >&2
  exit 1
fi

case "$DATA_MEMBER" in
  data.tar.gz)  ar p "$DEB_FILE" "$DATA_MEMBER" | tar -xzf - -C "$ROOTFS" ;;
  data.tar.xz)  ar p "$DEB_FILE" "$DATA_MEMBER" | tar -xJf - -C "$ROOTFS" ;;
  data.tar.zst) ar p "$DEB_FILE" "$DATA_MEMBER" | tar --zstd -xf - -C "$ROOTFS" ;;
  *)
    echo "Error: unsupported Tauri data archive: $DATA_MEMBER" >&2
    exit 1
    ;;
esac

PACKAGE_DIR="$PROJECT_ROOT/out/arch"
rm -rf "$PACKAGE_DIR"
mkdir -p "$PACKAGE_DIR"
cat > "$PACKAGE_DIR/PKGBUILD" <<EOF_PKGBUILD
pkgname=yt-dlp-gui
pkgver=$APP_VERSION
pkgrel=1
pkgdesc='A modern GUI for yt-dlp'
arch=('$PKG_ARCH')
url='https://github.com/Areo-RGB/yt-dlp'
license=('MIT')
depends=('libayatana-appindicator' 'webkit2gtk-4.1' 'gtk3' 'gst-plugins-good' 'gst-plugins-bad')
source=()
sha256sums=()

package() {
  cp -a "\$startdir/rootfs/." "\$pkgdir/"
}
EOF_PKGBUILD
cp -a "$ROOTFS" "$PACKAGE_DIR/rootfs"


echo "==> Creating Arch package..."
(
  cd "$PACKAGE_DIR"
  makepkg --force --clean
)
rm -rf "$PACKAGE_DIR/rootfs" "$PACKAGE_DIR/PKGBUILD"

echo "==> ZST package build complete: $PACKAGE_DIR/yt-dlp-gui-$APP_VERSION-1-$PKG_ARCH.pkg.tar.zst"
