pkgname=aether
pkgver=0.1.0
pkgrel=1
pkgdesc="Aether"
arch=('x86_64')
license=('custom')
depends=('gtk3' 'webkit2gtk-4.1' 'libxkbcommon-x11' 'fontconfig' 'vulkan-icd-loader' 'libsecret' 'git' 'hicolor-icon-theme')
optdepends=('vulkan-radeon: AMD GPU' 'vulkan-intel: Intel GPU' 'nvidia-utils: NVIDIA GPU')
makedepends=('cargo' 'cmake' 'pkgconf' 'clang')
options=('!lto')

build() {
  cd "$startdir"
  # makepkg sets SOURCE_DATE_EPOCH, which turns off ggml's native and AVX builds
  export GGML_NATIVE=ON
  cargo build --release --locked -p aether --target-dir target
}

package() {
  local id=dev.aether.Aether icons="$startdir/crates/app/assets/app-icons"
  install -Dm755 "$startdir/target/release/aether" "$pkgdir/usr/bin/aether"
  install -Dm644 "$startdir/packaging/linux/$id.desktop" "$pkgdir/usr/share/applications/$id.desktop"
  for s in 16 32 48 64 128 256 512 1024; do
    install -Dm644 "$icons/$s.png" "$pkgdir/usr/share/icons/hicolor/${s}x${s}/apps/$id.png"
  done
  install -Dm644 "$icons/aether.svg" "$pkgdir/usr/share/icons/hicolor/scalable/apps/$id.svg"
}
