" ============================================================
" Opções escolhidas no Hiraku Setup Manager (~/.vim/hiraku.json).
"
" - Aplicadas ao abrir (opções, atalhos, tema, plugins).
" - Reaplicadas sozinhas quando o app grava o arquivo de novo (o Vim confere a
"   cada 2 s): dá para mudar tudo com o Vim aberto.
" - :HirakuReload força a releitura.
" ============================================================
if exists('g:loaded_hiraku')
    finish
endif
let g:loaded_hiraku = 1

let s:file = expand('~/.vim/hiraku.json')
let s:mtime = -1
let s:applied = []
let g:hiraku = {}

function! HirakuLoad() abort
    let g:hiraku = {}
    let s:mtime = getftime(s:file)
    if !filereadable(s:file)
        return 0
    endif
    try
        let l:data = json_decode(join(readfile(s:file), "\n"))
        if type(l:data) == v:t_dict
            let g:hiraku = l:data
        endif
    catch
        echohl WarningMsg | echom 'Hiraku: ~/.vim/hiraku.json inválido' | echohl None
    endtry
    return !empty(g:hiraku)
endfunction

function! HirakuHas(key) abort
    return has_key(g:hiraku, a:key) && type(g:hiraku[a:key]) != v:t_none
endfunction

function! HirakuGet(key, default) abort
    return HirakuHas(a:key) ? g:hiraku[a:key] : a:default
endfunction

" Plugin ligado? (sem hiraku.json, todos ligados)
function! HirakuPlugin(name) abort
    return HirakuGet('plugin_' . a:name, v:true) ? 1 : 0
endfunction

call HirakuLoad()

" ---------------------------------------------------------------- templates
" Arquivo novo: o template do app (template_<lang> no hiraku.json) ou, sem ele, o
" arquivo padrão. __CLASSNAME__ vira o nome do arquivo; __CURSOR__ marca o cursor.
function! IcaroInsertTemplate(key, file) abort
    let l:key = 'template_' . a:key
    if HirakuHas(l:key)
        let l:text = g:hiraku[l:key]
        if type(l:text) != v:t_string || l:text =~# '^\_s*$'
            return
        endif
        let l:lines = split(l:text, "\n", 1)
        if len(l:lines) > 1 && l:lines[-1] ==# ''
            call remove(l:lines, -1)
        endif
    elseif !empty(a:file) && filereadable(expand(a:file))
        let l:lines = readfile(expand(a:file))
    else
        return
    endif
    let l:name = escape(expand('%:t:r'), '\&~')
    call map(l:lines, {_, l -> substitute(l, '__CLASSNAME__', l:name, 'g')})
    let l:pos = [1, 1]
    for l:i in range(len(l:lines))
        let l:c = stridx(l:lines[l:i], '__CURSOR__')
        if l:c >= 0
            let l:pos = [l:i + 1, l:c + 1]
            let l:lines[l:i] = substitute(l:lines[l:i], '__CURSOR__', '', '')
            break
        endif
    endfor
    silent! %delete _
    call setline(1, l:lines)
    call cursor(l:pos[0], l:pos[1])
endfunction

" ---------------------------------------------------------------- Central <-> app
" Um toggle mudou no editor (F3, F4, F10...): grava no app também, se o app
" controla essa chave. Assim o hiraku.json e o editor não discordam.
function! HirakuToApp(key, value) abort
    if !HirakuHas(a:key) || g:hiraku[a:key] == a:value
        return
    endif
    let g:hiraku[a:key] = a:value
    let l:exe = executable('hiraku') ? 'hiraku' : executable('hiraku-setup-manager') ? 'hiraku-setup-manager' : ''
    if empty(l:exe) || !exists('*job_start')
        return
    endif
    let l:text = type(a:value) == v:t_bool ? (a:value ? 'true' : 'false') : string(a:value)
    call job_start([l:exe, 'set', 'vim', a:key . '=' . l:text], {'in_io': 'null', 'out_io': 'null', 'err_io': 'null'})
endfunction

" ---------------------------------------------------------------- aplicar
function! HirakuLeader() abort
    let l:leader = {'space': ' ', 'comma': ',', 'backslash': '\'}
    let g:mapleader = get(l:leader, HirakuGet('leader', 'backslash'), '\')
    let g:maplocalleader = g:mapleader
endfunction

let s:bools = ['number', 'relativenumber', 'cursorline', 'cursorcolumn', 'list', 'wrap', 'spell',
      \ 'showmode', 'smartindent', 'autoindent', 'backup', 'writebackup',
      \ 'linebreak', 'expandtab', 'undofile', 'swapfile', 'confirm', 'splitright', 'splitbelow',
      \ 'ignorecase', 'smartcase', 'hlsearch', 'incsearch']

function! HirakuApplyOptions() abort
    for l:k in s:bools
        if HirakuHas(l:k) && type(g:hiraku[l:k]) == v:t_bool && exists('+' . l:k)
            execute 'set ' . (g:hiraku[l:k] ? '' : 'no') . l:k
        endif
    endfor
    if HirakuHas('colorcolumn') && exists('+colorcolumn')
        let &colorcolumn = g:hiraku.colorcolumn ==# '0' ? '' : g:hiraku.colorcolumn
    endif
    if HirakuHas('signcolumn') && exists('+signcolumn')
        silent! let &signcolumn = g:hiraku.signcolumn
    endif
    if HirakuHas('scrolloff')
        let &scrolloff = g:hiraku.scrolloff
    endif
    if HirakuHas('tabsize')
        let &tabstop = g:hiraku.tabsize
        let &shiftwidth = g:hiraku.tabsize
        let &softtabstop = g:hiraku.tabsize
    endif
    if HirakuHas('mouse')
        let &mouse = g:hiraku.mouse ==# 'off' ? '' : g:hiraku.mouse
    endif
    if HirakuHas('timeoutlen')
        let &timeoutlen = g:hiraku.timeoutlen
    endif
    if HirakuHas('clipboard') && has('clipboard')
        let &clipboard = g:hiraku.clipboard ==# 'system' ? 'unnamedplus' : ''
    endif
endfunction

" Atalhos criados por aqui; ao reaplicar, saem e o atalho anterior (do Ícaro) volta.
function! s:Map(mode, lhs, rhs, remap) abort
    let l:prev = maparg(a:lhs, a:mode, 0, 1)
    call add(s:applied, {'mode': a:mode, 'lhs': a:lhs, 'prev': l:prev})
    let l:lhs = substitute(a:lhs, '|', '<Bar>', 'g')
    let l:rhs = substitute(a:rhs, '|', '<Bar>', 'g')
    execute a:mode . (a:remap ? 'map' : 'noremap') . ' <silent> ' . l:lhs . ' ' . l:rhs
endfunction

function! s:UnmapAll() abort
    for l:a in reverse(copy(s:applied))
        silent! execute l:a.mode . 'unmap ' . substitute(l:a.lhs, '|', '<Bar>', 'g')
        if !empty(l:a.prev) && exists('*mapset')
            silent! call mapset(l:a.mode, 0, l:a.prev)
        endif
    endfor
    let s:applied = []
endfunction

function! HirakuApplyKeymaps() abort
    call s:UnmapAll()
    if exists('*HirakuApplyFkeys')
        call HirakuApplyFkeys()
    endif
    let l:mode = HirakuGet('delete_mode', 'copy')
    if l:mode ==# 'blackhole'
        " d, c, x, s jogam fora sem tocar no que foi copiado; <leader>d recorta.
        for l:k in ['d', 'D', 'c', 'C', 'x', 'X', 's', 'S']
            call s:Map('n', l:k, '"_' . l:k, 0)
            call s:Map('x', l:k, '"_' . l:k, 0)
        endfor
        call s:Map('n', '<leader>d', 'd', 0)
        call s:Map('x', '<leader>d', 'd', 0)
        call s:Map('n', '<leader>dd', 'dd', 0)
        call s:Map('n', '<leader>D', 'D', 0)
    elseif l:mode ==# 'x_only'
        for l:k in ['x', 'X', 'c', 'C', 's', 'S']
            call s:Map('n', l:k, '"_' . l:k, 0)
            call s:Map('x', l:k, '"_' . l:k, 0)
        endfor
    endif
    if HirakuGet('paste_keeps', v:true)
        " No visual, P troca a seleção sem copiar o texto substituído (Vim 8.2.4242+).
        call s:Map('x', 'p', has('patch-8.2.4242') ? 'P' : '"_dP', 0)
    endif
    for l:km in HirakuGet('keymaps', [])
        if type(l:km) != v:t_dict || empty(get(l:km, 'lhs', '')) || empty(get(l:km, 'rhs', ''))
            continue
        endif
        for l:m in split(get(l:km, 'mode', 'n'), '\zs')
            try
                call s:Map(l:m, l:km.lhs, l:km.rhs, get(l:km, 'remap', v:false) is v:true)
            catch
                echohl WarningMsg | echom 'Hiraku: atalho ' . l:km.lhs . ' inválido: ' . v:exception | echohl None
            endtry
        endfor
    endfor
endfunction

function! s:Transparent() abort
    for l:g in ['Normal', 'NonText', 'SignColumn', 'EndOfBuffer', 'LineNr', 'FoldColumn']
        execute 'highlight ' . l:g . ' guibg=NONE ctermbg=NONE'
    endfor
endfunction

function! HirakuApplyTheme() abort
    let l:name = HirakuGet('colorscheme', 'auto')
    if l:name !=# 'auto' && get(g:, 'colors_name', '') !=# l:name
        try
            execute 'colorscheme ' . l:name
            " Mantém o F3/:Th andando a partir do tema escolhido.
            for l:i in range(len(get(g:, 'icaro_themes', [])))
                if g:icaro_themes[l:i].id ==# l:name
                    let g:icaro_theme_index = l:i
                endif
            endfor
        catch
            echohl WarningMsg | echom 'Hiraku: tema ' . l:name . ' não encontrado' | echohl None
        endtry
    endif
    if HirakuGet('transparent', v:false)
        call s:Transparent()
    endif
endfunction

function! HirakuApplyAutopairs() abort
    if HirakuHas('autopairs') && exists('g:icaro_pairs_enabled') && exists('*ToggleAutoPairs')
          \ && (g:icaro_pairs_enabled ? 1 : 0) != (g:hiraku.autopairs ? 1 : 0)
        silent! call ToggleAutoPairs()
    endif
endfunction

" Central: com o Vim já aberto, aciona o toggle quando o app discorda; ao abrir,
" só ajusta o estado inicial (os recursos ainda não carregaram).
function! s:Toggle(appkey, current, fn) abort
    if !HirakuHas(a:appkey) || (a:current ? 1 : 0) == (g:hiraku[a:appkey] ? 1 : 0)
        return
    endif
    if v:vim_did_enter && exists('*' . a:fn)
        silent! call call(a:fn, [])
    endif
endfunction

function! HirakuApplyToggles() abort
    let l:live = v:vim_did_enter
    " autocomplete por linguagem
    if exists('g:icaro_ac_state')
        for l:lang in ['cpp', 'java', 'python']
            let l:k = 'autocomplete_' . l:lang
            if HirakuHas(l:k) && (get(g:icaro_ac_state, l:lang, 1) ? 1 : 0) != (g:hiraku[l:k] ? 1 : 0)
                let g:icaro_ac_state[l:lang] = g:hiraku[l:k] ? 1 : 0
                for l:b in range(1, bufnr('$'))
                    if bufexists(l:b) && getbufvar(l:b, '&filetype') ==# l:lang
                        call setbufvar(l:b, 'coc_suggest_disable', g:hiraku[l:k] ? 0 : 1)
                    endif
                endfor
            endif
        endfor
    endif
    if l:live
        call s:Toggle('inlay', get(g:, 'my_inlay_hints_enabled', 1), 'ToggleInlayHints')
        call s:Toggle('matchparen', get(g:, 'icaro_matchparen_enabled', 1), 'ToggleMatchParen')
        call s:Toggle('header', !get(g:, 'icaro_header_hidden', 0), 'ToggleHeader')
    else
        if HirakuHas('inlay') | let g:my_inlay_hints_enabled = g:hiraku.inlay ? 1 : 0 | endif
        if HirakuHas('matchparen') | let g:icaro_matchparen_enabled = g:hiraku.matchparen ? 1 : 0 | endif
        if HirakuHas('header') | let g:icaro_header_hidden = g:hiraku.header ? 0 : 1 | endif
    endif
endfunction

function! HirakuApplyExtra() abort
    let l:code = HirakuGet('extra', '')
    if type(l:code) != v:t_string || l:code =~# '^\_s*$'
        return
    endif
    let l:tmp = tempname() . '.vim'
    call writefile(split(l:code, "\n"), l:tmp)
    try
        execute 'source ' . fnameescape(l:tmp)
    catch
        echohl ErrorMsg | echom 'Hiraku: erro na configuração extra: ' . v:exception | echohl None
    finally
        call delete(l:tmp)
    endtry
endfunction

function! HirakuApplyAll() abort
    call HirakuApplyOptions()
    call HirakuApplyKeymaps()
    call HirakuApplyToggles()
    call HirakuApplyTheme()
    call HirakuApplyAutopairs()
    call HirakuApplyExtra()
    if exists('*HirakuApplyDiscover')
        call HirakuApplyDiscover()
    endif
endfunction

" ---------------------------------------------------------------- ao vivo
function! s:Check(...) abort
    if getftime(s:file) != s:mtime
        call HirakuLoad()
        call HirakuApplyAll()
        redraw
        echom 'Configurações do Hiraku aplicadas'
    endif
endfunction

function! HirakuSetup() abort
    augroup hiraku
        autocmd!
        autocmd ColorScheme * if HirakuGet('transparent', v:false) | call s:Transparent() | endif
        autocmd FocusGained * call s:Check()
    augroup END
    " O tema salvo do Ícaro é aplicado no VimEnter; o do app vem depois dele.
    autocmd hiraku VimEnter * ++once call HirakuApplyTheme() | call HirakuApplyExtra()
    command! HirakuReload let s:mtime = -2 | call s:Check()
    if has('timers')
        call timer_start(2000, function('s:Check'), {'repeat': -1})
    endif
endfunction
