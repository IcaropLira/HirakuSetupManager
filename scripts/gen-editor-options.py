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
            "Colorscheme do editor. \"O último escolhido no editor\" deixa a troca com F3/:Th.",
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
