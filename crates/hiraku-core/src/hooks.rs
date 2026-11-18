//! Hooks: scripts ou comandos do módulo para o que o modelo declarativo não cobre
//! (fc-cache, gsettings, bootstrap de plugins...).
//!
//! Roda sem shell (`sh script.sh`, PowerShell para `.ps1`, ou `argv` direto), com a
//! saída transmitida como eventos e o contexto em variáveis `HIRAKU_*`:
//!
//! `HIRAKU_MODULE`, `HIRAKU_MODULE_DIR`, `HIRAKU_MODE` (user|system), `HIRAKU_OS`,
//! `HIRAKU_HOME`, `HIRAKU_BIN_DIR`, `HIRAKU_STATE_DIR`, `HIRAKU_CHANGED` (1|0),
//! `HIRAKU_UNATTENDED` (1|0), `HIRAKU_PRIVILEGE` (root|sudo|none) e uma
//! `HIRAKU_OPT_<NOME>` por opção do módulo.
//!
//! Regra para autores: um hook NUNCA pode esperar senha. Se precisar de root, use
//! `sudo -n` e pule o passo quando falhar (a TUI está com o terminal ocupado).

use std::collections::BTreeMap;
use std::path::Path;

use crate::env::{command_exists, Environment, Privilege};
use crate::error::{Error, Result};
use crate::event::{Event, EventSink};
use crate::exec::run_command_env;
use crate::lockfile::{now_secs, LockedModule, Lockfile};
use crate::manifest::{Hook, HookTrigger, Manifest};
use crate::release::{sha256_file, sha256_hex_of};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookOutcome {
    Ran,
    Skipped(String),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookReport {
    pub name: String,
    pub outcome: HookOutcome,
    pub optional: bool,
}

pub struct HookContext<'a> {
    pub manifest: &'a Manifest,
    pub module_dir: &'a Path,
    pub env: &'a Environment,
    pub state_dir: &'a Path,
    /// Valores finais das opções (`configs::resolve_options`).
    pub vars: &'a BTreeMap<String, String>,
    /// O módulo instalou algo ou alterou configs neste sync.
    pub changed: bool,
    pub unattended: bool,
}

/// Chave do estado de um hook `once` no lockfile.
pub fn hook_key(module: &str, name: &str) -> String {
    format!("{module}!{name}")
}

/// Identifica "a versão" do hook: conteúdo do script (ou argv) mais os argumentos.
fn fingerprint(hook: &Hook, module_dir: &Path) -> Result<String> {
    let base = match (&hook.script, &hook.argv) {
        (Some(script), _) => sha256_file(&module_dir.join(script))?,
        (None, Some(argv)) => argv.join("\0"),
        (None, None) => String::new(),
    };
    Ok(sha256_hex_of(format!("{base}\0{}", hook.args.join("\0")).as_bytes()))
}

fn build_argv(hook: &Hook, module_dir: &Path) -> Result<Vec<String>> {
    let mut argv: Vec<String> = Vec::new();
    if let Some(script) = &hook.script {
        let path = module_dir.join(script);
        if !path.is_file() {
            return Err(Error::Unsupported(format!(
                "script do hook não existe: {}",
                path.display()
            )));
        }
        if script.ends_with(".ps1") {
            for part in ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File"] {
                argv.push(part.to_string());
            }
        } else {
            argv.push("sh".to_string());
        }
        argv.push(path.display().to_string());
    } else if let Some(command) = &hook.argv {
        argv.extend(command.iter().cloned());
    }
    argv.extend(hook.args.iter().cloned());
    Ok(argv)
}

fn env_name(option: &str) -> String {
    option
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
        .collect()
}

pub fn hook_env(ctx: &HookContext) -> Vec<(String, String)> {
    let home = ctx.env.home.as_ref();
    let mode = if ctx.env.use_root { "system" } else { "user" };
    let privilege = match ctx.env.privilege {
        Privilege::Root => "root",
        Privilege::Sudo => "sudo",
        Privilege::None => "none",
    };
    let flag = |on: bool| String::from(if on { "1" } else { "0" });

    let mut vars: Vec<(String, String)> = vec![
        ("HIRAKU_MODULE".to_string(), ctx.manifest.id.clone()),
        ("HIRAKU_MODULE_DIR".to_string(), ctx.module_dir.display().to_string()),
        ("HIRAKU_MODE".to_string(), mode.to_string()),
        ("HIRAKU_OS".to_string(), ctx.env.os.as_str().to_string()),
        (
            "HIRAKU_HOME".to_string(),
            home.map(|h| h.display().to_string()).unwrap_or_default(),
        ),
        (
            "HIRAKU_BIN_DIR".to_string(),
            home.map(|h| h.join(".local").join("bin").display().to_string())
                .unwrap_or_default(),
        ),
        ("HIRAKU_STATE_DIR".to_string(), ctx.state_dir.display().to_string()),
        ("HIRAKU_CHANGED".to_string(), flag(ctx.changed)),
        ("HIRAKU_UNATTENDED".to_string(), flag(ctx.unattended)),
        ("HIRAKU_PRIVILEGE".to_string(), privilege.to_string()),
    ];
    for (name, value) in ctx.vars {
        vars.push((format!("HIRAKU_OPT_{}", env_name(name)), value.clone()));
    }
    vars
}

/// `true` se `hook` precisa de `sudo -v` antes do sync: usa root, roda neste ambiente
/// e ainda está pendente (`always`, ou `once` ainda não executado nesta versão).
pub fn root_hook_pending(
    manifest: &Manifest,
    module_dir: &Path,
    env: &Environment,
    lock: &Lockfile,
) -> bool {
    manifest.hook.iter().any(|hook| {
        if !hook.root || !hook.when.matches(env) || !hook.needs.iter().all(|c| command_exists(c)) {
            return false;
        }
        match hook.on {
            HookTrigger::Always => true,
            HookTrigger::Change => false,
            HookTrigger::Once => match fingerprint(hook, module_dir) {
                Ok(fp) => lock
                    .modules
                    .get(&hook_key(&manifest.id, &hook.name))
                    .map_or(true, |m| m.version.as_deref() != Some(fp.as_str())),
                Err(_) => false,
            },
        }
    })
}

/// Executa os hooks do módulo, em ordem. Hooks que não se aplicam (outra plataforma,
/// ou sem mudança/já executados) não aparecem no resultado.
pub fn run_hooks(ctx: &HookContext, lock: &mut Lockfile, sink: &mut dyn EventSink) -> Vec<HookReport> {
    let id = ctx.manifest.id.as_str();
    let envs = hook_env(ctx);
    let mut reports = Vec::new();

    for hook in &ctx.manifest.hook {
        let make = |outcome: HookOutcome| HookReport {
            name: hook.name.clone(),
            outcome,
            optional: hook.optional,
        };
        if !hook.when.matches(ctx.env) {
            continue;
        }
        if let Some(missing) = hook.needs.iter().find(|c| !command_exists(c)) {
            reports.push(make(HookOutcome::Skipped(format!("falta o comando '{missing}'"))));
            continue;
        }

        let key = hook_key(id, &hook.name);
        let print = match fingerprint(hook, ctx.module_dir) {
            Ok(print) => print,
            Err(e) => {
                reports.push(make(HookOutcome::Failed(e.to_string())));
                continue;
            }
        };
        let should_run = match hook.on {
            HookTrigger::Always => true,
            HookTrigger::Change => ctx.changed,
            HookTrigger::Once => lock
                .modules
                .get(&key)
                .map_or(true, |m| m.version.as_deref() != Some(print.as_str())),
        };
        if !should_run {
            continue;
        }

        let argv = match build_argv(hook, ctx.module_dir) {
            Ok(argv) => argv,
            Err(e) => {
                reports.push(make(HookOutcome::Failed(e.to_string())));
                continue;
            }
        };
        sink.emit(Event::Started {
            module: id.to_string(),
            what: format!("hook {}", hook.name),
        });
        match run_command_env(id, &argv, &envs, sink) {
            Ok(()) => {
                if hook.on == HookTrigger::Once {
                    lock.record(
                        &key,
                        LockedModule {
                            method: "hook".to_string(),
                            version: Some(print),
                            sha256: None,
                            installed_at: now_secs(),
                            paths: Vec::new(),
                            pin: None,
                        },
                    );
                }
                reports.push(make(HookOutcome::Ran));
            }
            Err(e) => {
                if hook.optional {
                    sink.emit(Event::Log {
                        module: id.to_string(),
                        line: format!("aviso: hook '{}' falhou: {e}", hook.name),
                    });
                }
                reports.push(make(HookOutcome::Failed(e.to_string())));
            }
        }
    }
    reports
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::env::{Arch, Os};
    use crate::event::VecSink;
    use std::fs;
    use std::path::PathBuf;

    struct Fx {
        _tmp: tempfile::TempDir,
        home: PathBuf,
        module_dir: PathBuf,
        state_dir: PathBuf,
        env: Environment,
    }

    fn fx() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let module_dir = tmp.path().join("mod");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(module_dir.join("hooks")).unwrap();
        let env = Environment {
            os: Os::Linux,
            arch: Some(Arch::X86_64),
            distro: None,
            pkg_manager: None,
            privilege: Privilege::None,
            use_root: false,
            home: Some(home.clone()),
        };
        Fx { state_dir: tmp.path().join("state"), _tmp: tmp, home, module_dir, env }
    }

    fn manifest(hooks: &str) -> Manifest {
        Manifest::from_toml_str(&format!("schema = 1\nid = \"mod\"\nname = \"mod\"\n{hooks}"), "t").unwrap()
    }

    fn run(f: &Fx, m: &Manifest, changed: bool, vars: &BTreeMap<String, String>, lock: &mut Lockfile) -> Vec<HookReport> {
        run_hooks(
            &HookContext {
                manifest: m,
                module_dir: &f.module_dir,
                env: &f.env,
                state_dir: &f.state_dir,
                vars,
                changed,
                unattended: true,
            },
            lock,
            &mut VecSink::default(),
        )
    }

    fn script(f: &Fx, name: &str, body: &str) {
        fs::write(f.module_dir.join("hooks").join(name), body).unwrap();
    }

    const ONE: &str = "[[hook]]\nname = \"h\"\nscript = \"hooks/h.sh\"\n";

    #[test]
    fn change_trigger_runs_only_when_something_changed() {
        let f = fx();
        script(&f, "h.sh", "echo ran >> \"$HIRAKU_HOME/out.txt\"\n");
        let m = manifest(ONE);
        let mut lock = Lockfile::default();
        let vars = BTreeMap::new();

        assert!(run(&f, &m, false, &vars, &mut lock).is_empty(), "nada mudou: nem aparece");
        assert!(!f.home.join("out.txt").exists());
        let reports = run(&f, &m, true, &vars, &mut lock);
        assert_eq!(reports[0].outcome, HookOutcome::Ran);
        assert_eq!(fs::read_to_string(f.home.join("out.txt")).unwrap().trim(), "ran");
        assert!(lock.modules.is_empty(), "só `once` grava estado");
    }

    #[test]
    fn hooks_receive_the_context_in_environment_variables() {
        let f = fx();
        script(
            &f,
            "h.sh",
            "echo \"$HIRAKU_MODULE|$HIRAKU_MODE|$HIRAKU_OS|$HIRAKU_CHANGED|$HIRAKU_UNATTENDED|$HIRAKU_PRIVILEGE|$HIRAKU_OPT_BLUR_RADIUS|$HIRAKU_BIN_DIR\" > \"$HIRAKU_HOME/ctx.txt\"\n",
        );
        let m = manifest(ONE);
        let mut vars = BTreeMap::new();
        vars.insert("blur-radius".to_string(), "12".to_string());
        run(&f, &m, true, &vars, &mut Lockfile::default());
        let text = fs::read_to_string(f.home.join("ctx.txt")).unwrap();
        assert_eq!(
            text.trim(),
            format!("mod|user|linux|1|1|none|12|{}", f.home.join(".local/bin").display())
        );
    }

    #[test]
    fn always_runs_every_time() {
        let f = fx();
        script(&f, "h.sh", "echo x >> \"$HIRAKU_HOME/n.txt\"\n");
        let m = manifest(&format!("{ONE}on = \"always\"\n"));
        let mut lock = Lockfile::default();
        run(&f, &m, false, &BTreeMap::new(), &mut lock);
        run(&f, &m, false, &BTreeMap::new(), &mut lock);
        assert_eq!(fs::read_to_string(f.home.join("n.txt")).unwrap().lines().count(), 2);
    }

    #[test]
    fn once_runs_once_per_script_version() {
        let f = fx();
        script(&f, "h.sh", "echo a >> \"$HIRAKU_HOME/o.txt\"\n");
        let m = manifest(&format!("{ONE}on = \"once\"\n"));
        let mut lock = Lockfile::default();
        let vars = BTreeMap::new();

        assert_eq!(run(&f, &m, false, &vars, &mut lock)[0].outcome, HookOutcome::Ran);
        assert!(run(&f, &m, false, &vars, &mut lock).is_empty(), "já executado");
        assert_eq!(lock.modules["mod!h"].method, "hook");

        // o script muda: roda de novo
        script(&f, "h.sh", "echo b >> \"$HIRAKU_HOME/o.txt\"\n");
        assert_eq!(run(&f, &m, false, &vars, &mut lock)[0].outcome, HookOutcome::Ran);
        assert_eq!(fs::read_to_string(f.home.join("o.txt")).unwrap(), "a\nb\n");
    }

    #[test]
    fn a_failed_once_hook_is_not_recorded_so_it_retries() {
        let f = fx();
        script(&f, "h.sh", "exit 3\n");
        let m = manifest(&format!("{ONE}on = \"once\"\n"));
        let mut lock = Lockfile::default();
        let reports = run(&f, &m, false, &BTreeMap::new(), &mut lock);
        assert!(matches!(reports[0].outcome, HookOutcome::Failed(_)));
        assert!(lock.modules.is_empty());
        assert_eq!(run(&f, &m, false, &BTreeMap::new(), &mut lock).len(), 1, "tenta de novo");
    }

    #[test]
    fn missing_needs_skips_with_a_reason() {
        let f = fx();
        script(&f, "h.sh", "echo x > \"$HIRAKU_HOME/never.txt\"\n");
        let m = manifest(&format!("{ONE}needs = [\"comando-inexistente-hiraku\"]\n"));
        let reports = run(&f, &m, true, &BTreeMap::new(), &mut Lockfile::default());
        match &reports[0].outcome {
            HookOutcome::Skipped(reason) => assert!(reason.contains("comando-inexistente-hiraku")),
            other => panic!("esperava Skipped, veio {other:?}"),
        }
        assert!(!f.home.join("never.txt").exists());
    }

    #[test]
    fn hooks_for_other_platforms_do_not_appear() {
        let f = fx();
        script(&f, "h.sh", "true\n");
        let m = manifest(&format!("{ONE}when = {{ os = \"windows\" }}\n"));
        assert!(run(&f, &m, true, &BTreeMap::new(), &mut Lockfile::default()).is_empty());
    }

    #[test]
    fn failures_are_reported_and_later_hooks_still_run() {
        let f = fx();
        script(&f, "bad.sh", "exit 7\n");
        script(&f, "good.sh", "echo ok > \"$HIRAKU_HOME/good.txt\"\n");
        let m = manifest(
            "[[hook]]\nname = \"bad\"\nscript = \"hooks/bad.sh\"\noptional = true\n\n[[hook]]\nname = \"good\"\nscript = \"hooks/good.sh\"\n",
        );
        let reports = run(&f, &m, true, &BTreeMap::new(), &mut Lockfile::default());
        assert_eq!(reports.len(), 2);
        assert!(matches!(reports[0].outcome, HookOutcome::Failed(_)));
        assert!(reports[0].optional);
        assert_eq!(reports[1].outcome, HookOutcome::Ran);
        assert!(f.home.join("good.txt").exists());
    }

    #[test]
    fn argv_hooks_run_without_a_shell() {
        let f = fx();
        let m = manifest("[[hook]]\nname = \"a\"\nargv = [\"sh\", \"-c\", \"echo $0 $1 > \\\"$HIRAKU_HOME/argv.txt\\\"\"]\nargs = [\"um\", \"dois\"]\n");
        let reports = run(&f, &m, true, &BTreeMap::new(), &mut Lockfile::default());
        assert_eq!(reports[0].outcome, HookOutcome::Ran);
        assert_eq!(fs::read_to_string(f.home.join("argv.txt")).unwrap().trim(), "um dois");
    }

    #[test]
    fn a_missing_script_is_a_failure_not_a_panic() {
        let f = fx();
        let m = manifest(ONE);
        let reports = run(&f, &m, true, &BTreeMap::new(), &mut Lockfile::default());
        assert!(matches!(reports[0].outcome, HookOutcome::Failed(_)));
    }

    #[test]
    fn root_hook_pending_follows_trigger_and_state() {
        let f = fx();
        script(&f, "h.sh", "true\n");
        let once = manifest(&format!("{ONE}on = \"once\"\nroot = true\n"));
        let mut lock = Lockfile::default();
        assert!(root_hook_pending(&once, &f.module_dir, &f.env, &lock));
        run(&f, &once, false, &BTreeMap::new(), &mut lock);
        assert!(!root_hook_pending(&once, &f.module_dir, &f.env, &lock), "já executado");

        let change = manifest(&format!("{ONE}root = true\n"));
        assert!(!root_hook_pending(&change, &f.module_dir, &f.env, &Lockfile::default()));
        let no_root = manifest(&format!("{ONE}on = \"once\"\n"));
        assert!(!root_hook_pending(&no_root, &f.module_dir, &f.env, &Lockfile::default()));
    }
}
