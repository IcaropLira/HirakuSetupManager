#!/bin/sh
# Pré-instala as extensões do coc.nvim (java, c/c++, python). O coc termina sozinho
# na primeira vez que o vim abrir de verdade, se isto não der tempo aqui.
set -u
PATH="$HIRAKU_BIN_DIR:$PATH"
export PATH
STATE="$HIRAKU_HOME/.local/share/icaro-vim/real-vim"
VIM="$(cat "$STATE" 2>/dev/null || true)"
if [ -z "$VIM" ] || [ ! -x "$VIM" ]; then VIM="$(command -v vim 2>/dev/null || true)"; fi
if [ -z "$VIM" ]; then echo "vim não encontrado: extensões do coc ficam para a primeira abertura"; exit 0; fi
if ! command -v node >/dev/null 2>&1; then echo "node não encontrado: pulando"; exit 0; fi
LOG="${TMPDIR:-/tmp}/coc-install.log"
run() {
    if command -v timeout >/dev/null 2>&1; then timeout 180 "$@"; else "$@"; fi
}
if run "$VIM" -es -u "$HIRAKU_HOME/.vimrc" -c "CocInstall -sync coc-java coc-clangd coc-pyright" -c "qa!" >"$LOG" 2>&1; then
    echo "coc-java, coc-clangd e coc-pyright instalados"
else
    echo "não terminou agora (sem problema, o coc conclui ao abrir o vim); log em $LOG"
fi
exit 0
