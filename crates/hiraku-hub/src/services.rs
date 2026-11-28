//! Tudo o que a TUI faz com o mundo exterior: ler o estado, alterar settings,
//! resolver conflitos e rodar o sync numa thread.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use std::collections::BTreeMap;

use hiraku_core::configs::{self, FirstApply, Resolution};
use hiraku_core::configstate::{state_path, ConfigState};
use hiraku_core::env::Privilege;
use hiraku_core::paths;
use hiraku_core::registry;
use hiraku_core::regsource::{self, Source};
use hiraku_core::settings::settings_path;
use hiraku_core::lockfile::Lockfile;
use hiraku_core::manifest::Manifest;
use hiraku_core::schedule::{self, ProcessRunner, ScheduleDirs};
use hiraku_core::settings::{Mode, Settings};
use hiraku_core::sync::{self, SyncContext, SyncLock, SyncReport};
use hiraku_core::{Environment, Event, NonInteractiveSudo};

use crate::model::{build_conflicts, build_modules, remove_option, Snapshot};
use crate::catalog::build_catalog;

/// Cores nomeadas de um arquivo de paleta: (nome, rgb).
pub type Colors = Vec<(String, (u8, u8, u8))>;

pub enum WorkerMsg {
    Event(Event),
    Done(std::result::Result<SyncReport, String>),
}

#[derive(Clone)]
pub struct Services {
    pub env: Environment,
    pub state_dir: PathBuf,
    pub settings_file: PathBuf,
    pub manifests: Vec<(Manifest, PathBuf)>,
    pub registry_version: u64,
    pub registry_source: String,
    /// Pasta `modules/` local no lugar do registry ativo (desenvolvimento).
    pub registry_override: Option<PathBuf>,
}

impl Services {
    /// Detecta o ambiente e carrega o registry: `registry_override` (ou
    /// `$HIRAKU_REGISTRY`) aponta para uma pasta `modules/` local; sem isso vale o
    /// registry ativo (o remoto assinado, se for mais novo que o embutido).
    pub fn open(registry_override: Option<PathBuf>) -> Result<Services, String> {
        let env = Environment::detect().map_err(|e| e.to_string())?;
        let state_dir = paths::state_dir(&env).ok_or("não foi possível determinar o diretório de estado")?;
        let config_dir = paths::config_dir(&env).ok_or("não foi possível determinar o diretório de configuração")?;
        let mut services = Services {
            env,
            state_dir,
            settings_file: settings_path(&config_dir),
            manifests: Vec::new(),
            registry_version: 0,
            registry_source: String::new(),
            registry_override: registry_override
                .or_else(|| std::env::var_os("HIRAKU_REGISTRY").map(PathBuf::from)),
        };
        services.reload_registry()?;
        Ok(services)
    }

    /// Relê os manifests (depois de atualizar o registry remoto, por exemplo).
    pub fn reload_registry(&mut self) -> Result<(), String> {
        match &self.registry_override {
            Some(dir) => {
                self.manifests = registry::load_dir_with_paths(dir).map_err(|e| e.to_string())?;
                self.registry_version = 0;
                self.registry_source = "local".to_string();
            }
            None => {
                let active = regsource::resolve_active(&regsource::embedded_files(), &self.state_dir)
                    .map_err(|e| e.to_string())?;
                self.manifests =
                    registry::load_dir_with_paths(&active.modules_dir()).map_err(|e| e.to_string())?;
                self.registry_version = active.version;
                self.registry_source = match active.source {
                    Source::Embedded => "embedded",
                    Source::Remote => "remote",
                }
                .to_string();
            }
        }
        Ok(())
    }

    /// Busca o registry remoto assinado (se houver URL configurada). `Ok(None)` =
    /// nada a fazer ou já atualizado; `Ok(Some(nota))` = trocou de versão.
    pub fn refresh_registry(&mut self) -> Result<Option<String>, String> {
        if self.registry_override.is_some() {
            return Ok(None);
        }
        let settings = Settings::load(&self.settings_file).map_err(|e| e.to_string())?;
        let note = regsource::refresh(settings.registry_url.clone(), &self.state_dir).map_err(|e| e.to_string())?;
        if note.is_some() {
            self.reload_registry()?;
        }
        Ok(note)
    }

    /// Cores de todas as escolhas de uma opção (cartões de paleta no app).
    pub fn all_previews(&self, module: &str, key: &str) -> BTreeMap<String, Colors> {
        let Some((manifest, _)) = self.manifests.iter().find(|(m, _)| m.id == module) else {
            return BTreeMap::new();
        };
        let Some(hiraku_core::manifest::OptionKind::Choice { values, .. }) = manifest.options.get(key).map(|d| &d.kind) else {
            return BTreeMap::new();
        };
        values
            .iter()
            .map(|v| (v.clone(), self.preview(module, key, v)))
            .filter(|(_, colors)| !colors.is_empty())
            .collect()
    }

    /// Valida a senha com `sudo -S -v` (o sudo guarda o "ticket"; os `sudo -n` do
    /// sync, filhos deste mesmo processo, passam a funcionar). A senha só passa
    /// pelo stdin do sudo; nada é guardado.
    pub fn sudo_authenticate(&self, password: &str) -> Result<(), String> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        if self.env.privilege == Privilege::Root {
            return Ok(());
        }
        let mut child = Command::new("sudo")
            .args(["-S", "-v", "-p", ""])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("não consegui rodar o sudo: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "{password}");
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err("senha incorreta ou usuário sem permissão de sudo".to_string())
        }
    }

    /// "Lembrar o sudo" está ligado? (o install do gerenciador roda sem senha)
    pub fn sudo_saved(&self) -> bool {
        self.env.privilege == Privilege::Sudo
            && self.env.pkg_manager.is_some_and(hiraku_core::elevate::install_without_password)
    }

    /// Liga/desliga o "lembrar o sudo": grava (ou apaga) a regra em /etc/sudoers.d,
    /// validada pelo `visudo` antes de entrar no lugar. A senha só vai para o sudo.
    pub fn set_sudo_saved(&self, password: &str, on: bool) -> Result<String, String> {
        use hiraku_core::elevate::{sudo_rule, sudoers_file};
        use std::process::Command;
        let pm = self
            .env
            .pkg_manager
            .filter(|pm| pm.needs_root())
            .ok_or("aqui os programas não precisam de sudo para instalar")?;
        if self.env.privilege != Privilege::Sudo {
            return Err("esta máquina não tem sudo para você".to_string());
        }
        let user = Command::new("id")
            .arg("-un")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        let file = sudoers_file(&user);
        self.sudo_authenticate(password)?;
        let sudo = |args: &[&str]| -> Result<(), String> {
            let out = Command::new("sudo").arg("-n").args(args).output().map_err(|e| e.to_string())?;
            if out.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
            }
        };
        if !on {
            sudo(&["rm", "-f", &file.display().to_string()])?;
            return Ok("O sudo volta a pedir senha para instalar.".to_string());
        }
        let rule = sudo_rule(&user, pm).ok_or_else(|| format!("não consegui montar a regra para '{user}'"))?;
        std::fs::create_dir_all(&self.state_dir).map_err(|e| e.to_string())?;
        let tmp = self.state_dir.join("sudoers.tmp");
        std::fs::write(&tmp, rule).map_err(|e| e.to_string())?;
        let tmp_s = tmp.display().to_string();
        let result = sudo(&["visudo", "-cf", &tmp_s])
            .map_err(|e| format!("o visudo recusou a regra: {e}"))
            .and_then(|_| {
                sudo(&["install", "-m", "0440", "-o", "root", "-g", "root", &tmp_s, &file.display().to_string()])
            });
        let _ = std::fs::remove_file(&tmp);
        result?;
        Ok(format!("Pronto: o {} instala sem pedir senha.", pm.as_str()))
    }

    pub fn lock_path(&self) -> PathBuf {
        self.state_dir.join("lock.toml")
    }

    pub fn settings_and_lock(&self) -> Result<(Settings, Lockfile), String> {
        let settings = Settings::load(&self.settings_file).map_err(|e| e.to_string())?;
        let lock = Lockfile::load(&self.lock_path()).map_err(|e| e.to_string())?;
        Ok((settings, lock))
    }

    pub fn snapshot(&self) -> Result<Snapshot, String> {
        let (settings, lock) = self.settings_and_lock()?;
        let cstate = ConfigState::load(&state_path(&self.state_dir)).map_err(|e| e.to_string())?;
        let report = SyncReport::load(&sync::report_path(&self.state_dir)).map_err(|e| e.to_string())?;
        let mut modules = build_modules(&self.manifests, self.env.os, &settings, &lock);
        // Módulos só de configs não entram no lockfile; contam como aplicados se o
        // estado das configs tem arquivos deles.
        for row in modules.iter_mut().filter(|r| r.installed.is_none()) {
            if cstate.files.values().any(|f| f.module == row.id) {
                row.installed = Some("configs".to_string());
            }
        }
        Ok(Snapshot {
            modules,
            conflicts: build_conflicts(&cstate),
            report,
            catalog: build_catalog(&self.manifests, self.env.os),
            settings,
            scheduled: self.is_scheduled(),
        })
    }

    /// Primeira execução: ainda não existe settings.toml.
    pub fn is_first_run(&self) -> bool {
        !self.settings_file.exists()
    }

    pub fn can_root(&self) -> bool {
        self.env.privilege.can_root()
    }

    /// "linux · fedora · x86_64 · sudo disponível"
    pub fn env_label(&self) -> String {
        let mut parts = vec![self.env.os.as_str().to_string()];
        if let Some(d) = &self.env.distro {
            parts.push(d.clone());
        }
        if let Some(a) = self.env.arch {
            parts.push(format!("{a:?}").to_lowercase());
        }
        parts.push(
            match self.env.privilege {
                Privilege::Root => "rodando como root",
                Privilege::Sudo => "sudo disponível",
                Privilege::None => "sem sudo",
            }
            .to_string(),
        );
        parts.join(" · ")
    }

    /// Ambiente com o `mode` do settings.toml aplicado (igual ao `hiraku sync`).
    pub fn env_for(&self, settings: &Settings) -> Environment {
        match settings.mode {
            Some(Mode::User) => self.env.clone().user_mode(),
            Some(Mode::System) => self.env.clone().system_mode(),
            None => self.env.clone(),
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        settings.save(&self.settings_file).map_err(|e| e.to_string())
    }

    /// Valida contra o manifest (com as outras opções do módulo) e grava.
    pub fn set_option(&self, module: &str, key: &str, value: Option<&str>) -> Result<(), String> {
        let manifest = self
            .manifests
            .iter()
            .map(|(m, _)| m)
            .find(|m| m.id == module)
            .ok_or_else(|| format!("módulo '{module}' não existe no registry"))?;
        let mut settings = Settings::load(&self.settings_file).map_err(|e| e.to_string())?;
        match value {
            Some(v) => settings.set_option(module, key, v),
            None => remove_option(&mut settings, module, key),
        }
        let empty = BTreeMap::new();
        let chosen = settings.options.get(module).unwrap_or(&empty);
        configs::resolve_options(manifest, chosen).map_err(|e| e.to_string())?;
        self.save_settings(&settings)
    }

    pub fn set_pin(&self, module: &str, pin: Option<&str>) -> Result<(), String> {
        let mut settings = Settings::load(&self.settings_file).map_err(|e| e.to_string())?;
        match pin {
            Some(p) => {
                settings.pins.insert(module.to_string(), p.to_string());
            }
            None => {
                settings.pins.remove(module);
            }
        }
        self.save_settings(&settings)
    }

    /// Cores do arquivo que a opção escolhe, para pré-visualizar antes de aplicar.
    /// Procura um `[[config]]` cujo `src` use `{{ chave }}`, troca pelo valor e lê
    /// as linhas `nome #rrggbb` (formato do kitty, alacritty-like, etc.).
    pub fn preview(&self, module: &str, key: &str, value: &str) -> Colors {
        let Some((manifest, dir)) = self.manifests.iter().find(|(m, _)| m.id == module) else {
            return Vec::new();
        };
        if value.is_empty() || value.contains('/') || value.contains('\\') || value.contains("..") {
            return Vec::new();
        }
        // 1. `preview = "../icaro-themes/files/.../{value}.vim"` declarado na opção.
        if let Some(pattern) = manifest.options.get(key).and_then(|d| d.preview.as_deref()) {
            return match std::fs::read_to_string(dir.join(pattern.replace("{value}", value))) {
                Ok(text) => parse_colors(&text),
                Err(_) => Vec::new(),
            };
        }
        // 2. Um `[[config]]` cujo `src` usa `{{ chave }}` (paletas do kitty).
        let patterns = [format!("{{{{ {key} }}}}"), format!("{{{{{key}}}}}")];
        let Some(src) = manifest
            .config
            .iter()
            .map(|c| c.src.as_str())
            .find(|src| patterns.iter().any(|p| src.contains(p.as_str())))
        else {
            return Vec::new();
        };
        let mut rel = src.to_string();
        for p in &patterns {
            rel = rel.replace(p.as_str(), value);
        }
        match std::fs::read_to_string(dir.join(rel)) {
            Ok(text) => parse_colors(&text),
            Err(_) => Vec::new(),
        }
    }

    pub fn is_scheduled(&self) -> bool {
        ScheduleDirs::for_env(&self.env, &self.state_dir)
            .map(|dirs| schedule::is_scheduled(self.env.os, &dirs, &ProcessRunner))
            .unwrap_or(false)
    }

    /// Instala (`Some(intervalo)`) ou remove (`None`) o sync agendado.
    pub fn set_schedule(&self, every: Option<u64>) -> Result<String, String> {
        let dirs = ScheduleDirs::for_env(&self.env, &self.state_dir).map_err(|e| e.to_string())?;
        match every {
            Some(secs) => {
                let exe = schedule::scheduled_exe().map_err(|e| format!("caminho do executável: {e}"))?;
                schedule::install_schedule(self.env.os, &dirs, &exe, secs, &ProcessRunner)
                    .map_err(|e| e.to_string())?;
                Ok(format!("atualização automática a cada {}h", secs / 3600))
            }
            None => {
                if self.is_scheduled() {
                    schedule::remove_schedule(self.env.os, &dirs, &ProcessRunner).map_err(|e| e.to_string())?;
                }
                Ok("atualização manual".to_string())
            }
        }
    }

    /// Devolve `true` se o módulo ficou habilitado.
    pub fn toggle_enable(&self, id: &str) -> Result<bool, String> {
        let mut settings = Settings::load(&self.settings_file).map_err(|e| e.to_string())?;
        let enabled = if settings.modules.iter().any(|m| m == id) {
            settings.disable(id);
            false
        } else {
            settings.enable(id);
            // Módulos que escrevem os mesmos arquivos (ex.: Vim clássico x Vim) não
            // ficam ligados juntos: habilitar um desliga o outro.
            for other in self.conflicting(id) {
                settings.disable(&other);
            }
            true
        };
        settings.save(&self.settings_file).map_err(|e| e.to_string())?;
        Ok(enabled)
    }

    /// Aplica o estilo `preset` do módulo `module`: grava as opções dele e habilita o
    /// módulo certo (o próprio ou o do estilo, ex.: Vim clássico), desligando o que
    /// conflita. Devolve o módulo que precisa de sync.
    pub fn apply_preset(&self, module: &str, preset: &str) -> Result<String, String> {
        let mut settings = Settings::load(&self.settings_file).map_err(|e| e.to_string())?;
        let manifests: Vec<&Manifest> = self.manifests.iter().map(|(m, _)| m).collect();
        let target = settings.apply_preset(&manifests, module, preset)?;
        settings.save(&self.settings_file).map_err(|e| e.to_string())?;
        Ok(target)
    }

    /// Módulos em conflito com `id` (declarados em qualquer um dos dois lados).
    pub fn conflicting(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (m, _) in &self.manifests {
            if m.id == id {
                out.extend(m.conflicts.iter().cloned());
            } else if m.conflicts.iter().any(|c| c == id) {
                out.push(m.id.clone());
            }
        }
        out.sort();
        out.dedup();
        out
    }

    pub fn diff(&self, path: &str) -> Result<String, String> {
        let cstate = ConfigState::load(&state_path(&self.state_dir)).map_err(|e| e.to_string())?;
        configs::conflict_diff(Path::new(path), &self.state_dir, &cstate).map_err(|e| e.to_string())
    }

    /// Falha com "outro hiraku já está em execução" se houver um sync rodando.
    pub fn resolve(&self, path: &str, resolution: Resolution) -> Result<(), String> {
        let _guard = SyncLock::acquire(&self.state_dir).map_err(|e| e.to_string())?;
        let file = state_path(&self.state_dir);
        let mut cstate = ConfigState::load(&file).map_err(|e| e.to_string())?;
        configs::resolve_conflict(Path::new(path), resolution, &self.state_dir, &mut cstate)
            .map_err(|e| e.to_string())?;
        cstate.save(&file).map_err(|e| e.to_string())
    }

    pub fn modules_needing_root(&self, settings: &Settings, lock: &Lockfile) -> Result<Vec<String>, String> {
        let list = sync::modules_needing_root(&self.manifests, settings, &self.env_for(settings), lock)
            .map_err(|e| e.to_string())?;
        // Com o sudo lembrado não há senha a pedir.
        Ok(if !list.is_empty() && self.sudo_saved() { Vec::new() } else { list })
    }

    /// Roda o sync numa thread. O root já deve ter sido obtido antes (`sudo -v`);
    /// aqui só se usa `sudo -n`, que nunca bloqueia pedindo senha.
    pub fn spawn_sync(&self, settings: Settings) -> Receiver<WorkerMsg> {
        self.spawn_sync_with(settings, false)
    }

    /// Sync que repara: grava as configs exatamente como esperadas (com backup).
    pub fn spawn_repair(&self, settings: Settings) -> Receiver<WorkerMsg> {
        self.spawn_sync_with(settings, true)
    }

    fn spawn_sync_with(&self, settings: Settings, repair: bool) -> Receiver<WorkerMsg> {
        let (tx, rx) = mpsc::channel();
        let services = self.clone();
        let env = self.env_for(&settings);
        thread::spawn(move || {
            let event_tx = tx.clone();
            let mut sink = move |event: Event| {
                let _ = event_tx.send(WorkerMsg::Event(event));
            };
            let result = sync::run_sync(
                SyncContext {
                    manifests: &services.manifests,
                    settings: &settings,
                    env: &env,
                    state_dir: &services.state_dir,
                    first_apply: FirstApply::default(),
                    registry_version: services.registry_version,
                    registry_source: services.registry_source.clone(),
                    notes: Vec::new(),
                    unattended: false,
                    repair,
                },
                &mut NonInteractiveSudo,
                &mut sink,
            )
            .map_err(|e| e.to_string());
            let _ = tx.send(WorkerMsg::Done(result));
        });
        rx
    }
}

/// Cores nomeadas de um arquivo de tema. Entende:
/// - `nome  #rrggbb` (paletas do kitty);
/// - `let s:nome = ['#rrggbb', 233]` (colorschemes do Vim do Ícaro).
pub fn parse_colors(text: &str) -> Colors {
    let hex = |h: &str| -> Option<(u8, u8, u8)> {
        let h = h.get(..6)?;
        if !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some((byte(0)?, byte(2)?, byte(4)?))
    };
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("let s:") {
                let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                let at = rest.find('#')?;
                return Some((name, hex(&rest[at + 1..])?));
            }
            if line.starts_with('#') || line.starts_with('"') {
                return None;
            }
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            let value = parts.next()?.strip_prefix('#')?;
            if value.len() != 6 {
                return None;
            }
            Some((name.to_string(), hex(value)?))
        })
        .collect()
}

/// Renova a credencial do sudo enquanto o sync roda (instalações longas passam do
/// tempo de cache). Parar = soltar o valor ou chamar `stop`.
pub struct Keepalive {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Keepalive {
    pub fn start() -> Keepalive {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = thread::spawn(move || loop {
            for _ in 0..60 {
                if flag.load(Ordering::Relaxed) {
                    return;
                }
                thread::sleep(Duration::from_secs(1));
            }
            let _ = std::process::Command::new("sudo")
                .args(["-n", "-v"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        });
        Keepalive {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for Keepalive {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Não faz join: pode levar até 1s e o laço da TUI não deve esperar.
        self.handle.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hiraku_core::env::{Arch, Os, Privilege};
    use std::fs;

    struct Fx {
        _tmp: tempfile::TempDir,
        services: Services,
    }

    fn fixture() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();

        let module_dir = root.join("mods/app");
        fs::create_dir_all(module_dir.join("files")).unwrap();
        fs::write(module_dir.join("files/app.conf"), "v1\n").unwrap();
        let manifest = Manifest::from_toml_str(
            "schema = 1\nid = \"app\"\nname = \"app\"\ndescription = \"um app\"\n[[config]]\nsrc = \"files/app.conf\"\ndest = { unix = \"~/.config/app/app.conf\" }\n",
            "t",
        )
        .unwrap();

        let services = Services {
            env: Environment {
                os: Os::Linux,
                arch: Some(Arch::X86_64),
                distro: None,
                pkg_manager: None,
                privilege: Privilege::None,
                use_root: false,
                home: Some(home),
            },
            state_dir: root.join("state"),
            settings_file: root.join("cfg/settings.toml"),
            manifests: vec![(manifest, module_dir)],
            registry_version: 1,
            registry_source: "embedded".to_string(),
            registry_override: None,
        };
        Fx { _tmp: tmp, services }
    }

    #[test]
    fn toggle_enable_roundtrips_through_the_settings_file() {
        let fx = fixture();
        assert!(fx.services.toggle_enable("app").unwrap());
        let snap = fx.services.snapshot().unwrap();
        assert!(snap.modules[0].enabled);
        assert!(!fx.services.toggle_enable("app").unwrap());
        assert!(!fx.services.snapshot().unwrap().modules[0].enabled);
    }

    #[test]
    fn worker_runs_a_sync_and_reports_back() {
        let fx = fixture();
        fx.services.toggle_enable("app").unwrap();
        let (settings, _) = fx.services.settings_and_lock().unwrap();
        let rx = fx.services.spawn_sync(settings);

        let mut saw_event = false;
        let report = loop {
            match rx.recv_timeout(Duration::from_secs(20)).expect("o worker deve responder") {
                WorkerMsg::Event(_) => saw_event = true,
                WorkerMsg::Done(result) => break result.unwrap(),
            }
        };
        assert!(saw_event, "a criação do arquivo emite um evento de log");
        assert_eq!(report.modules.len(), 1);
        assert!(fx.services.env.home.as_ref().unwrap().join(".config/app/app.conf").is_file());
        let snap = fx.services.snapshot().unwrap();
        assert!(snap.report.is_some());
        assert_eq!(
            snap.modules[0].installed.as_deref(),
            Some("configs"),
            "módulo só de configs conta como aplicado (libera o ao vivo)"
        );
    }

    #[test]
    fn conflicts_are_listed_diffed_and_resolved() {
        let fx = fixture();
        fx.services.toggle_enable("app").unwrap();
        let (settings, _) = fx.services.settings_and_lock().unwrap();
        let run = |s: &Settings| {
            let rx = fx.services.spawn_sync(s.clone());
            loop {
                if let WorkerMsg::Done(r) = rx.recv_timeout(Duration::from_secs(20)).unwrap() {
                    break r.unwrap();
                }
            }
        };
        run(&settings);

        let dest = fx.services.env.home.as_ref().unwrap().join(".config/app/app.conf");
        let module_file = fx.services.manifests[0].1.join("files/app.conf");
        let many = |line: &str| (1..=20).map(|i| if i == 10 { format!("{line}\n") } else { format!("l{i}\n") }).collect::<String>();
        fs::write(&module_file, many("original")).unwrap();
        run(&settings);
        fs::write(&dest, many("minha")).unwrap();
        fs::write(&module_file, many("deles")).unwrap();
        let report = run(&settings);
        assert_eq!(report.conflicts(), 1);

        let snap = fx.services.snapshot().unwrap();
        assert_eq!(snap.conflicts.len(), 1);
        let path = snap.conflicts[0].path.clone();
        assert!(fx.services.diff(&path).unwrap().contains("deles"));

        fx.services.resolve(&path, Resolution::UseNew).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), many("deles"));
        assert!(fx.services.snapshot().unwrap().conflicts.is_empty());
    }

    #[test]
    fn resolving_while_a_sync_holds_the_lock_is_refused() {
        let fx = fixture();
        let _held = SyncLock::acquire(&fx.services.state_dir).unwrap();
        let err = fx.services.resolve("/qualquer", Resolution::KeepMine).unwrap_err();
        assert!(err.contains("em execução"));
    }

    #[test]
    fn set_option_validates_against_the_manifest_and_removes_defaults() {
        let mut fx = fixture();
        let manifest = Manifest::from_toml_str(
            "schema = 1\nid = \"app\"\nname = \"app\"\n[options.opacity]\ntype = \"number\"\nmin = 30\nmax = 100\ndefault = 80\n",
            "t",
        )
        .unwrap();
        fx.services.manifests[0].0 = manifest;

        fx.services.set_option("app", "opacity", Some("90")).unwrap();
        let (s, _) = fx.services.settings_and_lock().unwrap();
        assert_eq!(s.options["app"]["opacity"], "90");

        let err = fx.services.set_option("app", "opacity", Some("5")).unwrap_err();
        assert!(err.contains("fora do intervalo"), "{err}");
        assert!(fx.services.set_option("app", "nada", Some("1")).is_err());
        assert!(fx.services.set_option("outro", "x", Some("1")).is_err());
        let (s, _) = fx.services.settings_and_lock().unwrap();
        assert_eq!(s.options["app"]["opacity"], "90", "valor inválido não é gravado");

        fx.services.set_option("app", "opacity", None).unwrap();
        let (s, _) = fx.services.settings_and_lock().unwrap();
        assert!(s.options.is_empty(), "sem escolhas, sem tabela vazia");
    }

    #[test]
    fn apply_preset_resets_other_presets_and_swaps_conflicting_modules() {
        let mut fx = fixture();
        let ed = Manifest::from_toml_str(
            "schema = 1\nid = \"ed\"\nname = \"ed\"\ndescription = \"d\"\n\
             [options.perfil]\ntype = \"choice\"\ndefault = \"completo\"\nvalues = [\"bruto\", \"dalton\", \"completo\"]\n\
             [options.tela]\ntype = \"bool\"\ndefault = true\n\
             [[preset]]\nid = \"bruto\"\nname = \"Bruto\"\noptions = { perfil = \"bruto\", tela = \"false\" }\n\
             [[preset]]\nid = \"dalton\"\nname = \"Dalton\"\noptions = { perfil = \"dalton\" }\n\
             [[preset]]\nid = \"velho\"\nname = \"Velho\"\nmodule = \"ed-velho\"\n",
            "t",
        )
        .unwrap();
        let velho = Manifest::from_toml_str(
            "schema = 1\nid = \"ed-velho\"\nname = \"v\"\ndescription = \"d\"\nconflicts = [\"ed\"]\n",
            "t",
        )
        .unwrap();
        let dir = fx.services.manifests[0].1.clone();
        fx.services.manifests.push((ed, dir.clone()));
        fx.services.manifests.push((velho, dir));
        let s = &fx.services;

        assert_eq!(s.apply_preset("ed", "bruto").unwrap(), "ed");
        let st = Settings::load(&s.settings_file).unwrap();
        assert_eq!(st.options["ed"]["tela"], "false");
        assert!(st.modules.contains(&"ed".to_string()));

        // Dalton não fala de "tela": ela volta ao padrão em vez de herdar do Bruto
        s.apply_preset("ed", "dalton").unwrap();
        let st = Settings::load(&s.settings_file).unwrap();
        assert_eq!(
            st.options["ed"].get("perfil").map(String::as_str),
            Some("dalton")
        );
        assert!(!st.options["ed"].contains_key("tela"));

        // o estilo que é outro módulo liga ele e desliga o conflitante
        assert_eq!(s.apply_preset("ed", "velho").unwrap(), "ed-velho");
        let st = Settings::load(&s.settings_file).unwrap();
        assert!(st.modules.contains(&"ed-velho".to_string()));
        assert!(!st.modules.contains(&"ed".to_string()));
        assert_eq!(
            st.options["ed"].get("perfil").map(String::as_str),
            Some("dalton")
        );

        // e voltar para um estilo normal desfaz a troca
        s.apply_preset("ed", "bruto").unwrap();
        let st = Settings::load(&s.settings_file).unwrap();
        assert!(st.modules.contains(&"ed".to_string()));
        assert!(!st.modules.contains(&"ed-velho".to_string()));

        assert!(s.apply_preset("ed", "nada").is_err());
        assert!(s.apply_preset("nada", "bruto").is_err());
    }

    #[test]
    fn pins_roundtrip() {
        let fx = fixture();
        fx.services.set_pin("app", Some("1.*")).unwrap();
        assert_eq!(fx.services.snapshot().unwrap().modules[0].pin.as_deref(), Some("1.*"));
        fx.services.set_pin("app", None).unwrap();
        assert_eq!(fx.services.snapshot().unwrap().modules[0].pin, None);
    }

    #[test]
    fn first_run_and_mode_follow_the_settings_file() {
        let mut fx = fixture();
        fx.services.env.privilege = Privilege::Sudo;
        fx.services.env.use_root = true;
        assert!(fx.services.is_first_run());
        let mut s = Settings {
            mode: Some(Mode::User),
            ..Settings::default()
        };
        fx.services.save_settings(&s).unwrap();
        assert!(!fx.services.is_first_run());
        assert!(!fx.services.env_for(&s).use_root, "user = nunca root");
        s.mode = Some(Mode::System);
        assert!(fx.services.env_for(&s).use_root);
        assert!(fx.services.env_label().contains("sudo disponível"));
    }

    #[test]
    fn preview_reads_the_real_kitty_palettes() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry/modules/kitty");
        let manifest = Manifest::from_toml_str(&fs::read_to_string(dir.join("module.toml")).unwrap(), "kitty").unwrap();
        let mut fx = fixture();
        fx.services.manifests = vec![(manifest, dir)];
        let colors = fx.services.preview("kitty", "palette", "nord");
        let get = |n: &str| colors.iter().find(|(k, _)| k == n).map(|(_, c)| *c);
        assert_eq!(get("background"), Some((0x2e, 0x34, 0x40)));
        assert!(get("color15").is_some());
        assert!(fx.services.preview("kitty", "palette", "../../etc").is_empty());
        assert!(fx.services.preview("kitty", "opacity", "80").is_empty(), "opção sem arquivo próprio");
        assert!(fx.services.preview("nada", "palette", "nord").is_empty());
        let all = fx.services.all_previews("kitty", "palette");
        assert_eq!(all.len(), 28, "uma pré-visualização por paleta do registry");
        assert!(fx.services.all_previews("kitty", "opacity").is_empty());
    }

    #[test]
    fn vim_colorscheme_palettes_are_read() {
        let colors = parse_colors("\" comentário\nlet s:bg0      = ['#10161c', 233]\nlet s:fg0 = [\"#eaf2f7\", 255]\nlet s:none = ['NONE', 'NONE']\nhi clear\n");
        assert_eq!(
            colors,
            vec![("bg0".to_string(), (0x10, 0x16, 0x1c)), ("fg0".to_string(), (0xea, 0xf2, 0xf7))]
        );
    }

    #[test]
    fn parse_colors_ignores_noise() {
        let colors = parse_colors("# comentário #ffffff\nbackground #102030\nvisual_bell_color none\ncolor1 #12345\ncursor   #ABCDEF extra\n");
        assert_eq!(
            colors,
            vec![
                ("background".to_string(), (0x10, 0x20, 0x30)),
                ("cursor".to_string(), (0xab, 0xcd, 0xef)),
            ]
        );
    }

    #[test]
    fn keepalive_stops_promptly_when_dropped() {
        let keepalive = Keepalive::start();
        drop(keepalive);
    }
}
