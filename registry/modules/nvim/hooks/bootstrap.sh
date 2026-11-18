#!/bin/sh
# Instala/restaura os plugins do lazy.nvim e roda o bootstrap da config.
# ~/.local/bin entra primeiro no PATH para achar o nvim, o node e o java do hiraku.
set -u
PATH="$HIRAKU_BIN_DIR:$PATH"
export PATH
NVIM="$(command -v nvim 2>/dev/null || true)"
if [ -z "$NVIM" ]; then echo "nvim não encontrado: bootstrap pulado"; exit 0; fi

run() {
    if command -v timeout >/dev/null 2>&1; then timeout 900 "$@"; else "$@"; fi
}
run "$NVIM" --headless "+Lazy! restore" +qa || echo "aviso: Lazy restore terminou com erro"
run "$NVIM" --headless "+Lazy! sync" +qa || echo "aviso: Lazy sync terminou com erro"
run "$NVIM" --headless "+lua require('icaro.bootstrap').run()" +qa || echo "aviso: bootstrap da config terminou com erro"
echo "bootstrap do nvim concluído"
