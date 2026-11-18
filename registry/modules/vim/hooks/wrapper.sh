#!/bin/sh
# Cria ~/.local/bin/vim: um wrapper que guarda o estado do TTY e, depois do Vim,
# restaura as cores do terminal e limpa a tela (evita o "retângulo preto").
# Guarda sempre o Vim REAL, nunca o próprio wrapper (senão a 2ª execução recursa).
set -u
BIN="$HIRAKU_BIN_DIR"
WRAPPER="$BIN/vim"
STATE="$HIRAKU_HOME/.local/share/icaro-vim/real-vim"
mkdir -p "$BIN" "$(dirname "$STATE")"

real=""
if [ -f "$STATE" ]; then
    c="$(cat "$STATE" 2>/dev/null)"
    if [ -x "$c" ] && [ "$c" != "$WRAPPER" ]; then real="$c"; fi
fi
if [ -z "$real" ]; then
    for c in /usr/bin/vim /usr/local/bin/vim /opt/homebrew/bin/vim /usr/bin/vim.basic /usr/bin/vim.nox; do
        if [ -x "$c" ]; then real="$c"; break; fi
    done
fi
if [ -z "$real" ]; then
    OLDIFS="$IFS"; IFS=:
    for d in $PATH; do
        c="$d/vim"
        if [ "$c" != "$WRAPPER" ] && [ -x "$c" ]; then real="$c"; break; fi
    done
    IFS="$OLDIFS"
fi
if [ -z "$real" ]; then
    echo "Vim não encontrado: instale-o e rode o sync de novo (wrapper não criado)"; exit 0
fi
printf '%s\n' "$real" > "$STATE"

{
    printf '#!/bin/sh\n# ICARO_VIM_WRAPPER\nREAL_VIM="%s"\n' "$real"
    cat <<'EOF'
TTY_STATE=""
if [ -t 0 ]; then
    TTY_STATE="$(stty -g 2>/dev/null || true)"
fi
"$REAL_VIM" "$@"
status=$?
if [ -n "$TTY_STATE" ]; then
    stty "$TTY_STATE" 2>/dev/null || true
fi
printf '\033]110\a\033]111\a\033[0m\033[?25h\033[2J\033[H' 2>/dev/null || true
exit $status
EOF
} > "$WRAPPER"
chmod +x "$WRAPPER"
echo "wrapper instalado: $WRAPPER (vim real: $real)"
