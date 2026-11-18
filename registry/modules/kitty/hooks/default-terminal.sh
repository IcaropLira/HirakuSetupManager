#!/bin/sh
# Define o kitty como terminal padrão: xdg-terminals.list, update-alternatives
# (só em modo system e com `sudo -n`, nunca pergunta senha) e gsettings do GNOME.
# Cada mecanismo só é tocado se existir; nada aqui falha a instalação.
set -u
KITTY_BIN="$(command -v kitty 2>/dev/null || true)"
[ -n "$KITTY_BIN" ] || [ ! -x "$HIRAKU_BIN_DIR/kitty" ] || KITTY_BIN="$HIRAKU_BIN_DIR/kitty"
if [ -z "$KITTY_BIN" ]; then echo "kitty não encontrado: nada a fazer"; exit 0; fi

# a) xdg-terminal-exec: kitty.desktop como primeira opção
CFG="${XDG_CONFIG_HOME:-$HIRAKU_HOME/.config}"
LIST="$CFG/xdg-terminals.list"
mkdir -p "$CFG"
if [ "$(head -n1 "$LIST" 2>/dev/null)" != "kitty.desktop" ]; then
    {
        echo "kitty.desktop"
        if [ -f "$LIST" ]; then grep -vx "kitty.desktop" "$LIST" 2>/dev/null || true; fi
    } > "$LIST.tmp" && mv "$LIST.tmp" "$LIST"
    echo "kitty é o primeiro de $LIST"
fi

# b) x-terminal-emulator (Debian/Ubuntu/Mint), só com sudo sem senha
if [ "$HIRAKU_MODE" = "system" ] && command -v update-alternatives >/dev/null 2>&1; then
    if sudo -n true 2>/dev/null; then
        if sudo -n update-alternatives --install /usr/bin/x-terminal-emulator x-terminal-emulator "$KITTY_BIN" 50 >/dev/null 2>&1 \
           && sudo -n update-alternatives --set x-terminal-emulator "$KITTY_BIN" >/dev/null 2>&1; then
            echo "kitty definido como x-terminal-emulator"
        else
            echo "aviso: não consegui registrar o kitty em update-alternatives"
        fi
    else
        echo "sem sudo não interativo: update-alternatives foi pulado"
    fi
fi

# c) chaves de terminal padrão do GNOME / Cinnamon (Mint)
if command -v gsettings >/dev/null 2>&1; then
    for schema in org.gnome.desktop.default-applications.terminal org.cinnamon.desktop.default-applications.terminal; do
        if gsettings list-schemas 2>/dev/null | grep -qx "$schema"; then
            gsettings set "$schema" exec "$KITTY_BIN" 2>/dev/null || true
            gsettings set "$schema" exec-arg '-e' 2>/dev/null || true
            echo "kitty definido em $schema"
        fi
    done
fi
exit 0
