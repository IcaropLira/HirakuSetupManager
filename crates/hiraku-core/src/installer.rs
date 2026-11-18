//! Instala um módulo: para cada grupo de instalação escolhe o passo, executa e
//! registra o resultado no lockfile.
//!
//! Um módulo pode ter vários componentes (ex.: o programa + plugins + fonte). O
//! componente principal (grupo `main`) é gravado no lockfile com a chave `<módulo>`;
//! os demais com `<módulo>#<grupo>`.
//!
//! O chamador (CLI/TUI/GUI) decide quando gravar o lockfile; aqui ele só é mutado.

use std::path::Path;

use crate::elevate::Elevator;
use crate::env::{command_exists, Environment, Privilege};
use crate::error::{Error, Result};
use crate::event::{Event, EventSink};
use crate::exec::{run_command, run_command_env};
use crate::lockfile::{now_secs, LockedModule, Lockfile};
use crate::manifest::{BinLink, InstallStep, Manifest, Method, MAIN_GROUP};
use crate::paths::{expand_tilde, file_name_from_url, render_version};
use crate::pkg::{pkg_command, pkg_upgrade_command};
use crate::release;
use crate::sync::pin_matches;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentState {
    Installed,
    Updated,
    Unchanged,
    /// O comando já existia no PATH e não é gerenciado por nós (`provides`).
    External,
    /// Componente `optional` que falhou (o erro vai em `error`).
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComponentOutcome {
    /// Chave no lockfile.
    pub key: String,
    pub group: String,
    pub state: ComponentState,
    pub from: Option<String>,
    pub to: Option<String>,
    /// Motivo de não ter sido atualizado (pin).
    pub skip_reason: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct InstallOutcome {
    pub components: Vec<ComponentOutcome>,
}

pub fn component_key(module: &str, group: &str) -> String {
    if group == MAIN_GROUP {
        module.to_string()
    } else {
        format!("{module}#{group}")
    }
}

/// `pin` vale só para o componente principal, quando ele é um release.
pub fn install_module(
    manifest: &Manifest,
    env: &Environment,
    state_dir: &Path,
    lock: &mut Lockfile,
    elevator: &mut dyn Elevator,
    sink: &mut dyn EventSink,
    pin: Option<&str>,
) -> Result<InstallOutcome> {
    let id = manifest.id.as_str();
    let steps = manifest.select_installs(env);
    if steps.is_empty() {
        return Err(Error::NoCompatibleStep { id: id.to_string() });
    }

    let mut outcome = InstallOutcome::default();
    for step in steps {
        let group = step.group_name().to_string();
        let key = component_key(id, &group);
        let before = lock.modules.get(&key).cloned();
        let mut component = ComponentOutcome {
            key: key.clone(),
            group: group.clone(),
            state: ComponentState::Unchanged,
            from: before.as_ref().and_then(|b| b.version.clone()),
            to: None,
            skip_reason: None,
            error: None,
        };

        // Já existe na máquina e não fomos nós que instalamos: deixa em paz.
        if !step.provides.is_empty()
            && before.is_none()
            && step.provides.iter().all(|c| command_exists(c))
        {
            sink.emit(Event::Skipped {
                module: id.to_string(),
                reason: format!("{} já existe no PATH (não gerenciado pelo hiraku)", step.provides.join(", ")),
            });
            component.state = ComponentState::External;
            outcome.components.push(component);
            continue;
        }

        if group == MAIN_GROUP {
            if let (Some(pin), Method::Release { version, .. }) = (pin, &step.method) {
                if !pin_matches(pin, version) {
                    let reason = format!("fixado em {pin}; o registry oferece {version}");
                    sink.emit(Event::Skipped {
                        module: id.to_string(),
                        reason: reason.clone(),
                    });
                    component.skip_reason = Some(reason);
                    outcome.components.push(component);
                    continue;
                }
            }
        }

        match run_step(id, step, env, state_dir, elevator, sink, before.as_ref()) {
            Ok((mut locked, skipped)) => {
                locked.pin = before.as_ref().and_then(|b| b.pin.clone());
                component.to = locked.version.clone();
                component.state = if skipped {
                    ComponentState::Unchanged
                } else if before.is_none() {
                    ComponentState::Installed
                } else {
                    ComponentState::Updated
                };
                lock.record(&key, locked);
            }
            Err(e) if step.optional => {
                sink.emit(Event::Log {
                    module: id.to_string(),
                    line: format!("aviso: componente '{group}' não instalado: {e}"),
                });
                component.state = ComponentState::Failed;
                component.error = Some(e.to_string());
            }
            Err(e) => return Err(e),
        }
        outcome.components.push(component);
    }

    sink.emit(Event::Finished {
        module: id.to_string(),
    });
    Ok(outcome)
}

/// Devolve o registro para o lockfile e se nada precisou ser feito.
fn run_step(
    id: &str,
    step: &InstallStep,
    env: &Environment,
    state_dir: &Path,
    elevator: &mut dyn Elevator,
    sink: &mut dyn EventSink,
    existing: Option<&LockedModule>,
) -> Result<(LockedModule, bool)> {
    match &step.method {
        Method::Pkg { names, cask } => install_pkg(id, names, *cask, env, elevator, sink, existing),
        Method::Release {
            version,
            url,
            sha256,
            dest,
            bins,
            strip_components,
        } => install_release(
            id,
            version,
            url,
            sha256,
            dest,
            bins,
            *strip_components,
            env,
            state_dir,
            existing,
            sink,
        ),
        Method::Git { url, rev, dest } => install_git(id, url, rev, dest, env, existing, sink),
        Method::Script { .. } => Err(Error::Unsupported(
            "o método `script` ainda não foi implementado".to_string(),
        )),
    }
}

fn install_pkg(
    id: &str,
    names: &std::collections::BTreeMap<String, String>,
    cask: bool,
    env: &Environment,
    elevator: &mut dyn Elevator,
    sink: &mut dyn EventSink,
    existing: Option<&LockedModule>,
) -> Result<(LockedModule, bool)> {
    let pm = env
        .pkg_manager
        .ok_or_else(|| Error::NoCompatibleStep { id: id.to_string() })?;
    let package = names
        .get(pm.as_str())
        .ok_or_else(|| Error::NoCompatibleStep { id: id.to_string() })?;

    // Já instalado por nós: só atualiza onde isso não exige root; os pacotes do
    // sistema ficam a cargo do gerenciador do SO.
    if let Some(existing) = existing.filter(|m| m.method.starts_with("pkg:")) {
        return match pkg_upgrade_command(pm, package, cask) {
            None => {
                sink.emit(Event::Skipped {
                    module: id.to_string(),
                    reason: format!("{package} é atualizado pelo {}", pm.as_str()),
                });
                Ok((existing.clone(), true))
            }
            Some(argv) => {
                sink.emit(Event::Started {
                    module: id.to_string(),
                    what: format!("atualizando {package} via {}", pm.as_str()),
                });
                run_command(id, &argv, sink)?;
                Ok((
                    LockedModule {
                        installed_at: now_secs(),
                        ..existing.clone()
                    },
                    false,
                ))
            }
        };
    }

    let needs_sudo = pm.needs_root() && env.privilege != Privilege::Root;
    if pm.needs_root() && !env.use_root {
        return Err(Error::Unsupported(format!(
            "instalar '{package}' via {} exige root, mas o modo é --user",
            pm.as_str()
        )));
    }
    let argv = pkg_command(pm, package, cask, needs_sudo);
    // Com "lembrar o sudo" (regra NOPASSWD só para o install), não há o que pedir.
    if needs_sudo && !elevator.allows_without_password(&argv[1..]) {
        elevator.ensure_root()?;
    }

    sink.emit(Event::Started {
        module: id.to_string(),
        what: format!("instalando {package} via {}", pm.as_str()),
    });
    run_command(id, &argv, sink)?;

    Ok((
        LockedModule {
            method: format!("pkg:{}", pm.as_str()),
            version: None,
            sha256: None,
            installed_at: now_secs(),
            paths: Vec::new(),
            pin: None,
        },
        false,
    ))
}

#[allow(clippy::too_many_arguments)]
fn install_release(
    id: &str,
    version: &str,
    url_template: &str,
    sha256: &str,
    dest_template: &str,
    bins: &[BinLink],
    strip_components: u32,
    env: &Environment,
    state_dir: &Path,
    existing: Option<&LockedModule>,
    sink: &mut dyn EventSink,
) -> Result<(LockedModule, bool)> {
    if sha256.chars().all(|c| c == '0') {
        return Err(Error::Unsupported(format!(
            "'{id}' usa sha256 de placeholder; atualize version e sha256 no manifest"
        )));
    }

    let home = env.home.as_deref();
    let dest = expand_tilde(dest_template, home)?;
    let url = render_version(url_template, version);

    if let Some(existing) = existing {
        let same = existing.method == "release"
            && existing.version.as_deref() == Some(version)
            && existing
                .sha256
                .as_deref()
                .map_or(false, |s| s.eq_ignore_ascii_case(sha256))
            && dest.exists();
        if same {
            sink.emit(Event::Skipped {
                module: id.to_string(),
                reason: format!("já instalado na versão {version}"),
            });
            return Ok((existing.clone(), true));
        }
    }

    let file_name = file_name_from_url(&url);
    let archive = state_dir
        .join("downloads")
        .join(format!("{id}-{version}-{file_name}"));

    sink.emit(Event::Started {
        module: id.to_string(),
        what: format!("baixando {url}"),
    });
    release::download(id, &url, &archive, sha256, sink)?;

    sink.emit(Event::Log {
        module: id.to_string(),
        line: format!("checksum ok; extraindo em {}", dest.display()),
    });
    let installed = release::install_archive(&archive, &file_name, &dest, strip_components);
    let _ = std::fs::remove_file(&archive);
    installed?;

    let mut link_specs = Vec::with_capacity(bins.len());
    for bin in bins {
        link_specs.push((bin.src.clone(), expand_tilde(&bin.link, home)?));
    }
    let links = release::link_bins(&dest, &link_specs)?;

    let mut paths = vec![dest.display().to_string()];
    paths.extend(links.iter().map(|p| p.display().to_string()));

    Ok((
        LockedModule {
            method: "release".to_string(),
            version: Some(version.to_string()),
            sha256: Some(sha256.to_ascii_lowercase()),
            installed_at: now_secs(),
            paths,
            pin: None,
        },
        false,
    ))
}

fn git(args: &[&str]) -> Vec<String> {
    std::iter::once("git")
        .chain(args.iter().copied())
        .map(str::to_string)
        .collect()
}

/// Clona/atualiza `url` fixado em `rev` (commit completo) em `dest`. Usa
/// `git fetch --depth 1 origin <rev>`, que o GitHub aceita para qualquer commit.
fn install_git(
    id: &str,
    url: &str,
    rev: &str,
    dest_template: &str,
    env: &Environment,
    existing: Option<&LockedModule>,
    sink: &mut dyn EventSink,
) -> Result<(LockedModule, bool)> {
    if rev.chars().all(|c| c == '0') {
        return Err(Error::Unsupported(format!(
            "'{id}' usa rev de placeholder; rode `hiraku registry bump` para fixar um commit"
        )));
    }
    let dest = expand_tilde(dest_template, env.home.as_deref())?;

    if let Some(existing) = existing {
        if existing.method == "git"
            && existing.version.as_deref() == Some(rev)
            && dest.join(".git").exists()
        {
            sink.emit(Event::Skipped {
                module: id.to_string(),
                reason: format!("{} já está em {}", dest.display(), &rev[..rev.len().min(12)]),
            });
            return Ok((existing.clone(), true));
        }
    }
    if !command_exists("git") {
        return Err(Error::Unsupported("git não encontrado no PATH".to_string()));
    }

    let dest_text = dest.display().to_string();
    let d = dest_text.as_str();
    let envs = vec![("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())];
    sink.emit(Event::Started {
        module: id.to_string(),
        what: format!("baixando {url} @ {}", &rev[..rev.len().min(12)]),
    });

    if dest.join(".git").exists() {
        run_command_env(id, &git(&["-C", d, "remote", "set-url", "origin", url]), &envs, sink)?;
    } else {
        let occupied = std::fs::read_dir(&dest)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        if occupied {
            return Err(Error::Unsupported(format!(
                "{dest_text} existe e não é um repositório git; remova-o ou mova-o"
            )));
        }
        std::fs::create_dir_all(&dest).map_err(crate::error::io_err(&dest))?;
        run_command_env(id, &git(&["init", "-q", d]), &envs, sink)?;
        run_command_env(id, &git(&["-C", d, "remote", "add", "origin", url]), &envs, sink)?;
    }
    run_command_env(id, &git(&["-C", d, "fetch", "-q", "--depth", "1", "origin", rev]), &envs, sink)?;
    run_command_env(
        id,
        &git(&["-C", d, "checkout", "-q", "--detach", "--force", "FETCH_HEAD"]),
        &envs,
        sink,
    )?;

    Ok((
        LockedModule {
            method: "git".to_string(),
            version: Some(rev.to_string()),
            sha256: None,
            installed_at: now_secs(),
            paths: vec![dest_text],
            pin: None,
        },
        false,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elevate::NoElevation;
    use crate::env::{Arch, Os, PkgManager};
    use crate::event::VecSink;

    fn env(home: &Path, pm: Option<PkgManager>, use_root: bool) -> Environment {
        Environment {
            os: Os::Linux,
            arch: Some(Arch::X86_64),
            distro: None,
            pkg_manager: pm,
            privilege: if use_root { Privilege::Sudo } else { Privilege::None },
            use_root,
            home: Some(home.to_path_buf()),
        }
    }

    fn release_manifest(sha: &str) -> Manifest {
        let src = r#"
schema = 1
id = "tool"
name = "tool"

[[install]]
when = { os = "linux" }
method = "release"
version = "1.0.0"
url = "https://example.com/tool-{version}.tar.gz"
sha256 = "SHA"
dest = "~/.local/tool"
bins = [{ src = "bin/tool", link = "~/.local/bin/tool" }]
"#
        .replace("SHA", sha);
        Manifest::from_toml_str(&src, "test").unwrap()
    }

    fn pkg_manifest(id: &str) -> Manifest {
        Manifest::from_toml_str(
            &format!("schema = 1\nid = \"{id}\"\nname = \"{id}\"\n[[install]]\nmethod = \"pkg\"\nnames = {{ dnf = \"{id}\" }}\n"),
            "test",
        )
        .unwrap()
    }

    struct Recorder(u32);
    impl Elevator for Recorder {
        fn ensure_root(&mut self) -> Result<()> {
            self.0 += 1;
            Ok(())
        }
    }

    fn locked(method: &str, version: Option<&str>, sha: Option<String>) -> LockedModule {
        LockedModule {
            method: method.to_string(),
            version: version.map(str::to_string),
            sha256: sha,
            installed_at: 7,
            paths: vec![],
            pin: None,
        }
    }

    fn install(manifest: &Manifest, env: &Environment, dir: &Path, lock: &mut Lockfile, pin: Option<&str>) -> Result<InstallOutcome> {
        install_module(manifest, env, dir, lock, &mut NoElevation, &mut VecSink::default(), pin)
    }

    #[test]
    fn placeholder_checksum_is_refused_before_any_network() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = release_manifest(&"0".repeat(64));
        let mut lock = Lockfile::default();
        let err = install(&manifest, &env(dir.path(), None, false), dir.path(), &mut lock, None).unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
        assert!(lock.modules.is_empty());
    }

    #[test]
    fn already_installed_release_is_skipped_and_keeps_its_pin() {
        let dir = tempfile::tempdir().unwrap();
        let sha = "ab".repeat(32);
        let manifest = release_manifest(&sha);
        let env = env(dir.path(), None, false);

        std::fs::create_dir_all(dir.path().join(".local/tool")).unwrap();
        let mut lock = Lockfile::default();
        let mut entry = locked("release", Some("1.0.0"), Some(sha));
        entry.pin = Some("1.*".to_string());
        lock.record("tool", entry);

        let mut sink = VecSink::default();
        let outcome =
            install_module(&manifest, &env, dir.path(), &mut lock, &mut NoElevation, &mut sink, Some("1.*")).unwrap();
        assert!(sink.0.iter().any(|e| matches!(e, Event::Skipped { .. })));
        assert!(!sink.0.iter().any(|e| matches!(e, Event::Started { .. })));
        assert_eq!(outcome.components[0].state, ComponentState::Unchanged);
        assert_eq!(lock.modules["tool"].installed_at, 7);
        assert_eq!(lock.modules["tool"].pin.as_deref(), Some("1.*"));
    }

    #[test]
    fn pin_blocks_a_non_matching_release_without_touching_the_network() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = release_manifest(&"ab".repeat(32));
        let mut lock = Lockfile::default();
        let mut sink = VecSink::default();
        let outcome = install_module(
            &manifest,
            &env(dir.path(), None, false),
            dir.path(),
            &mut lock,
            &mut NoElevation,
            &mut sink,
            Some("0.*"),
        )
        .unwrap();
        assert!(outcome.components[0].skip_reason.as_deref().unwrap().contains("0.*"));
        assert!(!sink.0.iter().any(|e| matches!(e, Event::Started { .. })));
        assert!(lock.modules.is_empty());
    }

    #[test]
    fn no_compatible_step_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = release_manifest(&"ab".repeat(32));
        let mut env = env(dir.path(), None, false);
        env.os = Os::Macos;
        let err = install(&manifest, &env, dir.path(), &mut Lockfile::default(), None).unwrap_err();
        assert!(matches!(err, Error::NoCompatibleStep { .. }));
    }

    #[test]
    fn pkg_in_user_mode_is_refused_and_never_elevates() {
        let dir = tempfile::tempdir().unwrap();
        let mut elevator = Recorder(0);
        let err = install_module(
            &pkg_manifest("tmux"),
            &env(dir.path(), Some(PkgManager::Dnf), false),
            dir.path(),
            &mut Lockfile::default(),
            &mut elevator,
            &mut VecSink::default(),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
        assert_eq!(elevator.0, 0);
    }

    #[test]
    fn already_installed_system_package_is_skipped_without_elevating() {
        let dir = tempfile::tempdir().unwrap();
        let mut lock = Lockfile::default();
        lock.record("tmux", locked("pkg:dnf", None, None));
        let mut elevator = Recorder(0);
        let mut sink = VecSink::default();
        let outcome = install_module(
            &pkg_manifest("tmux"),
            &env(dir.path(), Some(PkgManager::Dnf), true),
            dir.path(),
            &mut lock,
            &mut elevator,
            &mut sink,
            None,
        )
        .unwrap();
        assert_eq!(elevator.0, 0);
        assert!(sink.0.iter().any(|e| matches!(e, Event::Skipped { .. })));
        assert_eq!(outcome.components[0].state, ComponentState::Unchanged);
        assert_eq!(lock.modules["tmux"].installed_at, 7);
    }

    #[test]
    fn script_method_is_not_implemented_yet() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = Manifest::from_toml_str(
            &format!(
                "schema = 1\nid = \"s\"\nname = \"s\"\n[[install]]\nmethod = \"script\"\nurl = \"https://example.com/i.sh\"\nsha256 = \"{}\"\n",
                "ab".repeat(32)
            ),
            "test",
        )
        .unwrap();
        let err = install(&manifest, &env(dir.path(), None, false), dir.path(), &mut Lockfile::default(), None).unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
    }

    // ---- vários componentes, provides e optional

    #[cfg(unix)]
    fn multi_manifest(second_optional: bool) -> Manifest {
        let src = r#"
schema = 1
id = "multi"
name = "multi"

[[install]]
when = { os = "linux" }
method = "pkg"
names = { dnf = "multi" }
provides = ["sh"]

[[install]]
group = "extra"
OPTIONAL
method = "release"
version = "1.0.0"
url = "https://example.com/extra-{version}.tar.gz"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
dest = "~/.local/extra"
"#
        .replace("OPTIONAL", if second_optional { "optional = true" } else { "" });
        Manifest::from_toml_str(&src, "test").unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn provides_skips_a_command_that_already_exists_and_optional_failures_are_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let mut lock = Lockfile::default();
        let mut sink = VecSink::default();
        let outcome = install_module(
            &multi_manifest(true),
            &env(dir.path(), Some(PkgManager::Dnf), true),
            dir.path(),
            &mut lock,
            &mut NoElevation,
            &mut sink,
            None,
        )
        .unwrap();

        assert_eq!(outcome.components.len(), 2);
        assert_eq!(outcome.components[0].state, ComponentState::External, "`sh` já existe");
        assert_eq!(outcome.components[1].group, "extra");
        assert_eq!(outcome.components[1].state, ComponentState::Failed);
        assert!(outcome.components[1].error.is_some());
        assert!(sink.0.iter().any(|e| matches!(e, Event::Log { line, .. } if line.contains("aviso"))));
        assert!(lock.modules.is_empty(), "nada foi instalado, nada é registrado");
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_required_component_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = install(
            &multi_manifest(false),
            &env(dir.path(), Some(PkgManager::Dnf), true),
            dir.path(),
            &mut Lockfile::default(),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
    }

    #[cfg(unix)]
    #[test]
    fn provides_does_not_apply_once_we_manage_the_component() {
        let dir = tempfile::tempdir().unwrap();
        let mut lock = Lockfile::default();
        lock.record("multi", locked("pkg:dnf", None, None));
        let outcome = install_module(
            &multi_manifest(true),
            &env(dir.path(), Some(PkgManager::Dnf), true),
            dir.path(),
            &mut lock,
            &mut NoElevation,
            &mut VecSink::default(),
            None,
        )
        .unwrap();
        // gerenciado por nós: segue o fluxo normal (pacote do sistema fica com o SO)
        assert_eq!(outcome.components[0].state, ComponentState::Unchanged);
    }

    #[test]
    fn component_keys() {
        assert_eq!(component_key("vim", "main"), "vim");
        assert_eq!(component_key("vim", "plugin-airline"), "vim#plugin-airline");
    }

    // ---- git (precisa do binário `git`; sem ele o teste é ignorado em silêncio)

    fn run_git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit(dir: &Path, file: &str, content: &str) -> String {
        std::fs::write(dir.join(file), content).unwrap();
        run_git(dir, &["add", "."]);
        run_git(
            dir,
            &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", content],
        );
        run_git(dir, &["rev-parse", "HEAD"])
    }

    #[test]
    fn git_component_installs_updates_and_is_idempotent() {
        if !command_exists("git") {
            eprintln!("git ausente: teste ignorado");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let origin = tmp.path().join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        run_git(&origin, &["init", "-q"]);
        run_git(&origin, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
        let rev1 = commit(&origin, "plugin.vim", "v1");
        let url = format!("file://{}", origin.display());
        let dest = tmp.path().join("plugins/opt/p");
        let environment = env(tmp.path(), None, false);
        let dest_text = dest.display().to_string();

        let (entry, skipped) =
            install_git("vim", &url, &rev1, &dest_text, &environment, None, &mut VecSink::default()).unwrap();
        assert!(!skipped);
        assert_eq!(std::fs::read_to_string(dest.join("plugin.vim")).unwrap(), "v1");
        assert_eq!(entry.version.as_deref(), Some(rev1.as_str()));

        let (_, skipped) =
            install_git("vim", &url, &rev1, &dest_text, &environment, Some(&entry), &mut VecSink::default()).unwrap();
        assert!(skipped, "mesma revisão: nada a fazer");

        let rev2 = commit(&origin, "plugin.vim", "v2");
        let (entry2, skipped) =
            install_git("vim", &url, &rev2, &dest_text, &environment, Some(&entry), &mut VecSink::default()).unwrap();
        assert!(!skipped);
        assert_eq!(std::fs::read_to_string(dest.join("plugin.vim")).unwrap(), "v2");
        assert_eq!(entry2.version.as_deref(), Some(rev2.as_str()));
    }

    #[test]
    fn git_component_refuses_a_placeholder_rev_before_touching_anything() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("plugin");
        let err = install_git(
            "vim",
            "https://github.com/a/b",
            &"0".repeat(40),
            &dest.display().to_string(),
            &env(tmp.path(), None, false),
            None,
            &mut VecSink::default(),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
        assert!(!dest.exists());
    }

    #[test]
    fn git_component_refuses_a_non_empty_directory_that_is_not_a_repo() {
        if !command_exists("git") {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("ocupado");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("arquivo-do-usuario"), "x").unwrap();
        let err = install_git(
            "vim",
            "file:///nao/importa",
            &"a".repeat(40),
            &dest.display().to_string(),
            &env(tmp.path(), None, false),
            None,
            &mut VecSink::default(),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
        assert!(dest.join("arquivo-do-usuario").exists());
    }
}
