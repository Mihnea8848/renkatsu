#!/bin/sh
# Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
# SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0
#
# Installs Renkatsu for the current user: the programs in ~/.local/bin, and
# Renkatsu Studio in the application menu. Run it again to update, or with
# --uninstall to remove it. Nothing outside the home folder is touched.
set -eu
here="$(dirname "$(readlink -f "$0")")"
bin="$HOME/.local/bin"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
icons="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/256x256/apps"

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$bin/renkatsu" "$bin/rk" "$apps/renkatsu.desktop" "$icons/renkatsu.png"
    echo "Renkatsu is uninstalled. Projects and settings were kept."
    exit 0
fi

mkdir -p "$bin" "$apps" "$icons"
install -m 755 "$here/renkatsu" "$here/rk" "$bin/"
install -m 644 "$here/renkatsu.png" "$icons/renkatsu.png"
sed "s|^Exec=renkatsu|Exec=$bin/renkatsu|" "$here/renkatsu.desktop" >"$apps/renkatsu.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$apps" || true
echo "Renkatsu Studio is installed and in the application menu."
echo "Run it, then choose your Koikatsu folder in the project selector."
