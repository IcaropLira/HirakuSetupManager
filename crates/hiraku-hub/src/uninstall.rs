//! Desinstalar módulos, tudo, ou o próprio app (dados e executável).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use hiraku_core::configstate::{state_path, ConfigState};
use hiraku_core::event::VecSink;
use hiraku_core::uninstall::{uninstall_module, UninstallReport};

use crate::services::Services;

#[derive(Debug, Clone, Serialize)]
pub struct AppRemoval {
    /// Como o app foi instalado: "appimage", "rpm", "deb", "macos", "windows", "dev".
    pub kind: String,
    /// O app já foi removido (falta só fechar).
    pub done: bool,
    /// Comando para o usuário rodar, se não deu para remover daqui.
    pub command: Option<String>,
    pub message: String,
}

impl Services {
    /// Desinstala os módulos `ids` (ou, vazio, todos os que o hiraku tocou nesta
    /// máquina) e os tira das configurações.
    pub fn uninstall(&self, ids: &[String]) -> Result<Vec<UninstallReport>, String> {
        let (mut settings, mut lock) = self.settings_and_lock()?;
        let cpath = state_path(&self.state_dir);
        let mut cstate = ConfigState::load(&cpath).map_err(|e| e.to_string())?;
        let env = self.env_for(&settings);

        let mut targets: Vec<String> = if ids.is_empty() {
            let mut all: Vec<String> = settings.modules.clone();
            all.extend(lock.modules.keys().map(|k| k.split('/').next().unwrap_or(k).to_string()));
            all.extend(cstate.files.values().map(|f| f.module.clone()));
            all
        } else {
            ids.to_vec()
        };
        targets.sort();
        targets.dedup();

        let mut out = Vec::new();
        let mut sink = VecSink::default();
        for id in &targets {
            let Some((m, dir)) = self.manifests.iter().find(|(m, _)| &m.id == id) else {
                // módulo que saiu do registry: tira só dos registros
                lock.modules.retain(|k, _| k != id && !k.starts_with(&format!("{id}/")));
                cstate.files.retain(|_, f| &f.module != id);
                settings.disable(id);
                continue;
            };
            let choices = settings.options.get(id).cloned().unwrap_or_default();
            let rep = uninstall_module(m, dir, &env, &choices, &self.state_dir, &mut lock, &mut cstate, &mut sink)
                .map_err(|e| format!("{id}: {e}"))?;
            settings.disable(id);
            out.push(rep);
        }
        lock.save(&self.lock_path()).map_err(|e| e.to_string())?;
        cstate.save(&cpath).map_err(|e| e.to_string())?;
        settings.save(&self.settings_file).map_err(|e| e.to_string())?;
        Ok(out)
    }

    /// Desinstala tudo, tira o sync agendado e apaga os dados do app (configurações,
    /// registry baixado, lockfile; os backups ficam se `keep_backups`). Depois tenta
    /// remover o próprio executável.
    pub fn uninstall_app(&self, keep_backups: bool) -> Result<(Vec<UninstallReport>, AppRemoval), String> {
        let reports = self.uninstall(&[])?;
        let _ = self.set_schedule(None);
        if let Some(dir) = self.settings_file.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
        if let Ok(entries) = std::fs::read_dir(&self.state_dir) {
            for e in entries.flatten() {
                if keep_backups && e.file_name() == "backups" {
                    continue;
                }
                let p = e.path();
                let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
            }
        }
        if !keep_backups {
            let _ = std::fs::remove_dir(&self.state_dir);
        }
        Ok((reports, remove_executable(self.env.home.as_deref())))
    }
}

fn run_ok(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd).args(args).status().map(|s| s.success()).unwrap_or(false)
}

fn owned_by(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Remove o executável do app conforme o jeito que ele foi instalado.
pub fn remove_executable(home: Option<&Path>) -> AppRemoval {
    let exe = std::env::current_exe().unwrap_or_default();
    let mk = |kind: &str, done: bool, command: Option<String>, message: String| AppRemoval {
        kind: kind.to_string(),
        done,
        command,
        message,
    };

    if cfg!(target_os = "linux") {
        // AppImage: o arquivo baixado + o atalho do menu criado pelo tutorial
        if let Some(img) = std::env::var_os("APPIMAGE").map(PathBuf::from) {
            let removed = std::fs::remove_file(&img).is_ok();
            if let Some(h) = home {
                let _ = std::fs::remove_file(h.join(".local/share/applications/hiraku-setup-manager.desktop"));
            }
            return mk(
                "appimage",
                removed,
                (!removed).then(|| format!("rm {:?}", img)),
                if removed { format!("O AppImage ({}) foi apagado.", img.display()) } else { "Não consegui apagar o AppImage.".to_string() },
            );
        }
        let exe_s = exe.display().to_string();
        if let Some(pkg) = owned_by("rpm", &["-qf", "--qf", "%{NAME}", &exe_s]) {
            let cmd = format!("sudo dnf remove {pkg}");
            let done = Path::new("/usr/bin/pkexec").exists() && run_ok("pkexec", &["dnf", "remove", "-y", &pkg]);
            return mk("rpm", done, (!done).then(|| cmd.clone()), if done { "O pacote foi removido.".to_string() } else { "Falta só tirar o app: isso pede a senha de administrador.".to_string() });
        }
        if let Some(line) = owned_by("dpkg", &["-S", &exe_s]) {
            let pkg = line.split(':').next().unwrap_or("hiraku-setup-manager").to_string();
            let cmd = format!("sudo apt remove {pkg}");
            let done = Path::new("/usr/bin/pkexec").exists() && run_ok("pkexec", &["apt-get", "remove", "-y", &pkg]);
            return mk("deb", done, (!done).then(|| cmd.clone()), if done { "O pacote foi removido.".to_string() } else { "Falta só tirar o app: isso pede a senha de administrador.".to_string() });
        }
        return mk("dev", false, None, format!("Este executável ({exe_s}) não veio de um pacote: apague-o à mão."));
    }
    if cfg!(target_os = "macos") {
        // .../Hiraku Setup Manager.app/Contents/MacOS/hiraku-setup-manager -> lixeira
        if let Some(app) = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")) {
            let script = format!("tell application \"Finder\" to delete POSIX file \"{}\"", app.display());
            let done = run_ok("osascript", &["-e", &script]);
            return mk("macos", done, (!done).then(|| format!("rm -rf {:?}", app)), if done { "O app foi para a Lixeira.".to_string() } else { "Arraste o app para a Lixeira.".to_string() });
        }
        return mk("dev", false, None, "Apague o app à mão.".to_string());
    }
    // Windows (instalador NSIS do Tauri): uninstall.exe na pasta do app
    if let Some(dir) = exe.parent() {
        let un = dir.join("uninstall.exe");
        if un.is_file() && Command::new(&un).spawn().is_ok() {
            return mk("windows", true, None, "O desinstalador do Windows foi aberto.".to_string());
        }
    }
    mk("windows", false, None, "Remova o app em Configurações → Aplicativos do Windows.".to_string())
}
