" ============================================================
" Extras da aba "Descobrir" do Hiraku Setup Manager (Vim → Descobrir).
" Todos começam desligados. Ao ligar no app, o Vim aberto baixa o plugin
" (git clone em segundo plano) e já o carrega; desligar vale ao reabrir.
" Ficam em ~/.vim/pack/hiraku-extras/opt/<nome>.
" ============================================================

" chave do app -> lista de [repositório, pasta]
let s:extras = {
      \ 'surround':       [['https://github.com/tpope/vim-surround', 'vim-surround'],
      \                    ['https://github.com/tpope/vim-repeat', 'vim-repeat']],
      \ 'commentary':     [['https://github.com/tpope/vim-commentary', 'vim-commentary']],
      \ 'fzf':            [['https://github.com/junegunn/fzf', 'fzf'],
      \                    ['https://github.com/junegunn/fzf.vim', 'fzf.vim']],
      \ 'gitgutter':      [['https://github.com/airblade/vim-gitgutter', 'vim-gitgutter']],
      \ 'tmux_navigator': [['https://github.com/christoomey/vim-tmux-navigator', 'vim-tmux-navigator']],
      \ 'startify':       [['https://github.com/mhinz/vim-startify', 'vim-startify']],
      \ 'goyo':           [['https://github.com/junegunn/goyo.vim', 'goyo.vim']],
      \ 'indentline':     [['https://github.com/Yggdroot/indentLine', 'indentLine']],
      \ }

let s:dir = expand('~/.vim/pack/hiraku-extras/opt')
let s:loaded = {}
let s:jobs = {}

function! s:Enabled(key) abort
    return HirakuGet('plugin_' . a:key, v:false) ? 1 : 0
endfunction

function! s:Load(key) abort
    if has_key(s:loaded, a:key)
        return
    endif
    for [l:url, l:name] in s:extras[a:key]
        execute 'silent! packadd ' . l:name
    endfor
    let s:loaded[a:key] = 1
    call s:Setup(a:key)
endfunction

" Atalhos e ajustes de cada extra (só depois de carregado).
function! s:Setup(key) abort
    if a:key ==# 'fzf'
        nnoremap <silent> <leader>ff :Files<CR>
        nnoremap <silent> <leader>fg :Rg<CR>
        nnoremap <silent> <leader>fb :Buffers<CR>
    elseif a:key ==# 'goyo'
        nnoremap <silent> <leader>z :Goyo<CR>
    elseif a:key ==# 'gitgutter'
        set updatetime=300
        silent! GitGutterEnable
    elseif a:key ==# 'indentline'
        let g:indentLine_char = '│'
        silent! IndentLinesReset
    endif
endfunction

function! s:Cloned(key, name, job, status) abort
    if !has_key(s:jobs, a:key)
        return
    endif
    if a:status != 0
        call remove(s:jobs, a:key)
        call delete(s:dir . '/' . a:name, 'rf')
        echohl WarningMsg | echom 'Hiraku: não consegui baixar ' . a:name . ' (git/internet?)' | echohl None
        return
    endif
    let s:jobs[a:key] -= 1
    if s:jobs[a:key] == 0
        call remove(s:jobs, a:key)
        call s:Load(a:key)
        echom 'Hiraku: ' . a:key . ' instalado e carregado'
    endif
endfunction

" Carrega o que está ligado e já foi baixado; baixa o que falta.
function! HirakuApplyDiscover() abort
    if HirakuLevel() < 4
        return
    endif
    for l:key in keys(s:extras)
        if !s:Enabled(l:key)
            continue
        endif
        let l:missing = filter(copy(s:extras[l:key]), {_, v -> !isdirectory(s:dir . '/' . v[1])})
        if empty(l:missing)
            call s:Load(l:key)
        elseif !has_key(s:jobs, l:key) && executable('git') && exists('*job_start')
            call mkdir(s:dir, 'p')
            let s:jobs[l:key] = len(l:missing)
            for [l:url, l:name] in l:missing
                call job_start(['git', 'clone', '--depth', '1', '--quiet', l:url, s:dir . '/' . l:name],
                      \ {'exit_cb': function('s:Cloned', [l:key, l:name])})
            endfor
        endif
    endfor
endfunction
