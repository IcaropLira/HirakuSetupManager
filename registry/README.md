# Registry do Hiraku

Cada pasta em `modules/` é um módulo (`module.toml` + `files/` + `hooks/`).

| módulo | o que faz | observações |
|---|---|---|
| `font-jetbrains-mono` | JetBrainsMono Nerd Font | user-space; hook de `fc-cache` e fonte do GNOME Terminal |
| `icaro-themes` | 90 colorschemes do vim/nvim | requerido por `vim` e `nvim` |
| `node`, `jdk` | Node LTS e Temurin 21 portáteis | Linux: sempre em `~/.local` (as distros ficam para trás) |
| `kitty` | terminal, 28 paletas, opacidade/blur | `hiraku set kitty palette=nord opacity=90 blur=12` |
| `tmux` | multiplexador | pacote com root, build estático sem |
| `vim` | vimrc, 7 plugins git, wrapper de TTY | o Vim em si só entra com root |
| `nvim` | config + lazy.nvim + bootstrap | build oficial em `~/.local` |
| `ripgrep`, `dev-prereqs` | `rg` (telescope) e git/gcc/make/unzip | `dev-prereqs` só com root |
| `shell` | bloco do `.bashrc`/`.zshrc`, aliases, visualizer | mesmos marcadores do install.sh atual |
| `starship`, `eza`, `bat`, `zoxide`, `fzf`, `fastfetch` | utilitários de terminal | release fixado, sem sudo |

## Antes de publicar

As versões e checksums estão como placeholder (`0.0.0` / zeros) e os commits dos plugins do vim
como `0000…`: o `hiraku install` recusa placeholder. Preencha com:

```sh
hiraku registry bump --registry registry/modules
```

Substitui o antigo seletor interativo de aparência do kitty (`appearance-picker.sh`) e o
`palettes.sh`: as paletas viraram arquivos em `kitty/files/palettes/` e a escolha, uma opção.
