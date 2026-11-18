//! `hiraku registry bump`: descobre a versão mais recente de cada componente do
//! registry (via `bump = {...}` no manifest), baixa o arquivo, calcula o sha256 e
//! reescreve `version`/`sha256` (ou `rev`) preservando comentários e formatação.
//!
//! É a ferramenta de quem MANTÉM o registry (ou do CI dele): o usuário final nunca
//! roda isto. Os valores resultantes são fixados e assinados no registry publicado.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

use crate::error::{io_err, Error, Result};
use crate::manifest::Manifest;
use crate::paths::render_version;
use crate::release::to_hex;

/// Tudo que precisa da rede (ou do `git`), atrás de uma trait para os testes.
pub trait Resolver {
    /// Tag do último release de `dono/repo` (ex.: `v0.10.2`).
    fn latest_tag(&self, repo: &str) -> Result<String>;
    /// Versão encontrada numa página de índice: o texto entre `prefix` e `suffix`.
    fn index_version(&self, url: &str, prefix: &str, suffix: &str) -> Result<String>;
    /// URL do arquivo versionado para onde `url` ("sempre a última") redireciona.
    fn redirect_target(&self, url: &str) -> Result<String>;
    fn sha256_of(&self, url: &str) -> Result<String>;
    /// Commit mais recente de `branch` (ou HEAD).
    fn git_head(&self, url: &str, branch: Option<&str>) -> Result<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub module: String,
    pub group: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Default)]
pub struct BumpReport {
    pub changes: Vec<Change>,
    /// (módulo, erro): um módulo com problema não impede os outros.
    pub errors: Vec<(String, String)>,
    /// (antes, depois), se o `registry.toml` foi atualizado.
    pub registry_version: Option<(u64, u64)>,
}

/// Memoriza as respostas: várias arquiteturas do mesmo programa compartilham a tag.
pub struct CachingResolver<'a> {
    inner: &'a dyn Resolver,
    cache: RefCell<HashMap<String, String>>,
}

impl<'a> CachingResolver<'a> {
    pub fn new(inner: &'a dyn Resolver) -> CachingResolver<'a> {
        CachingResolver {
            inner,
            cache: RefCell::new(HashMap::new()),
        }
    }

    fn cached(&self, key: String, fetch: impl FnOnce() -> Result<String>) -> Result<String> {
        if let Some(hit) = self.cache.borrow().get(&key) {
            return Ok(hit.clone());
        }
        let value = fetch()?;
        self.cache.borrow_mut().insert(key, value.clone());
        Ok(value)
    }
}

impl Resolver for CachingResolver<'_> {
    fn latest_tag(&self, repo: &str) -> Result<String> {
        self.cached(format!("tag:{repo}"), || self.inner.latest_tag(repo))
    }
    fn index_version(&self, url: &str, prefix: &str, suffix: &str) -> Result<String> {
        self.cached(format!("index:{url}|{prefix}|{suffix}"), || {
            self.inner.index_version(url, prefix, suffix)
        })
    }
    fn redirect_target(&self, url: &str) -> Result<String> {
        self.cached(format!("redirect:{url}"), || self.inner.redirect_target(url))
    }
    fn sha256_of(&self, url: &str) -> Result<String> {
        self.cached(format!("sha:{url}"), || self.inner.sha256_of(url))
    }
    fn git_head(&self, url: &str, branch: Option<&str>) -> Result<String> {
        self.cached(format!("head:{url}|{}", branch.unwrap_or("")), || {
            self.inner.git_head(url, branch)
        })
    }
}

#[derive(Default)]
struct Source {
    github: Option<String>,
    tag_prefix: Option<String>,
    index_url: Option<String>,
    index_prefix: Option<String>,
    index_suffix: Option<String>,
    redirect: Option<String>,
    branch: Option<String>,
}

fn field(table: &dyn TableLike, key: &str) -> Option<String> {
    table.get(key)?.as_str().map(str::to_string)
}

fn read_source(step: &Table) -> Option<Source> {
    let bump = step.get("bump")?.as_table_like()?;
    Some(Source {
        github: field(bump, "github"),
        tag_prefix: field(bump, "tag_prefix"),
        index_url: field(bump, "index_url"),
        index_prefix: field(bump, "index_prefix"),
        index_suffix: field(bump, "index_suffix"),
        redirect: field(bump, "redirect"),
        branch: field(bump, "branch"),
    })
}

/// Troca o valor mantendo o comentário que vem depois dele na linha.
fn set_str(table: &mut Table, key: &str, new: &str) {
    match table.get_mut(key).and_then(Item::as_value_mut) {
        Some(value) => {
            let decor = value.decor().clone();
            *value = Value::from(new);
            *value.decor_mut() = decor;
        }
        None => table[key] = toml_edit::value(new),
    }
}

fn text_of(table: &Table, key: &str) -> String {
    table
        .get(key)
        .and_then(Item::as_str)
        .unwrap_or("")
        .to_string()
}

/// Último segmento do caminho da URL, sem query nem fragmento.
pub(crate) fn last_segment(url: &str) -> String {
    let clean = url.split(|c| c == '?' || c == '#').next().unwrap_or(url);
    clean.rsplit('/').next().unwrap_or("").to_string()
}

fn short(rev: &str) -> String {
    rev.chars().take(12).collect()
}

fn bump_release(
    module: &str,
    group: &str,
    table: &mut Table,
    source: &Source,
    resolver: &dyn Resolver,
    changes: &mut Vec<Change>,
) -> Result<()> {
    let current_version = text_of(table, "version");
    let current_sha = text_of(table, "sha256");
    let url_template = text_of(table, "url");

    let (version, url, literal_url) = if let Some(repo) = &source.github {
        let tag = resolver.latest_tag(repo)?;
        let prefix = source.tag_prefix.as_deref().unwrap_or("v");
        let version = tag.strip_prefix(prefix).unwrap_or(&tag).to_string();
        let url = render_version(&url_template, &version);
        (version, url, None)
    } else if let Some(index) = &source.index_url {
        let version = resolver.index_version(
            index,
            source.index_prefix.as_deref().unwrap_or(""),
            source.index_suffix.as_deref().unwrap_or(""),
        )?;
        let url = render_version(&url_template, &version);
        (version, url, None)
    } else if let Some(entry) = &source.redirect {
        let target = resolver.redirect_target(entry)?;
        (last_segment(&target), target.clone(), Some(target))
    } else {
        return Ok(());
    };

    let placeholder = current_sha.is_empty() || current_sha.chars().all(|c| c == '0');
    let url_unchanged = literal_url.as_deref().map_or(true, |u| u == url_template);
    if version == current_version && !placeholder && url_unchanged {
        return Ok(());
    }

    let sha = resolver.sha256_of(&url)?;
    set_str(table, "version", &version);
    set_str(table, "sha256", &sha);
    if let Some(literal) = literal_url {
        set_str(table, "url", &literal);
    }
    changes.push(Change {
        module: module.to_string(),
        group: group.to_string(),
        from: if current_version.is_empty() { "-".to_string() } else { current_version },
        to: version,
    });
    Ok(())
}

fn bump_git(
    module: &str,
    group: &str,
    table: &mut Table,
    source: &Source,
    resolver: &dyn Resolver,
    changes: &mut Vec<Change>,
) -> Result<()> {
    let url = text_of(table, "url");
    let current = text_of(table, "rev");
    let head = resolver.git_head(&url, source.branch.as_deref())?;
    if head == current {
        return Ok(());
    }
    set_str(table, "rev", &head);
    changes.push(Change {
        module: module.to_string(),
        group: group.to_string(),
        from: short(&current),
        to: short(&head),
    });
    Ok(())
}

/// Reescreve o texto de UM manifest. Passos sem `bump` não são tocados.
pub fn bump_manifest_text(
    module: &str,
    src: &str,
    resolver: &dyn Resolver,
) -> Result<(String, Vec<Change>)> {
    let mut doc: DocumentMut = src
        .parse()
        .map_err(|e: toml_edit::TomlError| Error::Unsupported(format!("{module}: TOML inválido: {e}")))?;
    let mut changes = Vec::new();

    if let Some(steps) = doc.get_mut("install").and_then(Item::as_array_of_tables_mut) {
        for table in steps.iter_mut() {
            let group = table
                .get("group")
                .and_then(Item::as_str)
                .unwrap_or("main")
                .to_string();
            let Some(source) = read_source(table) else {
                continue;
            };
            match table.get("method").and_then(Item::as_str) {
                Some("release") => bump_release(module, &group, table, &source, resolver, &mut changes)?,
                Some("git") => bump_git(module, &group, table, &source, resolver, &mut changes)?,
                _ => {}
            }
        }
    }
    Ok((doc.to_string(), changes))
}

/// Soma 1 em `version` do `registry.toml`. Devolve (antes, depois).
pub fn bump_registry_version(path: &Path) -> Result<(u64, u64)> {
    let text = fs::read_to_string(path).map_err(io_err(path))?;
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
        Error::Unsupported(format!("{}: TOML inválido: {e}", path.display()))
    })?;
    let before = doc
        .get("version")
        .and_then(Item::as_integer)
        .filter(|v| *v >= 0)
        .ok_or_else(|| Error::Unsupported(format!("{}: falta `version`", path.display())))? as u64;
    doc["version"] = toml_edit::value((before + 1) as i64);
    write_atomic(path, &doc.to_string())?;
    Ok((before, before + 1))
}

fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, text).map_err(io_err(&tmp))?;
    fs::rename(&tmp, path).map_err(io_err(path))
}

/// Atualiza todos os módulos de `modules_dir` (ou só os de `only`). Com `check_only`
/// nada é gravado. Se algo mudou, sobe a `version` do `registry.toml` ao lado.
pub fn bump_dir(
    modules_dir: &Path,
    only: &[String],
    check_only: bool,
    resolver: &dyn Resolver,
) -> Result<BumpReport> {
    let entries = fs::read_dir(modules_dir).map_err(io_err(modules_dir))?;
    let mut files: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let file = e.path().join("module.toml");
            file.is_file()
                .then(|| (e.file_name().to_string_lossy().to_string(), file))
        })
        .filter(|(id, _)| only.is_empty() || only.contains(id))
        .collect();
    files.sort();

    let caching = CachingResolver::new(resolver);
    let mut report = BumpReport::default();
    for (id, file) in files {
        let outcome = (|| -> Result<Vec<Change>> {
            let src = fs::read_to_string(&file).map_err(io_err(&file))?;
            let (updated, changes) = bump_manifest_text(&id, &src, &caching)?;
            if changes.is_empty() || check_only {
                return Ok(changes);
            }
            // Nunca grava um manifest que deixou de validar.
            Manifest::from_toml_str(&updated, &file.display().to_string())?;
            write_atomic(&file, &updated)?;
            Ok(changes)
        })();
        match outcome {
            Ok(changes) => report.changes.extend(changes),
            Err(e) => report.errors.push((id, e.to_string())),
        }
    }

    if !report.changes.is_empty() && !check_only {
        if let Some(meta) = modules_dir.parent().map(|p| p.join("registry.toml")) {
            if meta.is_file() {
                report.registry_version = Some(bump_registry_version(&meta)?);
            }
        }
    }
    Ok(report)
}

/// O texto entre `prefix` e `suffix` numa página de índice. Para cada ocorrência do
/// sufixo, olha o prefixo mais próximo antes dela; só aceita caracteres de versão.
pub(crate) fn extract_version(text: &str, prefix: &str, suffix: &str) -> Option<String> {
    if prefix.is_empty() || suffix.is_empty() {
        return None;
    }
    let valid = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-');
    for (position, _) in text.match_indices(suffix) {
        let head = &text[..position];
        if let Some(start) = head.rfind(prefix) {
            let candidate = &head[start + prefix.len()..];
            if !candidate.is_empty() && candidate.len() <= 40 && candidate.chars().all(valid) {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Resolve um `Location` possivelmente relativo contra a URL de origem.
pub(crate) fn absolutize(base: &str, location: &str) -> String {
    if location.starts_with("http://") || location.starts_with("https://") {
        return location.to_string();
    }
    let origin_end = base
        .find("://")
        .and_then(|scheme| base[scheme + 3..].find('/').map(|slash| scheme + 3 + slash))
        .unwrap_or(base.len());
    format!("{}{}", &base[..origin_end], location)
}

pub(crate) fn looks_like_archive(url: &str) -> bool {
    let name = last_segment(url).to_ascii_lowercase();
    [".tar.gz", ".tgz", ".tar.xz", ".txz", ".zip", ".tar.bz2"]
        .iter()
        .any(|ext| name.ends_with(ext))
}

/// Resolver de verdade: ureq para a rede e o binário `git` para `ls-remote`.
pub struct NetResolver;

fn agent(redirects: u32) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .redirects(redirects)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(120))
        .build()
}

fn net_error(url: &str, reason: impl ToString) -> Error {
    Error::Download {
        url: url.to_string(),
        reason: reason.to_string(),
    }
}

/// `Location` da primeira resposta, sem seguir o redirecionamento.
fn location_of(url: &str) -> Result<Option<String>> {
    let response = match agent(0).get(url).call() {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(e) => return Err(net_error(url, e)),
    };
    Ok(response
        .header("location")
        .map(|location| absolutize(url, location)))
}

impl Resolver for NetResolver {
    fn latest_tag(&self, repo: &str) -> Result<String> {
        let url = format!("https://github.com/{repo}/releases/latest");
        let location = location_of(&url)?
            .ok_or_else(|| net_error(&url, "o GitHub não redirecionou para uma tag"))?;
        let tag = location
            .split_once("/releases/tag/")
            .map(|(_, tag)| percent_decode(tag))
            .filter(|tag| !tag.is_empty())
            .ok_or_else(|| net_error(&url, format!("redirecionamento inesperado: {location}")))?;
        Ok(tag)
    }

    fn index_version(&self, url: &str, prefix: &str, suffix: &str) -> Result<String> {
        let mut text = String::new();
        agent(5)
            .get(url)
            .call()
            .map_err(|e| net_error(url, e))?
            .into_reader()
            .take(4 * 1024 * 1024)
            .read_to_string(&mut text)
            .map_err(|e| net_error(url, e))?;
        extract_version(&text, prefix, suffix)
            .ok_or_else(|| net_error(url, format!("não achei '{prefix}…{suffix}' na página")))
    }

    fn redirect_target(&self, url: &str) -> Result<String> {
        let mut current = url.to_string();
        for _ in 0..6 {
            let next = location_of(&current)?
                .ok_or_else(|| net_error(&current, "não redireciona para um arquivo"))?;
            if looks_like_archive(&next) {
                return Ok(next);
            }
            current = next;
        }
        Err(net_error(url, "redirecionamentos demais sem chegar a um arquivo"))
    }

    fn sha256_of(&self, url: &str) -> Result<String> {
        let response = agent(5).get(url).call().map_err(|e| net_error(url, e))?;
        let mut reader = response.into_reader().take(2 * 1024 * 1024 * 1024);
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buffer).map_err(|e| net_error(url, e))?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(to_hex(&hasher.finalize()))
    }

    fn git_head(&self, url: &str, branch: Option<&str>) -> Result<String> {
        let reference = branch.map(|b| format!("refs/heads/{b}")).unwrap_or_else(|| "HEAD".to_string());
        let output = std::process::Command::new("git")
            .args(["ls-remote", url, &reference])
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| net_error(url, format!("não consegui executar git: {e}")))?;
        if !output.status.success() {
            return Err(net_error(url, String::from_utf8_lossy(&output.stderr).trim().to_string()));
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().next())
            .filter(|sha| sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()))
            .map(str::to_string)
            .ok_or_else(|| net_error(url, format!("ls-remote não achou '{reference}'")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::sha256_hex_of;

    #[derive(Default)]
    struct Fake {
        tags: HashMap<String, String>,
        versions: HashMap<String, String>,
        redirects: HashMap<String, String>,
        heads: HashMap<String, String>,
        calls: RefCell<Vec<String>>,
    }

    impl Fake {
        fn missing(what: &str) -> Error {
            Error::Download {
                url: what.to_string(),
                reason: "sem resposta no teste".to_string(),
            }
        }
        fn count(&self, prefix: &str) -> usize {
            self.calls.borrow().iter().filter(|c| c.starts_with(prefix)).count()
        }
    }

    impl Resolver for Fake {
        fn latest_tag(&self, repo: &str) -> Result<String> {
            self.calls.borrow_mut().push(format!("tag:{repo}"));
            self.tags.get(repo).cloned().ok_or_else(|| Fake::missing(repo))
        }
        fn index_version(&self, url: &str, _p: &str, _s: &str) -> Result<String> {
            self.calls.borrow_mut().push(format!("index:{url}"));
            self.versions.get(url).cloned().ok_or_else(|| Fake::missing(url))
        }
        fn redirect_target(&self, url: &str) -> Result<String> {
            self.calls.borrow_mut().push(format!("redirect:{url}"));
            self.redirects.get(url).cloned().ok_or_else(|| Fake::missing(url))
        }
        fn sha256_of(&self, url: &str) -> Result<String> {
            self.calls.borrow_mut().push(format!("sha:{url}"));
            Ok(sha256_hex_of(url.as_bytes()))
        }
        fn git_head(&self, url: &str, _branch: Option<&str>) -> Result<String> {
            self.calls.borrow_mut().push(format!("head:{url}"));
            self.heads.get(url).cloned().ok_or_else(|| Fake::missing(url))
        }
    }

    const TOOL: &str = r#"# comentário do topo
schema = 1
id = "tool"
name = "tool"

[[install]]
when = { os = "linux", arch = "x86_64" }
method = "release"
version = "1.0.0" # versão atual
url = "https://github.com/owner/tool/releases/download/v{version}/tool-{version}-x86_64.tar.gz"
sha256 = "AAAA"
dest = "~/.local/tool"
bump = { github = "owner/tool" }

[[install]]
when = { os = "linux", arch = "aarch64" }
method = "release"
version = "1.0.0"
url = "https://github.com/owner/tool/releases/download/v{version}/tool-{version}-aarch64.tar.gz"
sha256 = "BBBB"
dest = "~/.local/tool"
bump = { github = "owner/tool" }
"#;

    fn tool() -> String {
        TOOL.replace("AAAA", &"a".repeat(64)).replace("BBBB", &"b".repeat(64))
    }

    fn fake_with_tag(tag: &str) -> Fake {
        let mut fake = Fake::default();
        fake.tags.insert("owner/tool".to_string(), tag.to_string());
        fake
    }

    const X86_URL: &str = "https://github.com/owner/tool/releases/download/v1.2.0/tool-1.2.0-x86_64.tar.gz";

    #[test]
    fn bumps_every_step_and_preserves_comments_and_layout() {
        let fake = fake_with_tag("v1.2.0");
        let (text, changes) = bump_manifest_text("tool", &tool(), &fake).unwrap();

        assert!(text.starts_with("# comentário do topo\n"));
        assert!(text.contains("version = \"1.2.0\" # versão atual"), "comentário da linha preservado");
        assert_eq!(text.matches("version = \"1.2.0\"").count(), 2);
        assert!(text.contains(&format!("sha256 = \"{}\"", sha256_hex_of(X86_URL.as_bytes()))));
        assert!(text.contains("bump = { github = \"owner/tool\" }"), "a fonte do bump é mantida");
        assert_eq!(changes.len(), 2);
        assert_eq!(
            changes[0],
            Change { module: "tool".into(), group: "main".into(), from: "1.0.0".into(), to: "1.2.0".into() }
        );
        // o resultado continua sendo um manifest válido
        Manifest::from_toml_str(&text, "tool").unwrap();
    }

    #[test]
    fn nothing_changes_and_nothing_is_downloaded_when_up_to_date() {
        let fake = fake_with_tag("v1.0.0");
        let (text, changes) = bump_manifest_text("tool", &tool(), &fake).unwrap();
        assert!(changes.is_empty());
        assert_eq!(text, tool());
        assert_eq!(fake.count("sha:"), 0);
    }

    #[test]
    fn a_placeholder_checksum_is_filled_even_when_the_version_matches() {
        let src = tool().replacen(&"a".repeat(64), &"0".repeat(64), 1);
        let fake = fake_with_tag("v1.0.0");
        let (text, changes) = bump_manifest_text("tool", &src, &fake).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!((changes[0].from.as_str(), changes[0].to.as_str()), ("1.0.0", "1.0.0"));
        assert!(!text.contains(&"0".repeat(64)));
        assert_eq!(fake.count("sha:"), 1, "só o passo com placeholder baixa");
    }

    #[test]
    fn an_empty_tag_prefix_keeps_the_tag_as_the_version() {
        let src = tool().replace("bump = { github = \"owner/tool\" }", "bump = { github = \"owner/tool\", tag_prefix = \"\" }");
        let fake = fake_with_tag("2.30.1");
        let (text, _) = bump_manifest_text("tool", &src, &fake).unwrap();
        assert!(text.contains("version = \"2.30.1\""));
        let with_v = fake_with_tag("v2.30.1");
        let (text, _) = bump_manifest_text("tool", &src, &with_v).unwrap();
        assert!(text.contains("version = \"v2.30.1\""), "sem prefixo configurado, nada é removido");
    }

    #[test]
    fn index_sources_use_the_page_version() {
        let src = r#"schema = 1
id = "node"
name = "node"

[[install]]
method = "release"
version = "20.0.0"
url = "https://nodejs.org/dist/v{version}/node-v{version}-linux-x64.tar.xz"
sha256 = "SHA"
dest = "~/.local/node"
strip_components = 1
bump = { index_url = "https://nodejs.org/dist/latest-v24.x/", index_prefix = "node-v", index_suffix = "-linux-x64.tar.xz" }
"#
        .replace("SHA", &"c".repeat(64));
        let mut fake = Fake::default();
        fake.versions.insert("https://nodejs.org/dist/latest-v24.x/".to_string(), "22.11.0".to_string());
        let (text, changes) = bump_manifest_text("node", &src, &fake).unwrap();
        assert!(text.contains("version = \"22.11.0\""));
        assert_eq!(changes[0].to, "22.11.0");
        let url = "https://nodejs.org/dist/v22.11.0/node-v22.11.0-linux-x64.tar.xz";
        assert!(text.contains(&sha256_hex_of(url.as_bytes())));
        assert!(text.contains("strip_components = 1"));
    }

    #[test]
    fn redirect_sources_pin_the_final_url() {
        let src = r#"schema = 1
id = "jdk"
name = "jdk"

[[install]]
method = "release"
version = "0"
url = "https://api.adoptium.net/v3/binary/latest/21/ga/linux/x64/jdk/hotspot/normal/eclipse"
sha256 = "SHA"
dest = "~/.local/jdk"
bump = { redirect = "https://api.adoptium.net/v3/binary/latest/21/ga/linux/x64/jdk/hotspot/normal/eclipse" }
"#
        .replace("SHA", &"0".repeat(64));
        let target = "https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.5%2B11/OpenJDK21U-jdk_x64_linux_hotspot_21.0.5_11.tar.gz";
        let mut fake = Fake::default();
        fake.redirects.insert(
            "https://api.adoptium.net/v3/binary/latest/21/ga/linux/x64/jdk/hotspot/normal/eclipse".to_string(),
            target.to_string(),
        );
        let (text, changes) = bump_manifest_text("jdk", &src, &fake).unwrap();
        assert!(text.contains(&format!("url = \"{target}\"")));
        assert!(text.contains("version = \"OpenJDK21U-jdk_x64_linux_hotspot_21.0.5_11.tar.gz\""));
        assert_eq!(changes.len(), 1);

        // de novo com o mesmo destino: nada a fazer
        let (_, again) = bump_manifest_text("jdk", &text, &fake).unwrap();
        assert!(again.is_empty());
    }

    #[test]
    fn git_steps_pin_the_branch_head() {
        let old = "1".repeat(40);
        let new = "2".repeat(40);
        let src = format!(
            "schema = 1\nid = \"vim\"\nname = \"vim\"\n\n[[install]]\ngroup = \"plugin-a\"\nmethod = \"git\"\nurl = \"https://github.com/a/a\"\nrev = \"{old}\"\ndest = \"~/.vim/a\"\nbump = {{ branch = \"master\" }}\n"
        );
        let mut fake = Fake::default();
        fake.heads.insert("https://github.com/a/a".to_string(), new.clone());
        let (text, changes) = bump_manifest_text("vim", &src, &fake).unwrap();
        assert!(text.contains(&format!("rev = \"{new}\"")));
        assert_eq!(changes[0].group, "plugin-a");
        assert_eq!(changes[0].from, "111111111111");
        assert_eq!(changes[0].to, "222222222222");
        Manifest::from_toml_str(&text, "vim").unwrap();
    }

    #[test]
    fn steps_without_bump_are_left_alone() {
        let src = "schema = 1\nid = \"x\"\nname = \"x\"\n\n[[install]]\nmethod = \"pkg\"\nnames = { dnf = \"x\" }\n";
        let (text, changes) = bump_manifest_text("x", src, &Fake::default()).unwrap();
        assert_eq!(text, src);
        assert!(changes.is_empty());
    }

    #[test]
    fn a_resolver_failure_is_an_error_for_that_manifest() {
        assert!(bump_manifest_text("tool", &tool(), &Fake::default()).is_err());
    }

    // ---- bump_dir

    fn registry_dir(tmp: &Path, modules: &[(&str, String)], version: u64) -> PathBuf {
        let root = tmp.join("registry");
        for (id, text) in modules {
            let dir = root.join("modules").join(id);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("module.toml"), text).unwrap();
        }
        fs::write(root.join("registry.toml"), format!("# meta\nschema = 1\nversion = {version}\n")).unwrap();
        root.join("modules")
    }

    #[test]
    fn bump_dir_updates_files_shares_lookups_and_bumps_the_registry_version() {
        let tmp = tempfile::tempdir().unwrap();
        let modules = registry_dir(tmp.path(), &[("tool", tool())], 5);
        let fake = fake_with_tag("v1.2.0");

        let report = bump_dir(&modules, &[], false, &fake).unwrap();
        assert_eq!(report.changes.len(), 2);
        assert!(report.errors.is_empty());
        assert_eq!(report.registry_version, Some((5, 6)));
        assert_eq!(fake.count("tag:"), 1, "as duas arquiteturas compartilham a consulta da tag");
        assert!(fs::read_to_string(modules.join("tool/module.toml")).unwrap().contains("version = \"1.2.0\""));
        let meta = fs::read_to_string(modules.parent().unwrap().join("registry.toml")).unwrap();
        assert!(meta.contains("version = 6"));
        assert!(meta.starts_with("# meta"), "comentário do registry.toml preservado");

        // segunda execução: nada novo, a versão do registry não sobe
        let again = bump_dir(&modules, &[], false, &fake).unwrap();
        assert!(again.changes.is_empty());
        assert_eq!(again.registry_version, None);
        assert!(fs::read_to_string(modules.parent().unwrap().join("registry.toml")).unwrap().contains("version = 6"));
    }

    #[test]
    fn check_only_reports_but_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let modules = registry_dir(tmp.path(), &[("tool", tool())], 5);
        let report = bump_dir(&modules, &[], true, &fake_with_tag("v9.9.9")).unwrap();
        assert_eq!(report.changes.len(), 2);
        assert_eq!(report.registry_version, None);
        assert_eq!(fs::read_to_string(modules.join("tool/module.toml")).unwrap(), tool());
        assert!(fs::read_to_string(modules.parent().unwrap().join("registry.toml")).unwrap().contains("version = 5"));
    }

    #[test]
    fn one_failing_module_does_not_stop_the_others() {
        let tmp = tempfile::tempdir().unwrap();
        let broken = tool().replace("owner/tool", "owner/sumiu");
        let modules = registry_dir(tmp.path(), &[("a-quebrado", broken), ("tool", tool())], 1);
        let report = bump_dir(&modules, &[], false, &fake_with_tag("v1.2.0")).unwrap();
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].0, "a-quebrado");
        assert_eq!(report.changes.len(), 2);
        assert!(fs::read_to_string(modules.join("tool/module.toml")).unwrap().contains("1.2.0"));
    }

    #[test]
    fn only_filters_by_module_id() {
        let tmp = tempfile::tempdir().unwrap();
        let other = tool().replace("id = \"tool\"", "id = \"outro\"");
        let modules = registry_dir(tmp.path(), &[("outro", other.clone()), ("tool", tool())], 1);
        let report = bump_dir(&modules, &["tool".to_string()], false, &fake_with_tag("v1.2.0")).unwrap();
        assert_eq!(report.changes.len(), 2);
        assert!(report.changes.iter().all(|c| c.module == "tool"));
        assert_eq!(fs::read_to_string(modules.join("outro/module.toml")).unwrap(), other);
    }

    #[test]
    fn registry_version_helper_errors_without_a_version() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("registry.toml");
        fs::write(&path, "schema = 1\n").unwrap();
        assert!(bump_registry_version(&path).is_err());
    }

    // ---- funções puras

    #[test]
    fn extract_version_ignores_unrelated_html() {
        let html = r#"<a href="node-v22.11.0-aix-ppc64.tar.gz">x</a>
<a href="node-v22.11.0-linux-x64.tar.xz">node-v22.11.0-linux-x64.tar.xz</a>
<a href="node-v22.11.0-win-x64.zip">y</a>"#;
        assert_eq!(
            extract_version(html, "node-v", "-linux-x64.tar.xz").as_deref(),
            Some("22.11.0")
        );
        assert_eq!(extract_version(html, "node-v", "-linux-arm64.tar.xz"), None);
        assert_eq!(extract_version(html, "", "-linux-x64.tar.xz"), None);
        // texto entre prefixo e sufixo com lixo (aspas, espaços) é rejeitado
        assert_eq!(extract_version("node-v \"quebrado\" -linux-x64.tar.xz", "node-v", "-linux-x64.tar.xz"), None);
    }

    #[test]
    fn url_helpers() {
        assert_eq!(last_segment("https://x.dev/a/b/file.tar.gz?raw=1"), "file.tar.gz");
        assert_eq!(percent_decode("jdk-21.0.5%2B11"), "jdk-21.0.5+11");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
        assert_eq!(absolutize("https://github.com/o/r/releases/latest", "/o/r/releases/tag/v1"), "https://github.com/o/r/releases/tag/v1");
        assert_eq!(absolutize("https://a.dev/x", "https://b.dev/y"), "https://b.dev/y");
        assert!(looks_like_archive("https://x/y/Tool-1.0.TAR.GZ"));
        assert!(looks_like_archive("https://x/y/z.zip?sig=1"));
        assert!(!looks_like_archive("https://api.x/v3/binary/latest/21"));
    }
}
