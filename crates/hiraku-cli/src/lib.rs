//! `hiraku`: por enquanto só os subcomandos de diagnóstico da fatia vertical.
//! `env`, `validate` e `plan` já usam o core exatamente como a TUI e a GUI usarão.

use std::path::PathBuf;
use std::process::ExitCode;

use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::io::Write;

use hiraku_core::manifest::{Manifest, Method, OptionDef};
use hiraku_core::bump::{self, NetResolver};
use hiraku_core::configs::{self, FirstApply, Outcome, Resolution};
use hiraku_core::configstate::state_path;
use hiraku_core::regsource::{self, Source, UpdateOutcome, UreqFetcher};
use hiraku_core::schedule::{self, ProcessRunner, ScheduleDirs};
use hiraku_core::settings::{settings_path, Mode, Settings};
use hiraku_core::sync::{self, SyncContext, SyncLock, SyncReport};
use hiraku_core::verify::MinisignVerifier;
use hiraku_core::{Elevator, NonInteractiveSudo};
use hiraku_core::{
    installer, paths, registry, ConfigState, Environment, Event, Lockfile, SudoElevator,
};

const HELP: &str = "\
hiraku (fatia vertical)

USO:
  hiraku                              abre a TUI: assistente na 1ª vez, depois o hub (também: hiraku tui)
  hiraku env                          mostra o ambiente detectado
  hiraku validate [--registry DIR]    valida todos os manifests
  hiraku plan <módulo> [--user|--system] [--registry DIR]
                                      mostra o que seria instalado (nada é executado)
  hiraku install <módulo> [--user|--system] [--registry DIR]
                                      instala de fato e grava o lockfile
  hiraku status                       lista o que o hiraku instalou (lockfile)
  hiraku configure <módulo> [--set chave=valor]... [--first-apply backup|pending]
                                      aplica as configs com merge de 3 vias
  hiraku conflicts                    lista configs com versão nova pendente
  hiraku diff <arquivo>               mostra o diff (atual -> versão nova)
  hiraku resolve <arquivo> keep|new   resolve um conflito (keep = manter o meu)

  hiraku enable|disable <módulo>...   escolhe quais módulos o sync mantém em dia
  hiraku set <módulo> chave=valor     define uma opção de módulo (ex.: style=solid)
  hiraku pin <módulo> <versão>        fixa a versão do binário (ex.: 0.10.*); unpin remove
  hiraku mode user|system|auto        define o modo de instalação padrão
  hiraku settings                     mostra as preferências
  hiraku sync [--unattended] [--no-registry-update] [--first-apply backup|pending]
                                      atualiza registry, instala/atualiza e configura tudo
  hiraku report                       mostra o relatório do último sync
  hiraku schedule install [--every 6h]|remove|status
                                      agenda o sync (systemd --user, launchd, Task Scheduler)
  hiraku registry bump [módulo]... [--registry DIR] [--check]
                                      (mantenedor do registry) busca as versões mais novas,
                                      baixa, calcula o sha256 e reescreve os manifests
  hiraku registry info                mostra qual registry está ativo (embutido ou remoto)
  hiraku registry update [--url URL] [--pubkey CHAVE]
                                      baixa o registry remoto, verifica a assinatura e ativa

Sem --registry (nem $HIRAKU_REGISTRY, que aponta para uma pasta modules/ local, útil em
desenvolvimento), os manifests vêm do registry ativo: o remoto assinado, se for mais novo
que o embutido no binário.
";

/// Ponto de entrada do binário `hiraku` (e do app desktop quando recebe argumentos,
/// como o `sync --unattended` do agendador).
pub fn main_with(args: &[String]) -> ExitCode {
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("erro: {message}");
            ExitCode::FAILURE
        }
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("env") => cmd_env(),
        Some("validate") => cmd_validate(&args[1..]),
        Some("plan") => cmd_plan(&args[1..]),
        Some("install") => cmd_install(&args[1..]),
        Some("status") => cmd_status(),
        Some("configure") => cmd_configure(&args[1..]),
        Some("conflicts") => cmd_conflicts(),
        Some("diff") => cmd_diff(&args[1..]),
        Some("resolve") => cmd_resolve(&args[1..]),
        Some("registry") => cmd_registry(&args[1..]),
        Some("enable") => cmd_enable(&args[1..], true),
        Some("disable") => cmd_enable(&args[1..], false),
        Some("set") => cmd_set(&args[1..]),
        Some("pin") => cmd_pin(&args[1..], true),
        Some("unpin") => cmd_pin(&args[1..], false),
        Some("mode") => cmd_mode(&args[1..]),
        Some("settings") => cmd_settings(),
        Some("sync") => cmd_sync(&args[1..]),
        Some("report") => cmd_report(),
        Some("schedule") => cmd_schedule(&args[1..]),
        Some("tui") => cmd_tui(&args[1..]),
        // Sem argumentos num terminal interativo abre a TUI; em script, mostra a ajuda.
        None if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() => cmd_tui(&[]),
        _ => {
            print!("{HELP}");
            Ok(())
        }
    }
}

#[derive(Default)]
struct Flags {
    registry: Option<PathBuf>,
    user: bool,
    system: bool,
    set: BTreeMap<String, String>,
    first_apply: Option<String>,
    unattended: bool,
    check: bool,
    no_registry_update: bool,
    every: Option<String>,
    url: Option<String>,
    pubkeys: Vec<String>,
    positional: Vec<String>,
}

fn parse_flags(args: &[String]) -> Result<Flags, String> {
    let mut flags = Flags::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--registry" => {
                let dir = iter.next().ok_or("--registry exige um diretório")?;
                flags.registry = Some(PathBuf::from(dir));
            }
            "--set" => {
                let pair = iter.next().ok_or("--set exige chave=valor")?;
                let (key, value) = pair.split_once('=').ok_or("--set espera chave=valor")?;
                flags.set.insert(key.to_string(), value.to_string());
            }
            "--first-apply" => {
                let mode = iter.next().ok_or("--first-apply exige backup ou pending")?;
                flags.first_apply = Some(mode.clone());
            }
            "--unattended" => flags.unattended = true,
            "--check" => flags.check = true,
            "--no-registry-update" => flags.no_registry_update = true,
            "--every" => {
                let every = iter.next().ok_or("--every exige um intervalo (ex.: 6h)")?;
                flags.every = Some(every.clone());
            }
            "--url" => {
                let url = iter.next().ok_or("--url exige um endereço https")?;
                flags.url = Some(url.clone());
            }
            "--pubkey" => {
                let key = iter.next().ok_or("--pubkey exige uma chave minisign (base64)")?;
                flags.pubkeys.push(key.clone());
            }
            "--user" => flags.user = true,
            "--system" => flags.system = true,
            other if other.starts_with("--") => return Err(format!("flag desconhecida: {other}")),
            other => flags.positional.push(other.to_string()),
        }
    }
    if flags.user && flags.system {
        return Err("use --user ou --system, não os dois".to_string());
    }
    Ok(flags)
}

/// Pasta `modules/` a usar: --registry/$HIRAKU_REGISTRY (desenvolvimento) ou o registry ativo.
fn registry_dir(flags: &Flags) -> Result<PathBuf, String> {
    if let Some(dir) = flags
        .registry
        .clone()
        .or_else(|| std::env::var_os("HIRAKU_REGISTRY").map(PathBuf::from))
    {
        return Ok(dir);
    }
    let env = Environment::detect().map_err(|e| e.to_string())?;
    let state_dir =
        paths::state_dir(&env).ok_or("não foi possível determinar o diretório de estado")?;
    let active = regsource::resolve_active(&regsource::embedded_files(), &state_dir)
        .map_err(|e| e.to_string())?;
    if let Some(note) = &active.note {
        eprintln!("aviso: {note}");
    }
    Ok(active.modules_dir())
}

fn cmd_env() -> Result<(), String> {
    let env = Environment::detect().map_err(|e| e.to_string())?;
    println!("os:           {}", env.os.as_str());
    println!("arch:         {:?}", env.arch);
    println!("distro:       {}", env.distro.as_deref().unwrap_or("-"));
    println!(
        "pkg manager:  {}",
        env.pkg_manager.map_or("-", |pm| pm.as_str())
    );
    println!("privilégio:   {:?}", env.privilege);
    println!("modo padrão:  {}", if env.use_root { "system" } else { "user" });
    println!("home:         {}", env.home.as_deref().map_or("-".into(), |p| p.display().to_string()));
    Ok(())
}

fn cmd_validate(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args)?;
    let dir = registry_dir(&flags)?;
    let manifests = registry::load_dir(&dir).map_err(|e| e.to_string())?;
    let mut warnings = 0;
    for m in &manifests {
        println!("ok  {} ({} passos de instalação, {} configs)", m.id, m.install.len(), m.config.len());
        for w in m.warnings() {
            warnings += 1;
            println!("aviso: {w}");
        }
    }
    println!("{} módulos válidos em {}", manifests.len(), dir.display());
    if warnings > 0 {
        println!("{warnings} aviso(s): resolva antes de publicar.");
    }
    Ok(())
}

fn cmd_plan(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args)?;
    let id = flags.positional.first().ok_or("informe o módulo: hiraku plan <módulo>")?;
    let manifests = registry::load_dir(&registry_dir(&flags)?).map_err(|e| e.to_string())?;
    let manifest: &Manifest = manifests
        .iter()
        .find(|m| &m.id == id)
        .ok_or_else(|| format!("módulo '{id}' não existe no registry"))?;

    let mut env = Environment::detect().map_err(|e| e.to_string())?;
    if flags.user {
        env = env.user_mode();
    } else if flags.system {
        env = env.system_mode();
    }

    println!("módulo: {} ({})", manifest.name, manifest.id);
    if !manifest.supports(env.os) {
        return Err(format!("'{}' não suporta {}", manifest.id, env.os.as_str()));
    }
    println!("modo:   {}", if env.use_root { "system" } else { "user" });

    for (key, option) in &manifest.options {
        match option {
            OptionDef::Choice { values, default, .. } => {
                println!("opção:  {key} = {default}  (valores: {})", values.join(", "));
            }
            OptionDef::Bool { default, .. } => println!("opção:  {key} = {default}"),
            OptionDef::Number { min, max, default, .. } => {
                println!("opção:  {key} = {default}  (de {min} a {max})");
            }
            OptionDef::Text { default, .. } => println!("opção:  {key} = {default:?}"),
        }
    }

    let steps = manifest.select_installs(&env);
    if steps.is_empty() {
        println!("instalar: nenhum passo compatível com este ambiente");
    }
    for step in steps {
        let tags = format!(
            "{}{}{}",
            if step.optional { " opcional" } else { "" },
            if step.provides.is_empty() {
                String::new()
            } else {
                format!(" (pula se {} já existir)", step.provides.join(", "))
            },
            step.bump.as_ref().map_or("", |_| " [bump]"),
        );
        println!("instalar: [{}] {}{tags}", step.group_name(), describe(&step.method));
        if step.needs_root(&env) {
            println!("          (precisa de root: a TUI vai pedir `sudo -v`)");
        }
    }

    for entry in &manifest.config {
        let dest = entry.dest.resolve(env.os).unwrap_or("(sem destino neste SO)");
        println!(
            "config:   {} -> {dest}  [{:?}, backup={}, template={}]",
            entry.src, entry.mode, entry.backup, entry.template
        );
    }
    Ok(())
}

fn describe(method: &Method) -> String {
    match method {
        Method::Pkg { names, cask } => {
            let list: Vec<String> = names.iter().map(|(k, v)| format!("{k}={v}")).collect();
            format!("pkg{} ({})", if *cask { " (cask)" } else { "" }, list.join(", "))
        }
        Method::Release { version, url, dest, strip_components, .. } => {
            let strip = if *strip_components > 0 {
                format!(" (strip {strip_components})")
            } else {
                String::new()
            };
            format!("release {version}: {url} -> {dest}{strip}")
        }
        Method::Git { url, rev, dest } => format!("git {url} @ {} -> {dest}", &rev[..rev.len().min(12)]),
        Method::Script { url, .. } => format!("script {url}"),
    }
}

fn print_event(event: &Event) {
    match event {
        Event::Started { module, what } => println!("▶ {module}: {what}"),
        Event::Progress { module, done, total } => {
            let mib = |n: u64| n as f64 / (1024.0 * 1024.0);
            match total {
                Some(t) => print!("\r  {module}: {:.1}/{:.1} MiB", mib(*done), mib(*t)),
                None => print!("\r  {module}: {:.1} MiB", mib(*done)),
            }
            if total.map_or(false, |t| t == *done) {
                println!();
            } else {
                let _ = std::io::stdout().flush();
            }
        }
        Event::Log { module, line } => println!("  [{module}] {line}"),
        Event::Skipped { module, reason } => println!("⏭ {module}: {reason}"),
        Event::Conflict { module, path } => println!("⚠ {module}: conflito em {path} (versão nova ficou pendente)"),
        Event::Finished { module } => println!("✔ {module}"),
    }
}

fn cmd_install(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args)?;
    let id = flags
        .positional
        .first()
        .ok_or("informe o módulo: hiraku install <módulo>")?;
    let manifests = registry::load_dir(&registry_dir(&flags)?).map_err(|e| e.to_string())?;
    let manifest = manifests
        .iter()
        .find(|m| &m.id == id)
        .ok_or_else(|| format!("módulo '{id}' não existe no registry"))?;

    let mut env = Environment::detect().map_err(|e| e.to_string())?;
    if flags.user {
        env = env.user_mode();
    } else if flags.system {
        env = env.system_mode();
    }
    if !manifest.supports(env.os) {
        return Err(format!("'{}' não suporta {}", manifest.id, env.os.as_str()));
    }

    let state_dir = paths::state_dir(&env).ok_or("não foi possível determinar o diretório de estado")?;
    let _guard = SyncLock::acquire(&state_dir).map_err(|e| e.to_string())?;
    let lock_path = state_dir.join("lock.toml");
    let mut lock = Lockfile::load(&lock_path).map_err(|e| e.to_string())?;

    let mut elevator = SudoElevator;
    let mut sink = |event: Event| print_event(&event);
    let outcome = installer::install_module(manifest, &env, &state_dir, &mut lock, &mut elevator, &mut sink, None)
        .map_err(|e| e.to_string())?;
    for c in &outcome.components {
        println!("  {} {:?}", c.key, c.state);
    }

    lock.save(&lock_path).map_err(|e| e.to_string())?;
    println!("lockfile atualizado: {}", lock_path.display());
    Ok(())
}

fn cmd_status() -> Result<(), String> {
    let env = Environment::detect().map_err(|e| e.to_string())?;
    let state_dir = paths::state_dir(&env).ok_or("não foi possível determinar o diretório de estado")?;
    let lock = Lockfile::load(&state_dir.join("lock.toml")).map_err(|e| e.to_string())?;
    if lock.modules.is_empty() {
        println!("nada instalado pelo hiraku ainda.");
    }
    for (id, m) in &lock.modules {
        println!(
            "{id}: {} {}{}",
            m.method,
            m.version.as_deref().unwrap_or("-"),
            m.pin.as_deref().map(|p| format!("  (pin {p})")).unwrap_or_default()
        );
    }
    Ok(())
}

fn state_context() -> Result<(Environment, std::path::PathBuf), String> {
    let env = Environment::detect().map_err(|e| e.to_string())?;
    let state_dir =
        paths::state_dir(&env).ok_or("não foi possível determinar o diretório de estado")?;
    Ok((env, state_dir))
}

fn cmd_configure(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args)?;
    let id = flags
        .positional
        .first()
        .ok_or("informe o módulo: hiraku configure <módulo>")?;
    let loaded = registry::load_dir_with_paths(&registry_dir(&flags)?).map_err(|e| e.to_string())?;
    let (manifest, module_dir) = loaded
        .iter()
        .find(|(m, _)| &m.id == id)
        .ok_or_else(|| format!("módulo '{id}' não existe no registry"))?;

    let first = match flags.first_apply.as_deref() {
        None | Some("backup") => FirstApply::BackupOverwrite,
        Some("pending") => FirstApply::KeepPending,
        Some(other) => return Err(format!("--first-apply inválido: {other} (use backup ou pending)")),
    };

    let (env, state_dir) = state_context()?;
    let _guard = SyncLock::acquire(&state_dir).map_err(|e| e.to_string())?;
    let path = state_path(&state_dir);
    let mut state = ConfigState::load(&path).map_err(|e| e.to_string())?;

    let mut sink = |event: Event| print_event(&event);
    let result = configs::apply_module_configs(
        manifest, module_dir, &env, &state_dir, &flags.set, first, &mut state, &mut sink,
    );
    // Grava o estado mesmo se falhou no meio: o que já foi escrito precisa de base.
    state.save(&path).map_err(|e| e.to_string())?;
    let reports = result.map_err(|e| e.to_string())?;

    let conflicts = reports.iter().filter(|r| r.outcome == Outcome::Conflict).count();
    println!("{} arquivo(s) processado(s), {conflicts} conflito(s)", reports.len());
    if conflicts > 0 {
        println!("resolva com: hiraku conflicts | hiraku diff <arquivo> | hiraku resolve <arquivo> keep|new");
    }
    Ok(())
}

fn cmd_conflicts() -> Result<(), String> {
    let (_, state_dir) = state_context()?;
    let state = ConfigState::load(&state_path(&state_dir)).map_err(|e| e.to_string())?;
    let pending = state.pending();
    if pending.is_empty() {
        println!("nenhum conflito pendente.");
    }
    for (path, file) in pending {
        println!("{}: {path}", file.module);
    }
    Ok(())
}

fn cmd_diff(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args)?;
    let target = flags.positional.first().ok_or("uso: hiraku diff <arquivo>")?;
    let (env, state_dir) = state_context()?;
    let state = ConfigState::load(&state_path(&state_dir)).map_err(|e| e.to_string())?;
    let dest = paths::expand_tilde(target, env.home.as_deref()).map_err(|e| e.to_string())?;
    let diff = configs::conflict_diff(&dest, &state_dir, &state).map_err(|e| e.to_string())?;
    print!("{diff}");
    Ok(())
}

fn cmd_resolve(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args)?;
    let (target, choice) = match flags.positional.as_slice() {
        [target, choice] => (target, choice.as_str()),
        _ => return Err("uso: hiraku resolve <arquivo> keep|new".to_string()),
    };
    let resolution = match choice {
        "keep" => Resolution::KeepMine,
        "new" => Resolution::UseNew,
        other => return Err(format!("escolha inválida '{other}' (use keep ou new)")),
    };
    let (env, state_dir) = state_context()?;
    let _guard = SyncLock::acquire(&state_dir).map_err(|e| e.to_string())?;
    let path = state_path(&state_dir);
    let mut state = ConfigState::load(&path).map_err(|e| e.to_string())?;
    let dest = paths::expand_tilde(target, env.home.as_deref()).map_err(|e| e.to_string())?;
    configs::resolve_conflict(&dest, resolution, &state_dir, &mut state).map_err(|e| e.to_string())?;
    state.save(&path).map_err(|e| e.to_string())?;
    println!("resolvido: {}", dest.display());
    Ok(())
}

fn cmd_registry(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args)?;
    match flags.positional.first().map(String::as_str) {
        Some("info") => {
            let (_, state_dir) = state_context()?;
            let active = regsource::resolve_active(&regsource::embedded_files(), &state_dir)
                .map_err(|e| e.to_string())?;
            let source = match active.source {
                Source::Embedded => "embutido no binário",
                Source::Remote => "cache remoto (assinado)",
            };
            println!("fonte:   {source}");
            println!("versão:  {}", active.version);
            println!("caminho: {}", active.root.display());
            if let Some(note) = &active.note {
                println!("aviso:   {note}");
            }
            Ok(())
        }
        Some("update") => {
            let url = flags
                .url
                .clone()
                .or_else(|| std::env::var("HIRAKU_REGISTRY_URL").ok())
                .or_else(|| regsource::DEFAULT_REMOTE_URL.map(str::to_string))
                .ok_or("informe --url ou defina HIRAKU_REGISTRY_URL")?;
            let keys: Vec<String> = if flags.pubkeys.is_empty() {
                regsource::trusted_public_keys().iter().map(|k| k.to_string()).collect()
            } else {
                flags.pubkeys.clone()
            };
            let verifier = MinisignVerifier::from_base64_keys(&keys).map_err(|e| e.to_string())?;
            let (_, state_dir) = state_context()?;
            let outcome = regsource::update_remote(
                &url,
                &UreqFetcher,
                &verifier,
                &regsource::embedded_files(),
                &state_dir,
            )
            .map_err(|e| e.to_string())?;
            match outcome {
                UpdateOutcome::UpToDate { version } => {
                    println!("registry já está na versão {version}")
                }
                UpdateOutcome::Updated { from, to } => {
                    println!("registry atualizado: {from} -> {to}")
                }
            }
            Ok(())
        }
        Some("bump") => {
            let dir = flags
                .registry
                .clone()
                .or_else(|| std::env::var_os("HIRAKU_REGISTRY").map(std::path::PathBuf::from))
                .unwrap_or_else(|| std::path::PathBuf::from("registry/modules"));
            let only: Vec<String> = flags.positional.iter().skip(1).cloned().collect();
            let report = bump::bump_dir(&dir, &only, flags.check, &NetResolver).map_err(|e| e.to_string())?;

            for change in &report.changes {
                let component = if change.group == "main" {
                    String::new()
                } else {
                    format!("#{}", change.group)
                };
                println!("{}{component}: {} -> {}", change.module, change.from, change.to);
            }
            for (module, error) in &report.errors {
                eprintln!("erro em {module}: {error}");
            }
            if let Some((before, after)) = report.registry_version {
                println!("registry.toml: versão {before} -> {after}");
            }
            if report.changes.is_empty() && report.errors.is_empty() {
                println!("tudo em dia.");
            }
            if !report.errors.is_empty() {
                return Err(format!("{} módulo(s) com erro", report.errors.len()));
            }
            if flags.check && !report.changes.is_empty() {
                return Err(format!("há {} atualização(ões) pendente(s)", report.changes.len()));
            }
            Ok(())
        }
        _ => Err("uso: hiraku registry info | update [--url URL] [--pubkey CHAVE] | bump [módulo]... [--check]".to_string()),
    }
}

fn settings_file(env: &Environment) -> Result<std::path::PathBuf, String> {
    let dir = paths::config_dir(env).ok_or("não foi possível determinar o diretório de configuração")?;
    Ok(settings_path(&dir))
}

fn load_settings() -> Result<(Environment, std::path::PathBuf, Settings), String> {
    let env = Environment::detect().map_err(|e| e.to_string())?;
    let file = settings_file(&env)?;
    let settings = Settings::load(&file).map_err(|e| e.to_string())?;
    Ok((env, file, settings))
}

fn save_settings(file: &std::path::Path, settings: &Settings) -> Result<(), String> {
    settings.save(file).map_err(|e| e.to_string())
}

fn cmd_enable(args: &[String], enable: bool) -> Result<(), String> {
    let flags = parse_flags(args)?;
    if flags.positional.is_empty() {
        return Err("informe ao menos um módulo".to_string());
    }
    let known = registry::load_dir(&registry_dir(&flags)?).map_err(|e| e.to_string())?;
    let (_, file, mut settings) = load_settings()?;
    for id in &flags.positional {
        if enable {
            if !known.iter().any(|m| &m.id == id) {
                return Err(format!("módulo '{id}' não existe no registry"));
            }
            let added = settings.enable(id);
            println!("{id}: {}", if added { "habilitado" } else { "já estava habilitado" });
        } else {
            let removed = settings.disable(id);
            println!("{id}: {}", if removed { "desabilitado (o que já foi instalado permanece)" } else { "não estava habilitado" });
        }
    }
    save_settings(&file, &settings)
}

fn cmd_set(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args)?;
    let (id, pair) = match flags.positional.as_slice() {
        [id, pair] => (id, pair),
        _ => return Err("uso: hiraku set <módulo> chave=valor".to_string()),
    };
    let (key, value) = pair.split_once('=').ok_or("esperado chave=valor")?;
    let known = registry::load_dir(&registry_dir(&flags)?).map_err(|e| e.to_string())?;
    let manifest = known
        .iter()
        .find(|m| &m.id == id)
        .ok_or_else(|| format!("módulo '{id}' não existe no registry"))?;
    // Valida a chave e o valor contra as opções declaradas no manifest.
    let mut candidate = BTreeMap::new();
    candidate.insert(key.to_string(), value.to_string());
    configs::resolve_options(manifest, &candidate).map_err(|e| e.to_string())?;

    let (_, file, mut settings) = load_settings()?;
    settings.set_option(id, key, value);
    save_settings(&file, &settings)?;
    println!("{id}: {key} = {value}");
    Ok(())
}

fn cmd_pin(args: &[String], pin: bool) -> Result<(), String> {
    let flags = parse_flags(args)?;
    let (_, file, mut settings) = load_settings()?;
    match (pin, flags.positional.as_slice()) {
        (true, [id, pattern]) => {
            settings.pins.insert(id.clone(), pattern.clone());
            println!("{id}: fixado em {pattern}");
        }
        (false, [id]) => {
            let removed = settings.pins.remove(id).is_some();
            println!("{id}: {}", if removed { "pin removido" } else { "não estava fixado" });
        }
        (true, _) => return Err("uso: hiraku pin <módulo> <versão ou padrão, ex.: 0.10.*>".to_string()),
        (false, _) => return Err("uso: hiraku unpin <módulo>".to_string()),
    }
    save_settings(&file, &settings)
}

fn cmd_mode(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args)?;
    let mode = match flags.positional.first().map(String::as_str) {
        Some("user") => Some(Mode::User),
        Some("system") => Some(Mode::System),
        Some("auto") => None,
        _ => return Err("uso: hiraku mode user|system|auto".to_string()),
    };
    let (_, file, mut settings) = load_settings()?;
    settings.mode = mode;
    save_settings(&file, &settings)?;
    println!("modo: {}", match mode { Some(Mode::User) => "user", Some(Mode::System) => "system", None => "auto" });
    Ok(())
}

fn cmd_settings() -> Result<(), String> {
    let (_, file, s) = load_settings()?;
    println!("arquivo:  {}", file.display());
    println!("modo:     {}", match s.mode { Some(Mode::User) => "user", Some(Mode::System) => "system", None => "auto" });
    println!("registry: {}", s.registry_url.as_deref().unwrap_or("-"));
    println!("módulos:  {}", if s.modules.is_empty() { "-".to_string() } else { s.modules.join(", ") });
    for (id, pin) in &s.pins {
        println!("pin:      {id} = {pin}");
    }
    for (id, options) in &s.options {
        for (key, value) in options {
            println!("opção:    {id}.{key} = {value}");
        }
    }
    Ok(())
}

fn print_report(report: &SyncReport) {
    for note in &report.notes {
        println!("nota: {note}");
    }
    for m in &report.modules {
        let major = if m.major { " [MAJOR]" } else { "" };
        println!("{:<14} {:?}{major} {}", m.id, m.status, m.detail.as_deref().unwrap_or(""));
        if let Some(error) = &m.error {
            println!("{:<14} erro: {error}", "");
        }
        for c in m.configs.iter().filter(|c| c.outcome != "Unchanged") {
            println!("{:<14} {}: {}", "", c.outcome, c.dest);
        }
        for h in &m.hooks {
            println!("{:<14} hook {}: {}", "", h.name, h.outcome);
        }
    }
    let conflicts = report.conflicts();
    if conflicts > 0 {
        println!("{conflicts} conflito(s) de config pendente(s): hiraku conflicts");
    }
}

fn refresh_registry(settings: &Settings, state_dir: &std::path::Path) -> Result<Option<String>, String> {
    regsource::refresh(settings.registry_url.clone(), state_dir).map_err(|e| e.to_string())
}

fn cmd_sync(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args)?;
    let unattended = flags.unattended;
    let (mut env, _, settings) = load_settings()?;
    let state_dir = paths::state_dir(&env).ok_or("não foi possível determinar o diretório de estado")?;

    // Agendado: assume --user, a menos que o settings.toml diga outra coisa.
    env = if flags.user {
        env.user_mode()
    } else if flags.system {
        env.system_mode()
    } else {
        match settings.mode {
            Some(Mode::User) => env.user_mode(),
            Some(Mode::System) => env.system_mode(),
            None if unattended => env.user_mode(),
            None => env,
        }
    };

    let mut notes = Vec::new();
    if !flags.no_registry_update {
        match refresh_registry(&settings, &state_dir) {
            Ok(Some(note)) => notes.push(note),
            Ok(None) => {}
            Err(e) => notes.push(format!("registry remoto não atualizado: {e}")),
        }
    }
    let active = regsource::resolve_active(&regsource::embedded_files(), &state_dir).map_err(|e| e.to_string())?;
    if let Some(note) = &active.note {
        notes.push(note.clone());
    }
    let loaded = registry::load_dir_with_paths(&active.modules_dir()).map_err(|e| e.to_string())?;

    // Agendado: arquivo seu já existente vira conflito pendente, nunca é sobrescrito.
    let first_apply = match flags.first_apply.as_deref() {
        Some("backup") => FirstApply::BackupOverwrite,
        Some("pending") => FirstApply::KeepPending,
        Some(other) => return Err(format!("--first-apply inválido: {other} (use backup ou pending)")),
        None if unattended => FirstApply::KeepPending,
        None => FirstApply::BackupOverwrite,
    };

    let mut elevator: Box<dyn Elevator> = if unattended {
        Box::new(NonInteractiveSudo)
    } else {
        Box::new(SudoElevator)
    };
    let log_dir = state_dir.clone();
    let mut sink: Box<dyn FnMut(Event)> = if unattended {
        Box::new(move |event: Event| {
            if matches!(event, Event::Progress { .. }) {
                return;
            }
            let line = event.describe();
            println!("{line}");
            sync::append_log(&log_dir, &line);
        })
    } else {
        Box::new(|event: Event| print_event(&event))
    };

    let report = sync::run_sync(
        SyncContext {
            manifests: &loaded,
            settings: &settings,
            env: &env,
            state_dir: &state_dir,
            first_apply,
            registry_version: active.version,
            registry_source: match active.source {
                Source::Embedded => "embedded".to_string(),
                Source::Remote => "remote".to_string(),
            },
            notes,
            unattended,
        },
        elevator.as_mut(),
        &mut sink,
    )
    .map_err(|e| e.to_string())?;

    print_report(&report);
    if unattended {
        sync::append_log(
            &state_dir,
            &format!(
                "sync concluído: {} módulo(s), {} conflito(s), falhas: {}",
                report.modules.len(),
                report.conflicts(),
                report.has_failures()
            ),
        );
    }
    if report.modules.is_empty() {
        println!("nenhum módulo habilitado: use `hiraku enable <módulo>`.");
    }
    if report.has_failures() {
        return Err("o sync terminou com falhas (veja acima)".to_string());
    }
    Ok(())
}

fn cmd_report() -> Result<(), String> {
    let (_, state_dir) = state_context()?;
    match SyncReport::load(&sync::report_path(&state_dir)).map_err(|e| e.to_string())? {
        Some(report) => {
            println!("registry v{} ({})", report.registry_version, report.registry_source);
            print_report(&report);
        }
        None => println!("nenhum sync executado ainda."),
    }
    Ok(())
}

fn cmd_schedule(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args)?;
    let (env, state_dir) = state_context()?;
    let dirs = ScheduleDirs::for_env(&env, &state_dir).map_err(|e| e.to_string())?;
    match flags.positional.first().map(String::as_str) {
        Some("install") => {
            let every = flags
                .every
                .as_deref()
                .map(schedule::parse_interval)
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or(schedule::DEFAULT_INTERVAL_SECS);
            let exe = schedule::scheduled_exe()
                .map_err(|e| format!("não consegui descobrir o caminho do executável: {e}"))?;
            let exe_text = exe.display().to_string();
            if exe_text.contains("/target/debug/") || exe_text.contains("/target/release/") {
                eprintln!("aviso: o agendamento aponta para {exe_text}; ele quebra se você recompilar ou mover o binário.");
            }
            schedule::install_schedule(env.os, &dirs, &exe, every, &ProcessRunner).map_err(|e| e.to_string())?;
            println!("agendado: `hiraku sync --unattended` a cada {}s (log em {})", every, dirs.log_file.display());
            if env.os == hiraku_core::Os::Linux {
                println!("dica: para rodar sem sessão aberta, `loginctl enable-linger $USER`.");
            }
            Ok(())
        }
        Some("remove") => {
            schedule::remove_schedule(env.os, &dirs, &ProcessRunner).map_err(|e| e.to_string())?;
            println!("agendamento removido.");
            Ok(())
        }
        Some("status") => {
            let on = schedule::is_scheduled(env.os, &dirs, &ProcessRunner);
            println!("agendamento: {}", if on { "ativo" } else { "não instalado" });
            Ok(())
        }
        _ => Err("uso: hiraku schedule install [--every 6h] | remove | status".to_string()),
    }
}

fn cmd_tui(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args)?;
    let services = hiraku_tui::services::Services::open(flags.registry.clone())?;
    hiraku_tui::run(services)
}
