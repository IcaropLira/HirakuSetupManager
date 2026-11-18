if exists('g:loaded_my_java')
    finish
endif
let g:loaded_my_java = 1

" ============================================================
" Java
" ============================================================
"
" Regra de template:
"   NomeQualquer.java      -> template "IDE" (public class com o nome do arquivo)
"   NomeQualquer_cp.java   -> template de programação competitiva (classe Main, IO rápida)
"
" O sufixo pode ser trocado abaixo.
let g:java_cp_suffix = '_cp'

function! s:IsCpFile(basename) abort
    let l:suffix = g:java_cp_suffix
    let l:len = len(l:suffix)
    return a:basename[-l:len :] ==? l:suffix
endfunction

function! InsertJavaTemplate() abort
    if s:IsCpFile(expand('%:t:r'))
        call IcaroInsertTemplate('java_cp', '~/.vim/templates/java_cp.java')
    else
        call IcaroInsertTemplate('java_main', '~/.vim/templates/java_main.java')
    endif
endfunction

augroup java_template
    autocmd!
    autocmd BufNewFile *.java call InsertJavaTemplate()
augroup END

" Nome da classe "pública" a executar
function! s:JavaMainClass() abort
    let l:basename = expand('%:t:r')
    if s:IsCpFile(l:basename)
        return 'Main'
    endif
    return l:basename
endfunction

function! CompileJava() abort
    write
    let l:file = expand('%:p')
    let l:dir = expand('%:p:h')
    let l:cmd = 'javac -d ' . shellescape(l:dir) . ' ' . shellescape(l:file)
    call IcaroRunInTerminal(l:cmd)
endfunction

function! RunJava() abort
    write
    let l:dir = expand('%:p:h')
    let l:class = s:JavaMainClass()
    let l:cmd = 'java -cp ' . shellescape(l:dir) . ' ' . l:class
    call IcaroRunInTerminal(l:cmd)
endfunction

function! BuildRunJava() abort
    write
    let l:file = expand('%:p')
    let l:dir = expand('%:p:h')
    let l:class = s:JavaMainClass()
    let l:cmd = 'javac -d ' . shellescape(l:dir) . ' ' . shellescape(l:file) .
                \ ' && java -cp ' . shellescape(l:dir) . ' ' . l:class
    call IcaroRunInTerminal(l:cmd)
endfunction

function! TestJava() abort
    write

    if !filereadable('input.txt')
        echoerr 'input.txt não encontrado'
        return
    endif

    let l:file = expand('%:p')
    let l:dir = expand('%:p:h')
    let l:class = s:JavaMainClass()
    let l:cmd = 'javac -d ' . shellescape(l:dir) . ' ' . shellescape(l:file) .
                \ ' && java -cp ' . shellescape(l:dir) . ' ' . l:class . ' < input.txt'
    call IcaroRunInTerminal(l:cmd)
endfunction
" As teclas (F5-F8 por padrão) são globais e configuráveis no app:
" veja IcaroAction() em config/keymaps.vim.

