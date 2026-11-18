#!/bin/sh
# Integra o kitty instalado em ~/.local/kitty.app ao menu de aplicativos (.desktop).
# Se o kitty veio do pacote da distro, o .desktop já existe e não há o que fazer.
set -u
APP="$HIRAKU_HOME/.local/kitty.app"
if [ ! -d "$APP/share/applications" ]; then
    echo "kitty do sistema (ou não instalado em ~/.local): nada a integrar"; exit 0
fi
DEST="${XDG_DATA_HOME:-$HIRAKU_HOME/.local/share}/applications"
mkdir -p "$DEST"
for f in kitty.desktop kitty-open.desktop; do
    [ -f "$APP/share/applications/$f" ] || continue
    cp "$APP/share/applications/$f" "$DEST/$f"
    sed -i "s|^Icon=kitty|Icon=$APP/share/icons/hicolor/256x256/apps/kitty.png|g" "$DEST/$f"
    sed -i "s|^TryExec=kitty|TryExec=$APP/bin/kitty|g; s|^Exec=kitty|Exec=$APP/bin/kitty|g" "$DEST/$f"
done
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DEST" >/dev/null 2>&1 || true
fi
echo "kitty.desktop instalado em $DEST"
