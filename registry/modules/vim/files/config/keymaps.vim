if exists('g:loaded_my_keymaps')
    finish
endif
let g:loaded_my_keymaps = 1

" ============================================================
" Atalhos
" ============================================================

" Salvar
nnoremap <C-s> :write<CR>
inoremap <C-s> <Esc>:write<CR>a


" ------------------------------------------------------------
" Comandos rápidos equivalentes aos F-keys.
" Vim exige que comandos definidos pelo usuário comecem com maiúscula;
" por isso os aliases são curtos e fáceis de decorar:
"   :Th :Pv :Nt :Ih :Ac :Cp :Rn :Br :Ti :Te :Pa :Mh :Of :Hd :Cs
" ------------------------------------------------------------

" Navegação entre janelas
nnoremap <C-h> <C-w>h
nnoremap <C-j> <C-w>j
nnoremap <C-k> <C-w>k
nnoremap <C-l> <C-w>l

" ------------------------------------------------------------
" Teclas de função: cada ação tem uma tecla, configurável no Hiraku Setup
" Manager (Vim → Teclas de função) ou no hiraku.json (key_<ação>). Valem no
" modo normal e no de inserção (com <C-o>, sem te tirar do insert).
" ------------------------------------------------------------
" ação -> [tecla padrão, descrição na folha de atalhos, comando :X]
let g:icaro_actions = [
      \ ['theme_next', '<F1>', 'próximo tema', 'Th'],
      \ ['theme_prev', '<S-F1>', 'tema anterior', 'Pv'],
      \ ['explorer', '<F2>', 'abrir/fechar o explorer (NERDTree)', 'Nt'],
      \ ['inlay', '<F3>', 'liga/desliga dicas de parâmetro inline', 'Ih'],
      \ ['autocomplete', '<F4>', 'liga/desliga o autocomplete (da linguagem)', 'Ac'],
      \ ['compile', '<F5>', 'compilar', 'Cp'],
      \ ['run', '<F6>', 'executar', 'Rn'],
      \ ['build_run', '<F7>', 'compilar + executar (Python: salvar + executar)', 'Br'],
      \ ['test', '<F8>', 'compilar + executar com input.txt', 'Ti'],
      \ ['terminal', '<F9>', 'abrir terminal', 'Te'],
      \ ['autopairs', '<F10>', 'liga/desliga fecha-parênteses/aspas e assinatura', 'Pa'],
      \ ['matchparen', '<S-F10>', 'liga/desliga destaque do par de parênteses', 'Mh'],
      \ ['quiet', '<S-F11>', 'modo silencioso', 'Of'],
      \ ['cheatsheet', '<F12>', 'esta folha de atalhos', 'Cs'],
      \ ['header', '<S-F12>', 'mostra/esconde o header no topo', 'Hd'],
      \ ]

function! IcaroAction(id) abort
    let l:ft = &filetype
    if a:id ==# 'theme_next'
        call CycleTheme()
    elseif a:id ==# 'theme_prev'
        call PreviousTheme()
    elseif a:id ==# 'explorer'
        silent! NERDTreeToggle
    elseif a:id ==# 'inlay'
        call ToggleInlayHints()
    elseif a:id ==# 'autocomplete'
        call ToggleAutocomplete()
    elseif a:id ==# 'compile'
        call IcaroDispatchFunctionKey(5)
    elseif a:id ==# 'run'
        call IcaroDispatchFunctionKey(6)
    elseif a:id ==# 'build_run'
        call IcaroDispatchFunctionKey(7)
    elseif a:id ==# 'test'
        call IcaroDispatchFunctionKey(8)
    elseif a:id ==# 'terminal'
        execute 'terminal'
    elseif a:id ==# 'autopairs'
        call ToggleAutoPairs()
    elseif a:id ==# 'matchparen'
        call ToggleMatchParen()
    elseif a:id ==# 'quiet'
        call ToggleQuietMode()
    elseif a:id ==# 'cheatsheet'
        call ToggleCheatsheet()
    elseif a:id ==# 'header'
        call ToggleHeader()
    endif
endfunction

for s:a in g:icaro_actions
    execute 'command! ' . s:a[3] . ' call IcaroAction(' . string(s:a[0]) . ')'
endfor

" Tecla atual de cada ação: [tecla, descrição] (a folha de atalhos usa).
let g:icaro_fkeys = []
let s:fk_mapped = []

function! s:FkeyUnmap() abort
    for [l:mode, l:lhs] in s:fk_mapped
        silent! execute l:mode . 'unmap ' . l:lhs
    endfor
    let s:fk_mapped = []
endfunction

function! s:FkeyMap(lhs, id) abort
    let l:call = ':call IcaroAction(' . string(a:id) . ')<CR>'
    execute 'nnoremap <silent> ' . a:lhs . ' ' . l:call
    execute 'inoremap <silent> ' . a:lhs . ' <C-o>' . l:call
    call add(s:fk_mapped, ['n', a:lhs])
    call add(s:fk_mapped, ['i', a:lhs])
endfunction

function! HirakuApplyFkeys() abort
    call s:FkeyUnmap()
    let g:icaro_fkeys = []
    for [l:id, l:default, l:desc, l:cmd] in g:icaro_actions
        let l:key = exists('*HirakuGet') ? HirakuGet('key_' . l:id, l:default) : l:default
        if type(l:key) != v:t_string || l:key =~# '^\s*$'
            continue
        endif
        let l:key = substitute(l:key, '\s', '', 'g')
        try
            call s:FkeyMap(l:key, l:id)
        catch
            echohl WarningMsg | echom 'Hiraku: tecla ' . l:key . ' inválida para ' . l:id | echohl None
            continue
        endtry
        " Shift+F1..F12 chega como F13..F24 em vários terminais.
        let l:m = matchlist(l:key, '\c^<S-F\(\d\+\)>$')
        if !empty(l:m) && str2nr(l:m[1]) <= 12
            call s:FkeyMap('<F' . (str2nr(l:m[1]) + 12) . '>', l:id)
        endif
        call add(g:icaro_fkeys, [l:key, l:desc . '  (:' . l:cmd . ')'])
    endfor
endfunction
call HirakuApplyFkeys()

" ------------------------------------------------------------
" Shift+Backspace (ou Ctrl+Backspace, dependendo do terminal) apaga
" os dois lados de um parêntese/chave/colchete/aspas vazios de uma
" vez, se o cursor estiver bem no meio deles. Backspace normal
" continua apagando só um caractere, como sempre.
" ------------------------------------------------------------
function! s:SmartPairBackspace() abort
    let l:col = col('.')
    let l:line = getline('.')
    let l:before = l:col > 1 ? l:line[l:col - 2] : ''
    let l:after = l:line[l:col - 1]
    let l:pairs = {'(': ')', '[': ']', '{': '}', '"': '"', "'": "'", '`': '`'}
    if has_key(l:pairs, l:before) && l:after ==# l:pairs[l:before]
        return "\<BS>\<Del>"
    endif
    return "\<BS>"
endfunction
inoremap <silent><expr> <S-BS> <SID>SmartPairBackspace()
inoremap <silent><expr> <C-BS> <SID>SmartPairBackspace()

" Busca
nnoremap <leader>h :nohlsearch<CR>

" Tab navigation
nnoremap <Tab> :bnext<CR>
nnoremap <S-Tab> :bprevious<CR>
