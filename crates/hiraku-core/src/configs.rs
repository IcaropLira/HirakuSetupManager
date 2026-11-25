//! Motor de configs: aplica os `[[config]]` de um módulo com merge de 3 vias.
//!
//! Para cada arquivo existem três versões:
//! - *base*:  a última versão que o app aplicou (guardada em `blobs/`);
//! - *ours*:  o que está no disco agora, com as edições do usuário;
//! - *theirs*: a versão nova do manifest, já renderizada com as opções.
//!
//! Conflitos NUNCA viram marcadores dentro do arquivo: o arquivo do usuário fica
//! intacto, a versão nova é guardada como pendente e `resolve_conflict` decide.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::configstate::{read_blob, store_blob, ConfigState, FileState};
use crate::env::Environment;
use crate::error::{io_err, Error, Result};
use crate::event::{Event, EventSink};
use crate::lockfile::now_secs;
use crate::manifest::{is_safe_relative, BlockSpec, ConfigEntry, ConfigMode, Manifest, OptionKind};
use crate::paths::expand_path;
use crate::release::sha256_hex_of;

/// O que fazer quando o arquivo de destino já existe mas o app nunca o aplicou
/// (não há base para o merge).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FirstApply {
    /// Como o `install.sh` de hoje: backup e sobrescreve.
    #[default]
    BackupOverwrite,
    /// Mantém o arquivo do usuário e deixa a versão nova pendente.
    KeepPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Created,
    Unchanged,
    Updated,
    Merged,
    AdoptedWithBackup,
    Linked,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReport {
    pub dest: PathBuf,
    pub outcome: Outcome,
    /// Veio de um `[[config]]` com `triggers_hooks = false`: mudar este arquivo não
    /// dispara os hooks `on = "change"` do módulo.
    pub quiet: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Mantém o arquivo atual; a versão nova é descartada (e marcada como vista).
    KeepMine,
    /// Substitui pelo conteúdo novo (com backup do atual).
    UseNew,
}

/// Valores finais das opções: padrão do manifest sobrescrito pelas escolhas.
pub fn resolve_options(
    manifest: &Manifest,
    choices: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let bad = |reason: String| Error::Invalid {
        id: manifest.id.clone(),
        reason,
    };

    for key in choices.keys() {
        if !manifest.options.contains_key(key) {
            return Err(bad(format!("opção desconhecida '{key}'")));
        }
    }

    let mut out = BTreeMap::new();
    for (key, def) in &manifest.options {
        let chosen = choices.get(key);
        let value = match &def.kind {
            OptionKind::Choice { values, default, .. } => {
                let v = chosen.cloned().unwrap_or_else(|| default.clone());
                if !values.contains(&v) {
                    return Err(bad(format!(
                        "opção '{key}': '{v}' inválido (use: {})",
                        values.join(", ")
                    )));
                }
                v
            }
            OptionKind::Bool { default } => match chosen.map(String::as_str) {
                None => default.to_string(),
                Some("true") => "true".to_string(),
                Some("false") => "false".to_string(),
                Some(other) => {
                    return Err(bad(format!(
                        "opção '{key}': '{other}' inválido (use true ou false)"
                    )))
                }
            },
            OptionKind::Number {
                min,
                max,
                default,
                divisor,
            } => {
                let n = match chosen {
                    None => *default,
                    Some(text) => text.trim().parse::<i64>().map_err(|_| {
                        bad(format!("opção '{key}': '{text}' não é um número inteiro"))
                    })?,
                };
                if n < *min || n > *max {
                    return Err(bad(format!(
                        "opção '{key}': {n} fora do intervalo [{min}, {max}]"
                    )));
                }
                format_scaled(n, *divisor)
            }
            OptionKind::Text { default } | OptionKind::Lines { default } => {
                chosen.cloned().unwrap_or_else(|| default.clone())
            }
            OptionKind::Color { default } => {
                let v = chosen.cloned().unwrap_or_else(|| default.clone());
                if !is_hex_color(&v) {
                    return Err(bad(format!("opção '{key}': '{v}' não é uma cor #rrggbb")));
                }
                v
            }
            OptionKind::Keymaps { default } => {
                let v = chosen.cloned().unwrap_or_else(|| default.clone());
                check_keymaps(&v).map_err(|e| bad(format!("opção '{key}': {e}")))?;
                v
            }
        };
        out.insert(key.clone(), value);
    }
    Ok(out)
}

/// `#rrggbb`.
pub fn is_hex_color(v: &str) -> bool {
    v.len() == 7 && v.starts_with('#') && v[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Modos válidos de um atalho (letras do vim: normal, inserção, visual, seleção
/// visual, operador pendente, linha de comando, terminal).
const KEYMAP_MODES: &str = "nivxoct";

/// Confere a lista de atalhos (JSON). Cada item: `mode` (uma ou mais letras de
/// "nivxoct"), `lhs` e `rhs` não vazios; `desc` e `remap` opcionais.
pub fn check_keymaps(json: &str) -> std::result::Result<(), String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("lista de atalhos não é JSON válido: {e}"))?;
    let items = value.as_array().ok_or("a lista de atalhos deve ser um array")?;
    for (i, item) in items.iter().enumerate() {
        let n = i + 1;
        let obj = item.as_object().ok_or(format!("atalho {n}: deve ser um objeto"))?;
        let text = |k: &str| obj.get(k).and_then(|v| v.as_str()).unwrap_or("");
        let mode = text("mode");
        if mode.is_empty() || !mode.chars().all(|c| KEYMAP_MODES.contains(c)) {
            return Err(format!("atalho {n}: modo '{mode}' inválido (use letras de {KEYMAP_MODES})"));
        }
        if text("lhs").trim().is_empty() {
            return Err(format!("atalho {n}: falta a tecla"));
        }
        if text("rhs").is_empty() {
            return Err(format!("atalho {n} ({}): falta a ação (use <Nop> para desativar)", text("lhs")));
        }
        for k in ["mode", "lhs", "rhs", "desc"] {
            if obj.get(k).is_some_and(|v| !v.is_string()) {
                return Err(format!("atalho {n}: '{k}' deve ser texto"));
            }
        }
        if obj.get("remap").is_some_and(|v| !v.is_boolean()) {
            return Err(format!("atalho {n}: 'remap' deve ser true/false"));
        }
    }
    Ok(())
}

/// `85` com divisor 100 vira `0.85`; com divisor 1 continua `85`.
fn format_scaled(n: i64, divisor: i64) -> String {
    if divisor <= 1 {
        n.to_string()
    } else {
        format!("{:.2}", n as f64 / divisor as f64)
    }
}

/// Substitui `{{ nome }}` pelo valor da opção. Variável desconhecida é erro
/// (pega typos em vez de gravar a chave literal no arquivo do usuário).
pub fn render_template(src: &str, vars: &BTreeMap<String, String>) -> Result<String> {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find("}}").ok_or_else(|| {
            Error::Unsupported("template: '{{' sem '}}' correspondente".to_string())
        })?;
        // `{{ nome }}` insere o valor como está; `{{ nome | json }}` insere como string
        // JSON (aspas e escapes); `{{ nome | onoff }}` troca true/false por on/off.
        let expr = after[..end].trim();
        let (name, filter) = match expr.split_once('|') {
            Some((n, f)) => (n.trim(), Some(f.trim())),
            None => (expr, None),
        };
        let value = vars.get(name).ok_or_else(|| {
            Error::Unsupported(format!("template: variável desconhecida '{name}'"))
        })?;
        match filter {
            None => out.push_str(value),
            Some("json") => out.push_str(&serde_json::Value::String(value.clone()).to_string()),
            // Booleans no formato do tmux.
            Some("onoff") => out.push_str(if value == "true" { "on" } else { "off" }),
            Some(other) => {
                return Err(Error::Unsupported(format!("template: filtro desconhecido '{other}'")))
            }
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

struct Ctx<'a> {
    module: &'a str,
    state_dir: &'a Path,
    first_apply: FirstApply,
    backup_id: u64,
    /// Reparo: grava a versão esperada por cima de qualquer diferença (com backup).
    force: bool,
}

fn walk(base: &Path, rel: &Path, out: &mut Vec<(PathBuf, PathBuf)>) -> Result<()> {
    let dir = base.join(rel);
    for entry in fs::read_dir(&dir).map_err(io_err(&dir))? {
        let entry = entry.map_err(io_err(&dir))?;
        let name = entry.file_name();
        if matches!(name.to_str(), Some(".git" | ".gitkeep")) {
            continue;
        }
        let rel_child = rel.join(&name);
        let path = entry.path();
        let file_type = entry.file_type().map_err(io_err(&path))?;
        if file_type.is_dir() {
            walk(base, &rel_child, out)?;
        } else if file_type.is_file() {
            out.push((rel_child, path));
        }
    }
    Ok(())
}

/// (caminho relativo, caminho absoluto) de todos os arquivos sob `root`, ordenados.
fn collect_files(root: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut out = Vec::new();
    walk(root, Path::new(""), &mut out)?;
    out.sort();
    Ok(out)
}

fn read_if_exists(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Grava via arquivo temporário + rename. Se `dest` for um link simbólico, escreve
/// no alvo (não quebra quem gerencia dotfiles com stow). Preserva permissões do
/// destino existente ou, se for novo, copia as da origem.
fn write_atomic(dest: &Path, bytes: &[u8], perms_from: &Path) -> Result<()> {
    let real = fs::canonicalize(dest).unwrap_or_else(|_| dest.to_path_buf());
    if let Some(parent) = real.parent() {
        fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    let mut tmp_name = real.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".hiraku-tmp");
    let tmp = real.with_file_name(tmp_name);

    fs::write(&tmp, bytes).map_err(io_err(&tmp))?;
    let perms_src = if real.exists() { real.as_path() } else { perms_from };
    if let Ok(meta) = fs::metadata(perms_src) {
        let _ = fs::set_permissions(&tmp, meta.permissions());
    }
    fs::rename(&tmp, &real).map_err(io_err(&real))?;
    Ok(())
}

fn backup_name(dest: &Path) -> String {
    let name: String = dest
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let name = name.trim_start_matches('_');
    // `name` é só ASCII aqui, então fatiar por byte é seguro.
    let start = name.len().saturating_sub(200);
    name[start..].to_string()
}

fn backup_dir(ctx: &Ctx) -> PathBuf {
    ctx.state_dir
        .join("backups")
        .join(ctx.backup_id.to_string())
        .join(ctx.module)
}

fn backup_file(ctx: &Ctx, dest: &Path) -> Result<PathBuf> {
    let dir = backup_dir(ctx);
    fs::create_dir_all(&dir).map_err(io_err(&dir))?;
    let target = dir.join(backup_name(dest));
    fs::copy(dest, &target).map_err(io_err(&target))?;
    Ok(target)
}

fn sync_file(
    ctx: &Ctx,
    entry: &ConfigEntry,
    dest: &Path,
    src_file: &Path,
    theirs: Vec<u8>,
    state: &mut ConfigState,
) -> Result<Outcome> {
    let key = dest.display().to_string();
    let theirs_sha = store_blob(ctx.state_dir, &theirs)?;
    let ours = read_if_exists(dest)?;
    let base = match state.files.get(&key) {
        Some(file) => read_blob(ctx.state_dir, &file.base_sha256)?,
        None => None,
    };

    let record = |state: &mut ConfigState, base_sha: String, pending: Option<String>| {
        state.files.insert(
            key.clone(),
            FileState {
                module: ctx.module.to_string(),
                src: entry.src.clone(),
                base_sha256: base_sha,
                pending_sha256: pending,
                applied_at: now_secs(),
            },
        );
    };

    // `seed`: cria uma vez e nunca mais mexe (estado do usuário).
    if entry.mode == ConfigMode::Seed {
        if ours.is_some() {
            return Ok(Outcome::Unchanged);
        }
        write_atomic(dest, &theirs, src_file)?;
        record(state, theirs_sha, None);
        return Ok(Outcome::Created);
    }

    // Arquivo ausente: cria.
    let Some(ours) = ours else {
        write_atomic(dest, &theirs, src_file)?;
        record(state, theirs_sha, None);
        return Ok(Outcome::Created);
    };

    // Já está igual à versão nova.
    if ours == theirs {
        record(state, theirs_sha, None);
        return Ok(Outcome::Unchanged);
    }

    // Reparo: a versão esperada vale, sem merge (o arquivo atual vai para o backup).
    if ctx.force {
        backup_file(ctx, dest)?;
        write_atomic(dest, &theirs, src_file)?;
        record(state, theirs_sha, None);
        return Ok(Outcome::Updated);
    }

    // Modo `copy`: sobrescreve sempre (com backup).
    if entry.mode == ConfigMode::Copy {
        if entry.backup {
            backup_file(ctx, dest)?;
        }
        write_atomic(dest, &theirs, src_file)?;
        record(state, theirs_sha, None);
        return Ok(Outcome::Updated);
    }

    // Modo `merge`. Sem base conhecida não há como fazer 3 vias.
    let Some(base) = base else {
        return match ctx.first_apply {
            FirstApply::BackupOverwrite => {
                if entry.backup {
                    backup_file(ctx, dest)?;
                }
                write_atomic(dest, &theirs, src_file)?;
                record(state, theirs_sha, None);
                Ok(Outcome::AdoptedWithBackup)
            }
            FirstApply::KeepPending => {
                let ours_sha = store_blob(ctx.state_dir, &ours)?;
                record(state, ours_sha, Some(theirs_sha));
                Ok(Outcome::Conflict)
            }
        };
    };
    let base_sha = sha256_hex_of(&base);

    // Nada novo no upstream: mantém as edições do usuário.
    if theirs == base {
        record(state, base_sha, None);
        return Ok(Outcome::Unchanged);
    }

    // Usuário não mexeu: atualiza direto.
    if ours == base {
        if entry.backup {
            backup_file(ctx, dest)?;
        }
        write_atomic(dest, &theirs, src_file)?;
        record(state, theirs_sha, None);
        return Ok(Outcome::Updated);
    }

    // Os dois mudaram: merge de 3 vias (só para texto UTF-8).
    let merged = match (
        std::str::from_utf8(&base),
        std::str::from_utf8(&ours),
        std::str::from_utf8(&theirs),
    ) {
        (Ok(b), Ok(o), Ok(t)) => diffy::merge(b, o, t).ok(),
        _ => None,
    };
    match merged {
        Some(text) => {
            if entry.backup {
                backup_file(ctx, dest)?;
            }
            write_atomic(dest, text.as_bytes(), src_file)?;
            record(state, theirs_sha, None);
            Ok(Outcome::Merged)
        }
        None => {
            record(state, base_sha, Some(theirs_sha));
            Ok(Outcome::Conflict)
        }
    }
}

fn link_entry(
    ctx: &Ctx,
    src_root: &Path,
    dest_root: &Path,
) -> Result<Outcome> {
    let target = fs::canonicalize(src_root).map_err(io_err(src_root))?;
    match fs::symlink_metadata(dest_root) {
        Ok(meta) if meta.file_type().is_symlink() => {
            if fs::read_link(dest_root).map_err(io_err(dest_root))? == target {
                return Ok(Outcome::Unchanged);
            }
            fs::remove_file(dest_root).map_err(io_err(dest_root))?;
        }
        Ok(_) => {
            let dir = backup_dir(ctx);
            fs::create_dir_all(&dir).map_err(io_err(&dir))?;
            let moved = dir.join(backup_name(dest_root));
            fs::rename(dest_root, &moved).map_err(io_err(dest_root))?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                path: dest_root.to_path_buf(),
                source,
            })
        }
    }
    if let Some(parent) = dest_root.parent() {
        fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    crate::release::make_link(&target, dest_root).map_err(io_err(dest_root))?;
    Ok(Outcome::Linked)
}

/// Remove todos os blocos `begin`..`end` e coloca o novo no lugar do primeiro (ou no
/// fim do arquivo, se não havia). O resto do arquivo não é tocado.
pub(crate) fn upsert_block(original: &str, begin: &str, end: &str, content: &str) -> Result<String> {
    let body = content.trim_end_matches(|c| c == '\n' || c == '\r');
    let block_lines: Vec<&str> = std::iter::once(begin)
        .chain(body.lines())
        .chain(std::iter::once(end))
        .collect();

    let mut out: Vec<&str> = Vec::new();
    let mut inserted = false;
    let mut in_block = false;
    for line in original.lines() {
        let trimmed = line.trim_end();
        if in_block {
            if trimmed == end.trim_end() {
                in_block = false;
            }
            continue;
        }
        if trimmed == begin.trim_end() {
            in_block = true;
            if !inserted {
                out.extend(block_lines.iter().copied());
                inserted = true;
            }
            continue;
        }
        out.push(line);
    }
    if in_block {
        return Err(Error::Unsupported(format!(
            "o bloco '{begin}' foi aberto e nunca fechado; corrija o arquivo manualmente"
        )));
    }
    if !inserted {
        if out.last().map_or(false, |l| !l.is_empty()) {
            out.push("");
        }
        out.extend(block_lines.iter().copied());
    }
    let mut text = out.join("\n");
    text.push('\n');
    Ok(text)
}

/// `Some(true/false)` = o validador rodou; `None` = o programa não existe (não dá para validar).
fn run_check(check: &[String], path: &Path) -> Option<bool> {
    let (program, args) = check.split_first()?;
    let path_text = path.display().to_string();
    let args: Vec<String> = args.iter().map(|a| a.replace("{path}", &path_text)).collect();
    std::process::Command::new(program)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()
        .map(|status| status.success())
}

/// Valida o texto novo ANTES de gravar. Se o arquivo original já falhava na mesma
/// validação, o problema não é do bloco e a gravação segue.
fn validate_block_result(check: &[String], dest: &Path, new_text: &str, original: &str) -> Result<()> {
    if check.is_empty() {
        return Ok(());
    }
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".hiraku-check");
    let tmp = dest.with_file_name(name);
    if let Some(parent) = tmp.parent() {
        fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    fs::write(&tmp, new_text).map_err(io_err(&tmp))?;
    let result = match run_check(check, &tmp) {
        None | Some(true) => Ok(()),
        Some(false) => {
            fs::write(&tmp, original).map_err(io_err(&tmp))?;
            if run_check(check, &tmp) == Some(false) {
                Ok(())
            } else {
                Err(Error::Unsupported(format!(
                    "a validação ({}) rejeitou o resultado; {} não foi alterado",
                    check.join(" "),
                    dest.display()
                )))
            }
        }
    };
    let _ = fs::remove_file(&tmp);
    result
}

fn apply_block(
    ctx: &Ctx,
    entry: &ConfigEntry,
    spec: &BlockSpec,
    dest: &Path,
    content: &str,
) -> Result<Outcome> {
    let existing = read_if_exists(dest)?;
    if existing.is_none() && spec.only_if_exists {
        return Ok(Outcome::Unchanged);
    }
    let original = match &existing {
        Some(bytes) => std::str::from_utf8(bytes)
            .map_err(|_| Error::Unsupported(format!("{} não é UTF-8", dest.display())))?
            .to_string(),
        None => String::new(),
    };
    let updated = upsert_block(&original, &spec.begin, &spec.end, content)?;
    if existing.is_some() && updated == original {
        return Ok(Outcome::Unchanged);
    }
    validate_block_result(&spec.check, dest, &updated, &original)?;
    if existing.is_some() && entry.backup {
        backup_file(ctx, dest)?;
    }
    write_atomic(dest, updated.as_bytes(), dest)?;
    Ok(if existing.is_none() {
        Outcome::Created
    } else {
        Outcome::Updated
    })
}

fn push_report(
    reports: &mut Vec<FileReport>,
    module: &str,
    dest: PathBuf,
    outcome: Outcome,
    quiet: bool,
    sink: &mut dyn EventSink,
) {
    match outcome {
        Outcome::Unchanged => {}
        Outcome::Conflict => sink.emit(Event::Conflict {
            module: module.to_string(),
            path: dest.display().to_string(),
        }),
        other => sink.emit(Event::Log {
            module: module.to_string(),
            line: format!("{other:?}: {}", dest.display()),
        }),
    }
    reports.push(FileReport { dest, outcome, quiet });
}

/// Aplica todos os `[[config]]` do módulo. O estado em memória é atualizado arquivo
/// a arquivo; quem chama deve gravá-lo (`ConfigState::save`) mesmo em caso de erro.
#[allow(clippy::too_many_arguments)]
pub fn apply_module_configs(
    manifest: &Manifest,
    module_dir: &Path,
    env: &Environment,
    state_dir: &Path,
    choices: &BTreeMap<String, String>,
    first_apply: FirstApply,
    state: &mut ConfigState,
    sink: &mut dyn EventSink,
) -> Result<Vec<FileReport>> {
    apply_configs_inner(manifest, module_dir, env, state_dir, choices, first_apply, state, sink, false)
}

/// Reparo ("Verificar integridade"): deixa todos os arquivos do módulo exatamente
/// como o hiraku geraria agora. Edições e conflitos pendentes vão para o backup
/// (`<state>/backups/<id>/<módulo>/`). Arquivos `seed` (estado do usuário) não mudam.
#[allow(clippy::too_many_arguments)]
pub fn repair_module_configs(
    manifest: &Manifest,
    module_dir: &Path,
    env: &Environment,
    state_dir: &Path,
    choices: &BTreeMap<String, String>,
    state: &mut ConfigState,
    sink: &mut dyn EventSink,
) -> Result<Vec<FileReport>> {
    apply_configs_inner(manifest, module_dir, env, state_dir, choices, FirstApply::BackupOverwrite, state, sink, true)
}

#[allow(clippy::too_many_arguments)]
fn apply_configs_inner(
    manifest: &Manifest,
    module_dir: &Path,
    env: &Environment,
    state_dir: &Path,
    choices: &BTreeMap<String, String>,
    first_apply: FirstApply,
    state: &mut ConfigState,
    sink: &mut dyn EventSink,
    force: bool,
) -> Result<Vec<FileReport>> {
    let vars = resolve_options(manifest, choices)?;
    let ctx = Ctx {
        module: &manifest.id,
        state_dir,
        first_apply,
        backup_id: now_secs(),
        force,
    };
    let mut reports = Vec::new();

    for entry in &manifest.config {
        let Some(dest_template) = entry.dest.resolve(env.os) else {
            sink.emit(Event::Log {
                module: manifest.id.clone(),
                line: format!("pulando '{}': sem destino para {}", entry.src, env.os.as_str()),
            });
            continue;
        };
        let dest_root = expand_path(dest_template, env.home.as_deref())?;
        // `src` pode ter template (ex.: files/palettes/{{ palette }}.conf).
        let src_rel = if entry.src.contains("{{") {
            render_template(&entry.src, &vars)?
        } else {
            entry.src.clone()
        };
        if !is_safe_relative(&src_rel) {
            return Err(Error::Unsupported(format!(
                "origem de config insegura depois do template: '{src_rel}'"
            )));
        }
        let src_root = module_dir.join(&src_rel);
        let meta = fs::metadata(&src_root).map_err(|_| {
            Error::Unsupported(format!("origem de config não existe: {}", src_root.display()))
        })?;

        if entry.mode == ConfigMode::Block {
            if meta.is_dir() {
                return Err(Error::Unsupported(format!(
                    "mode = \"block\" exige um arquivo, mas '{src_rel}' é uma pasta"
                )));
            }
            let spec = entry.block.as_ref().ok_or_else(|| {
                Error::Unsupported("mode = \"block\" sem [config.block]".to_string())
            })?;
            let bytes = fs::read(&src_root).map_err(io_err(&src_root))?;
            let mut text = String::from_utf8(bytes).map_err(|_| {
                Error::Unsupported(format!("o bloco precisa ser UTF-8: {}", src_root.display()))
            })?;
            if entry.template {
                text = render_template(&text, &vars)?;
            }
            let outcome = apply_block(&ctx, entry, spec, &dest_root, &text)?;
            push_report(&mut reports, &manifest.id, dest_root, outcome, !entry.triggers_hooks, sink);
            continue;
        }

        if entry.mode == ConfigMode::Symlink {
            let outcome = link_entry(&ctx, &src_root, &dest_root)?;
            push_report(&mut reports, &manifest.id, dest_root, outcome, !entry.triggers_hooks, sink);
            continue;
        }

        let files: Vec<(PathBuf, PathBuf)> = if meta.is_dir() {
            collect_files(&src_root)?
                .into_iter()
                .map(|(rel, abs)| (dest_root.join(rel), abs))
                .collect()
        } else {
            vec![(dest_root.clone(), src_root.clone())]
        };

        for (dest, src_file) in files {
            let mut theirs = fs::read(&src_file).map_err(io_err(&src_file))?;
            if entry.template {
                let text = String::from_utf8(theirs).map_err(|_| {
                    Error::Unsupported(format!(
                        "template precisa ser UTF-8: {}",
                        src_file.display()
                    ))
                })?;
                theirs = render_template(&text, &vars)?.into_bytes();
            }
            let outcome = sync_file(&ctx, entry, &dest, &src_file, theirs, state)?;
            push_report(&mut reports, &manifest.id, dest, outcome, !entry.triggers_hooks, sink);
        }
    }
    Ok(reports)
}

/// Como um arquivo de configuração está em relação ao que o hiraku geraria agora.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileHealth {
    /// Igual ao esperado.
    Ok,
    /// Não existe no disco.
    Missing,
    /// O hiraku gravou este arquivo, mas ele está desatualizado (opção mudou e o sync
    /// não chegou a aplicar).
    Outdated,
    /// Alguém editou o arquivo depois do último sync.
    EditedOutside,
    /// Existe, mas nunca foi gravado pelo hiraku (ex.: sobra de uma instalação antiga).
    NotManaged,
    /// Há uma versão nova esperando decisão na aba Conflitos.
    Pending,
    /// O bloco do hiraku não está no arquivo (ou está diferente).
    BlockMissing,
    /// Deveria ser um link para a pasta do módulo e não é.
    LinkWrong,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileCheck {
    pub dest: PathBuf,
    pub health: FileHealth,
}

/// Confere, sem gravar nada, cada arquivo que os `[[config]]` do módulo gerariam.
/// Arquivos `seed` só precisam existir.
pub fn check_module_configs(
    manifest: &Manifest,
    module_dir: &Path,
    env: &Environment,
    choices: &BTreeMap<String, String>,
    state_dir: &Path,
    state: &ConfigState,
) -> Result<Vec<FileCheck>> {
    let vars = resolve_options(manifest, choices)?;
    let mut out = Vec::new();
    for entry in &manifest.config {
        let Some(dest_template) = entry.dest.resolve(env.os) else { continue };
        let dest_root = expand_path(dest_template, env.home.as_deref())?;
        let src_rel = if entry.src.contains("{{") { render_template(&entry.src, &vars)? } else { entry.src.clone() };
        if !is_safe_relative(&src_rel) {
            continue;
        }
        let src_root = module_dir.join(&src_rel);
        let Ok(meta) = fs::metadata(&src_root) else { continue };

        if entry.mode == ConfigMode::Symlink {
            let ok = fs::read_link(&dest_root).ok().zip(fs::canonicalize(&src_root).ok()).map_or(false, |(a, b)| a == b);
            out.push(FileCheck { dest: dest_root, health: if ok { FileHealth::Ok } else { FileHealth::LinkWrong } });
            continue;
        }
        if entry.mode == ConfigMode::Block {
            let (Some(spec), false) = (entry.block.as_ref(), meta.is_dir()) else { continue };
            let mut text = String::from_utf8(fs::read(&src_root).map_err(io_err(&src_root))?).unwrap_or_default();
            if entry.template {
                text = render_template(&text, &vars)?;
            }
            let health = match read_if_exists(&dest_root)? {
                None if spec.only_if_exists => FileHealth::Ok,
                None => FileHealth::Missing,
                Some(bytes) => {
                    let original = String::from_utf8_lossy(&bytes).to_string();
                    match upsert_block(&original, &spec.begin, &spec.end, &text) {
                        Ok(updated) if updated == original => FileHealth::Ok,
                        _ => FileHealth::BlockMissing,
                    }
                }
            };
            out.push(FileCheck { dest: dest_root, health });
            continue;
        }

        let files: Vec<(PathBuf, PathBuf)> = if meta.is_dir() {
            collect_files(&src_root)?.into_iter().map(|(rel, abs)| (dest_root.join(rel), abs)).collect()
        } else {
            vec![(dest_root.clone(), src_root.clone())]
        };
        for (dest, src_file) in files {
            let ours = read_if_exists(&dest)?;
            let health = if entry.mode == ConfigMode::Seed {
                if ours.is_some() { FileHealth::Ok } else { FileHealth::Missing }
            } else {
                let mut theirs = fs::read(&src_file).map_err(io_err(&src_file))?;
                if entry.template {
                    theirs = render_template(&String::from_utf8_lossy(&theirs), &vars)?.into_bytes();
                }
                let known = state.files.get(&dest.display().to_string());
                match ours {
                    None => FileHealth::Missing,
                    Some(o) if o == theirs => FileHealth::Ok,
                    Some(o) => match known {
                        None => FileHealth::NotManaged,
                        Some(f) if f.pending_sha256.is_some() => FileHealth::Pending,
                        Some(f) => {
                            let base = read_blob(state_dir, &f.base_sha256)?;
                            if base.as_deref() == Some(o.as_slice()) {
                                FileHealth::Outdated
                            } else {
                                FileHealth::EditedOutside
                            }
                        }
                    },
                }
            };
            out.push(FileCheck { dest, health });
        }
    }
    Ok(out)
}

/// Diff unificado (atual -> versão nova pendente) para mostrar na TUI/GUI.
pub fn conflict_diff(dest: &Path, state_dir: &Path, state: &ConfigState) -> Result<String> {
    let key = dest.display().to_string();
    let pending = pending_sha(state, &key)?;
    let theirs = read_blob(state_dir, &pending)?
        .ok_or_else(|| Error::Unsupported("versão nova não está mais no armazenamento".to_string()))?;
    let ours = fs::read(dest).map_err(io_err(dest))?;
    match (std::str::from_utf8(&ours), std::str::from_utf8(&theirs)) {
        (Ok(o), Ok(t)) => Ok(diffy::create_patch(o, t).to_string()),
        _ => Err(Error::Unsupported("arquivo binário: sem diff de texto".to_string())),
    }
}

fn pending_sha(state: &ConfigState, key: &str) -> Result<String> {
    let file = state
        .files
        .get(key)
        .ok_or_else(|| Error::Unsupported(format!("'{key}' não é gerenciado pelo hiraku")))?;
    file.pending_sha256
        .clone()
        .ok_or_else(|| Error::Unsupported(format!("'{key}' não tem conflito pendente")))
}

pub fn resolve_conflict(
    dest: &Path,
    resolution: Resolution,
    state_dir: &Path,
    state: &mut ConfigState,
) -> Result<()> {
    let key = dest.display().to_string();
    let pending = pending_sha(state, &key)?;
    let module = state
        .files
        .get(&key)
        .map(|f| f.module.clone())
        .unwrap_or_default();

    if resolution == Resolution::UseNew {
        let bytes = read_blob(state_dir, &pending)?.ok_or_else(|| {
            Error::Unsupported("versão nova não está mais no armazenamento".to_string())
        })?;
        let ctx = Ctx {
            module: &module,
            state_dir,
            first_apply: FirstApply::default(),
            backup_id: now_secs(),
            force: false,
        };
        if dest.exists() {
            backup_file(&ctx, dest)?;
        }
        write_atomic(dest, &bytes, dest)?;
    }

    if let Some(file) = state.files.get_mut(&key) {
        file.base_sha256 = pending;
        file.pending_sha256 = None;
        file.applied_at = now_secs();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn template_json_filter_escapes_strings() {
        let mut vars = BTreeMap::new();
        vars.insert("a".to_string(), "linha 1\n\"aspas\" e \\".to_string());
        vars.insert("n".to_string(), "8".to_string());
        let out = render_template("{ \"a\": {{ a | json }}, \"n\": {{n}} }", &vars).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["a"], "linha 1\n\"aspas\" e \\");
        assert_eq!(parsed["n"], 8);
        assert!(render_template("{{ a | upper }}", &vars).is_err());
        vars.insert("b".to_string(), "true".to_string());
        assert_eq!(render_template("set -g mouse {{ b | onoff }}", &vars).unwrap(), "set -g mouse on");
    }

    #[test]
    fn keymaps_and_colors_are_validated() {
        assert!(check_keymaps("[]").is_ok());
        assert!(check_keymaps(r#"[{"mode":"nv","lhs":"dd","rhs":"\"_dd","desc":"x","remap":false}]"#).is_ok());
        assert!(check_keymaps(r#"[{"mode":"q","lhs":"dd","rhs":"x"}]"#).unwrap_err().contains("modo"));
        assert!(check_keymaps(r#"[{"mode":"n","lhs":"","rhs":"x"}]"#).unwrap_err().contains("tecla"));
        assert!(check_keymaps(r#"[{"mode":"n","lhs":"x","rhs":""}]"#).unwrap_err().contains("<Nop>"));
        assert!(check_keymaps(r#"{"mode":"n"}"#).is_err());
        assert!(check_keymaps("não é json").is_err());
        assert!(is_hex_color("#1A2b3c"));
        assert!(!is_hex_color("1a2b3c") && !is_hex_color("#12345g") && !is_hex_color("#123"));
    }

    use super::*;
    use crate::env::{Arch, Os, Privilege};
    use crate::event::VecSink;

    const MANIFEST: &str = r#"
schema = 1
id = "app"
name = "app"

[options.theme]
type = "choice"
values = ["dark", "light"]
default = "dark"

[[config]]
src = "files/app.conf"
dest = { unix = "~/.config/app/app.conf" }
template = true
"#;

    struct Fixture {
        _tmp: tempfile::TempDir,
        module_dir: PathBuf,
        state_dir: PathBuf,
        home: PathBuf,
        env: Environment,
        manifest: Manifest,
        state: ConfigState,
    }

    impl Fixture {
        fn new(manifest_src: &str) -> Fixture {
            let tmp = tempfile::tempdir().unwrap();
            let module_dir = tmp.path().join("module");
            let state_dir = tmp.path().join("state");
            let home = tmp.path().join("home");
            fs::create_dir_all(module_dir.join("files")).unwrap();
            fs::create_dir_all(&home).unwrap();
            let env = Environment {
                os: Os::Linux,
                arch: Some(Arch::X86_64),
                distro: None,
                pkg_manager: None,
                privilege: Privilege::None,
                use_root: false,
                home: Some(home.clone()),
            };
            Fixture {
                _tmp: tmp,
                module_dir,
                state_dir,
                home,
                env,
                manifest: Manifest::from_toml_str(manifest_src, "test").unwrap(),
                state: ConfigState::default(),
            }
        }

        fn upstream(&self, name: &str, content: &str) {
            fs::write(self.module_dir.join("files").join(name), content).unwrap();
        }

        fn dest(&self) -> PathBuf {
            self.home.join(".config/app/app.conf")
        }

        fn run_with(&mut self, first: FirstApply, choices: &[(&str, &str)]) -> Vec<FileReport> {
            let choices: BTreeMap<String, String> = choices
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            apply_module_configs(
                &self.manifest,
                &self.module_dir,
                &self.env,
                &self.state_dir,
                &choices,
                first,
                &mut self.state,
                &mut VecSink::default(),
            )
            .unwrap()
        }

        fn try_run(&mut self) -> Result<Vec<FileReport>> {
            apply_module_configs(
                &self.manifest,
                &self.module_dir,
                &self.env,
                &self.state_dir,
                &BTreeMap::new(),
                FirstApply::default(),
                &mut self.state,
                &mut VecSink::default(),
            )
        }

        fn run(&mut self) -> Outcome {
            self.run_with(FirstApply::default(), &[])[0].outcome
        }

        fn backups(&self) -> usize {
            fn count(dir: &Path) -> usize {
                let Ok(entries) = fs::read_dir(dir) else { return 0 };
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| {
                        let p = e.path();
                        if p.is_dir() { count(&p) } else { 1 }
                    })
                    .sum()
            }
            count(&self.state_dir.join("backups"))
        }
    }

    fn lines(n: usize) -> String {
        (1..=n).map(|i| format!("linha {i}\n")).collect()
    }

    #[test]
    fn template_renders_and_rejects_unknown_variables() {
        let mut vars = BTreeMap::new();
        vars.insert("theme".to_string(), "dark".to_string());
        assert_eq!(render_template("a {{theme}} b {{ theme }}", &vars).unwrap(), "a dark b dark");
        assert_eq!(render_template("sem marcadores", &vars).unwrap(), "sem marcadores");
        assert!(render_template("{{ nope }}", &vars).is_err());
        assert!(render_template("aberto {{ theme", &vars).is_err());
    }

    #[test]
    fn options_use_defaults_and_validate_choices() {
        let f = Fixture::new(MANIFEST);
        let mut choices = BTreeMap::new();
        assert_eq!(resolve_options(&f.manifest, &choices).unwrap()["theme"], "dark");
        choices.insert("theme".to_string(), "light".to_string());
        assert_eq!(resolve_options(&f.manifest, &choices).unwrap()["theme"], "light");
        choices.insert("theme".to_string(), "neon".to_string());
        assert!(resolve_options(&f.manifest, &choices).is_err());
        let mut unknown = BTreeMap::new();
        unknown.insert("cor".to_string(), "x".to_string());
        assert!(resolve_options(&f.manifest, &unknown).is_err());
    }

    #[test]
    fn creates_then_is_idempotent_and_applies_template() {
        let mut f = Fixture::new(MANIFEST);
        f.upstream("app.conf", "tema = {{ theme }}\n");
        assert_eq!(f.run(), Outcome::Created);
        assert_eq!(fs::read_to_string(f.dest()).unwrap(), "tema = dark\n");
        assert_eq!(f.run(), Outcome::Unchanged);
        assert_eq!(f.state.files.len(), 1);
    }

    #[test]
    fn changing_an_option_updates_an_untouched_file() {
        let mut f = Fixture::new(MANIFEST);
        f.upstream("app.conf", "tema = {{ theme }}\n");
        f.run();
        let reports = f.run_with(FirstApply::default(), &[("theme", "light")]);
        assert_eq!(reports[0].outcome, Outcome::Updated);
        assert_eq!(fs::read_to_string(f.dest()).unwrap(), "tema = light\n");
        assert_eq!(f.backups(), 1);
    }

    #[test]
    fn upstream_change_without_user_edits_updates() {
        let mut f = Fixture::new(MANIFEST);
        f.upstream("app.conf", &lines(10));
        f.run();
        f.upstream("app.conf", &lines(11));
        assert_eq!(f.run(), Outcome::Updated);
        assert_eq!(fs::read_to_string(f.dest()).unwrap(), lines(11));
    }

    #[test]
    fn user_edits_are_kept_when_upstream_did_not_change() {
        let mut f = Fixture::new(MANIFEST);
        f.upstream("app.conf", &lines(10));
        f.run();
        let edited = lines(10).replace("linha 3", "linha 3 minha");
        fs::write(f.dest(), &edited).unwrap();
        assert_eq!(f.run(), Outcome::Unchanged);
        assert_eq!(fs::read_to_string(f.dest()).unwrap(), edited);
    }

    #[test]
    fn non_overlapping_edits_are_merged() {
        let mut f = Fixture::new(MANIFEST);
        f.upstream("app.conf", &lines(20));
        f.run();
        // usuário edita o começo, upstream edita o fim
        let mine = lines(20).replace("linha 2\n", "linha 2 minha\n");
        fs::write(f.dest(), &mine).unwrap();
        f.upstream("app.conf", &lines(20).replace("linha 19\n", "linha 19 nova\n"));

        assert_eq!(f.run(), Outcome::Merged);
        let merged = fs::read_to_string(f.dest()).unwrap();
        assert!(merged.contains("linha 2 minha"));
        assert!(merged.contains("linha 19 nova"));
        assert!(!merged.contains("<<<<<<<"));
        assert!(f.state.pending().is_empty());
        assert_eq!(f.backups(), 1);
    }

    #[test]
    fn overlapping_edits_become_a_pending_conflict_and_keep_the_user_file() {
        let mut f = Fixture::new(MANIFEST);
        f.upstream("app.conf", &lines(20));
        f.run();
        let mine = lines(20).replace("linha 10\n", "linha 10 minha\n");
        fs::write(f.dest(), &mine).unwrap();
        let theirs = lines(20).replace("linha 10\n", "linha 10 deles\n");
        f.upstream("app.conf", &theirs);

        let mut sink = VecSink::default();
        let reports = apply_module_configs(
            &f.manifest, &f.module_dir, &f.env, &f.state_dir,
            &BTreeMap::new(), FirstApply::default(), &mut f.state, &mut sink,
        )
        .unwrap();
        assert_eq!(reports[0].outcome, Outcome::Conflict);
        assert!(sink.0.iter().any(|e| matches!(e, Event::Conflict { .. })));
        assert_eq!(fs::read_to_string(f.dest()).unwrap(), mine);
        assert_eq!(f.state.pending().len(), 1);

        let diff = conflict_diff(&f.dest(), &f.state_dir, &f.state).unwrap();
        assert!(diff.contains("linha 10 deles"));

        // rodar de novo com a mesma versão nova mantém o conflito, sem duplicar nada
        assert_eq!(f.run(), Outcome::Conflict);
        assert_eq!(f.state.pending().len(), 1);
        assert_eq!(fs::read_to_string(f.dest()).unwrap(), mine);
    }

    #[test]
    fn resolving_with_use_new_replaces_file_and_backs_up() {
        let mut f = Fixture::new(MANIFEST);
        f.upstream("app.conf", &lines(20));
        f.run();
        fs::write(f.dest(), lines(20).replace("linha 10\n", "minha\n")).unwrap();
        let theirs = lines(20).replace("linha 10\n", "deles\n");
        f.upstream("app.conf", &theirs);
        assert_eq!(f.run(), Outcome::Conflict);

        resolve_conflict(&f.dest(), Resolution::UseNew, &f.state_dir, &mut f.state).unwrap();
        assert_eq!(fs::read_to_string(f.dest()).unwrap(), theirs);
        assert!(f.state.pending().is_empty());
        assert_eq!(f.backups(), 1);
        assert_eq!(f.run(), Outcome::Unchanged);
    }

    #[test]
    fn resolving_with_keep_mine_keeps_file_and_marks_upstream_as_seen() {
        let mut f = Fixture::new(MANIFEST);
        f.upstream("app.conf", &lines(20));
        f.run();
        let mine = lines(20).replace("linha 10\n", "minha\n");
        fs::write(f.dest(), &mine).unwrap();
        f.upstream("app.conf", &lines(20).replace("linha 10\n", "deles\n"));
        assert_eq!(f.run(), Outcome::Conflict);

        resolve_conflict(&f.dest(), Resolution::KeepMine, &f.state_dir, &mut f.state).unwrap();
        assert_eq!(fs::read_to_string(f.dest()).unwrap(), mine);
        assert!(f.state.pending().is_empty());
        // mesma versão upstream de novo: agora é "nada novo", sem conflito
        assert_eq!(f.run(), Outcome::Unchanged);
        assert_eq!(fs::read_to_string(f.dest()).unwrap(), mine);
    }

    #[test]
    fn resolve_without_pending_conflict_is_an_error() {
        let mut f = Fixture::new(MANIFEST);
        f.upstream("app.conf", "x\n");
        f.run();
        assert!(resolve_conflict(&f.dest(), Resolution::KeepMine, &f.state_dir, &mut f.state).is_err());
    }

    #[test]
    fn existing_unmanaged_file_is_backed_up_and_adopted_by_default() {
        let mut f = Fixture::new(MANIFEST);
        f.upstream("app.conf", "novo\n");
        fs::create_dir_all(f.dest().parent().unwrap()).unwrap();
        fs::write(f.dest(), "antigo do usuario\n").unwrap();
        assert_eq!(f.run(), Outcome::AdoptedWithBackup);
        assert_eq!(fs::read_to_string(f.dest()).unwrap(), "novo\n");
        assert_eq!(f.backups(), 1);
    }

    #[test]
    fn existing_unmanaged_file_can_be_kept_as_pending() {
        let mut f = Fixture::new(MANIFEST);
        f.upstream("app.conf", "novo\n");
        fs::create_dir_all(f.dest().parent().unwrap()).unwrap();
        fs::write(f.dest(), "antigo do usuario\n").unwrap();
        let reports = f.run_with(FirstApply::KeepPending, &[]);
        assert_eq!(reports[0].outcome, Outcome::Conflict);
        assert_eq!(fs::read_to_string(f.dest()).unwrap(), "antigo do usuario\n");
        resolve_conflict(&f.dest(), Resolution::UseNew, &f.state_dir, &mut f.state).unwrap();
        assert_eq!(fs::read_to_string(f.dest()).unwrap(), "novo\n");
    }

    #[test]
    fn binary_files_conflict_instead_of_merging() {
        let src = MANIFEST.replace("template = true", "template = false");
        let mut f = Fixture::new(&src);
        fs::write(f.module_dir.join("files/app.conf"), [0u8, 159, 146, 150, 1]).unwrap();
        f.run();
        fs::write(f.dest(), [0u8, 159, 146, 150, 2]).unwrap();
        fs::write(f.module_dir.join("files/app.conf"), [0u8, 159, 146, 150, 3]).unwrap();
        assert_eq!(f.run(), Outcome::Conflict);
        assert_eq!(fs::read(f.dest()).unwrap(), vec![0u8, 159, 146, 150, 2]);
        assert!(conflict_diff(&f.dest(), &f.state_dir, &f.state).is_err());
    }

    #[test]
    fn copy_mode_always_overwrites_with_backup() {
        let src = MANIFEST.replace("template = true", "mode = \"copy\"");
        let mut f = Fixture::new(&src);
        f.upstream("app.conf", "versao\n");
        f.run();
        fs::write(f.dest(), "edicao do usuario\n").unwrap();
        assert_eq!(f.run(), Outcome::Updated);
        assert_eq!(fs::read_to_string(f.dest()).unwrap(), "versao\n");
        assert_eq!(f.backups(), 1);
    }

    #[test]
    fn directories_are_copied_recursively_skipping_gitkeep() {
        let src = r#"
schema = 1
id = "dir"
name = "dir"
[[config]]
src = "files/conf.d"
dest = { unix = "~/.config/dir" }
"#;
        let mut f = Fixture::new(src);
        let root = f.module_dir.join("files/conf.d");
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("a.conf"), "a").unwrap();
        fs::write(root.join("sub/b.conf"), "b").unwrap();
        fs::write(root.join(".gitkeep"), "").unwrap();

        let reports = f.run_with(FirstApply::default(), &[]);
        assert_eq!(reports.len(), 2);
        assert_eq!(fs::read_to_string(f.home.join(".config/dir/a.conf")).unwrap(), "a");
        assert_eq!(fs::read_to_string(f.home.join(".config/dir/sub/b.conf")).unwrap(), "b");
        assert!(!f.home.join(".config/dir/.gitkeep").exists());
    }

    #[test]
    fn entries_without_destination_for_this_os_are_skipped() {
        let src = r#"
schema = 1
id = "win"
name = "win"
[[config]]
src = "files/app.conf"
dest = { windows = "%APPDATA%/app" }
"#;
        let mut f = Fixture::new(src);
        f.upstream("app.conf", "x");
        assert!(f.run_with(FirstApply::default(), &[]).is_empty());
    }

    #[test]
    fn missing_source_is_an_error() {
        let mut f = Fixture::new(MANIFEST);
        let err = apply_module_configs(
            &f.manifest, &f.module_dir, &f.env, &f.state_dir,
            &BTreeMap::new(), FirstApply::default(), &mut f.state, &mut VecSink::default(),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
    }

    // ---- opções numéricas e src com template

    const NUMBERS: &str = r#"
schema = 1
id = "term"
name = "term"

[options.opacity]
type = "number"
min = 30
max = 100
default = 85
divisor = 100

[options.blur]
type = "number"
min = 0
max = 64
default = 12

[options.palette]
type = "choice"
values = ["nord", "dracula"]
default = "nord"

[[config]]
src = "files/transparency.conf"
dest = { unix = "~/.config/term/transparency.conf" }
template = true

[[config]]
src = "files/palettes/{{ palette }}.conf"
dest = { unix = "~/.config/term/current-theme.conf" }
"#;

    fn choices(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn number_options_scale_and_validate() {
        let f = Fixture::new(NUMBERS);
        let defaults = resolve_options(&f.manifest, &BTreeMap::new()).unwrap();
        assert_eq!(defaults["opacity"], "0.85");
        assert_eq!(defaults["blur"], "12");
        let custom = resolve_options(&f.manifest, &choices(&[("opacity", "100"), ("blur", "0")])).unwrap();
        assert_eq!(custom["opacity"], "1.00");
        assert_eq!(custom["blur"], "0");
        assert!(resolve_options(&f.manifest, &choices(&[("opacity", "10")])).is_err());
        assert!(resolve_options(&f.manifest, &choices(&[("blur", "65")])).is_err());
        assert!(resolve_options(&f.manifest, &choices(&[("blur", "muito")])).is_err());
    }

    #[test]
    fn templated_src_picks_the_file_for_the_chosen_option() {
        let mut f = Fixture::new(NUMBERS);
        f.upstream("transparency.conf", "background_opacity {{ opacity }}\nblur {{ blur }}\n");
        fs::create_dir_all(f.module_dir.join("files/palettes")).unwrap();
        fs::write(f.module_dir.join("files/palettes/nord.conf"), "cor nord\n").unwrap();
        fs::write(f.module_dir.join("files/palettes/dracula.conf"), "cor dracula\n").unwrap();

        f.run_with(FirstApply::default(), &[]);
        let theme = f.home.join(".config/term/current-theme.conf");
        let transparency = f.home.join(".config/term/transparency.conf");
        assert_eq!(fs::read_to_string(&theme).unwrap(), "cor nord\n");
        assert_eq!(fs::read_to_string(&transparency).unwrap(), "background_opacity 0.85\nblur 12\n");

        // trocar a paleta e a opacidade atualiza os arquivos que o usuário não editou
        let reports = f.run_with(FirstApply::default(), &[("palette", "dracula"), ("opacity", "60")]);
        assert!(reports.iter().all(|r| r.outcome == Outcome::Updated));
        assert_eq!(fs::read_to_string(&theme).unwrap(), "cor dracula\n");
        assert!(fs::read_to_string(&transparency).unwrap().contains("0.60"));
    }

    #[test]
    fn templated_src_cannot_escape_the_module_directory() {
        let manifest = r#"
schema = 1
id = "evil"
name = "evil"

[options.which]
type = "string"
default = "ok"

[[config]]
src = "files/{{ which }}.conf"
dest = { unix = "~/.config/evil/x.conf" }
"#;
        let mut f = Fixture::new(manifest);
        f.upstream("ok.conf", "x");
        assert!(f.run_with(FirstApply::default(), &[]).len() == 1);
        let err = apply_module_configs(
            &f.manifest, &f.module_dir, &f.env, &f.state_dir,
            &choices(&[("which", "../../../etc/passwd")]), FirstApply::default(), &mut f.state, &mut VecSink::default(),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
    }

    // ---- seed

    const SEED: &str = r#"
schema = 1
id = "seeded"
name = "seeded"

[[config]]
src = "files/lock.json"
dest = { unix = "~/.config/seeded/lock.json" }
mode = "seed"
"#;

    #[test]
    fn seed_creates_once_and_never_touches_it_again() {
        let mut f = Fixture::new(SEED);
        f.upstream("lock.json", "{\"v\":1}\n");
        let dest = f.home.join(".config/seeded/lock.json");
        assert_eq!(f.run_with(FirstApply::default(), &[])[0].outcome, Outcome::Created);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "{\"v\":1}\n");

        // o usuário (ou o plugin manager) altera; o upstream também
        fs::write(&dest, "{\"v\":99}\n").unwrap();
        f.upstream("lock.json", "{\"v\":2}\n");
        assert_eq!(f.run_with(FirstApply::default(), &[])[0].outcome, Outcome::Unchanged);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "{\"v\":99}\n");
        assert_eq!(f.backups(), 0);
    }

    // ---- block

    const BLOCK: &str = r##"
schema = 1
id = "sh"
name = "sh"

[[config]]
src = "files/block.sh"
dest = { unix = "~/.bashrc" }
mode = "block"

[config.block]
begin = "# >>> hiraku >>>"
end = "# <<< hiraku <<<"
"##;

    fn rc(f: &Fixture) -> PathBuf {
        f.home.join(".bashrc")
    }

    #[test]
    fn upsert_block_unit_cases() {
        let b = "# >>> x >>>";
        let e = "# <<< x <<<";
        // arquivo vazio
        assert_eq!(upsert_block("", b, e, "a\nb\n").unwrap(), format!("{b}\na\nb\n{e}\n"));
        // anexa depois de uma linha em branco
        assert_eq!(upsert_block("export A=1\n", b, e, "c").unwrap(), format!("export A=1\n\n{b}\nc\n{e}\n"));
        // não duplica a linha em branco
        assert_eq!(upsert_block("export A=1\n\n", b, e, "c").unwrap(), format!("export A=1\n\n{b}\nc\n{e}\n"));
        // substitui no lugar, preservando o que vem antes e depois
        let before = format!("topo\n{b}\nvelho\n{e}\nfim\n");
        assert_eq!(upsert_block(&before, b, e, "novo\n").unwrap(), format!("topo\n{b}\nnovo\n{e}\nfim\n"));
        // remove blocos duplicados
        let dup = format!("{b}\n1\n{e}\nmeio\n{b}\n2\n{e}\n");
        assert_eq!(upsert_block(&dup, b, e, "x").unwrap(), format!("{b}\nx\n{e}\nmeio\n"));
        // CRLF nos marcadores é tolerado
        let crlf = format!("{b}\r\nvelho\r\n{e}\r\n");
        assert_eq!(upsert_block(&crlf, b, e, "n").unwrap(), format!("{b}\nn\n{e}\n"));
        // bloco aberto e nunca fechado é erro
        assert!(upsert_block(&format!("{b}\nsem fim\n"), b, e, "x").is_err());
        // conteúdo vazio gera só os marcadores
        assert_eq!(upsert_block("", b, e, "").unwrap(), format!("{b}\n{e}\n"));
    }

    #[test]
    fn block_creates_the_file_then_is_idempotent() {
        let mut f = Fixture::new(BLOCK);
        f.upstream("block.sh", "export HIRAKU=1\n");
        assert_eq!(f.run(), Outcome::Created);
        assert_eq!(fs::read_to_string(rc(&f)).unwrap(), "# >>> hiraku >>>\nexport HIRAKU=1\n# <<< hiraku <<<\n");
        assert_eq!(f.run(), Outcome::Unchanged);
        assert_eq!(f.backups(), 0);
    }

    #[test]
    fn block_preserves_the_users_content_and_backs_up_before_editing() {
        let mut f = Fixture::new(BLOCK);
        f.upstream("block.sh", "export HIRAKU=1\n");
        fs::write(rc(&f), "alias ll='ls -l'\nexport PATH=$PATH:/x\n").unwrap();
        assert_eq!(f.run(), Outcome::Updated);
        let text = fs::read_to_string(rc(&f)).unwrap();
        assert!(text.starts_with("alias ll='ls -l'\nexport PATH=$PATH:/x\n\n# >>> hiraku >>>\n"));
        assert_eq!(f.backups(), 1);

        // o usuário acrescenta linhas depois; uma atualização do bloco não as perde
        fs::write(rc(&f), format!("{text}alias gs='git status'\n")).unwrap();
        f.upstream("block.sh", "export HIRAKU=2\n");
        assert_eq!(f.run(), Outcome::Updated);
        let after = fs::read_to_string(rc(&f)).unwrap();
        assert!(after.contains("export HIRAKU=2"));
        assert!(!after.contains("export HIRAKU=1"));
        assert!(after.contains("alias gs='git status'"));
        assert!(after.starts_with("alias ll='ls -l'"));
    }

    const BLOCK_TEMPLATE: &str = r##"
schema = 1
id = "sh"
name = "sh"

[options.editor]
type = "choice"
values = ["nvim", "vim"]
default = "nvim"

[[config]]
src = "files/block.sh"
dest = { unix = "~/.bashrc" }
mode = "block"
template = true

[config.block]
begin = "# >>> hiraku >>>"
end = "# <<< hiraku <<<"
"##;

    #[test]
    fn block_template_renders_options() {
        let mut f = Fixture::new(BLOCK_TEMPLATE);
        f.upstream("block.sh", "export EDITOR={{ editor }}\n");
        f.run();
        assert!(fs::read_to_string(rc(&f)).unwrap().contains("export EDITOR=nvim"));
        let reports = f.run_with(FirstApply::default(), &[("editor", "vim")]);
        assert_eq!(reports[0].outcome, Outcome::Updated);
        assert!(fs::read_to_string(rc(&f)).unwrap().contains("export EDITOR=vim"));
    }

    #[test]
    fn block_only_if_exists_never_creates_the_file() {
        let manifest = BLOCK.replace("[config.block]", "[config.block]\nonly_if_exists = true");
        let mut f = Fixture::new(&manifest);
        f.upstream("block.sh", "x\n");
        assert_eq!(f.run(), Outcome::Unchanged);
        assert!(!rc(&f).exists());
        fs::write(rc(&f), "# meu\n").unwrap();
        assert_eq!(f.run(), Outcome::Updated);
    }

    #[test]
    fn block_with_an_unterminated_marker_is_an_error_and_leaves_the_file_alone() {
        let mut f = Fixture::new(BLOCK);
        f.upstream("block.sh", "x\n");
        let broken = "# >>> hiraku >>>\nsem fechamento\n";
        fs::write(rc(&f), broken).unwrap();
        assert!(f.try_run().is_err());
        assert_eq!(fs::read_to_string(rc(&f)).unwrap(), broken);
    }

    #[test]
    fn block_requires_a_file_source() {
        let mut f = Fixture::new(BLOCK);
        fs::create_dir_all(f.module_dir.join("files/block.sh")).unwrap();
        assert!(f.try_run().is_err());
    }

    #[cfg(unix)]
    fn checked(f_manifest: &str) -> String {
        f_manifest.replace(
            "end = \"# <<< hiraku <<<\"",
            "end = \"# <<< hiraku <<<\"\ncheck = [\"sh\", \"-n\", \"{path}\"]",
        )
    }

    #[cfg(unix)]
    #[test]
    fn block_check_rejects_a_result_that_breaks_the_file() {
        let mut f = Fixture::new(&checked(BLOCK));
        f.upstream("block.sh", "if then\n"); // sintaxe inválida em sh
        let original = "export OK=1\n";
        fs::write(rc(&f), original).unwrap();
        let err = f.try_run().unwrap_err();
        assert!(err.to_string().contains("não foi alterado"));
        assert_eq!(fs::read_to_string(rc(&f)).unwrap(), original);
        assert!(!f.home.join(".bashrc.hiraku-check").exists(), "o temporário é removido");
    }

    #[cfg(unix)]
    #[test]
    fn block_check_does_not_blame_the_block_for_an_already_broken_file() {
        let mut f = Fixture::new(&checked(BLOCK));
        f.upstream("block.sh", "export OK=1\n");
        fs::write(rc(&f), "if then\n").unwrap(); // já estava quebrado
        assert_eq!(f.run(), Outcome::Updated);
        assert!(fs::read_to_string(rc(&f)).unwrap().contains("export OK=1"));
    }

    #[cfg(unix)]
    #[test]
    fn block_check_passes_a_valid_result() {
        let mut f = Fixture::new(&checked(BLOCK));
        f.upstream("block.sh", "export OK=1\n");
        assert_eq!(f.run(), Outcome::Created);
    }

    #[test]
    fn block_check_is_skipped_when_the_validator_does_not_exist() {
        let manifest = BLOCK.replace(
            "end = \"# <<< hiraku <<<\"",
            "end = \"# <<< hiraku <<<\"\ncheck = [\"validador-que-nao-existe-hiraku\", \"{path}\"]",
        );
        let mut f = Fixture::new(&manifest);
        f.upstream("block.sh", "x\n");
        assert_eq!(f.run(), Outcome::Created);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_mode_links_and_is_idempotent() {
        let src = r#"
schema = 1
id = "lnk"
name = "lnk"
[[config]]
src = "files/app.conf"
dest = { unix = "~/.config/app/app.conf" }
mode = "symlink"
"#;
        let mut f = Fixture::new(src);
        f.upstream("app.conf", "x");
        assert_eq!(f.run(), Outcome::Linked);
        assert!(fs::symlink_metadata(f.dest()).unwrap().file_type().is_symlink());
        assert_eq!(f.run(), Outcome::Unchanged);
    }

    #[cfg(unix)]
    #[test]
    fn writing_through_a_symlinked_destination_updates_the_target() {
        let mut f = Fixture::new(MANIFEST);
        f.upstream("app.conf", &lines(10));
        let real = f.home.join("dotfiles/app.conf");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(&real, "antigo\n").unwrap();
        fs::create_dir_all(f.dest().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real, f.dest()).unwrap();

        f.run();
        assert!(fs::symlink_metadata(f.dest()).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), lines(10));
    }
}
