#!/bin/sh
# Recarrega o tmux em execução depois que o tmux.conf mudou. Sem servidor aberto, nada a fazer.
set -u
CONF="$HIRAKU_HOME/.tmux.conf"
if tmux list-sessions >/dev/null 2>&1; then
    tmux source-file "$CONF" && tmux display-message "Hiraku: configurações do tmux aplicadas" 2>/dev/null
    echo "tmux: configuração recarregada"
else
    echo "nenhum tmux aberto: vale na próxima sessão"
fi
exit 0
