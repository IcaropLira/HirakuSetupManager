# ~/.local/bin primeiro: aqui ficam o nvim, node, java, kitty... instalados sem root.
# Sempre na frente (mesmo se já estiver no PATH mais para trás), para o kitty (que
# abre o shell em modo de login) e o terminal normal acharem o mesmo vim/nvim.
_hk_p=":$PATH:"; _hk_p="${_hk_p//:$HOME\/.local\/bin:/:}"; _hk_p="${_hk_p#:}"; export PATH="$HOME/.local/bin:${_hk_p%:}"; unset _hk_p
[ -f "$HOME/.config/icaro-visualizer/visualizer.sh" ] && source "$HOME/.config/icaro-visualizer/visualizer.sh"
[ -f "$HOME/.config/dotfiles-shell/shellrc.sh" ] && source "$HOME/.config/dotfiles-shell/shellrc.sh"
