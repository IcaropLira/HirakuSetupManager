#!/bin/sh
# Fixa o kitty nos favoritos do GNOME Shell (e do Cinnamon/Mint, se for o caso).
set -u
desktop_id=""
for candidate in \
    "$HIRAKU_HOME/.local/share/applications/kitty.desktop" \
    "/usr/share/applications/kitty.desktop" \
    "/var/lib/flatpak/exports/share/applications/org.kovidgoyal.kitty.desktop"; do
    if [ -f "$candidate" ]; then desktop_id="$(basename "$candidate")"; break; fi
done
if [ -z "$desktop_id" ]; then echo "kitty.desktop não encontrado: nada a fixar"; exit 0; fi
command -v gsettings >/dev/null 2>&1 || exit 0

pin() {  # $1 schema  $2 chave
    gsettings list-schemas 2>/dev/null | grep -qx "$1" || return 0
    current="$(gsettings get "$1" "$2" 2>/dev/null || echo "[]")"
    case "$current" in *"$desktop_id"*) echo "kitty já está fixado ($1)"; return 0 ;; esac
    if [ "$current" = "@as []" ] || [ "$current" = "[]" ]; then
        new="['$desktop_id']"
    else
        new="$(printf '%s' "$current" | sed "s/]$/, '$desktop_id']/")"
    fi
    if gsettings set "$1" "$2" "$new" 2>/dev/null; then echo "kitty fixado ($1)"
    else echo "aviso: não consegui fixar o kitty ($1)"; fi
}
pin org.gnome.shell favorite-apps
pin org.cinnamon favorite-apps
exit 0
