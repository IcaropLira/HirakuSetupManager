//! Agendamento do `hiraku sync --unattended` no agendador nativo, sem root:
//! systemd --user (Linux), LaunchAgent (macOS) e Task Scheduler (Windows).
//!
//! A geração dos arquivos e comandos é pura (testável); a execução passa por `Runner`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::env::{Environment, Os};
use crate::error::{io_err, Error, Result};

pub const SERVICE_NAME: &str = "hiraku-sync";
pub const LAUNCHD_LABEL: &str = "dev.hiraku.sync";
pub const WINDOWS_TASK: &str = "Hiraku Sync";

const MIN_INTERVAL_SECS: u64 = 15 * 60;
const MAX_INTERVAL_SECS: u64 = 30 * 24 * 60 * 60;
pub const DEFAULT_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// `30m`, `6h`, `1d` (mínimo 15 minutos, máximo 30 dias).
pub fn parse_interval(text: &str) -> Result<u64> {
    let text = text.trim();
    let invalid = || Error::Unsupported(format!("intervalo inválido '{text}' (use 30m, 6h ou 1d)"));
    if text.len() < 2 || !text.is_char_boundary(text.len() - 1) {
        return Err(invalid());
    }
    let (number, unit) = text.split_at(text.len() - 1);
    let n: u64 = number.parse().map_err(|_| invalid())?;
    let secs = match unit {
        "m" => n.checked_mul(60),
        "h" => n.checked_mul(3600),
        "d" => n.checked_mul(86_400),
        _ => return Err(invalid()),
    }
    .ok_or_else(invalid)?;
    if !(MIN_INTERVAL_SECS..=MAX_INTERVAL_SECS).contains(&secs) {
        return Err(Error::Unsupported(
            "o intervalo deve ficar entre 15 minutos e 30 dias".to_string(),
        ));
    }
    Ok(secs)
}

fn systemd_quote(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%")
    )
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn systemd_service(exe: &Path) -> String {
    format!(
        "[Unit]\nDescription=Hiraku: sincronização (modo não interativo)\n\n[Service]\nType=oneshot\nExecStart={} sync --unattended\nNice=10\n",
        systemd_quote(&exe.display().to_string())
    )
}

pub fn systemd_timer(every_secs: u64) -> String {
    format!(
        "[Unit]\nDescription=Hiraku: sincronização periódica\n\n[Timer]\nOnStartupSec=5min\nOnUnitActiveSec={every_secs}s\nRandomizedDelaySec=10min\n\n[Install]\nWantedBy=timers.target\n"
    )
}

pub fn launchd_plist(exe: &Path, every_secs: u64, log: &Path) -> String {
    let exe = xml_escape(&exe.display().to_string());
    let log = xml_escape(&log.display().to_string());
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>{LAUNCHD_LABEL}</string>\n  <key>ProgramArguments</key>\n  <array>\n    <string>{exe}</string>\n    <string>sync</string>\n    <string>--unattended</string>\n  </array>\n  <key>StartInterval</key><integer>{every_secs}</integer>\n  <key>RunAtLoad</key><true/>\n  <key>ProcessType</key><string>Background</string>\n  <key>LowPriorityIO</key><true/>\n  <key>StandardOutPath</key><string>{log}</string>\n  <key>StandardErrorPath</key><string>{log}</string>\n</dict>\n</plist>\n"
    )
}

/// O Task Scheduler só aceita unidades inteiras: arredonda para minutos, horas
/// (< 24h) ou dias.
pub fn schtasks_create_args(exe: &Path, every_secs: u64) -> Vec<String> {
    let minutes = (every_secs / 60).max(1);
    let (kind, every) = if minutes < 60 {
        ("MINUTE", minutes)
    } else if minutes < 24 * 60 {
        ("HOURLY", minutes / 60)
    } else {
        ("DAILY", (minutes / (24 * 60)).max(1))
    };
    vec![
        "schtasks".to_string(),
        "/Create".to_string(),
        "/F".to_string(),
        "/TN".to_string(),
        WINDOWS_TASK.to_string(),
        "/SC".to_string(),
        kind.to_string(),
        "/MO".to_string(),
        every.to_string(),
        "/TR".to_string(),
        format!("\"{}\" sync --unattended", exe.display()),
    ]
}

/// Alternativa para Linux sem systemd de usuário (o app não mexe no crontab).
pub fn cron_line(exe: &Path, every_secs: u64) -> String {
    let minutes = (every_secs / 60).max(1);
    let schedule = if minutes < 60 {
        format!("*/{minutes} * * * *")
    } else if minutes < 24 * 60 {
        format!("0 */{} * * *", minutes / 60)
    } else {
        format!("0 3 */{} * *", (minutes / (24 * 60)).max(1))
    };
    format!("{schedule} \"{}\" sync --unattended", exe.display())
}

pub trait Runner {
    fn run(&self, argv: &[String]) -> Result<()>;
}

pub struct ProcessRunner;

impl Runner for ProcessRunner {
    fn run(&self, argv: &[String]) -> Result<()> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| Error::Unsupported("comando vazio".to_string()))?;
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| Error::Command {
                cmd: argv.join(" "),
                reason: e.to_string(),
            })?;
        if output.status.success() {
            Ok(())
        } else {
            Err(Error::Command {
                cmd: argv.join(" "),
                reason: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }
}

/// Onde gravar os arquivos do agendador. `for_env` lê XDG_CONFIG_HOME; os testes
/// montam a struct à mão.
#[derive(Debug, Clone)]
pub struct ScheduleDirs {
    pub systemd_user: PathBuf,
    pub launch_agents: PathBuf,
    pub uid: String,
    pub log_file: PathBuf,
}

impl ScheduleDirs {
    pub fn for_env(env: &Environment, state_dir: &Path) -> Result<ScheduleDirs> {
        let home = env
            .home
            .clone()
            .ok_or_else(|| Error::Unsupported("HOME não definido".to_string()))?;
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        Ok(ScheduleDirs {
            systemd_user: config_home.join("systemd/user"),
            launch_agents: home.join("Library/LaunchAgents"),
            uid: crate::env::current_uid().unwrap_or_default(),
            log_file: state_dir.join("sync.log"),
        })
    }

    fn service_file(&self) -> PathBuf {
        self.systemd_user.join(format!("{SERVICE_NAME}.service"))
    }

    fn timer_file(&self) -> PathBuf {
        self.systemd_user.join(format!("{SERVICE_NAME}.timer"))
    }

    fn plist_file(&self) -> PathBuf {
        self.launch_agents.join(format!("{LAUNCHD_LABEL}.plist"))
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// Caminho estável do executável para o agendador rodar. Dentro de um AppImage o
/// `current_exe` é um ponto de montagem temporário; o arquivo de verdade está em
/// `$APPIMAGE`.
pub fn scheduled_exe() -> std::io::Result<PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE").map(PathBuf::from) {
        if appimage.is_file() {
            return Ok(appimage);
        }
    }
    std::env::current_exe().and_then(std::fs::canonicalize)
}

pub fn install_schedule(
    os: Os,
    dirs: &ScheduleDirs,
    exe: &Path,
    every_secs: u64,
    runner: &dyn Runner,
) -> Result<()> {
    match os {
        Os::Linux => {
            fs::create_dir_all(&dirs.systemd_user).map_err(io_err(&dirs.systemd_user))?;
            let service = dirs.service_file();
            let timer = dirs.timer_file();
            fs::write(&service, systemd_service(exe)).map_err(io_err(&service))?;
            fs::write(&timer, systemd_timer(every_secs)).map_err(io_err(&timer))?;

            let timer_unit = format!("{SERVICE_NAME}.timer");
            let activated = runner
                .run(&argv(&["systemctl", "--user", "daemon-reload"]))
                .and_then(|_| {
                    runner.run(&argv(&[
                        "systemctl",
                        "--user",
                        "enable",
                        "--now",
                        timer_unit.as_str(),
                    ]))
                });
            if let Err(e) = activated {
                let _ = fs::remove_file(&service);
                let _ = fs::remove_file(&timer);
                return Err(Error::Unsupported(format!(
                    "systemd de usuário indisponível ({e}). Alternativa com cron: {}",
                    cron_line(exe, every_secs)
                )));
            }
            Ok(())
        }
        Os::Macos => {
            fs::create_dir_all(&dirs.launch_agents).map_err(io_err(&dirs.launch_agents))?;
            let plist = dirs.plist_file();
            fs::write(&plist, launchd_plist(exe, every_secs, &dirs.log_file))
                .map_err(io_err(&plist))?;
            let domain = format!("gui/{}", dirs.uid);
            let agent = format!("{domain}/{LAUNCHD_LABEL}");
            let plist_arg = plist.display().to_string();
            // Recarrega se já existia: ignora o erro do bootout.
            let _ = runner.run(&argv(&["launchctl", "bootout", agent.as_str()]));
            if let Err(e) = runner.run(&argv(&[
                "launchctl",
                "bootstrap",
                domain.as_str(),
                plist_arg.as_str(),
            ])) {
                let _ = fs::remove_file(&plist);
                return Err(e);
            }
            Ok(())
        }
        Os::Windows => runner.run(&schtasks_create_args(exe, every_secs)),
    }
}

pub fn remove_schedule(os: Os, dirs: &ScheduleDirs, runner: &dyn Runner) -> Result<()> {
    match os {
        Os::Linux => {
            let timer_unit = format!("{SERVICE_NAME}.timer");
            let _ = runner.run(&argv(&[
                "systemctl",
                "--user",
                "disable",
                "--now",
                timer_unit.as_str(),
            ]));
            for file in [dirs.timer_file(), dirs.service_file()] {
                if file.exists() {
                    fs::remove_file(&file).map_err(io_err(&file))?;
                }
            }
            let _ = runner.run(&argv(&["systemctl", "--user", "daemon-reload"]));
            Ok(())
        }
        Os::Macos => {
            let agent = format!("gui/{}/{LAUNCHD_LABEL}", dirs.uid);
            let _ = runner.run(&argv(&["launchctl", "bootout", agent.as_str()]));
            let plist = dirs.plist_file();
            if plist.exists() {
                fs::remove_file(&plist).map_err(io_err(&plist))?;
            }
            Ok(())
        }
        Os::Windows => {
            let _ = runner.run(&argv(&["schtasks", "/Delete", "/F", "/TN", WINDOWS_TASK]));
            Ok(())
        }
    }
}

/// `true` se o agendamento existe.
pub fn is_scheduled(os: Os, dirs: &ScheduleDirs, runner: &dyn Runner) -> bool {
    match os {
        Os::Linux => dirs.timer_file().is_file(),
        Os::Macos => dirs.plist_file().is_file(),
        Os::Windows => runner
            .run(&argv(&["schtasks", "/Query", "/TN", WINDOWS_TASK]))
            .is_ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Rec {
        calls: RefCell<Vec<String>>,
        fail_prefix: Option<&'static str>,
    }

    impl Rec {
        fn ok() -> Rec {
            Rec {
                calls: RefCell::new(Vec::new()),
                fail_prefix: None,
            }
        }
        fn failing(prefix: &'static str) -> Rec {
            Rec {
                calls: RefCell::new(Vec::new()),
                fail_prefix: Some(prefix),
            }
        }
        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl Runner for Rec {
        fn run(&self, argv: &[String]) -> Result<()> {
            let line = argv.join(" ");
            self.calls.borrow_mut().push(line.clone());
            match self.fail_prefix {
                Some(prefix) if line.starts_with(prefix) => Err(Error::Command {
                    cmd: line,
                    reason: "falhou no teste".to_string(),
                }),
                _ => Ok(()),
            }
        }
    }

    fn dirs(root: &Path) -> ScheduleDirs {
        ScheduleDirs {
            systemd_user: root.join("systemd/user"),
            launch_agents: root.join("LaunchAgents"),
            uid: "501".to_string(),
            log_file: root.join("sync.log"),
        }
    }

    #[test]
    fn parses_intervals() {
        assert_eq!(parse_interval("30m").unwrap(), 1800);
        assert_eq!(parse_interval("6h").unwrap(), 21_600);
        assert_eq!(parse_interval("1d").unwrap(), 86_400);
        assert!(parse_interval("10m").is_err());
        assert!(parse_interval("60d").is_err());
        assert!(parse_interval("abc").is_err());
        assert!(parse_interval("6").is_err());
        assert!(parse_interval("").is_err());
        assert!(parse_interval("xh").is_err());
        assert!(parse_interval("6é").is_err());
    }

    #[test]
    fn systemd_units_are_correct_and_escaped() {
        let service = systemd_service(Path::new("/opt/meu app/100%/hiraku"));
        assert!(service.contains("ExecStart=\"/opt/meu app/100%%/hiraku\" sync --unattended"));
        assert!(service.contains("Type=oneshot"));
        let timer = systemd_timer(21_600);
        assert!(timer.contains("OnUnitActiveSec=21600s"));
        assert!(timer.contains("OnStartupSec=5min"));
        assert!(timer.contains("WantedBy=timers.target"));
    }

    #[test]
    fn launchd_plist_is_escaped() {
        let plist = launchd_plist(Path::new("/Apps/A&B/hiraku"), 3600, Path::new("/tmp/sync.log"));
        assert!(plist.contains("<string>/Apps/A&amp;B/hiraku</string>"));
        assert!(plist.contains("<key>StartInterval</key><integer>3600</integer>"));
        assert!(plist.contains("<string>--unattended</string>"));
        assert!(plist.contains(LAUNCHD_LABEL));
    }

    #[test]
    fn schtasks_rounds_to_whole_units() {
        let exe = Path::new("C:/hiraku.exe");
        let args = |secs| schtasks_create_args(exe, secs).join(" ");
        assert!(args(30 * 60).contains("/SC MINUTE /MO 30"));
        assert!(args(6 * 3600).contains("/SC HOURLY /MO 6"));
        assert!(args(86_400).contains("/SC DAILY /MO 1"));
        assert!(args(3 * 86_400).contains("/SC DAILY /MO 3"));
        assert!(args(3600).contains("\"C:/hiraku.exe\" sync --unattended"));
    }

    #[test]
    fn cron_lines() {
        let exe = Path::new("/usr/bin/hiraku");
        assert_eq!(cron_line(exe, 6 * 3600), "0 */6 * * * \"/usr/bin/hiraku\" sync --unattended");
        assert_eq!(cron_line(exe, 86_400), "0 3 */1 * * \"/usr/bin/hiraku\" sync --unattended");
        assert_eq!(cron_line(exe, 30 * 60), "*/30 * * * * \"/usr/bin/hiraku\" sync --unattended");
    }

    #[test]
    fn linux_install_writes_units_and_enables_the_timer() {
        let tmp = tempfile::tempdir().unwrap();
        let d = dirs(tmp.path());
        let rec = Rec::ok();
        install_schedule(Os::Linux, &d, Path::new("/usr/bin/hiraku"), 3600, &rec).unwrap();
        assert!(d.systemd_user.join("hiraku-sync.service").is_file());
        assert!(d.systemd_user.join("hiraku-sync.timer").is_file());
        assert_eq!(
            rec.calls(),
            vec![
                "systemctl --user daemon-reload",
                "systemctl --user enable --now hiraku-sync.timer"
            ]
        );
        assert!(is_scheduled(Os::Linux, &d, &rec));
    }

    #[test]
    fn linux_without_user_systemd_cleans_up_and_suggests_cron() {
        let tmp = tempfile::tempdir().unwrap();
        let d = dirs(tmp.path());
        let rec = Rec::failing("systemctl");
        let err = install_schedule(Os::Linux, &d, Path::new("/usr/bin/hiraku"), 21_600, &rec)
            .unwrap_err()
            .to_string();
        assert!(err.contains("cron"));
        assert!(err.contains("0 */6 * * *"));
        assert!(!d.systemd_user.join("hiraku-sync.timer").exists());
        assert!(!d.systemd_user.join("hiraku-sync.service").exists());
        assert!(!is_scheduled(Os::Linux, &d, &rec));
    }

    #[test]
    fn linux_remove_disables_and_deletes() {
        let tmp = tempfile::tempdir().unwrap();
        let d = dirs(tmp.path());
        install_schedule(Os::Linux, &d, Path::new("/usr/bin/hiraku"), 3600, &Rec::ok()).unwrap();
        let rec = Rec::ok();
        remove_schedule(Os::Linux, &d, &rec).unwrap();
        assert!(!is_scheduled(Os::Linux, &d, &rec));
        assert!(rec.calls()[0].contains("disable --now hiraku-sync.timer"));
        // remover de novo não é erro
        remove_schedule(Os::Linux, &d, &Rec::ok()).unwrap();
    }

    #[test]
    fn macos_install_bootstraps_the_agent() {
        let tmp = tempfile::tempdir().unwrap();
        let d = dirs(tmp.path());
        let rec = Rec::ok();
        install_schedule(Os::Macos, &d, Path::new("/usr/local/bin/hiraku"), 7200, &rec).unwrap();
        let plist = d.launch_agents.join("dev.hiraku.sync.plist");
        assert!(plist.is_file());
        let calls = rec.calls();
        assert_eq!(calls[0], "launchctl bootout gui/501/dev.hiraku.sync");
        assert!(calls[1].starts_with("launchctl bootstrap gui/501 "));
        assert!(calls[1].ends_with("dev.hiraku.sync.plist"));
        assert!(is_scheduled(Os::Macos, &d, &rec));
    }

    #[test]
    fn macos_failed_bootstrap_removes_the_plist() {
        let tmp = tempfile::tempdir().unwrap();
        let d = dirs(tmp.path());
        let rec = Rec::failing("launchctl bootstrap");
        assert!(install_schedule(Os::Macos, &d, Path::new("/x/hiraku"), 7200, &rec).is_err());
        assert!(!d.launch_agents.join("dev.hiraku.sync.plist").exists());
    }

    #[test]
    fn windows_uses_schtasks() {
        let tmp = tempfile::tempdir().unwrap();
        let d = dirs(tmp.path());
        let rec = Rec::ok();
        install_schedule(Os::Windows, &d, Path::new("C:/hiraku.exe"), 86_400, &rec).unwrap();
        assert!(rec.calls()[0].starts_with("schtasks /Create /F /TN Hiraku Sync"));
        assert!(is_scheduled(Os::Windows, &d, &rec));
        remove_schedule(Os::Windows, &d, &rec).unwrap();
        assert!(rec.calls().last().unwrap().starts_with("schtasks /Delete"));
        assert!(!is_scheduled(Os::Windows, &d, &Rec::failing("schtasks /Query")));
    }
}
