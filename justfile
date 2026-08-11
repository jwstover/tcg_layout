# TCG Layout — task runner
# Run `just` or `just --list` to see available recipes.

app_name := "TCG Layout"
bin_name := "tcg_layout"
built_app := "target/release/bundle/osx" / app_name + ".app"
install_dir := "/Applications"
installed_app := install_dir / app_name + ".app"
linux_bin_dir := env_var("HOME") / ".local/bin"
linux_desktop_dir := env_var("HOME") / ".local/share/applications"

# List available recipes
default:
    @just --list

# Run the app in dev mode
run:
    cargo run --bin tcg_layout

# Build, bundle, and install the app for this OS in one go
app: (bundle-os os()) (install-os os())

# Ensure cargo-bundle is installed.
# On nix-darwin, cargo-bundle's build needs libiconv on LIBRARY_PATH; discover it
# from the nix store if present (harmless no-op on non-Nix systems).
_ensure-bundle:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v cargo-bundle >/dev/null 2>&1 && exit 0
    iconv_lib=$(ls -d /nix/store/*libiconv*/lib 2>/dev/null | head -1 || true)
    [ -n "$iconv_lib" ] && export LIBRARY_PATH="$iconv_lib${LIBRARY_PATH:+:$LIBRARY_PATH}"
    cargo install cargo-bundle --locked

# Dispatch to the bundle recipe for the given OS ("macos" or anything else -> linux)
bundle-os os:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{os}}" = "macos" ]; then
        just bundle
    else
        just build-release
    fi

# Dispatch to the install recipe for the given OS ("macos" or anything else -> linux)
install-os os:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{os}}" = "macos" ]; then
        just install
    else
        just install-linux
    fi

# Build a release .app bundle (osx only; skips the DMG)
bundle: _ensure-bundle
    cargo bundle --release --format osx --bin tcg_layout

# Build a plain release binary (Linux and other non-macOS platforms)
build-release:
    cargo build --release --bin tcg_layout

# Install the built .app bundle into /Applications, replacing any existing copy (macOS)
install:
    rm -rf "{{installed_app}}"
    cp -R "{{built_app}}" "{{installed_app}}"
    @echo "Installed to {{installed_app}}"

# Install the release binary and a desktop entry for the current user (Linux).
# The real binary is installed as `{{bin_name}}-bin`, launched through a thin
# wrapper. This is needed because on this machine `cargo` resolves to a
# Nix-provided toolchain, which links the binary against Nix's own glibc/ld.so.
# That loader's default search path doesn't cover ordinary distro library
# directories, so transitive deps (e.g. libssl -> libz.so.1) can go unfound
# even though the binary's own RUNPATH lists /usr/lib as a fallback — RUNPATH
# only applies to an object's *direct* dependencies, not deps-of-deps. Passing
# that same RUNPATH through as LD_LIBRARY_PATH extends it to transitive
# lookups too, in the same priority order, so nothing that already resolved
# correctly (libc, libm, ...) gets shadowed by a mismatched system copy. This
# is a no-op wrapper (empty RUNPATH) for a normally-linked binary.
install-linux:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p "{{linux_bin_dir}}" "{{linux_desktop_dir}}"
    install -m 755 "target/release/{{bin_name}}" "{{linux_bin_dir}}/{{bin_name}}-bin"
    cat > "{{linux_bin_dir}}/{{bin_name}}" <<'EOF'
    #!/usr/bin/env bash
    dir="$(dirname "$(readlink -f "$0")")"
    runpath=$(readelf -d "$dir/{{bin_name}}-bin" 2>/dev/null | sed -n 's/.*Library r\(un\|\)path: \[\(.*\)\]/\2/p')
    export LD_LIBRARY_PATH="${runpath}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    exec "$dir/{{bin_name}}-bin" "$@"
    EOF
    chmod 755 "{{linux_bin_dir}}/{{bin_name}}"
    cat > "{{linux_desktop_dir}}/{{bin_name}}.desktop" <<EOF
    [Desktop Entry]
    Type=Application
    Name={{app_name}}
    Comment=Lay out TCG card images on printable pages
    Exec={{linux_bin_dir}}/{{bin_name}}
    Terminal=false
    Categories=Graphics;
    EOF
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "{{linux_desktop_dir}}" || true
    echo "Installed to {{linux_bin_dir}}/{{bin_name}} (desktop entry: {{linux_desktop_dir}}/{{bin_name}}.desktop)"
