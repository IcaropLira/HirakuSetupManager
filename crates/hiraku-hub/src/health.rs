//! "Verificar integridade": confere, sem mudar nada, se o que o hiraku deveria ter
//! aplicado está mesmo no disco e se os programas que leem esses arquivos são os
//! certos. Cada problema vem com uma explicação em português e, quando dá, o
//! "Reparar" resolve (sync com `repair = true`).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;

use hiraku_core::configs::{self, FileHealth};
use hiraku_core::configstate::{state_path, ConfigState};
use hiraku_core::sync::{self, InstallStatus, SyncReport};

use crate::services::Services;

#[derive(Debug, Clone, Serialize)]
pub struct HealthItem {
    /// "ok" | "warn" | "error"
    pub level: &'static str,
    pub title: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// O "Reparar" resolve este item.
    pub fixable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleHealth {
    pub id: String,
    pub name: String,
    /// Arquivos conferidos e iguais ao esperado.
    pub ok_files: usize,
    pub items: Vec<HealthItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub general: Vec<HealthItem>,
    pub modules: Vec<ModuleHealth>,
    /// Quantos problemas o "Reparar" resolve.
    pub fixable: usize,
    pub problems: usize,
}

fn item(level: &'static str, title: impl Into<String>, detail: impl Into<String>) -> HealthItem {
    HealthItem { level, title: title.into(), detail: detail.into(), path: None, fixable: false }
}

fn file_item(h: FileHealth, dest: &Path) -> Option<HealthItem> {
    let (level, title, detail) = match h {
        FileHealth::Ok => return None,
        FileHealth::Missing => ("error", "Arquivo não existe", "O hiraku deveria ter criado este arquivo e ele não está lá."),
        FileHealth::Outdated => ("error", "Arquivo desatualizado", "As opções mudaram no app, mas a versão nova não chegou a ser gravada (o último sync parou antes ou falhou)."),
        FileHealth::EditedOutside => ("warn", "Editado fora do app", "O arquivo foi mudado à mão depois do último sync; por isso as opções do app não entram nele."),
        FileHealth::NotManaged => ("error", "Arquivo antigo no lugar", "Já existia um arquivo seu (de uma instalação antiga?) e o do hiraku nunca foi gravado. Enquanto ele estiver aí, nada do app vale."),
        FileHealth::Pending => ("warn", "Versão nova esperando você", "Há uma versão nova deste arquivo na aba Conflitos esperando você escolher."),
        FileHealth::BlockMissing => ("error", "Bloco do hiraku ausente", "O trecho que o hiraku coloca neste arquivo não está lá (ou está diferente)."),
        FileHealth::LinkWrong => ("error", "Link quebrado no caminho", "Uma pasta deste caminho é um link que não aponta para nada (sobra do install.sh antigo, que ligava as pastas a um clone do repositório) ou não é o link esperado. O Reparar (ou o próximo sync) troca por uma pasta de verdade."),
    };
    let mut it = item(level, title, detail);
    it.path = Some(dest.display().to_string());
    it.fixable = true;
    Some(it)
}

/// Muitos arquivos com o mesmo problema (ex.: uma pasta de temas inteira faltando)
/// viram um item só, com a pasta em comum.
fn group_files(items: Vec<HealthItem>) -> Vec<HealthItem> {
    let mut out: Vec<HealthItem> = Vec::new();
    let mut groups: Vec<(String, Vec<HealthItem>)> = Vec::new();
    for it in items {
        match groups.iter_mut().find(|(t, _)| *t == it.title) {
            Some((_, v)) => v.push(it),
            None => groups.push((it.title.clone(), vec![it])),
        }
    }
    for (_, mut v) in groups {
        if v.len() <= 4 {
            out.extend(v);
            continue;
        }
        let paths: Vec<PathBuf> = v.iter().filter_map(|i| i.path.as_ref().map(PathBuf::from)).collect();
        let mut common = paths.first().and_then(|p| p.parent()).map(Path::to_path_buf).unwrap_or_default();
        while !paths.iter().all(|p| p.starts_with(&common)) {
            if !common.pop() {
                break;
            }
        }
        let n = v.len();
        let mut first = v.remove(0);
        first.title = format!("{} ({n} arquivos)", first.title);
        first.detail = format!("{} Ex.: {}", first.detail, first.path.clone().unwrap_or_default());
        first.path = Some(format!("{}/…", common.display()));
        out.push(first);
    }
    out
}

/// Roda um comando com tempo limite; devolve a saída (stdout+stderr) se terminou.
fn run(program: &str, args: &[&str], timeout: Duration) -> Option<(bool, String)> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(30)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
    let out = child.wait_with_output().ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Some((out.status.success(), text))
}

/// PATH de um terminal novo do usuário (o app pode ter sido aberto com outro PATH).
fn shell_path() -> Option<String> {
    if cfg!(windows) {
        return std::env::var("PATH").ok();
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let (_, out) = run(&shell, &["-lic", "printf '@@%s@@' \"$PATH\""], Duration::from_secs(4))?;
    let start = out.find("@@")? + 2;
    let end = out[start..].find("@@")? + start;
    Some(out[start..end].to_string())
}

fn which_in(path: &str, cmd: &str) -> Option<PathBuf> {
    std::env::split_paths(path).map(|d| d.join(cmd)).find(|p| {
        p.is_file() || (cfg!(windows) && p.with_extension("exe").is_file())
    })
}

/// O vim de verdade por trás do wrapper ~/.local/bin/vim (se for o wrapper).
fn real_vim(found: &Path) -> PathBuf {
    if let Ok(text) = std::fs::read_to_string(found) {
        if text.contains("ICARO_VIM_WRAPPER") || text.contains("hiraku") {
            for line in text.lines() {
                if let Some(rest) = line.trim().strip_prefix("REAL_VIM=") {
                    let p = rest.trim_matches(|c| c == '"' || c == '\'');
                    if !p.is_empty() {
                        return PathBuf::from(p);
                    }
                }
            }
        }
    }
    found.to_path_buf()
}

fn vim_checks(path: &str) -> Vec<HealthItem> {
    let mut out = Vec::new();
    let Some(found) = which_in(path, "vim") else {
        out.push(item("error", "Comando vim não encontrado", "Num terminal novo, `vim` não existe. No Fedora: sudo dnf install vim-enhanced; no Ubuntu: sudo apt install vim."));
        return out;
    };
    let real = real_vim(&found);
    let Some((_, version)) = run(&real.display().to_string(), &["--version"], Duration::from_secs(4)) else {
        out.push(item("warn", "Não consegui rodar o vim", format!("{} não respondeu ao --version.", real.display())));
        return out;
    };
    let missing: Vec<&str> = ["+eval", "+timers", "+job", "+popupwin"]
        .into_iter()
        .filter(|f| !version.contains(f))
        .collect();
    if missing.contains(&"+eval") {
        out.push(item("error", "Seu vim é o vim-minimal", format!("{} não tem +eval: ele ignora toda a configuração (temas, atalhos, opções do app). Instale o vim completo (Fedora: sudo dnf install vim-enhanced; Ubuntu: sudo apt install vim) e rode o sync.", real.display())));
    } else if !missing.is_empty() {
        out.push(item("warn", "Vim sem recursos usados pelo app", format!("{} não tem {}: sem isso as mudanças do app só valem ao reabrir o Vim e os plugins baixados sob demanda não funcionam.", real.display(), missing.join(", "))));
    } else {
        out.push(item("ok", "Vim completo", format!("{} (via {})", real.display(), found.display())));
    }
    out
}

fn nvim_checks(path: &str) -> Vec<HealthItem> {
    let mut out = Vec::new();
    let Some(found) = which_in(path, "nvim") else {
        out.push(item("error", "Comando nvim não encontrado", "Num terminal novo, `nvim` não existe: rode o sync de novo ou confira o PATH."));
        return out;
    };
    match run(&found.display().to_string(), &["--version"], Duration::from_secs(4)) {
        Some((_, v)) => {
            let first = v.lines().next().unwrap_or("").to_string();
            let minor = first.strip_prefix("NVIM v").and_then(|r| r.split('.').nth(1)).and_then(|m| m.parse::<u32>().ok());
            let major = first.strip_prefix("NVIM v").and_then(|r| r.split('.').next()).and_then(|m| m.parse::<u32>().ok());
            if matches!((major, minor), (Some(0), Some(m)) if m < 10) {
                out.push(item("error", "Neovim antigo", format!("{first} em {}: a configuração precisa do 0.10 ou mais novo. O {} na frente do PATH é o antigo.", found.display(), found.display())));
            } else {
                out.push(item("ok", "Neovim", format!("{first} ({})", found.display())));
            }
        }
        None => out.push(item("warn", "Não consegui rodar o nvim", found.display().to_string())),
    }
    if std::env::var("NVIM_APPNAME").map_or(false, |v| !v.is_empty() && v != "nvim") {
        out.push(item("warn", "NVIM_APPNAME definido", "O Neovim está lendo outra pasta de configuração (variável NVIM_APPNAME)."));
    }
    out
}

impl Services {
    /// Confere tudo dos módulos habilitados. Não grava nada.
    pub fn health(&self) -> Result<HealthReport, String> {
        let (settings, _lock) = self.settings_and_lock()?;
        let env = self.env_for(&settings);
        let (order, unknown) = sync::order_modules(&self.manifests, &settings.modules).map_err(|e| e.to_string())?;
        let cstate = ConfigState::load(&state_path(&self.state_dir)).map_err(|e| e.to_string())?;
        let last: Option<SyncReport> = SyncReport::load(&sync::report_path(&self.state_dir)).ok().flatten();

        let mut general = Vec::new();
        if settings.modules.is_empty() {
            general.push(item("warn", "Nenhum programa habilitado", "Habilite os programas na aba Módulos e rode o sync."));
        }
        for u in &unknown {
            general.push(item("warn", format!("Módulo '{u}' não existe mais"), "Ele está nas suas configurações, mas não no registry atual."));
        }
        match &last {
            None => general.push(item("warn", "Nenhum sync registrado", "O app ainda não terminou um sync nesta máquina: clique em Sincronizar.")),
            Some(r) => {
                let ago = hiraku_core::lockfile::now_secs().saturating_sub(r.finished_at);
                let when = if ago < 3600 { format!("há {} min", ago / 60) } else if ago < 86400 { format!("há {} h", ago / 3600) } else { format!("há {} dias", ago / 86400) };
                general.push(item("ok", "Último sync", format!("{when}, registry v{} ({})", r.registry_version, r.registry_source)));
            }
        }
        let path = shell_path();
        let local_bin = env.home.as_ref().map(|h| h.join(".local").join("bin"));
        if let (Some(p), Some(lb), false) = (&path, &local_bin, cfg!(windows)) {
            if !std::env::split_paths(p).any(|d| &d == lb) {
                general.push(item("error", "~/.local/bin fora do PATH", format!("Os programas instalados pelo hiraku ficam em {} e um terminal novo não procura lá. Abra um terminal novo depois do sync; se continuar, adicione  export PATH=\"$HOME/.local/bin:$PATH\"  ao ~/.bashrc ou ~/.zshrc.", lb.display())));
            }
        }
        let path_text = path.clone().or_else(|| std::env::var("PATH").ok()).unwrap_or_default();

        let mut modules = Vec::new();
        for id in &order {
            let Some((m, dir)) = self.manifests.iter().find(|(m, _)| &m.id == id) else { continue };
            if !m.supports(env.os) {
                continue;
            }
            let mut items = Vec::new();
            let mut ok_files = 0;

            if let Some(r) = last.as_ref().and_then(|r| r.modules.iter().find(|x| &x.id == id)) {
                if r.status == InstallStatus::Failed || r.error.is_some() {
                    items.push(item("error", "O último sync deu erro aqui", r.error.clone().or(r.detail.clone()).unwrap_or_else(|| "falhou".to_string())));
                }
            } else if last.is_some() {
                items.push(item("warn", "Ainda não sincronizado", "Este programa está habilitado, mas não entrou no último sync."));
            }

            let choices = settings.options.get(id).cloned().unwrap_or_default();
            match configs::check_module_configs(m, dir, &env, &choices, &self.state_dir, &cstate) {
                Ok(files) => {
                    let mut bad = Vec::new();
                    for f in files {
                        match file_item(f.health, &f.dest) {
                            None => ok_files += 1,
                            Some(it) => bad.push(it),
                        }
                    }
                    items.extend(group_files(bad));
                }
                Err(e) => items.push(item("error", "Não consegui conferir os arquivos", e.to_string())),
            }

            // comandos que o módulo instala
            if !cfg!(windows) {
                let mut seen = Vec::new();
                for step in m.select_installs(&env) {
                    for cmd in &step.provides {
                        if seen.contains(cmd) {
                            continue;
                        }
                        seen.push(cmd.clone());
                        if which_in(&path_text, cmd).is_none() {
                            items.push(item("error", format!("Comando {cmd} não encontrado"), "Num terminal novo ele não existe: a instalação não terminou ou a pasta dele não está no PATH. Rode o sync."));
                        }
                    }
                }
            }
            match id.as_str() {
                "vim" => items.extend(vim_checks(&path_text)),
                "nvim" => items.extend(nvim_checks(&path_text)),
                _ => {}
            }
            modules.push(ModuleHealth { id: id.clone(), name: m.name.clone(), ok_files, items });
        }

        let all = general.iter().chain(modules.iter().flat_map(|m| m.items.iter()));
        let (mut problems, mut fixable) = (0, 0);
        for it in all {
            if it.level != "ok" {
                problems += 1;
                if it.fixable {
                    fixable += 1;
                }
            }
        }
        Ok(HealthReport { general, modules, fixable, problems })
    }
}
