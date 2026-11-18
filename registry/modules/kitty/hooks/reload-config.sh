#!/bin/sh
# Pede para as janelas do kitty já abertas relerem a configuração, para que paleta,
# opacidade e blur novos apareçam na hora. O kitty recarrega o kitty.conf (e os
# includes) ao receber SIGUSR1. Sem kitty aberto, não há nada a fazer.
set -u
if pkill -USR1 -u "$(id -u)" -x kitty 2>/dev/null; then
    echo "kitty: configuração recarregada nas janelas abertas"
else
    echo "nenhum kitty aberto: vale na próxima janela"
fi
exit 0
