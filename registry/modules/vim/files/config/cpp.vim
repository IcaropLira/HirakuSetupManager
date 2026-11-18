if exists('g:loaded_my_cpp')
    finish
endif
let g:loaded_my_cpp = 1

" ============================================================
" C++
" ============================================================

augroup cpp_template
    autocmd!
    autocmd BufNewFile *.cpp call IcaroInsertTemplate('cpp', '~/.vim/templates/cpp.cpp')
    autocmd BufNewFile *.c call IcaroInsertTemplate('c', '')
    autocmd BufNewFile *.py call IcaroInsertTemplate('python', '')
augroup END

function! CompileCpp() abort
    write
    let l:file = expand('%:p')
    let l:output = expand('%:p:r')
    let l:cmd = 'g++ -std=c++17 -O2 -Wall -Wextra ' .
                \ shellescape(l:file) . ' -o ' . shellescape(l:output)
    call IcaroRunInTerminal(l:cmd)
endfunction

function! RunCpp() abort
    write
    let l:output = expand('%:p:r')

    if !filereadable(l:output)
        echoerr 'Executável não encontrado. Use F5 ou F7 primeiro.'
        return
    endif

    let l:cmd = shellescape(l:output)
    call IcaroRunInTerminal(l:cmd)
endfunction

function! BuildRunCpp() abort
    write
    let l:file = expand('%:p')
    let l:output = expand('%:p:r')
    let l:cmd = 'g++ -std=c++17 -O2 -Wall -Wextra ' .
                \ shellescape(l:file) . ' -o ' . shellescape(l:output) .
                \ ' && ' . shellescape(l:output)
    call IcaroRunInTerminal(l:cmd)
endfunction

function! TestCpp() abort
    write

    if !filereadable('input.txt')
        echoerr 'input.txt não encontrado'
        return
    endif

    let l:file = expand('%:p')
    let l:output = expand('%:p:r')
    let l:cmd = 'g++ -std=c++17 -O2 -Wall -Wextra ' .
                \ shellescape(l:file) . ' -o ' . shellescape(l:output) .
                \ ' && ' . shellescape(l:output) . ' < input.txt'
    call IcaroRunInTerminal(l:cmd)
endfunction
" As teclas (F5-F8 por padrão) são globais e configuráveis no app:
" veja IcaroAction() em config/keymaps.vim.

