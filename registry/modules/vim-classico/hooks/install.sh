#!/bin/sh
# Instala o "Vim clássico": baixa o repositório no commit escolhido e roda o
# instalador que existia naquela época, sem perguntas (modo só-usuário).
set -eu

REV="${HIRAKU_OPT_VERSAO:?versão não informada}"
case "$REV" in
    *[!0-9a-f]*|"") echo "versão inválida: $REV" >&2; exit 1 ;;
esac
REPO="https://github.com/IcaropLira/HirakuSetupManager"
BASE="$HIRAKU_HOME/.local/share/hiraku/vim-classico"
CACHE="$BASE/repo"
WORK="$BASE/versoes/$REV"
mkdir -p "$BASE/versoes"

# Já instalada (e o ~/.vimrc está lá): nada a fazer.
if [ "$(cat "$BASE/instalada" 2>/dev/null || true)" = "$REV" ] && [ -f "$HIRAKU_HOME/.vimrc" ]; then
    exit 0
fi

# Repositório só com o histórico (sem arquivos), reaproveitado entre versões.
if [ ! -d "$CACHE/.git" ] && [ ! -f "$CACHE/HEAD" ]; then
    rm -rf "$CACHE"
    git clone --quiet --bare --filter=blob:none "$REPO" "$CACHE"
fi
if ! git -C "$CACHE" cat-file -e "$REV^{commit}" 2>/dev/null; then
    git -C "$CACHE" fetch --quiet origin "$REV" || git -C "$CACHE" fetch --quiet origin
fi

rm -rf "$WORK.tmp"
mkdir -p "$WORK.tmp"
git -C "$CACHE" archive -o "$BASE/versao.tar" "$REV"
tar -xf "$BASE/versao.tar" -C "$WORK.tmp"
rm -f "$BASE/versao.tar"
rm -rf "$WORK"
mv "$WORK.tmp" "$WORK"

# Até 26/09 o instalador ficava na raiz; depois, em vim/.
if [ -f "$WORK/vim/install.sh" ]; then
    DIR="$WORK/vim"
else
    DIR="$WORK"
fi

# A pasta de configs de outra versão (ou do Vim moderno) vai para o backup, para
# não sobrar arquivo que esta versão não conhece.
if [ -d "$HIRAKU_HOME/.vim/config" ]; then
    SAVE="$BASE/backups/$(date +%Y%m%d_%H%M%S)"
    mkdir -p "$SAVE"
    mv "$HIRAKU_HOME/.vim/config" "$SAVE/config"
    echo "configs anteriores guardadas em $SAVE/config"
fi

echo "Vim clássico: instalando a versão $(git -C "$CACHE" log -1 --format='%ad — %s' --date=format:'%d/%m/%Y' "$REV")"
cd "$DIR"
export HOME="$HIRAKU_HOME"
export ICARO_MODE=user
# Sem terminal: qualquer pergunta antiga recebe a resposta padrão.
bash ./install.sh </dev/null
printf '%s\n' "$REV" > "$BASE/instalada"
echo "Vim clássico pronto. Abra um terminal novo e rode vim."
