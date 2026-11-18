" ============================================================
" o_do_lol.vim — colorscheme "o do LOL" (Configuracao Vim Icaro Lira)
" Terminal de laboratório: fundo vinho-amarronzado, texto creme,
" palavras-chave e tipos em negrito (sem mudar de cor), strings em
" amarelo-claro, numeração amarelada e a linha do cursor cor de tijolo.
" ============================================================

set background=dark
hi clear
if exists('syntax_on')
    syntax reset
endif
let g:colors_name = 'o_do_lol'

" ------------------------------------------------------------
" Paleta (o app usa estes nomes na pré-visualização)
" ------------------------------------------------------------
let s:bg0     = ['#2b1a1e', 234]
let s:bg1     = ['#36222a', 235]
let s:bg2     = ['#4a2e26', 52]
let s:bg3     = ['#5e3a44', 95]
let s:fg0     = ['#f0dfc4', 223]
let s:fg1     = ['#fbf0d6', 230]
let s:comment = ['#d9c7a8', 187]
let s:keyword = ['#fbf0d6', 230]
let s:red     = ['#fbf0d6', 230]
let s:string  = ['#ece3a0', 229]
let s:green   = ['#ece3a0', 229]
let s:amber   = ['#ece3a0', 229]
let s:func    = ['#f0dfc4', 223]
let s:blue    = ['#f0dfc4', 223]
let s:linenr  = ['#e6c48e', 180]
let s:curnr   = ['#f6dc9c', 222]
let s:tilde   = ['#6e6370', 242]
let s:status  = ['#cdc8c2', 251]
let s:statusnc = ['#8d8580', 245]
let s:dark    = ['#1c1114', 233]
let s:err     = ['#e8806e', 174]
let s:warn    = ['#e8c06e', 179]
let s:info    = ['#b8c8a0', 151]
let s:visual  = ['#5c3b46', 95]
let s:search  = ['#e6c48e', 180]

function! s:hi(group, fg, bg, style) abort
    let l:cmd = 'hi ' . a:group
    let l:cmd .= empty(a:fg) ? ' guifg=NONE ctermfg=NONE' : ' guifg=' . a:fg[0] . ' ctermfg=' . a:fg[1]
    let l:cmd .= empty(a:bg) ? ' guibg=NONE ctermbg=NONE' : ' guibg=' . a:bg[0] . ' ctermbg=' . a:bg[1]
    let l:cmd .= empty(a:style) ? ' gui=NONE cterm=NONE' : ' gui=' . a:style . ' cterm=' . a:style
    execute l:cmd
endfunction

" ------------------------------------------------------------
" Interface
" ------------------------------------------------------------
call s:hi('Normal',        s:fg0, s:bg0, '')
call s:hi('NormalNC',      s:fg0, s:bg0, '')
call s:hi('NormalFloat',   s:fg0, s:bg1, '')
call s:hi('FloatBorder',   s:linenr, s:bg1, '')
call s:hi('NonText',       s:tilde, [], '')
call s:hi('EndOfBuffer',   s:tilde, [], '')
call s:hi('Cursor',        s:dark, s:curnr, '')
call s:hi('CursorLine',    [], s:bg2, 'NONE')
call s:hi('CursorColumn',  [], s:bg2, 'NONE')
call s:hi('ColorColumn',   [], s:bg1, 'NONE')
call s:hi('LineNr',        s:linenr, [], '')
call s:hi('CursorLineNr',  s:curnr, s:bg2, 'bold')
call s:hi('SignColumn',    s:linenr, [], '')
call s:hi('FoldColumn',    s:linenr, [], '')
call s:hi('Folded',        s:comment, s:bg1, '')
call s:hi('VertSplit',     s:statusnc, [], '')
call s:hi('WinSeparator',  s:statusnc, [], '')
call s:hi('StatusLine',    s:dark, s:status, 'bold')
call s:hi('StatusLineNC',  s:dark, s:statusnc, '')
call s:hi('TabLine',       s:dark, s:statusnc, '')
call s:hi('TabLineFill',   [], s:bg1, '')
call s:hi('TabLineSel',    s:dark, s:status, 'bold')
call s:hi('WinBar',        s:fg1, [], 'bold')
call s:hi('WinBarNC',      s:comment, [], '')
call s:hi('Visual',        [], s:visual, '')
call s:hi('VisualNOS',     [], s:visual, '')
call s:hi('Search',        s:dark, s:search, '')
call s:hi('IncSearch',     s:dark, s:curnr, 'bold')
call s:hi('CurSearch',     s:dark, s:curnr, 'bold')
call s:hi('MatchParen',    s:curnr, s:bg3, 'bold')
call s:hi('Directory',     s:fg1, [], 'bold')
call s:hi('Title',         s:fg1, [], 'bold')
call s:hi('ModeMsg',       s:fg1, [], 'bold')
call s:hi('MoreMsg',       s:info, [], 'bold')
call s:hi('Question',      s:info, [], 'bold')
call s:hi('WarningMsg',    s:warn, [], 'bold')
call s:hi('ErrorMsg',      s:fg1, s:err, 'bold')
call s:hi('WildMenu',      s:dark, s:status, 'bold')
call s:hi('Pmenu',         s:fg0, s:bg1, '')
call s:hi('PmenuSel',      s:dark, s:status, 'bold')
call s:hi('PmenuSbar',     [], s:bg1, '')
call s:hi('PmenuThumb',    [], s:statusnc, '')
call s:hi('SpecialKey',    s:tilde, [], '')
call s:hi('Whitespace',    s:tilde, [], '')
call s:hi('DiffAdd',       s:info, s:bg1, '')
call s:hi('DiffChange',    s:warn, s:bg1, '')
call s:hi('DiffDelete',    s:err, s:bg1, '')
call s:hi('DiffText',      s:dark, s:warn, 'bold')
call s:hi('SpellBad',      [], [], 'undercurl')
call s:hi('SpellCap',      [], [], 'undercurl')

" ------------------------------------------------------------
" Código: quase tudo creme; o que muda é o negrito e as strings
" ------------------------------------------------------------
call s:hi('Comment',       s:comment, [], '')
call s:hi('Constant',      s:string, [], '')
call s:hi('String',        s:string, [], '')
call s:hi('Character',     s:string, [], '')
call s:hi('Number',        s:string, [], '')
call s:hi('Boolean',       s:fg1, [], 'bold')
call s:hi('Float',         s:string, [], '')
call s:hi('Identifier',    s:fg0, [], '')
call s:hi('Function',      s:fg0, [], '')
call s:hi('Statement',     s:keyword, [], 'bold')
call s:hi('Conditional',   s:keyword, [], 'bold')
call s:hi('Repeat',        s:keyword, [], 'bold')
call s:hi('Label',         s:keyword, [], 'bold')
call s:hi('Operator',      s:fg0, [], '')
call s:hi('Keyword',       s:keyword, [], 'bold')
call s:hi('Exception',     s:keyword, [], 'bold')
call s:hi('PreProc',       s:fg1, [], '')
call s:hi('Include',       s:fg1, [], '')
call s:hi('Define',        s:fg1, [], '')
call s:hi('Macro',         s:fg1, [], '')
call s:hi('PreCondit',     s:fg1, [], '')
call s:hi('Type',          s:keyword, [], 'bold')
call s:hi('StorageClass',  s:keyword, [], 'bold')
call s:hi('Structure',     s:keyword, [], 'bold')
call s:hi('Typedef',       s:keyword, [], 'bold')
call s:hi('Special',       s:fg1, [], '')
call s:hi('SpecialChar',   s:string, [], '')
call s:hi('Tag',           s:fg1, [], '')
call s:hi('Delimiter',     s:fg0, [], '')
call s:hi('SpecialComment', s:comment, [], 'bold')
call s:hi('Debug',         s:warn, [], '')
call s:hi('Underlined',    s:fg1, [], 'underline')
call s:hi('Ignore',        s:tilde, [], '')
call s:hi('Error',         s:fg1, s:err, 'bold')
call s:hi('Todo',          s:dark, s:curnr, 'bold')

" ------------------------------------------------------------
" coc.nvim / diagnósticos / NERDTree
" ------------------------------------------------------------
call s:hi('CocErrorSign',      s:err, [], '')
call s:hi('CocWarningSign',    s:warn, [], '')
call s:hi('CocInfoSign',       s:info, [], '')
call s:hi('CocHintSign',       s:comment, [], '')
call s:hi('CocErrorHighlight',   [], [], 'undercurl')
call s:hi('CocWarningHighlight', [], [], 'undercurl')
call s:hi('CocFloating',       s:fg0, s:bg1, '')
call s:hi('CocMenuSel',        s:dark, s:status, 'bold')
call s:hi('CocInlayHint',      s:tilde, [], 'italic')
call s:hi('NERDTreeDir',       s:fg1, [], 'bold')
call s:hi('NERDTreeDirSlash',  s:linenr, [], '')
call s:hi('NERDTreeFile',      s:fg0, [], '')
call s:hi('NERDTreeExecFile',  s:string, [], '')
call s:hi('NERDTreeCWD',       s:curnr, [], 'bold')

let g:terminal_ansi_colors = [
      \ s:dark[0], s:err[0], s:info[0], s:warn[0],
      \ s:linenr[0], s:bg3[0], s:string[0], s:fg0[0],
      \ s:tilde[0], s:err[0], s:info[0], s:curnr[0],
      \ s:linenr[0], s:visual[0], s:string[0], s:fg1[0]
      \ ]
