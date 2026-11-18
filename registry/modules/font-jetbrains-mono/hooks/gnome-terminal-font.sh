#!/bin/sh
# Seleciona a Nerd Font no perfil padrão do GNOME Terminal (se ele existir).
# Nunca falha: em outros terminais a fonte só fica instalada.
set -u
command -v gsettings >/dev/null 2>&1 || exit 0
gsettings writable org.gnome.Terminal.Legacy.ProfilesList default >/dev/null 2>&1 || {
    echo "GNOME Terminal não detectado: nada a configurar"; exit 0; }
profile="$(gsettings get org.gnome.Terminal.Legacy.ProfilesList default 2>/dev/null | tr -d "'")"
if [ -z "$profile" ] || [ "$profile" = "@as []" ]; then
    echo "perfil padrão do GNOME Terminal não encontrado"; exit 0
fi
schema="org.gnome.Terminal.Legacy.Profile:/org/gnome/terminal/legacy/profiles:/:$profile/"
gsettings set "$schema" use-system-font false >/dev/null 2>&1 || true
gsettings set "$schema" font 'JetBrainsMono Nerd Font 11' >/dev/null 2>&1 || true
echo "fonte do GNOME Terminal ajustada (perfil $profile)"
