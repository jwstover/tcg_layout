# TCG Layout — task runner
# Run `just` or `just --list` to see available recipes.

app_name := "TCG Layout"
built_app := "target/release/bundle/osx" / app_name + ".app"
install_dir := "/Applications"
installed_app := install_dir / app_name + ".app"

# List available recipes
default:
    @just --list

# Run the app in dev mode
run:
    cargo run --bin tcg_layout

# Build, bundle, and install the .app into /Applications in one go
app: bundle install

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

# Build a release .app bundle (osx only; skips the DMG)
bundle: _ensure-bundle
    cargo bundle --release --format osx --bin tcg_layout

# Install the built bundle into /Applications, replacing any existing copy
install:
    rm -rf "{{installed_app}}"
    cp -R "{{built_app}}" "{{installed_app}}"
    @echo "Installed to {{installed_app}}"
