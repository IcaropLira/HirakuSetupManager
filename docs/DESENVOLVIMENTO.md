# Hiraku — desenvolvimento

Workspace Rust com o núcleo do instalador/gestor de configs:

- `hiraku-core`
  - `manifest`, `registry`, `env`: schema TOML, validação, seleção do passo por ambiente.
  - `installer`: orquestra a instalação de um módulo e atualiza o lockfile.
  - `pkg` + `exec` + `elevate`: pacotes do sistema (dnf/apt/pacman/zypper/brew/winget/scoop),
    saída em streaming como eventos e `sudo -v` via trait `Elevator`.
  - `release`: download com sha256 (confere antes de extrair), `.tar.gz`/`.tar.xz`/binário único,
    troca atômica do destino e links de binários que nunca sobrescrevem arquivos alheios.
  - `lockfile`, `paths`: estado em `<state_dir>/lock.toml` (gravação atômica).
  - `configs` + `configstate`: merge de 3 vias com base guardada em `<state_dir>/blobs/<sha256>`,
    backup antes de alterar, conflito pendente sem marcadores no arquivo, `resolve_conflict`
    (manter o meu / usar o novo) e `conflict_diff`. Templates `{{ opção }}`.
  - `regsource` + `verify`: registry embutido no binário (`registry/`) e cache remoto assinado
    com minisign. Ordem: assinatura -> extração segura (só arquivos/diretórios, com limites)
    -> versão sem regressão -> todos os manifests válidos -> troca atômica. Qualquer falha
    deixa o registry atual intacto.
  - `settings` + `sync`: `settings.toml` (módulos, opções, pins, modo) e o `sync`, que
    atualiza tudo (dependências primeiro; um módulo que falha não derruba os outros e
    seus dependentes são pulados), grava o estado a cada módulo e deixa o relatório em
    `<state_dir>/last-sync.toml`. Lock de exclusão mútua contra execuções simultâneas.
  - `schedule`: systemd --user (Linux), LaunchAgent (macOS) e Task Scheduler (Windows).
- **`hiraku-gui`: Hiraku Setup Manager**, o app desktop (Tauri 2, mesmo visual e mesma
  infraestrutura do Hiraku Comics Manager). Veja a seção abaixo.
- `hiraku-hub`: camada de aplicação sem interface, usada pela TUI e pelo app: catálogo de
  opções, `Snapshot` do estado, presets do assistente e `Services` (settings, conflitos,
  pré-visualização de paletas, agendamento, sudo, sync numa thread, registry remoto).
- `hiraku-tui`: TUI (ratatui). `hiraku` sem argumentos abre a TUI num terminal interativo.
  - **Primeira execução (sem `settings.toml`): assistente de instalação** em 6 passos —
    permissões (automático / só usuário / sistema com sudo), editor (Vim, Neovim, os dois),
    o que instalar (checklist com o conjunto do `install.sh` pré-marcado), aparência e
    opções (com pré-visualização da paleta), atualizações automáticas (6h / diária / manual)
    e revisão. Só grava no último passo; aí ajusta o agendamento e roda o primeiro sync.
    `w` no hub refaz o assistente partindo das configurações atuais.
  - **Hub** depois de instalado: Módulos (habilitar, versão instalada, `o` vai às opções,
    `p` fixa versão), **Opções** (todas as opções de todos os módulos: `←/→` muda,
    `Shift` ±10, `Enter` abre a lista com pré-visualização de cores ou o campo de texto,
    `d` volta ao padrão), Conflitos (diff colorido e `m`/`n`), Relatório e Ajuda.
  - **Aplicar ao vivo** (`a` liga/desliga, padrão ligado): cada mudança de opção grava o
    `settings.toml` e, 0,7s depois da última tecla, roda um sync só daquele módulo. O
    kitty tem um hook `reload-config` (SIGUSR1) que faz as janelas abertas relerem a config.
    Se o módulo precisar de root, não pede senha: avisa para usar `s`.
  - O sync roda numa thread; se precisar de root, a TUI sai do modo raw, roda `sudo -v` no
    terminal normal e volta (o app nunca vê a senha), com um keepalive do sudo enquanto roda.
    Respeita o `mode` do `settings.toml` (igual ao `hiraku sync`).
- `hiraku-cli`: `env`, `validate`, `plan`, `install`, `status`, `configure`, `conflicts`, `diff`, `resolve`, `registry info|update`, `enable|disable`, `set`, `pin|unpin`, `mode`, `settings`, `sync`, `report`, `schedule`.
- `registry/modules`: manifests do `kitty` e do `tmux`.

## App desktop: Hiraku Setup Manager

`crates/hiraku-gui` (backend Rust) + `crates/hiraku-gui/ui` (HTML/CSS/JS sem build, como o
Comics Manager). Na primeira execução abre o **assistente** (permissões, editor, o que
instalar, aparência com pré-visualização, atualizações automáticas, revisão); depois vira o
**hub**:

- **Início**: estado do setup, anéis (instalados, conflitos, atualização automática, último
  sync), acesso rápido e o relatório do último sync.
- **Módulos**: cartões com interruptor (habilitar no sync), versão instalada, fixar versão,
  aplicar agora.
- **Personalizar**: todas as opções de todos os módulos. Paletas como cartões com as cores
  reais dos arquivos do módulo, sliders para opacidade/blur, uma janela de terminal de
  exemplo que muda na hora. Com "Aplicar ao vivo", cada mudança grava o `settings.toml` e
  roda o sync só daquele módulo (o hook `reload-config` recarrega os kitty abertos).
- **Conflitos**: diff colorido, "manter o meu" / "usar o novo".
- **Atualizações**: agendamento (6h / diária / manual), registry remoto, atualização do
  próprio app (tauri-plugin-updater, releases do GitHub).
- **Configurações** (⚙): os mesmos 15 temas, cor de destaque e fonte dos títulos do Comics
  Manager; modo user/system; refazer o assistente.

Quando precisa de root, o app pede a senha numa janela e a entrega ao `sudo -S -v` (nada é
guardado); os `sudo -n` do sync reaproveitam o ticket. Com argumentos o binário vira o CLI
(`hiraku-setup-manager sync --unattended` é o que o agendador roda), e o `.deb`/`.rpm`
instala também `/usr/bin/hiraku` (sem argumentos abre a TUI).

```sh
sudo apt install libwebkit2gtk-4.1-dev librsvg2-dev   # Linux, uma vez
cargo run -p hiraku-gui                                # app em modo dev
npm install && npx tauri build                         # instaladores em target/release/bundle
```

Abrir `crates/hiraku-gui/ui/index.html` direto no navegador usa dados de exemplo (bom para
mexer só no visual; `?hub` pula o assistente).

**Release**: `.github/workflows/release.yml` gera .deb/.rpm/AppImage, .dmg e .msi/.exe e o
`latest.json` do atualizador a cada tag `v*`. Precisa dos secrets `TAURI_SIGNING_PRIVATE_KEY`
e `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — a chave pública no `tauri.conf.json` é a mesma do
Comics Manager, então são os mesmos secrets de lá.

## Rodar

```sh
cargo test
cargo run -p hiraku-cli -- validate
cargo run -p hiraku-cli -- plan kitty --user
cargo run -p hiraku-cli -- install kitty --user   # recusa enquanto o sha256 for placeholder
cargo run -p hiraku-cli -- status
cargo run -p hiraku-cli -- configure kitty --set style=solid --first-apply pending
cargo run -p hiraku-cli -- conflicts
```

## Registry real (migrado do HirakuSetupManager)

`registry/modules/` traz 17 módulos equivalentes aos instaladores atuais (kitty com 28 paletas,
tmux, vim + 7 plugins, nvim + lazy, node, jdk, shell, starship, eza, bat, zoxide, fzf,
fastfetch, ripgrep, fonte, temas, pré-requisitos). Veja `registry/README.md` e
`examples/settings.toml`.

**Antes do primeiro uso real:** versões e checksums estão como placeholder e o `install`
recusa placeholder. Preencha com `hiraku registry bump --registry registry/modules` (precisa de
rede) e rode `cargo test` de novo (o registry é embutido no binário).

## Vários componentes por módulo

`[[install]]` com o mesmo `group` (padrão `main`) são **alternativas**: vale o primeiro que
casa com o ambiente. Grupos diferentes são **todos** instalados (programa + plugins + fonte).
O lockfile usa a chave `<módulo>` para o principal e `<módulo>#<grupo>` para os demais.

```toml
[[install]]                       # principal: pacote do sistema se houver root...
when = { os = "linux", root = true }
method = "pkg"
names = { dnf = "vim", apt = "vim" }
provides = ["vim"]                # ...e pula se já existir um vim no PATH

[[install]]
group = "plugin-airline"          # componente extra, falha vira aviso
optional = true
method = "git"
url = "https://github.com/vim-airline/vim-airline"
rev = "<commit completo>"
dest = "~/.vim/pack/plugins/opt/vim-airline"
bump = { branch = "master" }

[[install]]
group = "node"
method = "release"
version = "22.0.0"
url = "https://nodejs.org/dist/v{version}/node-v{version}-linux-x64.tar.xz"
sha256 = "..."
dest = "~/.local/share/hiraku/node"
strip_components = 1              # tira a pasta versionada de nível único
bins = [{ src = "bin/node", link = "~/.local/bin/node" }]
bump = { index_url = "https://nodejs.org/dist/latest-v24.x/", index_prefix = "node-v", index_suffix = "-linux-x64.tar.xz" }
```

## Configs: modos, opções e hooks

| `mode` | comportamento |
|---|---|
| `merge` (padrão) | cópia com merge de 3 vias; conflito fica pendente, sem marcadores no arquivo |
| `copy` | sobrescreve (com backup) |
| `symlink` | link para o arquivo do módulo |
| `seed` | cria só se não existir e nunca mais toca (estado do usuário: `lazy-lock.json`) |
| `block` | gerencia só um bloco com marcadores dentro de um arquivo seu (`~/.bashrc`) |

```toml
[[config]]
src = "files/shell.sh"
dest = { unix = "~/.bashrc" }
mode = "block"
[config.block]
begin = "# >>> ícaro-lira >>>"      # reaproveita os marcadores do install.sh atual
end = "# <<< ícaro-lira <<<"
check = ["bash", "-n", "{path}"]    # valida ANTES de gravar; se falhar, nada muda
only_if_exists = false              # true para ~/.zshrc

[options.opacity]
type = "number"                     # inteiro; com divisor = 100, 85 vira 0.85 no template
min = 30
max = 100
default = 85
divisor = 100

[[config]]
src = "files/palettes/{{ palette }}.conf"   # `src` também aceita template
dest = { unix = "~/.config/kitty/current-theme.conf" }

[[hook]]                            # scripts do módulo; nunca esperam senha (use `sudo -n`)
name = "fc-cache"
script = "hooks/fc-cache.sh"        # .sh roda com `sh`, .ps1 com PowerShell; ou argv = [...]
on = "once"                         # change (padrão) | always | once (por versão do script)
needs = ["fc-cache"]                # sem o comando, pula sem erro
optional = true
root = false                        # true: o sync pede `sudo -v` antes, enquanto estiver pendente
```

Variáveis disponíveis nos hooks: `HIRAKU_MODULE`, `HIRAKU_MODULE_DIR`, `HIRAKU_MODE`
(user|system), `HIRAKU_OS`, `HIRAKU_HOME`, `HIRAKU_BIN_DIR`, `HIRAKU_STATE_DIR`,
`HIRAKU_CHANGED`, `HIRAKU_UNATTENDED`, `HIRAKU_PRIVILEGE` e `HIRAKU_OPT_<OPÇÃO>`.

## Manter as versões do registry em dia

`hiraku registry bump` (para quem mantém o registry, ou o CI dele) lê o `bump = {...}` de cada
passo, descobre a versão mais nova, baixa, calcula o sha256 e reescreve o manifest preservando
comentários; se algo mudou, sobe a `version` do `registry.toml`.

```sh
hiraku registry bump --registry registry/modules              # tudo
hiraku registry bump nvim tmux --registry registry/modules     # só alguns
hiraku registry bump --check --registry registry/modules       # só lista; sai com erro se houver pendência
```

Fontes: `github = "dono/repo"` (tag de `/releases/latest`, sem API), `index_url` + `index_prefix` +
`index_suffix` (ex.: nodejs.org), `redirect` (URL "sempre a última" que vira um arquivo fixado,
ex.: Adoptium) e, em passos `git`, `branch`.

## Sync e agendamento

```sh
hiraku enable kitty tmux
hiraku set kitty style=solid
hiraku pin nvim 0.10.*        # trava só o binário; configs continuam sincronizando
hiraku sync                   # interativo (pede sudo -v se precisar)
hiraku schedule install --every 6h
hiraku report
```

`sync --unattended` (o que o agendador roda) nunca pergunta nada: usa `sudo -n`, assume
modo `--user` e trata arquivo seu já existente como conflito pendente. Pacotes do sistema
(dnf/apt/pacman/zypper) são instalados uma vez e depois ficam a cargo do gerenciador do SO.

## Atualizações automáticas (configurar uma vez)

1. No seu computador, na raiz do repo: `scripts/setup-signing.sh`. Ele gera a chave minisign
   do registry, cadastra os secrets (`MINISIGN_SECRET_KEY`, `TAURI_SIGNING_PRIVATE_KEY` e a
   senha, reusando a chave do atualizador do Comics Manager), grava `keys/registry.pub` e faz
   o commit. Depois: `git push`.
2. `gh workflow run registry.yml` (ou Actions → Registry → Run workflow): busca as versões,
   commita no `main`, assina e publica a release `registry` (pré-release).
3. `git tag v0.1.0 && git push origin v0.1.0`: o `release.yml` gera os instaladores e o
   `latest.json` do atualizador.

4. Repositório DNF: depois do primeiro `dnf.yml` (roda no fim de cada release, ou
   `gh workflow run dnf.yml -f tag=v0.1.0`), ative o GitHub Pages em Settings → Pages →
   Deploy from a branch → `gh-pages` / root. O `scripts/publish-dnf.sh` assina os .rpm e o
   `repomd.xml` com a chave GPG do secret `RPM_GPG_PRIVATE_KEY` e mantém as últimas 5 versões.

Daí em diante: o `registry.yml` roda toda segunda (e a cada push em `registry/`), e cada tag
`v*` vira uma atualização do app. O app só aceita registry assinado por uma chave de
`keys/registry.pub` (embutida no binário); a URL padrão é a release `registry` deste repo
(`DEFAULT_REMOTE_URL`, sobrescrevível por `registry_url` no settings.toml ou
`$HIRAKU_REGISTRY_URL`). Sem chave em `keys/registry.pub`, o registry remoto fica desligado
sem erro.

Publicar manualmente: `MINISIGN_KEY_FILE=... scripts/publish-registry.sh` gera
`dist/registry.tar.gz` e `.minisig`.

## Pendências

- Versões e `sha256` ainda de placeholder até o primeiro `registry.yml` rodar (`install`
  recusa; `validate` avisa).
- Confirmar o nome real do asset do tmux-builds (o `bump` avisa se não achar).
- Método `script` e `.zip` ainda não implementados (retornam erro explícito).
