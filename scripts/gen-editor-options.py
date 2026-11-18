"""Uso: python3 scripts/gen-editor-options.py .

Gera as [options] do vim e do nvim (mesmo conjunto, padrões de cada editor) e o
template files/hiraku.json que cada editor lê. Rodar da raiz do repo."""
import json, re, sys

ROOT = sys.argv[1]
MOD = f"{ROOT}/registry/modules"

# id -> título dos temas do Ícaro (o catálogo do nvim é gerado a partir do vim).
cat = open(f"{MOD}/nvim/files/config/lua/icaro/catalog.lua").read()
icaro = re.findall(r'id = "([^"]+)", title = "([^"]+)"', cat)
plug = re.findall(r'\{ id = "([^"]+)",\s*title = "([^"]+)"', open(f"{MOD}/nvim/files/config/lua/icaro/themes.lua").read())
files = {f[:-4] for f in __import__("os").listdir(f"{MOD}/icaro-themes/files/icaro-theme/colors")}
icaro = [(i, t) for i, t in icaro if i in files]

def q(s):
    return json.dumps(s, ensure_ascii=False)

# (ação, nome, padrão no Vim, padrão no Neovim). None = não existe naquele editor.
FKEYS = [
    ("theme_next", "Próximo tema (:Th)", "<F1>", "<F1>"),
    ("theme_prev", "Tema anterior (:Pv)", "<S-F1>", "<S-F1>"),
    ("explorer", "Árvore de arquivos (:Nt)", "<F2>", "<F2>"),
    ("inlay", "Dicas de parâmetro no código (:Ih)", "<F3>", "<F3>"),
    ("hints", "Dicas de método (:Dc)", None, "<S-F3>"),
    ("autocomplete", "Autocomplete da linguagem (:Ac)", "<F4>", "<F4>"),
    ("diagnostics", "Avisos e erros (:Dg)", None, "<S-F4>"),
    ("compile", "Compilar (:Cp)", "<F5>", "<F5>"),
    ("run", "Executar (:Rn)", "<F6>", "<F6>"),
    ("build_run", "Compilar e executar (:Br)", "<F7>", "<F7>"),
    ("test", "Testar com input.txt (:Ti)", "<F8>", "<F8>"),
    ("terminal", "Terminal (:Te)", "<F9>", "<F9>"),
    ("autopairs", "Fechar pares e aspas (:Pa)", "<F10>", "<F10>"),
    ("matchparen", "Destaque do par de parênteses (:Mh)", "<S-F10>", "<S-F10>"),
    ("quiet", "Modo silencioso (:Of)", "<S-F11>", "<S-F11>"),
    ("cheatsheet", "Folha de atalhos (:Cs)", "<F12>", "<F12>"),
    ("header", "Mostrar/esconder o header (:Hd)", "<S-F12>", ""),
    ("central", "Central de configurações (:RF)", None, "<S-F12>"),
]

# (chave, nome, extensão, observação)
TEMPLATES = [
    ("cpp", "C++", ".cpp", ""),
    ("c", "C", ".c", ""),
    ("python", "Python", ".py", ""),
    ("java_cp", "Java (programação competitiva)", ".java terminado em _cp", "; __CLASSNAME__ vira o nome do arquivo"),
    ("java_main", "Java (classe comum)", ".java", "; __CLASSNAME__ vira o nome do arquivo"),
]

def template_default(k):
    f = {"cpp": "cpp.cpp", "java_cp": "java_cp.java", "java_main": "java_main.java"}.get(k)
    return open(f"{MOD}/vim/files/templates/{f}").read() if f else ""

def opt(key, kind, default=None, label="", section="", description="", **extra):
    return dict(key=key, kind=kind, default=default, label=label, section=section, description=description, **extra)

def editor(name):
    nvim = name == "nvim"
    themes = icaro + (plug if nvim else [])
    values = ["auto"] + [i for i, _ in themes]
    labels = {"auto": "O último escolhido no editor"} | {i: t for i, t in themes}
    O = []
    A, E, C, B, T, P, X = "Aparência", "Edição", "Copiar, recortar e colar", "Busca", "Atalhos", "Plugins", "Avançado"
    O += [
        opt("colorscheme", "choice", "auto", "Tema", A,
            "Colorscheme do editor. \"O último escolhido no editor\" deixa a troca com F1/:Th.",
            values=values, labels=labels, preview="../icaro-themes/files/icaro-theme/colors/{value}.vim"),
        opt("transparent", "bool", False, "Fundo transparente", A,
            "Usa o fundo do terminal no lugar do fundo do tema (combina com a opacidade do kitty)."),
        opt("number", "bool", True, "Numeração das linhas", A),
        opt("relativenumber", "bool", True, "Numeração relativa", A, "Mostra a distância até o cursor (bom para 5j, 3dd...)."),
        opt("cursorline", "bool", True, "Realçar a linha do cursor", A),
        opt("cursorcolumn", "bool", False, "Realçar a coluna do cursor", A),
        opt("colorcolumn", "choice", "0", "Linha-guia vertical", A, "Uma coluna marcada para limitar o comprimento das linhas.",
            values=["0", "80", "100", "120"], labels={"0": "Nenhuma", "80": "80", "100": "100", "120": "120"}),
        opt("signcolumn", "choice", "yes", "Coluna de sinais", A, "Espaço à esquerda para erros, git e marcas.",
            values=["yes", "auto", "no", "number"], labels={"yes": "Sempre", "auto": "Quando houver", "no": "Nunca", "number": "Junto da numeração"}),
        opt("list", "bool", nvim, "Mostrar tabs e espaços no fim", A),
        opt("wrap", "bool", False, "Quebrar linhas longas", A),
        opt("linebreak", "bool", True, "Quebrar entre palavras", A, "Só vale com a quebra de linhas ligada."),
        opt("scrolloff", "number", 10 if nvim else 5, "Linhas de contexto ao rolar", A, "Quantas linhas ficam visíveis acima/abaixo do cursor.", min=0, max=30),
        opt("nerd_font", "bool", True, "Ícones (Nerd Font)", A, "Ícones na árvore de arquivos, abas e barra de status."),
    ]
    O += [
        opt("leader", "choice", "space" if nvim else "backslash", "Tecla líder (<leader>)", E,
            "A tecla que começa os atalhos <leader>... Trocar muda todos esses atalhos.",
            values=["space", "comma", "backslash"], labels={"space": "Espaço", "comma": "Vírgula", "backslash": "Barra invertida"}),
        opt("tabsize", "number", 4, "Tamanho da indentação", E, "Colunas por nível (tabstop, shiftwidth e softtabstop).", min=1, max=8),
        opt("expandtab", "bool", True, "Indentar com espaços", E, "Desligado, usa o caractere TAB."),
        opt("autopairs", "bool", True, "Fechar parênteses e aspas sozinho", E),
        opt("mouse", "choice", "a", "Mouse", E, values=["a", "nv", "off"],
            labels={"a": "Em todos os modos", "nv": "Só normal e visual", "off": "Desligado"}),
        opt("undofile", "bool", True, "Desfazer mesmo depois de fechar", E, "Guarda o histórico de undo em disco."),
        opt("swapfile", "bool", False, "Arquivo de recuperação (swap)", E),
        opt("confirm", "bool", True, "Perguntar antes de perder alterações", E, "Ao sair com :q de um arquivo não salvo."),
        opt("splitright", "bool", True, "Divisões verticais abrem à direita", E),
        opt("splitbelow", "bool", True, "Divisões horizontais abrem embaixo", E),
    ]
    O += [
        opt("clipboard", "choice", "system", "Área de transferência", C,
            "Integrada: y, d e p usam a área de transferência do sistema (Ctrl+V cola fora do editor). Separada: só \"+y copia para o sistema.",
            values=["system", "separate"], labels={"system": "Integrada ao sistema", "separate": "Separada"}),
        opt("delete_mode", "choice", "copy", "O que acontece ao apagar (dd, x, c...)", C,
            "\"Apagar não copia\": d, c, x e s jogam o texto fora sem mexer no que você copiou; para recortar, use <leader>d. "
            "\"Só x e c\": d continua recortando, x/c/s não copiam.",
            values=["copy", "blackhole", "x_only"],
            labels={"copy": "Apagar também copia (padrão do Vim)", "blackhole": "Apagar não copia", "x_only": "Só x e c não copiam"}),
        opt("paste_keeps", "bool", True, "Colar por cima não perde o que foi copiado", C,
            "No modo visual, p substitui a seleção sem trocar o conteúdo copiado."),
    ]
    if nvim:
        O.append(opt("yank_highlight", "bool", True, "Piscar o texto copiado", C))
    O += [
        opt("ignorecase", "bool", True, "Ignorar maiúsculas na busca", B),
        opt("smartcase", "bool", True, "...a não ser que a busca tenha maiúscula", B),
        opt("hlsearch", "bool", True, "Destacar os resultados", B),
        opt("incsearch", "bool", True, "Buscar enquanto digita", B),
    ]
    O += [
        opt("timeoutlen", "number", 400 if nvim else 700, "Tempo para completar um atalho (ms)", T,
            "Quanto o editor espera pela próxima tecla de um atalho com várias teclas.", min=100, max=2000),
        opt("keymaps", "keymaps", None, "Meus atalhos", T,
            "Valem por cima dos atalhos do Ícaro. Para desativar um atalho existente, use <Nop> como ação."),
    ]
    # Teclas de função: cada ação do Ícaro com a tecla que a dispara (F1, Shift+F1...).
    FK = "Teclas de função"
    first = True
    for k, label, dv, dn in FKEYS:
        d = dn if nvim else dv
        if d is None:
            continue
        desc = f"Padrão: {d or 'nenhuma'}."
        if first:
            desc += " Vale qualquer tecla do Vim: <F5>, <S-F5> (Shift+F5), <C-t>, <leader>t. Vazio = sem tecla. A folha de atalhos mostra as teclas atuais."
            first = False
        O.append(opt(f"key_{k}", "string", d, label, FK, desc))
    # Central: o que a Central / as teclas ligam e desligam, e com que valor o editor abre.
    CE = "Central"
    O += [
        opt("autocomplete_cpp", "bool", True, "Autocomplete em C++", CE, "O mesmo que a tecla de autocomplete (F4) num arquivo C++."),
        opt("autocomplete_java", "bool", True, "Autocomplete em Java", CE),
        opt("autocomplete_python", "bool", True, "Autocomplete em Python", CE),
    ]
    if nvim:
        O.append(opt("autocomplete_outros", "bool", True, "Autocomplete nas outras linguagens", CE))
    O += [
        opt("inlay", "bool", True, "Dicas de parâmetro no código (inlay hints)", CE, "Texto fantasma tipo nome: valor ao chamar funções."),
        opt("matchparen", "bool", True, "Destacar o par de parênteses", CE),
        opt("header", "bool", True, "Header no topo da janela", CE),
    ]
    if nvim:
        O += [
            opt("hints", "bool", False, "Dicas de método (assinatura e documentação)", CE, "Popups com a assinatura do método e a documentação ao lado das sugestões."),
            opt("diagnostics", "bool", True, "Avisos e erros no código", CE, "Texto, sublinhado e sinais de erro/aviso do LSP."),
            opt("lsp_progress", "bool", False, "Mensagens de progresso do LSP", CE, "Ex.: \"Validate documents jdtls\" no canto."),
            opt("spell", "bool", False, "Correção ortográfica", CE),
            opt("showmode", "bool", False, "Mostrar o modo na linha de comando", CE, "A barra de status já mostra o modo."),
            opt("smartindent", "bool", True, "Indentação inteligente", CE),
            opt("autoindent", "bool", True, "Manter a indentação da linha anterior", CE),
            opt("backup", "bool", False, "Criar backups automáticos", CE),
            opt("writebackup", "bool", True, "Backup antes de sobrescrever", CE),
        ]
    # Templates: texto de todo arquivo novo de cada linguagem.
    TP = "Templates"
    tdesc = "Texto de todo arquivo {ext} novo. __CURSOR__ marca onde o cursor começa{extra}. Vazio = arquivo em branco."
    for k, label, ext, extra in TEMPLATES:
        O.append(opt(f"template_{k}", "lines", template_default(k), label, TP, tdesc.format(ext=ext, extra=extra)))
    if nvim:
        plugins = [
            ("neo_tree", "Árvore de arquivos (Neo-tree, F2)"), ("telescope", "Busca de arquivos e texto (Telescope)"),
            ("which_key", "Ajuda de atalhos ao digitar (which-key)"), ("bufferline", "Abas de buffers no topo"),
            ("lualine", "Barra de status (lualine)"), ("dashboard", "Tela inicial (dashboard)"),
            ("gitsigns", "Marcas de git na margem"), ("noice", "Linha de comando e mensagens flutuantes (noice)"),
            ("notify", "Notificações (nvim-notify)"), ("indent_blankline", "Guias de indentação"),
            ("indent_scope", "Destaque do bloco atual (mini.indentscope)"), ("todo_comments", "Destacar TODO/FIXME"),
            ("trouble", "Lista de erros (trouble)"), ("illuminate", "Destacar outras ocorrências da palavra"),
            ("cursorword", "Sublinhar a palavra sob o cursor"), ("scrollbar", "Barra de rolagem com marcas"),
        ]
        desc = "Desligar não apaga o plugin; ele só deixa de carregar (reabra o Neovim)."
    else:
        plugins = [("airline", "Barra de status (airline)"), ("nerdtree", "Árvore de arquivos (NERDTree, F2)"),
                   ("fugitive", "Comandos de git (fugitive)"), ("coc", "Autocomplete e LSP (coc.nvim)")]
        desc = "Desligar não apaga o plugin; ele só deixa de carregar (reabra o Vim)."
    for k, label in plugins:
        O.append(opt(f"plugin_{k}", "bool", True, label, P, desc))
    # Descobrir: extras que começam desligados; ligar baixa o plugin.
    D = "Descobrir"
    if nvim:
        found = [
            ("flash", "Pular para qualquer lugar (flash)", "<leader>j + duas letras leva o cursor a qualquer ponto visível da tela.", "https://github.com/folke/flash.nvim"),
            ("surround", "Trocar o que está em volta (surround)", "cs\"' troca aspas por apóstrofos, ds( tira parênteses, ysiw] cerca a palavra.", "https://github.com/kylechui/nvim-surround"),
            ("oil", "Editar pastas como texto (oil)", "\"-\" abre a pasta do arquivo: renomeie, mova e apague editando as linhas e salve com :w.", "https://github.com/stevearc/oil.nvim"),
            ("toggleterm", "Terminal flutuante (toggleterm)", "Ctrl+\\ abre e fecha um terminal por cima do código.", "https://github.com/akinsho/toggleterm.nvim"),
            ("zen_mode", "Modo foco (zen-mode)", "<leader>z centraliza o texto e esconde números, barras e abas.", "https://github.com/folke/zen-mode.nvim"),
            ("tmux_navigator", "Navegar junto com o tmux", "Ctrl+h/j/k/l anda entre janelas do Neovim e painéis do tmux. Ligue também no Descobrir do tmux.", "https://github.com/christoomey/vim-tmux-navigator"),
            ("lazygit", "lazygit dentro do editor", "<leader>gg abre o lazygit numa janela flutuante. Precisa do módulo lazygit.", "https://github.com/kdheepak/lazygit.nvim"),
            ("persistence", "Lembrar os arquivos abertos (sessões)", "<leader>ps reabre os arquivos que estavam abertos nesta pasta; <leader>pl, a última sessão.", "https://github.com/folke/persistence.nvim"),
            ("render_markdown", "Markdown desenhado no editor", "Títulos, listas, tabelas e caixas de código aparecem formatados nos arquivos .md.", "https://github.com/MeanderingProgrammer/render-markdown.nvim"),
            ("colorizer", "Mostrar cores no código", "Pinta #rrggbb, rgb() e hsl() com a própria cor (CSS, configs de tema...).", "https://github.com/catgoose/nvim-colorizer.lua"),
        ]
        tail = " Baixa ao reabrir o Neovim."
    else:
        found = [
            ("surround", "Trocar o que está em volta (surround)", "cs\"' troca aspas por apóstrofos, ds( tira parênteses, ysiw] cerca a palavra.", "https://github.com/tpope/vim-surround"),
            ("commentary", "Comentar com gcc (commentary)", "gcc comenta a linha; gc no modo visual comenta a seleção, em qualquer linguagem.", "https://github.com/tpope/vim-commentary"),
            ("fzf", "Buscar arquivos e texto (fzf.vim)", "<leader>ff arquivos, <leader>fg texto (ripgrep), <leader>fb buffers. Precisa dos módulos fzf e ripgrep.", "https://github.com/junegunn/fzf.vim"),
            ("gitgutter", "Marcas de git na margem (gitgutter)", "Mostra linhas adicionadas, mudadas e apagadas ao lado da numeração.", "https://github.com/airblade/vim-gitgutter"),
            ("tmux_navigator", "Navegar junto com o tmux", "Ctrl+h/j/k/l anda entre janelas do Vim e painéis do tmux. Ligue também no Descobrir do tmux.", "https://github.com/christoomey/vim-tmux-navigator"),
            ("startify", "Tela inicial com arquivos recentes (startify)", "Abrir o Vim sem arquivo mostra os recentes e as sessões salvas.", "https://github.com/mhinz/vim-startify"),
            ("goyo", "Modo foco (goyo)", "<leader>z centraliza o texto e esconde o resto da interface.", "https://github.com/junegunn/goyo.vim"),
            ("indentline", "Guias de indentação (indentLine)", "Uma linha fininha marca cada nível de indentação.", "https://github.com/Yggdroot/indentLine"),
        ]
        tail = " Baixa na hora, com o Vim aberto."
    for k, label, d, link in found:
        O.append(opt(f"plugin_{k}", "bool", False, label, D, d + tail, link=link))
    O.append(opt("extra", "lines", "", "Configuração extra", X,
                 ("Código Lua" if nvim else "Vimscript") + " executado por último, a cada vez que as opções são aplicadas. Para o que não tem botão aqui."))
    return O

def toml_value(v):
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, int):
        return str(v)
    return q(v)

def options_toml(O):
    out = ["", "# ---------------------------------------------------------------- opções",
           "# Geradas por scripts/gen-editor-options.py; o app mostra na ordem abaixo,",
           "# agrupadas por seção. Vão para files/hiraku.json, que o editor lê e reaplica ao vivo."]
    for o in O:
        out.append(f"\n[options.{o['key']}]")
        out.append(f"type = {q(o['kind'])}")
        if o["default"] is not None:
            out.append(f"default = {toml_value(o['default'])}")
        for k in ("min", "max"):
            if k in o:
                out.append(f"{k} = {o[k]}")
        if "values" in o:
            out.append("values = [" + ", ".join(q(v) for v in o["values"]) + "]")
        out.append(f"label = {q(o['label'])}")
        out.append(f"section = {q(o['section'])}")
        if o["description"]:
            out.append(f"description = {q(o['description'])}")
        if "preview" in o:
            out.append(f"preview = {q(o['preview'])}")
        if "link" in o:
            out.append(f"link = {q(o['link'])}")
        if "labels" in o:
            out.append(f"[options.{o['key']}.labels]")
            for k, v in o["labels"].items():
                out.append(f"{q(k)} = {q(v)}")
    return "\n".join(out) + "\n"

def json_template(O, editor_name):
    lines = ["{", f'  "_aviso": "Gerado pelo Hiraku Setup Manager. Mude pelo app ou com: hiraku set {editor_name} chave=valor",']
    for i, o in enumerate(O):
        raw = o["kind"] in ("bool", "number", "keymaps")
        val = "{{ %s }}" % o["key"] if raw else "{{ %s | json }}" % o["key"]
        comma = "," if i < len(O) - 1 else ""
        lines.append(f'  "{o["key"]}": {val}{comma}')
    lines.append("}")
    return "\n".join(lines) + "\n"

MARK = "\n# ---------------------------------------------------------------- opções"
for name in ("vim", "nvim"):
    O = editor(name)
    p = f"{MOD}/{name}/module.toml"
    s = open(p).read()
    s = s.split(MARK)[0].rstrip("\n") + "\n"
    open(p, "w").write(s + options_toml(O))
    open(f"{MOD}/{name}/files/hiraku.json", "w").write(json_template(O, name))
    print(name, len(O), "opções")
