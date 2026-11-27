#!/usr/bin/env bash
set -euo pipefail
dest=$1
revision=557a81886f082cd57532cab0a9880e8393ecf52c
mkdir -p "$(dirname "$dest")"
temp=$(mktemp -d "$(dirname "$dest")/.plutovg.XXXXXX")
trap 'rm -rf "$temp"' EXIT
git -C "$temp" init -q
git -C "$temp" fetch -q --depth=1 https://github.com/sammycage/plutovg.git "$revision"
git -C "$temp" checkout -q --detach "$revision"
if ! mv -T "$temp" "$dest"; then
    test "$(git -C "$dest" rev-parse HEAD)" = "$revision"
fi
