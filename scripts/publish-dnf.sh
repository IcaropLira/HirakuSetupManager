#!/usr/bin/env bash
# Monta o repositório DNF (Fedora/RHEL) do Hiraku Setup Manager no GitHub Pages.
# O CI roda isto depois de cada release (.github/workflows/dnf.yml).
#
#   scripts/publish-dnf.sh <pasta-com-os-rpm> <pasta-do-site>
#
# Coloca os .rpm em <site>/rpm/<arch>/, mantém as últimas $KEEP versões, assina
# pacotes e metadados com GPG (se houver chave) e gera o hiraku.repo.
#
#   RPM_GPG_PRIVATE_KEY  chave GPG privada (armored). Sem ela, o repo sai sem
#                        assinatura (gpgcheck=0) e com aviso.
#   PAGES_URL            URL pública do site (padrão: GitHub Pages deste repo).
#   KEEP                 quantas versões manter por arquitetura (padrão: 5).
set -euo pipefail

IN=$1
SITE=$2
PAGES_URL=${PAGES_URL:-https://icaroplira.github.io/HirakuSetupManager}
KEEP=${KEEP:-5}
NAME=hiraku-setup-manager

shopt -s nullglob
rpms=("$IN"/*.rpm)
[ ${#rpms[@]} -gt 0 ] || { echo "erro: nenhum .rpm em $IN" >&2; exit 1; }

# GPG num diretório só nosso (não mexe no ~/.gnupg de quem roda).
SIGN=0
if [ -n "${RPM_GPG_PRIVATE_KEY:-}" ]; then
  export GNUPGHOME=$(mktemp -d)
  trap 'rm -rf "$GNUPGHOME"' EXIT
  printf '%s\n' "$RPM_GPG_PRIVATE_KEY" | gpg --batch --quiet --import
  KEYID=$(gpg --list-secret-keys --with-colons | awk -F: '/^sec/ {print $5; exit}')
  SIGN=1
else
  echo "::warning::RPM_GPG_PRIVATE_KEY ausente (rode scripts/setup-signing.sh): repositório DNF sem assinatura"
fi

mkdir -p "$SITE/rpm"
for rpm in "${rpms[@]}"; do
  arch=$(rpm -qp --qf '%{ARCH}' "$rpm" 2>/dev/null)
  ver=$(rpm -qp --qf '%{VERSION}-%{RELEASE}' "$rpm" 2>/dev/null)
  dest="$SITE/rpm/$arch/$NAME-$ver.$arch.rpm"
  mkdir -p "$SITE/rpm/$arch"
  cp "$rpm" "$dest"
  if [ "$SIGN" = 1 ]; then
    rpmsign --define "_gpg_name $KEYID" --define "_gpg_path $GNUPGHOME" --addsign "$dest" >/dev/null
  fi
  echo "adicionado: rpm/$arch/$(basename "$dest")"
done

for dir in "$SITE"/rpm/*/; do
  [ -d "$dir" ] || continue
  # Só as últimas $KEEP versões (a mais nova por último no sort -V).
  mapfile -t all < <(ls "$dir"/$NAME-*.rpm 2>/dev/null | sort -V)
  if [ ${#all[@]} -gt "$KEEP" ]; then
    for old in "${all[@]:0:${#all[@]}-$KEEP}"; do rm -f "$old"; echo "removido: ${old#$SITE/}"; done
  fi
  rm -rf "$dir/repodata"
  createrepo_c --quiet "$dir"
  if [ "$SIGN" = 1 ]; then
    gpg --batch --yes --local-user "$KEYID" --detach-sign --armor "$dir/repodata/repomd.xml"
  fi
done

if [ "$SIGN" = 1 ]; then
  gpg --batch --armor --export "$KEYID" > "$SITE/rpm/RPM-GPG-KEY-hiraku"
  CHECK="gpgcheck=1
repo_gpgcheck=1
gpgkey=$PAGES_URL/rpm/RPM-GPG-KEY-hiraku"
else
  CHECK="gpgcheck=0
repo_gpgcheck=0"
fi
cat > "$SITE/rpm/hiraku.repo" <<EOF
[hiraku]
name=Hiraku Setup Manager
baseurl=$PAGES_URL/rpm/\$basearch
enabled=1
$CHECK
metadata_expire=6h
EOF

LATEST=$(ls "$SITE"/rpm/*/$NAME-*.rpm | sort -V | tail -1 | xargs basename)
cat > "$SITE/index.html" <<EOF
<!doctype html><meta charset="utf-8"><title>Hiraku Setup Manager — repositórios</title>
<style>body{font:15px/1.6 system-ui,sans-serif;max-width:720px;margin:40px auto;padding:0 20px;color:#1c1b22}
pre{background:#f3f1ee;padding:12px 14px;border-radius:10px;overflow:auto}h1{font-family:Georgia,serif}a{color:#c2577f}</style>
<h1>Hiraku Setup Manager</h1>
<p>Fedora / RHEL (dnf). Versão mais nova: <code>$LATEST</code></p>
<pre>sudo curl -fsSL -o /etc/yum.repos.d/hiraku.repo $PAGES_URL/rpm/hiraku.repo
sudo dnf install $NAME</pre>
<p>Depois disso, <code>sudo dnf upgrade</code> traz as versões novas.
Outros sistemas: <a href="https://github.com/IcaropLira/HirakuSetupManager/releases/latest">releases no GitHub</a>.</p>
EOF
touch "$SITE/.nojekyll"
echo "ok: repositório em $SITE/rpm (assinado: $([ "$SIGN" = 1 ] && echo sim || echo não))"
