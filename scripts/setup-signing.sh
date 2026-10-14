#!/usr/bin/env bash
# Configura as assinaturas das atualizações. Rode UMA vez, no seu computador, na
# raiz do repositório. Precisa de: gh (logado: `gh auth login`) e minisign
# (Fedora: dnf install minisign · Ubuntu: apt install minisign · macOS: brew install minisign).
#
# 1. Registry: gera um par minisign novo. A chave secreta vai para o secret
#    MINISIGN_SECRET_KEY e fica guardada em ~/.config/hiraku-signing/ (backup);
#    a pública vai para keys/registry.pub (o app embute e só aceita registry
#    assinado por ela).
# 2. App: cadastra a chave do atualizador do Tauri. Por padrão reusa a do Hiraku
#    Comics Manager (a pública dela já está no tauri.conf.json). Com --new-app-key
#    gera uma nova e troca a pública no tauri.conf.json.
# 3. DNF: gera uma chave GPG (fica em ~/.config/hiraku-signing/gnupg) que assina
#    os .rpm e o repositório Fedora; vai para o secret RPM_GPG_PRIVATE_KEY.
#
# Uso: scripts/setup-signing.sh [--app-key ARQUIVO] [--new-app-key] [--skip-registry] [--skip-rpm]
set -euo pipefail

cd "$(dirname "$0")/.."
DIR="$HOME/.config/hiraku-signing"
APP_KEY=""
NEW_APP_KEY=0
SKIP_REGISTRY=0
SKIP_RPM=0
while [ $# -gt 0 ]; do
  case "$1" in
    --app-key) APP_KEY="$2"; shift 2 ;;
    --new-app-key) NEW_APP_KEY=1; shift ;;
    --skip-registry) SKIP_REGISTRY=1; shift ;;
    --skip-rpm) SKIP_RPM=1; shift ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "opção desconhecida: $1" >&2; exit 1 ;;
  esac
done

need() { command -v "$1" >/dev/null || { echo "erro: preciso do '$1' instalado ($2)" >&2; exit 1; }; }
need gh "https://cli.github.com"
gh auth status >/dev/null 2>&1 || { echo "erro: rode 'gh auth login' antes" >&2; exit 1; }
REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner)
echo "repositório: $REPO"
mkdir -p "$DIR"; chmod 700 "$DIR"

# ---------------------------------------------------------------- registry
if [ "$SKIP_REGISTRY" = 0 ]; then
  need minisign "dnf/apt/brew install minisign"
  if [ -f "$DIR/registry.key" ]; then
    echo "registry: reusando a chave já existente em $DIR/registry.key"
  else
    minisign -G -W -p "$DIR/registry.pub" -s "$DIR/registry.key"
    chmod 600 "$DIR/registry.key"
  fi
  gh secret set MINISIGN_SECRET_KEY -R "$REPO" < "$DIR/registry.key"
  PUB=$(grep '^RW' "$DIR/registry.pub")
  {
    echo "# Chave pública minisign do registry (gerada por scripts/setup-signing.sh)."
    echo "# O app só aceita registry remoto assinado por uma das chaves \"RW...\" deste arquivo."
    echo "$PUB"
  } > keys/registry.pub
  echo "registry: secret MINISIGN_SECRET_KEY cadastrado; chave pública em keys/registry.pub"
  echo "          GUARDE um backup de $DIR/registry.key (sem ela, os apps já instalados"
  echo "          só aceitam registry novo depois de atualizarem o app)."
fi

# ---------------------------------------------------------------- app (Tauri)
if [ "$NEW_APP_KEY" = 1 ]; then
  need npx "Node.js"
  npx --yes @tauri-apps/cli signer generate -w "$DIR/tauri.key"
  APP_KEY="$DIR/tauri.key"
  node -e '
    const fs = require("fs"), p = "crates/hiraku-gui/tauri.conf.json";
    const c = JSON.parse(fs.readFileSync(p, "utf8"));
    c.plugins.updater.pubkey = fs.readFileSync(process.argv[1], "utf8").trim();
    fs.writeFileSync(p, JSON.stringify(c, null, 2) + "\n");
  ' "$DIR/tauri.key.pub"
  echo "app: chave nova gerada; pública gravada no tauri.conf.json"
fi
if [ -z "$APP_KEY" ]; then
  GUESS=$(ls "$HOME"/.tauri/*.key 2>/dev/null | head -1 || true)
  read -r -p "Arquivo da chave privada do atualizador (a do Comics Manager) [${GUESS:-nenhum}]: " APP_KEY
  APP_KEY="${APP_KEY:-$GUESS}"
fi
if [ -n "$APP_KEY" ] && [ -f "$APP_KEY" ]; then
  gh secret set TAURI_SIGNING_PRIVATE_KEY -R "$REPO" < "$APP_KEY"
  read -r -s -p "Senha dessa chave (Enter se não tiver): " PW; echo
  if [ -n "$PW" ]; then
    printf '%s' "$PW" | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD -R "$REPO"
  fi
  echo "app: secrets do atualizador cadastrados"
else
  echo "app: nenhuma chave informada; pulei. Rode de novo com --app-key ARQUIVO ou --new-app-key."
fi

# ---------------------------------------------------------------- dnf (GPG)
if [ "$SKIP_RPM" = 0 ]; then
  need gpg "dnf/apt/brew install gnupg"
  export GNUPGHOME="$DIR/gnupg"
  mkdir -p "$GNUPGHOME"; chmod 700 "$GNUPGHOME"
  if ! gpg --batch --list-secret-keys --with-colons 2>/dev/null | grep -q '^sec'; then
    EMAIL=$(git config user.email || echo "hiraku@localhost")
    gpg --batch --quiet --passphrase '' --quick-gen-key "Hiraku Setup Manager <$EMAIL>" rsa4096 sign never
  fi
  gpg --batch --armor --export-secret-keys | gh secret set RPM_GPG_PRIVATE_KEY -R "$REPO"
  echo "dnf: secret RPM_GPG_PRIVATE_KEY cadastrado (chave em $GNUPGHOME; guarde um backup)"
  unset GNUPGHOME
fi

# ---------------------------------------------------------------- commit
if [ -n "$(git status --porcelain keys/registry.pub crates/hiraku-gui/tauri.conf.json)" ]; then
  git add keys/registry.pub crates/hiraku-gui/tauri.conf.json
  git commit -m "Chaves públicas das atualizações"
  echo
  echo "Commit criado. Agora:"
  echo "  git push"
  echo "  gh workflow run registry.yml        # publica o registry assinado"
  echo "  git tag v0.1.1 && git push origin v0.1.1   # release assinada (app + dnf)"
fi
