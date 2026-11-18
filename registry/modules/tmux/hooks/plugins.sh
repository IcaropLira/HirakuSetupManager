#!/bin/sh
# Baixa (git clone) os plugins do tmux ligados na aba Descobrir do app.
# Desligar não apaga: o tmux.conf só deixa de carregar.
set -u
DIR="$HIRAKU_HOME/.config/tmux/plugins"
want() { [ "$1" = "true" ]; }
get() {
    name="$1"; url="$2"
    if [ -d "$DIR/$name/.git" ]; then
        return 0
    fi
    mkdir -p "$DIR"
    rm -rf "$DIR/$name"
    if git clone --depth 1 --quiet "$url" "$DIR/$name"; then
        echo "tmux: $name baixado"
    else
        echo "tmux: não consegui baixar $name (internet?)" >&2
        return 1
    fi
}
st=0
if want "${HIRAKU_OPT_PLUGIN_RESURRECT:-false}" || want "${HIRAKU_OPT_PLUGIN_CONTINUUM:-false}"; then
    get tmux-resurrect https://github.com/tmux-plugins/tmux-resurrect || st=1
fi
if want "${HIRAKU_OPT_PLUGIN_CONTINUUM:-false}"; then
    get tmux-continuum https://github.com/tmux-plugins/tmux-continuum || st=1
fi
if want "${HIRAKU_OPT_PLUGIN_FZF_URL:-false}"; then
    get tmux-fzf-url https://github.com/wfxr/tmux-fzf-url || st=1
fi
exit $st
