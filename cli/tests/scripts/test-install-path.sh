#!/usr/bin/env bash
# The installer's note about PATH: the line for the user's shell, with the real
# prefix, only when the prefix's bin is not on PATH, and no profile file is
# ever created or changed. Fake release on disk (file:// URLs). No network,
# sudo or Docker.
#
#   cli/tests/scripts/test-install-path.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
ASSET=smart-commit-x86_64-unknown-linux-gnu.tar.gz
failures=0

stage=$work/stage/smart-commit-x86_64-unknown-linux-gnu
mkdir -p "$stage/bin" "$stage/share/smart-commit/scripts" "$stage/share/doc/smart-commit"
printf '#!/bin/sh\necho "smart-commit 1.0.0"\n' >"$stage/bin/smart-commit"
chmod +x "$stage/bin/smart-commit"
echo 1.0.0 >"$stage/share/smart-commit/VERSION"
echo licence >"$stage/share/doc/smart-commit/LICENSE"
mkdir -p "$work/releases/latest/download"
tar -C "$work/stage" -czf "$work/releases/latest/download/$ASSET" smart-commit-x86_64-unknown-linux-gnu
(cd "$work/releases/latest/download" && sha256sum "$ASSET" >SHA256SUMS)

home=$work/home

# run SHELL PATH ARGS...: installs with a clean HOME (bash itself sets an unset
# SHELL, so "no shell" is an empty one).
run() {
    local shell=$1 path=$2
    shift 2
    rm -rf "$home"
    mkdir -p "$home"
    env -i HOME="$home" PATH="$path" SHELL="$shell" SMART_COMMIT_RELEASE_BASE=file://"$work"/releases \
        bash "$ROOT/install.sh" --no-setup "$@" </dev/null >"$work/out" 2>&1
}

# expect NAME FILE LINE: the note names FILE and LINE, in at most 6 lines,
# and the home holds no profile file.
expect() {
    local name=$1 file=$2 line=$3 ok=1
    grep -qxF "  Add this line to $file:" "$work/out" || ok=0
    grep -qxF "    $line" "$work/out" || ok=0
    grep -q "open a new terminal" "$work/out" || ok=0
    [ "$(grep -c -e '^note: ' -e '^  ' "$work/out")" -le 6 ] || ok=0
    [ -z "$(find "$home" -maxdepth 1 \( -name .bashrc -o -name .zshrc -o -name .profile -o -name .config \))" ] || ok=0
    report "$name" "$ok"
}

report() {
    if [ "$2" = 1 ]; then
        echo "ok    $1"
    else
        echo "FAIL  $1"
        sed 's/^/        /' "$work/out"
        failures=$((failures + 1))
    fi
}

base=/usr/bin:/bin
# shellcheck disable=SC2016,SC2088  # $HOME, $PATH and ~ are literal in the line
{
    run /bin/bash "$base"
    expect "bash: ~/.bashrc" "~/.bashrc" 'export PATH="$HOME/.local/bin:$PATH"'
    run /usr/bin/zsh "$base"
    expect "zsh: ~/.zshrc" "~/.zshrc" 'export PATH="$HOME/.local/bin:$PATH"'
    run /usr/bin/fish "$base"
    expect "fish: config.fish" "~/.config/fish/config.fish" 'fish_add_path $HOME/.local/bin'
    run "" "$base"
    expect "unset SHELL: ~/.profile" "~/.profile" 'export PATH="$HOME/.local/bin:$PATH"'
    run /bin/dash "$base"
    expect "other shell: ~/.profile" "~/.profile" 'export PATH="$HOME/.local/bin:$PATH"'
    run /bin/bash "$base" --prefix "$home/tools"
    expect "prefix under HOME" "~/.bashrc" 'export PATH="$HOME/tools/bin:$PATH"'
    run /bin/bash "$base" --prefix "$work/opt"
    expect "prefix outside HOME" "~/.bashrc" "export PATH=\"$work/opt/bin:\$PATH\""
    run /usr/bin/fish "$base" --prefix "$work/opt"
    expect "prefix outside HOME, fish" "~/.config/fish/config.fish" "fish_add_path $work/opt/bin"
}

run /bin/bash "$home/.local/bin:$base"
ok=1
grep -q "is not on your PATH" "$work/out" && ok=0
report "prefix on PATH: no note" "$ok"

run /bin/bash "$base"
ok=1
grep -q "installed smart-commit 1.0.0" "$work/out" || ok=0
report "the install still happens" "$ok"

[ "$failures" = 0 ] || exit 1
