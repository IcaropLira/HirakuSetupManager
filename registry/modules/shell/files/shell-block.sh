# ~/.local/bin primeiro: aqui ficam o nvim, node, java, kitty... instalados sem root
case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) export PATH="$HOME/.local/bin:$PATH" ;; esac
[ -f "$HOME/.config/icaro-visualizer/visualizer.sh" ] && source "$HOME/.config/icaro-visualizer/visualizer.sh"
[ -f "$HOME/.config/dotfiles-shell/shellrc.sh" ] && source "$HOME/.config/dotfiles-shell/shellrc.sh"
