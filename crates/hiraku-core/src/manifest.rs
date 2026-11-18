//! Schema dos manifests de módulo (`module.toml`) e a lógica de seleção.

use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path};

use indexmap::IndexMap;
use serde::Deserialize;

use crate::env::{Arch, Environment, Os, PkgManager};
use crate::error::{Error, Result};

pub const SUPPORTED_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Vazio = todas as plataformas.
    #[serde(default)]
    pub platforms: Vec<Os>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    /// Na ordem em que foram declaradas (é a ordem em que o app mostra).
    pub options: IndexMap<String, OptionDef>,
    /// A ordem é a preferência: vale o primeiro passo que casa com o ambiente.
    #[serde(default)]
    pub install: Vec<InstallStep>,
    #[serde(default)]
    pub config: Vec<ConfigEntry>,
    /// Scripts/comandos executados depois da instalação e das configs.
    #[serde(default)]
    pub hook: Vec<Hook>,
}

/// Uma opção configurável do módulo: o tipo (`kind`) e os metadados comuns que a
/// interface usa (nome, seção, texto de ajuda, arquivo de pré-visualização).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct OptionDef {
    #[serde(flatten)]
    pub kind: OptionKind,
    #[serde(default)]
    pub description: String,
    /// Nome amigável ("Numeração relativa"); sem ele a interface mostra a chave.
    #[serde(default)]
    pub label: Option<String>,
    /// Agrupa opções na interface ("Aparência", "Atalhos", "Plugins"...).
    #[serde(default)]
    pub section: Option<String>,
    /// Arquivo (relativo à pasta do módulo) com as cores de cada valor de uma
    /// escolha; `{value}` vira o valor. Ex.: `../icaro-themes/files/colors/{value}.vim`.
    #[serde(default)]
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OptionKind {
    Choice {
        values: Vec<String>,
        default: String,
        /// Nome de exibição de cada valor (opcional).
        #[serde(default)]
        labels: BTreeMap<String, String>,
    },
    Bool {
        default: bool,
    },
    /// Inteiro dentro de [min, max]. Com `divisor` > 1 o valor entra no template já
    /// dividido, com 2 casas (ex.: opacidade 85 e divisor 100 viram `0.85`).
    Number {
        min: i64,
        max: i64,
        default: i64,
        #[serde(default = "default_divisor")]
        divisor: i64,
    },
    #[serde(rename = "string")]
    Text {
        default: String,
    },
    /// Texto de várias linhas, entra no template como está (ex.: binds extras do tmux).
    Lines {
        #[serde(default)]
        default: String,
    },
    /// Cor `#rrggbb`.
    Color {
        default: String,
    },
    /// Lista de atalhos em JSON: `[{"mode":"n","lhs":"dd","rhs":"\"_dd","desc":"..."}]`.
    Keymaps {
        #[serde(default = "empty_json_list")]
        default: String,
    },
}

fn empty_json_list() -> String {
    "[]".to_string()
}

impl OptionDef {
    /// Valor padrão como texto (o formato guardado no settings.toml).
    pub fn default_text(&self) -> String {
        match &self.kind {
            OptionKind::Choice { default, .. }
            | OptionKind::Text { default }
            | OptionKind::Lines { default }
            | OptionKind::Color { default }
            | OptionKind::Keymaps { default } => default.clone(),
            OptionKind::Bool { default } => default.to_string(),
            OptionKind::Number { default, .. } => default.to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct When {
    pub os: Option<Os>,
    pub arch: Option<Arch>,
    /// `true` = só no modo `--system` (com root); `false` = só no modo `--user`.
    /// Ausente = vale nos dois.
    pub root: Option<bool>,
}

impl When {
    pub fn matches(&self, env: &Environment) -> bool {
        self.os.map_or(true, |os| os == env.os)
            && self.arch.map_or(true, |arch| Some(arch) == env.arch)
            && self.root.map_or(true, |root| root == env.use_root)
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct InstallStep {
    #[serde(default)]
    pub when: When,
    /// Passos com o mesmo `group` são alternativas (vale o primeiro que casa com o
    /// ambiente); grupos diferentes são todos instalados. Padrão: "main".
    #[serde(default)]
    pub group: Option<String>,
    /// Falha neste componente vira aviso em vez de erro do módulo.
    #[serde(default)]
    pub optional: bool,
    /// Pula o passo se TODOS estes comandos já existirem no PATH e o componente ainda
    /// não for gerenciado pelo hiraku (ex.: o tmux que já veio na máquina).
    #[serde(default)]
    pub provides: Vec<String>,
    /// De onde `hiraku registry bump` descobre a versão mais nova deste passo.
    #[serde(default)]
    pub bump: Option<BumpSource>,
    #[serde(flatten)]
    pub method: Method,
}

pub const MAIN_GROUP: &str = "main";

impl InstallStep {
    pub fn group_name(&self) -> &str {
        self.group.as_deref().unwrap_or(MAIN_GROUP)
    }
}

/// Como o `bump` descobre a versão mais recente. Exatamente uma das fontes
/// (`github`, `index_url` ou `redirect`) para passos `release`; passos `git` usam
/// só `branch`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct BumpSource {
    /// `dono/repo`: a tag vem do redirecionamento de `/releases/latest` (sem usar a API).
    pub github: Option<String>,
    /// Prefixo removido da tag para formar a versão (padrão `v`; use `""` se não houver).
    pub tag_prefix: Option<String>,
    /// Página de índice (ex.: nodejs.org/dist/latest-v24.x/); a versão é o texto entre
    /// `index_prefix` e `index_suffix`.
    pub index_url: Option<String>,
    pub index_prefix: Option<String>,
    pub index_suffix: Option<String>,
    /// URL "sempre a última" cujo redirecionamento final é o arquivo versionado
    /// (ex.: a API da Adoptium). O `url` do manifest passa a ser esse destino fixo.
    pub redirect: Option<String>,
    /// Passos `git`: ramo cujo commit mais recente é fixado em `rev` (padrão HEAD).
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "method", rename_all = "lowercase")]
pub enum Method {
    /// Pacote do gerenciador do sistema. Chaves de `names`: dnf, apt, pacman,
    /// zypper, brew, winget, scoop.
    Pkg {
        names: BTreeMap<String, String>,
        /// Só para brew: instala como cask (apps gráficos).
        #[serde(default)]
        cask: bool,
    },
    /// Binário baixado de uma URL, com checksum obrigatório.
    Release {
        version: String,
        url: String,
        sha256: String,
        dest: String,
        #[serde(default)]
        bins: Vec<BinLink>,
        /// Componentes de caminho removidos de cada entrada ao extrair (como
        /// `tar --strip-components`). Útil quando o pacote vem numa pasta versionada.
        #[serde(default)]
        strip_components: u32,
    },
    /// Repositório git fixado num commit (plugins do Vim, por exemplo).
    Git {
        url: String,
        /// Commit completo (40 hexadecimais).
        rev: String,
        dest: String,
    },
    /// Script remoto. Checksum obrigatório; nunca roda em sync automático.
    Script { url: String, sha256: String },
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct BinLink {
    pub src: String,
    pub link: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ConfigEntry {
    /// Caminho relativo ao diretório do módulo.
    pub src: String,
    pub dest: Dest,
    #[serde(default)]
    pub mode: ConfigMode,
    #[serde(default = "default_true")]
    pub backup: bool,
    #[serde(default)]
    pub template: bool,
    /// `false`: mudar este arquivo não dispara os hooks `on = "change"` do módulo
    /// (ex.: as opções do editor, que ele mesmo recarrega, não devem rodar o
    /// `Lazy sync` do nvim a cada clique no app).
    #[serde(default = "default_true")]
    pub triggers_hooks: bool,
    /// Obrigatório (e só válido) com `mode = "block"`.
    #[serde(default)]
    pub block: Option<BlockSpec>,
}

/// Bloco gerenciado dentro de um arquivo que pertence ao usuário (ex.: `~/.bashrc`).
/// O hiraku só mexe no trecho entre `begin` e `end`; o resto nunca é tocado.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct BlockSpec {
    /// Linha de abertura, exatamente como aparece no arquivo.
    pub begin: String,
    /// Linha de fechamento.
    pub end: String,
    /// Comando de validação com `{path}` (ex.: `["bash", "-n", "{path}"]`). Roda sobre
    /// o resultado ANTES de gravar; se falhar, o arquivo não é alterado.
    #[serde(default)]
    pub check: Vec<String>,
    /// Não cria o arquivo: só edita se ele já existir (ex.: `~/.zshrc`).
    #[serde(default)]
    pub only_if_exists: bool,
}

fn default_true() -> bool {
    true
}

fn default_divisor() -> i64 {
    1
}

/// Quando o hook roda.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HookTrigger {
    /// Só se o módulo instalou/atualizou algo ou alterou configs neste sync (padrão).
    #[default]
    Change,
    /// Todo sync.
    Always,
    /// Uma vez por versão do script (reexecuta se o conteúdo do script mudar).
    Once,
}

/// Passo imperativo que o modelo declarativo não cobre (fc-cache, gsettings, bootstrap
/// de plugins...). Roda sem shell: `script` (.sh via `sh`, .ps1 via PowerShell) ou `argv`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Hook {
    /// Identificador (único no módulo); também a chave do estado de `once`.
    pub name: String,
    #[serde(default)]
    pub when: When,
    /// Script do módulo, relativo à pasta dele.
    pub script: Option<String>,
    /// Alternativa ao script: comando direto.
    pub argv: Option<Vec<String>>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub on: HookTrigger,
    /// Comandos que precisam existir; se faltar algum, o hook é pulado sem erro.
    #[serde(default)]
    pub needs: Vec<String>,
    /// Falha vira aviso, não erro do módulo.
    #[serde(default)]
    pub optional: bool,
    /// O script usa `sudo -n`. Faz o sync pedir `sudo -v` antes de começar enquanto
    /// o hook (`once`) ainda estiver pendente.
    #[serde(default)]
    pub root: bool,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConfigMode {
    /// Cópia com merge de 3 vias (padrão).
    #[default]
    Merge,
    /// Cópia simples, sobrescreve (com backup).
    Copy,
    Symlink,
    /// Cria só se não existir e nunca mais toca (estado do usuário, ex.: lazy-lock.json).
    Seed,
    /// Gerencia apenas um bloco com marcadores dentro de um arquivo do usuário.
    Block,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Dest {
    Single(String),
    PerOs {
        unix: Option<String>,
        windows: Option<String>,
    },
}

impl Dest {
    pub fn resolve(&self, os: Os) -> Option<&str> {
        match self {
            Dest::Single(path) => Some(path),
            Dest::PerOs { unix, windows } => match os {
                Os::Windows => windows.as_deref(),
                Os::Linux | Os::Macos => unix.as_deref(),
            },
        }
    }
}

impl Method {
    fn usable_on(&self, env: &Environment) -> bool {
        match self {
            Method::Pkg { names, .. } => env
                .pkg_manager
                .map_or(false, |pm| names.contains_key(pm.as_str())),
            Method::Release { .. } | Method::Script { .. } | Method::Git { .. } => true,
        }
    }
}

impl InstallStep {
    pub fn needs_root(&self, env: &Environment) -> bool {
        match &self.method {
            Method::Pkg { .. } => env.pkg_manager.map_or(false, PkgManager::needs_root),
            Method::Release { .. } | Method::Script { .. } | Method::Git { .. } => false,
        }
    }
}

impl Manifest {
    pub fn from_toml_str(src: &str, origin: &str) -> Result<Manifest> {
        let manifest: Manifest = toml::from_str(src).map_err(|source| Error::Parse {
            origin: origin.to_string(),
            source,
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn supports(&self, os: Os) -> bool {
        self.platforms.is_empty() || self.platforms.contains(&os)
    }

    /// Um passo por grupo: o primeiro de cada grupo que casa com o ambiente, na ordem
    /// em que os grupos aparecem no manifest.
    pub fn select_installs(&self, env: &Environment) -> Vec<&InstallStep> {
        let mut chosen = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for step in &self.install {
            let group = step.group_name();
            if seen.contains(group) {
                continue;
            }
            if step.when.matches(env) && step.method.usable_on(env) {
                seen.insert(group);
                chosen.push(step);
            }
        }
        chosen
    }

    /// Primeiro passo escolhido (atalho para módulos de um grupo só).
    pub fn select_install(&self, env: &Environment) -> Option<&InstallStep> {
        self.select_installs(env).into_iter().next()
    }

    pub fn validate(&self) -> Result<()> {
        let bad = |reason: String| Error::Invalid {
            id: self.id.clone(),
            reason,
        };

        if self.schema != SUPPORTED_SCHEMA {
            return Err(bad(format!(
                "schema {} não suportado (esperado {SUPPORTED_SCHEMA})",
                self.schema
            )));
        }
        if !is_valid_id(&self.id) {
            return Err(bad(
                "id deve ter só minúsculas, dígitos e '-' (máx. 64)".to_string(),
            ));
        }
        if self.name.trim().is_empty() {
            return Err(bad("name vazio".to_string()));
        }

        for (key, option) in &self.options {
            if let OptionKind::Choice {
                values, default, ..
            } = &option.kind
            {
                if values.is_empty() {
                    return Err(bad(format!("option '{key}': values vazio")));
                }
                if !values.contains(default) {
                    return Err(bad(format!(
                        "option '{key}': default '{default}' não está em values"
                    )));
                }
            }
        }

        for (key, option) in &self.options {
            match &option.kind {
                OptionKind::Color { default } if !crate::configs::is_hex_color(default) => {
                    return Err(bad(format!("option '{key}': default '{default}' não é uma cor #rrggbb")));
                }
                OptionKind::Keymaps { default } => {
                    crate::configs::check_keymaps(default)
                        .map_err(|e| bad(format!("option '{key}': default inválido: {e}")))?;
                }
                _ => {}
            }
            if let OptionKind::Number {
                min,
                max,
                default,
                divisor,
                ..
            } = &option.kind
            {
                if min > max {
                    return Err(bad(format!("option '{key}': min maior que max")));
                }
                if default < min || default > max {
                    return Err(bad(format!("option '{key}': default fora de [min, max]")));
                }
                if *divisor < 1 {
                    return Err(bad(format!("option '{key}': divisor deve ser >= 1")));
                }
            }
        }

        for (i, step) in self.install.iter().enumerate() {
            let at = format!("install[{i}]");
            match &step.method {
                Method::Pkg { names, .. } => {
                    if names.is_empty() {
                        return Err(bad(format!("{at}: names vazio")));
                    }
                    for name in names.keys() {
                        if PkgManager::from_name(name).is_none() {
                            return Err(bad(format!(
                                "{at}: gerenciador desconhecido '{name}'"
                            )));
                        }
                    }
                }
                Method::Release {
                    version,
                    url,
                    sha256,
                    dest,
                    bins,
                    strip_components,
                } => {
                    if *strip_components > 8 {
                        return Err(bad(format!("{at}: strip_components deve ser <= 8")));
                    }
                    if version.trim().is_empty() {
                        return Err(bad(format!("{at}: version vazio")));
                    }
                    check_download(&at, url, sha256).map_err(&bad)?;
                    if dest.trim().is_empty() {
                        return Err(bad(format!("{at}: dest vazio")));
                    }
                    for bin in bins {
                        if !is_safe_relative(&bin.src) {
                            return Err(bad(format!(
                                "{at}: bins.src '{}' deve ser relativo, sem '..'",
                                bin.src
                            )));
                        }
                    }
                }
                Method::Script { url, sha256 } => {
                    check_download(&at, url, sha256).map_err(&bad)?;
                }
                Method::Git { url, rev, dest } => {
                    if !url.starts_with("https://") {
                        return Err(bad(format!("{at}: url git deve começar com https://")));
                    }
                    if rev.len() != 40 || !rev.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Err(bad(format!(
                            "{at}: rev deve ser um commit completo (40 hexadecimais)"
                        )));
                    }
                    if dest.trim().is_empty() {
                        return Err(bad(format!("{at}: dest vazio")));
                    }
                }
            }

            if let Some(group) = &step.group {
                let valid = !group.is_empty()
                    && group.len() <= 40
                    && group
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
                if !valid {
                    return Err(bad(format!(
                        "{at}: group '{group}' deve ter só minúsculas, dígitos e '-'"
                    )));
                }
            }
            for command in &step.provides {
                if command.is_empty() || command.contains(|c: char| c.is_whitespace() || c == '/' || c == '\\') {
                    return Err(bad(format!("{at}: provides '{command}' deve ser um nome de comando simples")));
                }
            }
            if let Some(source) = &step.bump {
                check_bump(&at, source, &step.method).map_err(&bad)?;
            }
        }

        for (i, entry) in self.config.iter().enumerate() {
            if !is_safe_relative(&entry.src) {
                return Err(bad(format!(
                    "config[{i}]: src '{}' deve ser relativo, sem '..'",
                    entry.src
                )));
            }
            match (&entry.mode, &entry.block) {
                (ConfigMode::Block, None) => {
                    return Err(bad(format!("config[{i}]: mode = \"block\" exige a tabela [config.block]")))
                }
                (ConfigMode::Block, Some(block)) => {
                    let single_line = |t: &str| !t.is_empty() && !t.contains('\n') && !t.contains('\r');
                    if !single_line(&block.begin) || !single_line(&block.end) {
                        return Err(bad(format!("config[{i}]: block.begin/end devem ser linhas únicas e não vazias")));
                    }
                    if block.begin.trim() == block.end.trim() {
                        return Err(bad(format!("config[{i}]: block.begin e block.end devem ser diferentes")));
                    }
                    if let Some(program) = block.check.first() {
                        if program.trim().is_empty() {
                            return Err(bad(format!("config[{i}]: block.check com programa vazio")));
                        }
                    }
                }
                (_, Some(_)) => {
                    return Err(bad(format!("config[{i}]: [config.block] só vale com mode = \"block\"")))
                }
                _ => {}
            }
            if entry.template && entry.mode == ConfigMode::Symlink {
                return Err(bad(format!(
                    "config[{i}]: template não combina com mode = \"symlink\""
                )));
            }
            if let Dest::PerOs {
                unix: None,
                windows: None,
            } = entry.dest
            {
                return Err(bad(format!("config[{i}]: dest sem unix nem windows")));
            }
        }

        let mut hook_names = HashSet::new();
        for (i, hook) in self.hook.iter().enumerate() {
            let at = format!("hook[{i}]");
            let valid_name = !hook.name.is_empty()
                && hook.name.len() <= 40
                && hook
                    .name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if !valid_name {
                return Err(bad(format!("{at}: name '{}' deve ter só minúsculas, dígitos e '-'", hook.name)));
            }
            if !hook_names.insert(hook.name.as_str()) {
                return Err(bad(format!("{at}: name '{}' duplicado", hook.name)));
            }
            match (&hook.script, &hook.argv) {
                (Some(script), None) => {
                    if !is_safe_relative(script) {
                        return Err(bad(format!("{at}: script '{script}' deve ser relativo, sem '..'")));
                    }
                    if !(script.ends_with(".sh") || script.ends_with(".ps1")) {
                        return Err(bad(format!("{at}: script deve terminar em .sh ou .ps1")));
                    }
                }
                (None, Some(argv)) => {
                    let program = argv.first().map(|p| p.trim()).unwrap_or("");
                    if program.is_empty() || program.contains(char::is_whitespace) {
                        return Err(bad(format!("{at}: argv deve começar com um programa sem espaços")));
                    }
                }
                _ => return Err(bad(format!("{at}: use exatamente um entre script e argv"))),
            }
            for command in &hook.needs {
                if command.is_empty() || command.contains(|c: char| c.is_whitespace() || c == '/' || c == '\\') {
                    return Err(bad(format!("{at}: needs '{command}' deve ser um nome de comando simples")));
                }
            }
        }

        Ok(())
    }

    /// Avisos que não invalidam o manifest, mas impedem de publicá-lo.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (i, step) in self.install.iter().enumerate() {
            let (kind, value) = match &step.method {
                Method::Release { sha256, .. } | Method::Script { sha256, .. } => ("sha256", sha256),
                Method::Git { rev, .. } => ("rev", rev),
                Method::Pkg { .. } => continue,
            };
            if value.chars().all(|c| c == '0') {
                out.push(format!(
                    "{}: install[{i}] usa {kind} de placeholder (só zeros)",
                    self.id
                ));
            }
        }
        out
    }
}

fn check_bump(at: &str, source: &BumpSource, method: &Method) -> std::result::Result<(), String> {
    match method {
        Method::Git { .. } => {
            if source.github.is_some()
                || source.index_url.is_some()
                || source.redirect.is_some()
                || source.tag_prefix.is_some()
            {
                return Err(format!("{at}: bump de passo git aceita só `branch`"));
            }
            Ok(())
        }
        Method::Release { .. } => {
            let kinds = [
                source.github.is_some(),
                source.index_url.is_some(),
                source.redirect.is_some(),
            ];
            if kinds.iter().filter(|k| **k).count() != 1 {
                return Err(format!(
                    "{at}: bump deve ter exatamente uma fonte (github, index_url ou redirect)"
                ));
            }
            if let Some(repo) = &source.github {
                let parts: Vec<&str> = repo.split('/').collect();
                if parts.len() != 2 || parts.iter().any(|p| p.is_empty()) {
                    return Err(format!("{at}: bump.github deve ser 'dono/repo'"));
                }
            }
            if let Some(url) = &source.index_url {
                if !url.starts_with("https://") {
                    return Err(format!("{at}: bump.index_url deve começar com https://"));
                }
                let prefix = source.index_prefix.as_deref().unwrap_or("");
                let suffix = source.index_suffix.as_deref().unwrap_or("");
                if prefix.is_empty() || suffix.is_empty() {
                    return Err(format!("{at}: bump.index_url exige index_prefix e index_suffix"));
                }
            }
            if let Some(url) = &source.redirect {
                if !url.starts_with("https://") {
                    return Err(format!("{at}: bump.redirect deve começar com https://"));
                }
            }
            Ok(())
        }
        Method::Pkg { .. } | Method::Script { .. } => {
            Err(format!("{at}: bump só vale para passos release e git"))
        }
    }
}

fn check_download(at: &str, url: &str, sha256: &str) -> std::result::Result<(), String> {
    if !url.starts_with("https://") {
        return Err(format!("{at}: url deve começar com https://"));
    }
    if !is_sha256(sha256) {
        return Err(format!("{at}: sha256 deve ter 64 dígitos hexadecimais"));
    }
    Ok(())
}

fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-')
        && !id.ends_with('-')
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

pub(crate) fn is_safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(':')
        && !path.contains('\\')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
mod tests {
    #[test]
    fn options_keep_declared_order_and_metadata() {
        let toml = r##"
schema = 1
id = "ed"
name = "ed"
[options.zeta]
type = "bool"
default = true
label = "Zeta"
section = "Aparência"
[options.alfa]
type = "color"
default = "#112233"
[options.mapas]
type = "keymaps"
section = "Atalhos"
[options.extra]
type = "lines"
[options.tema]
type = "choice"
values = ["a", "b"]
default = "a"
labels = { a = "Tema A" }
preview = "../t/{value}.vim"
"##;
        let m = super::Manifest::from_toml_str(toml, "t").unwrap();
        let keys: Vec<&String> = m.options.keys().collect();
        assert_eq!(keys, ["zeta", "alfa", "mapas", "extra", "tema"], "ordem do arquivo");
        assert_eq!(m.options["zeta"].label.as_deref(), Some("Zeta"));
        assert_eq!(m.options["zeta"].section.as_deref(), Some("Aparência"));
        assert_eq!(m.options["mapas"].default_text(), "[]");
        assert_eq!(m.options["tema"].preview.as_deref(), Some("../t/{value}.vim"));
        assert!(matches!(&m.options["tema"].kind, super::OptionKind::Choice { labels, .. } if labels["a"] == "Tema A"));

        let bad = toml.replace("\"#112233\"", "\"azul\"");
        assert!(super::Manifest::from_toml_str(&bad, "t").is_err(), "cor padrão inválida é recusada");
    }

    use super::*;
    use crate::env::Privilege;

    const SAMPLE: &str = r#"
schema = 1
id = "kitty"
name = "kitty"
platforms = ["linux", "macos"]

[options.style]
type = "choice"
values = ["transparent", "solid"]
default = "transparent"

[[install]]
when = { os = "linux", root = true }
method = "pkg"
names = { dnf = "kitty", apt = "kitty" }

[[install]]
when = { os = "linux", arch = "x86_64" }
method = "release"
version = "1.0.0"
url = "https://example.com/kitty-{version}.txz"
sha256 = "SHA"
dest = "~/.local/kitty.app"
bins = [{ src = "bin/kitty", link = "~/.local/bin/kitty" }]

[[config]]
src = "files/kitty"
dest = { unix = "~/.config/kitty" }
template = true
"#;

    fn sample() -> String {
        SAMPLE.replace("SHA", &"a".repeat(64))
    }

    fn env(pm: Option<PkgManager>, use_root: bool) -> Environment {
        Environment {
            os: Os::Linux,
            arch: Some(Arch::X86_64),
            distro: None,
            pkg_manager: pm,
            privilege: if use_root {
                Privilege::Sudo
            } else {
                Privilege::None
            },
            use_root,
            home: None,
        }
    }

    #[test]
    fn parses_sample() {
        let m = Manifest::from_toml_str(&sample(), "sample").unwrap();
        assert_eq!(m.id, "kitty");
        assert_eq!(m.install.len(), 2);
        assert_eq!(m.config[0].mode, ConfigMode::Merge);
        assert!(m.config[0].backup);
        assert!(m.supports(Os::Linux));
        assert!(!m.supports(Os::Windows));
    }

    #[test]
    fn system_mode_prefers_pkg() {
        let m = Manifest::from_toml_str(&sample(), "sample").unwrap();
        let env = env(Some(PkgManager::Dnf), true);
        let step = m.select_install(&env).unwrap();
        assert!(matches!(step.method, Method::Pkg { .. }));
        assert!(step.needs_root(&env));
    }

    #[test]
    fn user_mode_falls_back_to_release() {
        let m = Manifest::from_toml_str(&sample(), "sample").unwrap();
        let env = env(Some(PkgManager::Dnf), false);
        let step = m.select_install(&env).unwrap();
        assert!(matches!(step.method, Method::Release { .. }));
        assert!(!step.needs_root(&env));
    }

    #[test]
    fn unknown_distro_in_system_mode_falls_back_to_release() {
        let m = Manifest::from_toml_str(&sample(), "sample").unwrap();
        let step = m.select_install(&env(None, true)).unwrap();
        assert!(matches!(step.method, Method::Release { .. }));
    }

    #[test]
    fn no_step_when_nothing_matches() {
        let m = Manifest::from_toml_str(&sample(), "sample").unwrap();
        let mut aarch = env(None, false);
        aarch.arch = Some(Arch::Aarch64);
        assert!(m.select_install(&aarch).is_none());
    }

    #[test]
    fn dest_resolves_per_os() {
        let m = Manifest::from_toml_str(&sample(), "sample").unwrap();
        assert_eq!(m.config[0].dest.resolve(Os::Linux), Some("~/.config/kitty"));
        assert_eq!(m.config[0].dest.resolve(Os::Windows), None);
    }

    #[test]
    fn rejects_bad_sha256() {
        let src = SAMPLE.replace("SHA", "123");
        assert!(Manifest::from_toml_str(&src, "x").is_err());
    }

    #[test]
    fn rejects_non_https_url() {
        let src = sample().replace("https://example.com", "http://example.com");
        assert!(Manifest::from_toml_str(&src, "x").is_err());
    }

    #[test]
    fn rejects_path_traversal_in_config_src() {
        let src = sample().replace("files/kitty", "../../etc/passwd");
        assert!(Manifest::from_toml_str(&src, "x").is_err());
    }

    #[test]
    fn rejects_choice_default_outside_values() {
        let src = sample().replace("default = \"transparent\"", "default = \"nope\"");
        assert!(Manifest::from_toml_str(&src, "x").is_err());
    }

    #[test]
    fn rejects_unknown_schema_and_bad_id() {
        assert!(Manifest::from_toml_str(&sample().replace("schema = 1", "schema = 2"), "x").is_err());
        assert!(Manifest::from_toml_str(&sample().replace("id = \"kitty\"", "id = \"Kitty!\""), "x").is_err());
    }

    #[test]
    fn rejects_unknown_pkg_manager() {
        let src = sample().replace("dnf = \"kitty\"", "emerge = \"kitty\"");
        assert!(Manifest::from_toml_str(&src, "x").is_err());
    }

    const GROUPED: &str = r#"
schema = 1
id = "vim"
name = "vim"

[[install]]
when = { os = "linux", root = true }
method = "pkg"
names = { dnf = "vim" }
provides = ["sh"]

[[install]]
group = "plugin-airline"
optional = true
method = "git"
url = "https://github.com/vim-airline/vim-airline"
rev = "0123456789abcdef0123456789abcdef01234567"
dest = "~/.vim/pack/plugins/opt/vim-airline"
bump = { branch = "master" }

[[install]]
group = "plugin-airline"
when = { os = "windows" }
method = "git"
url = "https://github.com/vim-airline/vim-airline"
rev = "0123456789abcdef0123456789abcdef01234567"
dest = "~/vimfiles/pack/plugins/opt/vim-airline"

[[install]]
group = "tool"
method = "release"
version = "1.0.0"
url = "https://example.com/tool-{version}.tar.xz"
sha256 = "SHA"
dest = "~/.local/tool"
strip_components = 1
bump = { github = "owner/tool" }
"#;

    fn grouped() -> Manifest {
        Manifest::from_toml_str(&GROUPED.replace("SHA", &"a".repeat(64)), "grouped").unwrap()
    }

    #[test]
    fn groups_are_all_installed_but_alternatives_are_not() {
        let m = grouped();
        // linux em modo --system com dnf: pacote + plugin (variante linux) + tool
        let system = env(Some(PkgManager::Dnf), true);
        let chosen = m.select_installs(&system);
        let groups: Vec<&str> = chosen.iter().map(|s| s.group_name()).collect();
        assert_eq!(groups, vec!["main", "plugin-airline", "tool"]);
        // dentro de plugin-airline vale o primeiro que casa (o sem `when`)
        assert!(matches!(&chosen[1].method, Method::Git { dest, .. } if dest.starts_with("~/.vim/")));

        // modo --user: o pacote (root = true) cai fora, os outros grupos continuam
        let user = env(Some(PkgManager::Dnf), false);
        let groups: Vec<&str> = m.select_installs(&user).iter().map(|s| s.group_name()).collect();
        assert_eq!(groups, vec!["plugin-airline", "tool"]);
    }

    #[test]
    fn step_fields_parse() {
        let m = grouped();
        assert_eq!(m.install[0].provides, vec!["sh"]);
        assert!(m.install[1].optional);
        assert_eq!(m.install[1].bump.as_ref().unwrap().branch.as_deref(), Some("master"));
        assert!(!m.install[0].optional);
        assert!(matches!(&m.install[3].method, Method::Release { strip_components: 1, .. }));
        assert_eq!(m.install[3].bump.as_ref().unwrap().github.as_deref(), Some("owner/tool"));
    }

    #[test]
    fn rejects_bad_git_steps() {
        let short_rev = GROUPED
            .replace("SHA", &"a".repeat(64))
            .replace("0123456789abcdef0123456789abcdef01234567", "abc123");
        assert!(Manifest::from_toml_str(&short_rev, "x").is_err());
        let http = GROUPED
            .replace("SHA", &"a".repeat(64))
            .replace("https://github.com/vim-airline", "http://github.com/vim-airline");
        assert!(Manifest::from_toml_str(&http, "x").is_err());
    }

    #[test]
    fn rejects_bad_groups_and_provides() {
        let base = GROUPED.replace("SHA", &"a".repeat(64));
        assert!(Manifest::from_toml_str(&base.replace("plugin-airline", "Plugin Airline"), "x").is_err());
        assert!(Manifest::from_toml_str(&base.replace("provides = [\"sh\"]", "provides = [\"/bin/sh\"]"), "x").is_err());
    }

    #[test]
    fn rejects_inconsistent_bump_sources() {
        let base = GROUPED.replace("SHA", &"a".repeat(64));
        // release sem fonte
        assert!(Manifest::from_toml_str(&base.replace("bump = { github = \"owner/tool\" }", "bump = {}"), "x").is_err());
        // duas fontes
        assert!(Manifest::from_toml_str(
            &base.replace(
                "bump = { github = \"owner/tool\" }",
                "bump = { github = \"owner/tool\", redirect = \"https://example.com/latest\" }"
            ),
            "x"
        )
        .is_err());
        // github malformado
        assert!(Manifest::from_toml_str(&base.replace("owner/tool", "so-um-nome"), "x").is_err());
        // index sem prefixo/sufixo
        assert!(Manifest::from_toml_str(
            &base.replace(
                "bump = { github = \"owner/tool\" }",
                "bump = { index_url = \"https://example.com/dist/\" }"
            ),
            "x"
        )
        .is_err());
        // passo git não aceita github
        assert!(Manifest::from_toml_str(
            &base.replace("bump = { branch = \"master\" }", "bump = { github = \"a/b\" }"),
            "x"
        )
        .is_err());
        // bump em pkg
        assert!(Manifest::from_toml_str(
            &base.replace("provides = [\"sh\"]", "provides = [\"sh\"]\nbump = { branch = \"x\" }"),
            "x"
        )
        .is_err());
    }

    #[test]
    fn rejects_excessive_strip_components() {
        let base = GROUPED.replace("SHA", &"a".repeat(64));
        assert!(Manifest::from_toml_str(&base.replace("strip_components = 1", "strip_components = 9"), "x").is_err());
    }

    #[test]
    fn git_steps_never_need_root_and_have_no_checksum_warning() {
        let m = grouped();
        let system = env(Some(PkgManager::Dnf), true);
        let chosen = m.select_installs(&system);
        assert!(!chosen[1].needs_root(&system));
        assert!(m.warnings().is_empty());
    }

    const EXTRAS: &str = r##"
schema = 1
id = "shell"
name = "shell"

[options.opacity]
type = "number"
min = 30
max = 100
default = 85
divisor = 100

[[config]]
src = "files/block.sh"
dest = { unix = "~/.bashrc" }
mode = "block"

[config.block]
begin = "# >>> hiraku >>>"
end = "# <<< hiraku <<<"
check = ["bash", "-n", "{path}"]

[[config]]
src = "files/lock.json"
dest = { unix = "~/.config/x/lock.json" }
mode = "seed"

[[hook]]
name = "fc-cache"
script = "hooks/fc-cache.sh"
on = "once"
needs = ["fc-cache"]
optional = true

[[hook]]
name = "bootstrap"
argv = ["nvim", "--headless", "+qa"]
"##;

    fn extras() -> Manifest {
        Manifest::from_toml_str(EXTRAS, "extras").unwrap()
    }

    #[test]
    fn block_seed_number_and_hooks_parse() {
        let m = extras();
        assert!(matches!(&m.options["opacity"].kind, OptionKind::Number { min: 30, max: 100, default: 85, divisor: 100, .. }));
        assert_eq!(m.config[0].mode, ConfigMode::Block);
        let block = m.config[0].block.as_ref().unwrap();
        assert_eq!(block.begin, "# >>> hiraku >>>");
        assert_eq!(block.check, vec!["bash", "-n", "{path}"]);
        assert!(!block.only_if_exists);
        assert_eq!(m.config[1].mode, ConfigMode::Seed);
        assert_eq!(m.hook.len(), 2);
        assert_eq!(m.hook[0].on, HookTrigger::Once);
        assert!(m.hook[0].optional && !m.hook[0].root);
        assert_eq!(m.hook[1].on, HookTrigger::Change, "padrão");
        assert_eq!(m.hook[1].argv.as_ref().unwrap()[0], "nvim");
    }

    #[test]
    fn number_option_defaults_to_divisor_one_and_is_validated() {
        let no_divisor = EXTRAS.replace("divisor = 100\n", "");
        let m = Manifest::from_toml_str(&no_divisor, "x").unwrap();
        assert!(matches!(&m.options["opacity"].kind, OptionKind::Number { divisor: 1, .. }));
        assert!(Manifest::from_toml_str(&EXTRAS.replace("default = 85", "default = 20"), "x").is_err());
        assert!(Manifest::from_toml_str(&EXTRAS.replace("min = 30", "min = 200"), "x").is_err());
        assert!(Manifest::from_toml_str(&EXTRAS.replace("divisor = 100", "divisor = 0"), "x").is_err());
    }

    #[test]
    fn block_mode_requires_a_valid_block_table() {
        // sem a tabela
        let without = EXTRAS
            .replace("[config.block]\n", "")
            .replace("begin = \"# >>> hiraku >>>\"\n", "")
            .replace("end = \"# <<< hiraku <<<\"\n", "")
            .replace("check = [\"bash\", \"-n\", \"{path}\"]\n", "");
        assert!(Manifest::from_toml_str(&without, "x").is_err());
        // begin == end
        assert!(Manifest::from_toml_str(&EXTRAS.replace("# <<< hiraku <<<", "# >>> hiraku >>>"), "x").is_err());
        // begin vazio
        assert!(Manifest::from_toml_str(&EXTRAS.replace("begin = \"# >>> hiraku >>>\"", "begin = \"\""), "x").is_err());
        // block fora do modo block
        assert!(Manifest::from_toml_str(&EXTRAS.replace("mode = \"block\"", "mode = \"merge\""), "x").is_err());
    }

    #[test]
    fn rejects_bad_hooks() {
        // nem script nem argv
        assert!(Manifest::from_toml_str(&EXTRAS.replace("script = \"hooks/fc-cache.sh\"\n", ""), "x").is_err());
        // os dois
        assert!(Manifest::from_toml_str(
            &EXTRAS.replace("script = \"hooks/fc-cache.sh\"", "script = \"hooks/fc-cache.sh\"\nargv = [\"x\"]"),
            "x"
        )
        .is_err());
        // extensão inválida e caminho fora do módulo
        assert!(Manifest::from_toml_str(&EXTRAS.replace("hooks/fc-cache.sh", "hooks/fc-cache.py"), "x").is_err());
        assert!(Manifest::from_toml_str(&EXTRAS.replace("hooks/fc-cache.sh", "../fora.sh"), "x").is_err());
        // nome inválido e duplicado
        assert!(Manifest::from_toml_str(&EXTRAS.replace("name = \"fc-cache\"", "name = \"FC Cache\""), "x").is_err());
        assert!(Manifest::from_toml_str(&EXTRAS.replace("name = \"bootstrap\"", "name = \"fc-cache\""), "x").is_err());
        // argv com programa com espaço, needs inválido
        assert!(Manifest::from_toml_str(&EXTRAS.replace("[\"nvim\",", "[\"meu nvim\","), "x").is_err());
        assert!(Manifest::from_toml_str(&EXTRAS.replace("needs = [\"fc-cache\"]", "needs = [\"/usr/bin/fc-cache\"]"), "x").is_err());
    }

    #[test]
    fn warns_on_placeholder_git_rev() {
        let src = GROUPED
            .replace("SHA", &"a".repeat(64))
            .replacen("0123456789abcdef0123456789abcdef01234567", &"0".repeat(40), 1);
        let m = Manifest::from_toml_str(&src, "x").unwrap();
        let warnings = m.warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("rev"));
    }

    #[test]
    fn warns_on_placeholder_checksum() {
        let src = SAMPLE.replace("SHA", &"0".repeat(64));
        let m = Manifest::from_toml_str(&src, "x").unwrap();
        assert_eq!(m.warnings().len(), 1);
        let ok = Manifest::from_toml_str(&sample(), "x").unwrap();
        assert!(ok.warnings().is_empty());
    }
}
