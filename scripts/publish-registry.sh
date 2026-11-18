#!/bin/sh
# Empacota e assina o registry para publicação (o CI faz isso: .github/workflows/registry.yml).
# Requer o `minisign` e a chave secreta gerada por scripts/setup-signing.sh.
#
#   MINISIGN_KEY_FILE   chave secreta (padrão: ~/.config/hiraku-signing/registry.key)
#
# Saída: dist/registry.tar.gz e dist/registry.tar.gz.minisig
set -eu

cd "$(dirname "$0")/.."
KEY="${MINISIGN_KEY_FILE:-$HOME/.config/hiraku-signing/registry.key}"
[ -f "$KEY" ] || { echo "erro: chave secreta não encontrada em $KEY" >&2; exit 1; }
mkdir -p dist
rm -f dist/registry.tar.gz dist/registry.tar.gz.minisig

# COPYFILE_DISABLE evita arquivos ._* do bsdtar no macOS.
COPYFILE_DISABLE=1 tar -czf dist/registry.tar.gz -C registry registry.toml modules
VERSION=$(sed -n 's/^version *= *\([0-9]*\).*/\1/p' registry/registry.toml)
minisign -S -s "$KEY" -m dist/registry.tar.gz -t "hiraku registry v$VERSION"

echo "ok: dist/registry.tar.gz (v$VERSION) e dist/registry.tar.gz.minisig"
