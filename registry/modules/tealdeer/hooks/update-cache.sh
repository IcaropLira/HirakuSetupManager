#!/bin/sh
# Primeira carga das páginas do tldr.
tldr --update >/dev/null 2>&1 && echo "tldr: páginas baixadas" || echo "tldr: sem internet agora; rode tldr --update depois"
exit 0
