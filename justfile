name := 'mawaqit'
appid := 'io.github.doctorit.mawaqit'

rootdir := ''
prefix := '/usr'

appdata := appid + '.metainfo.xml'
desktop := appid + '.desktop'

# Installation paths
base-dir := absolute_path(clean(rootdir / prefix))
cargo-target-dir := env('CARGO_TARGET_DIR', 'target')
appdata-dst := base-dir / 'share' / 'appdata' / appdata
bin-dst := base-dir / 'bin' / name
desktop-dst := base-dir / 'share' / 'applications' / desktop
icon-dst := base-dir / 'share' / 'icons' / 'hicolor' / 'scalable' / 'apps' / appid + '.svg'

# Default recipe which runs `just build-release`
default: build-release

# Runs `cargo clean`
clean:
    cargo clean

# Removes vendored dependencies
clean-vendor:
    rm -rf .cargo vendor vendor.tar

# `cargo clean` and removes vendored dependencies
clean-dist: clean clean-vendor

# Compiles with debug profile
build-debug *args:
    cargo build {{args}}

# Compiles with release profile
build-release *args: (build-debug '--release' args)

# Compiles release profile with vendored dependencies
build-vendored *args: vendor-extract (build-release '--frozen --offline' args)

# Runs a clippy check
check *args:
    cargo clippy --all-features {{args}} -- -W clippy::pedantic

# Runs a clippy check with JSON message format
check-json: (check '--message-format=json')

# Run the application for testing purposes
run *args:
    env RUST_BACKTRACE=full cargo run --release {{args}}

# Run unit tests (prayer engine, no GUI needed)
test *args:
    cargo test {{args}}

# Installs files
install:
    install -Dm0755 {{ cargo-target-dir / 'release' / name }} {{bin-dst}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'app.desktop' }} {{desktop-dst}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'app.metainfo.xml' }} {{appdata-dst}}
    install -Dm0644 resources/icon.svg {{icon-dst}}

# Uninstalls installed files
uninstall:
    rm {{bin-dst}} {{desktop-dst}} {{icon-dst}}

# Builds a .deb package using cargo-deb
deb: build-release
    cargo deb --no-build

# Builds a .rpm package using cargo-generate-rpm
rpm: build-release
    cargo generate-rpm

# Builds an Arch Linux pacman package (.pkg.tar.zst)
arch: build-release
    mkdir -p /tmp/pkg-arch
    rm -rf /tmp/pkg-arch/*
    mkdir -p /tmp/pkg-arch/usr/bin /tmp/pkg-arch/usr/share/applications /tmp/pkg-arch/usr/share/appdata /tmp/pkg-arch/usr/share/icons/hicolor/scalable/apps
    install -Dm0755 target/release/mawaqit /tmp/pkg-arch/usr/bin/mawaqit
    install -Dm0644 resources/app.desktop /tmp/pkg-arch/usr/share/applications/io.github.doctorit.mawaqit.desktop
    install -Dm0644 resources/app.metainfo.xml /tmp/pkg-arch/usr/share/appdata/io.github.doctorit.mawaqit.metainfo.xml
    install -Dm0644 resources/icon.svg /tmp/pkg-arch/usr/share/icons/hicolor/scalable/apps/io.github.doctorit.mawaqit.svg
    printf 'pkgname = cosmic-ext-mawaqit\npkgbase = cosmic-ext-mawaqit\npkgver = 0.1.0-1\npkgdesc = Islamic prayer times applet for COSMIC Desktop (Mawaqit مواقيت)\nurl = https://github.com/doctorit9/cosmic-ext-mawaqit\nbuilddate = %s\npackager = doctorit9 <https://github.com/doctorit9>\nsize = 28000000\narch = x86_64\nlicense = MIT\ndepend = libxkbcommon\ndepend = wayland\ndepend = alsa-lib\n' "$$(date +%s)" > /tmp/pkg-arch/.PKGINFO
    tar --zstd -cf target/cosmic-ext-mawaqit-0.1.0-1-x86_64.pkg.tar.zst -C /tmp/pkg-arch .PKGINFO usr

# Builds a standalone universal tarball with install script
tarball: build-release
    mkdir -p /tmp/pkg-tarball/cosmic-ext-mawaqit-0.1.0-x86_64/bin /tmp/pkg-tarball/cosmic-ext-mawaqit-0.1.0-x86_64/share/applications /tmp/pkg-tarball/cosmic-ext-mawaqit-0.1.0-x86_64/share/appdata /tmp/pkg-tarball/cosmic-ext-mawaqit-0.1.0-x86_64/share/icons/hicolor/scalable/apps
    install -Dm0755 target/release/mawaqit /tmp/pkg-tarball/cosmic-ext-mawaqit-0.1.0-x86_64/bin/mawaqit
    install -Dm0644 resources/app.desktop /tmp/pkg-tarball/cosmic-ext-mawaqit-0.1.0-x86_64/share/applications/io.github.doctorit.mawaqit.desktop
    install -Dm0644 resources/app.metainfo.xml /tmp/pkg-tarball/cosmic-ext-mawaqit-0.1.0-x86_64/share/appdata/io.github.doctorit.mawaqit.metainfo.xml
    install -Dm0644 resources/icon.svg /tmp/pkg-tarball/cosmic-ext-mawaqit-0.1.0-x86_64/share/icons/hicolor/scalable/apps/io.github.doctorit.mawaqit.svg
    printf '#!/usr/bin/env sh\nset -e\nPREFIX="$${PREFIX:-/usr/local}"\necho "Installing Mawaqit to $$PREFIX..."\ninstall -Dm0755 bin/mawaqit "$$PREFIX/bin/mawaqit"\ninstall -Dm0644 share/applications/io.github.doctorit.mawaqit.desktop "$$PREFIX/share/applications/io.github.doctorit.mawaqit.desktop"\ninstall -Dm0644 share/appdata/io.github.doctorit.mawaqit.metainfo.xml "$$PREFIX/share/appdata/io.github.doctorit.mawaqit.metainfo.xml"\ninstall -Dm0644 share/icons/hicolor/scalable/apps/io.github.doctorit.mawaqit.svg "$$PREFIX/share/icons/hicolor/scalable/apps/io.github.doctorit.mawaqit.svg"\necho "Done! Run mawaqit or add it to your COSMIC panel."\n' > /tmp/pkg-tarball/cosmic-ext-mawaqit-0.1.0-x86_64/install.sh
    chmod +x /tmp/pkg-tarball/cosmic-ext-mawaqit-0.1.0-x86_64/install.sh
    tar -czf target/cosmic-ext-mawaqit-0.1.0-x86_64-linux-gnu.tar.gz -C /tmp/pkg-tarball cosmic-ext-mawaqit-0.1.0-x86_64

# Vendor dependencies locally
vendor:
    mkdir -p .cargo
    cargo vendor --sync Cargo.toml | head -n -1 > .cargo/config.toml
    echo 'directory = "vendor"' >> .cargo/config.toml
    echo >> .cargo/config.toml
    rm -rf .cargo vendor

# Extracts vendored dependencies
vendor-extract:
    rm -rf vendor
    tar pxf vendor.tar
