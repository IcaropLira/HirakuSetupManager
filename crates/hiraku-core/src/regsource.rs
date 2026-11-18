//! Origem dos manifests: registry embutido no binário + cache remoto assinado.
//!
//! - O registry embutido (pasta `registry/` do repositório) vai dentro do binário e é
//!   materializado em `<state_dir>/registry/embedded-<hash>/` (o motor de configs
//!   precisa de arquivos reais em disco).
//! - O registry remoto é um `registry.tar.gz` + `registry.tar.gz.minisig`. Só é
//!   ativado se: a assinatura confere, a extração é segura, a versão não regride e
//!   todos os manifests validam. Qualquer falha deixa o registry atual intacto.
//! - Vale o de maior `version`; em empate, o embutido.

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use include_dir::{Dir, DirEntry};
use serde::Deserialize;

use crate::error::{io_err, Error, Result};
use crate::manifest::is_safe_relative;
use crate::registry;
use crate::release::sha256_hex_of;
use crate::verify::SignatureVerifier;

pub const SUPPORTED_REGISTRY_SCHEMA: u32 = 1;
const META_FILE: &str = "registry.toml";
const COMPLETE_MARKER: &str = ".complete";

const MAX_ARCHIVE_BYTES: usize = 20 * 1024 * 1024;
const MAX_SIGNATURE_BYTES: usize = 16 * 1024;
const MAX_UNPACKED_BYTES: u64 = 100 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;

/// Arquivo com as chaves públicas minisign em que o app confia (uma linha `RW...` por
/// chave; comentários são ignorados). Gerado por `scripts/setup-signing.sh`.
const TRUSTED_KEYS_FILE: &str = include_str!("../../../keys/registry.pub");

/// Chaves públicas minisign (base64, linha que começa com `RW`) em que o app confia.
pub fn trusted_public_keys() -> Vec<&'static str> {
    parse_key_lines(TRUSTED_KEYS_FILE)
}

fn parse_key_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("RW"))
        .collect()
}

/// Onde o CI (`.github/workflows/registry.yml`) publica o registry assinado: uma
/// release fixa `registry`, marcada como pré-release para nunca virar a "latest"
/// (que é a do app).
pub const DEFAULT_REMOTE_URL: Option<&str> =
    Some("https://github.com/IcaropLira/HirakuSetupManager/releases/download/registry/registry.tar.gz");

/// Busca o registry remoto (URL das configurações, `$HIRAKU_REGISTRY_URL` ou a padrão)
/// e o adota se for mais novo. `Ok(None)` = nada mudou, ou ainda não há chave
/// confiável configurada (aí o registry remoto fica desligado, sem erro).
pub fn refresh(configured_url: Option<String>, state_dir: &Path) -> Result<Option<String>> {
    let Some(url) = configured_url
        .or_else(|| std::env::var("HIRAKU_REGISTRY_URL").ok())
        .or_else(|| DEFAULT_REMOTE_URL.map(str::to_string))
    else {
        return Ok(None);
    };
    let keys = trusted_public_keys();
    if keys.is_empty() {
        return Ok(None);
    }
    let verifier = crate::verify::MinisignVerifier::from_base64_keys(&keys)?;
    Ok(match update_remote(&url, &UreqFetcher, &verifier, &embedded_files(), state_dir)? {
        UpdateOutcome::UpToDate { .. } => None,
        UpdateOutcome::Updated { from, to } => Some(format!("registry atualizado: v{from} -> v{to}")),
    })
}

static EMBEDDED: Dir<'static> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../registry");

#[derive(Debug, Clone)]
pub struct EmbeddedFile<'a> {
    /// Caminho relativo, com `/`.
    pub path: String,
    pub contents: &'a [u8],
}

pub fn embedded_files() -> Vec<EmbeddedFile<'static>> {
    fn collect(dir: &Dir<'static>, out: &mut Vec<EmbeddedFile<'static>>) {
        for entry in dir.entries() {
            match entry {
                DirEntry::Dir(sub) => collect(sub, out),
                DirEntry::File(file) => out.push(EmbeddedFile {
                    path: file.path().to_string_lossy().replace('\\', "/"),
                    contents: file.contents(),
                }),
            }
        }
    }
    let mut out = Vec::new();
    collect(&EMBEDDED, &mut out);
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RegistryMeta {
    pub schema: u32,
    pub version: u64,
}

pub fn read_meta(root: &Path) -> Result<RegistryMeta> {
    let path = root.join(META_FILE);
    let src = fs::read_to_string(&path).map_err(io_err(&path))?;
    let meta: RegistryMeta = toml::from_str(&src).map_err(|source| Error::Parse {
        origin: path.display().to_string(),
        source,
    })?;
    if meta.schema != SUPPORTED_REGISTRY_SCHEMA {
        return Err(Error::Unsupported(format!(
            "registry com schema {} (suportado: {SUPPORTED_REGISTRY_SCHEMA})",
            meta.schema
        )));
    }
    Ok(meta)
}

fn remove_any(path: &Path) {
    if let Ok(meta) = fs::symlink_metadata(path) {
        let _ = if meta.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        };
    }
}

fn embedded_hash(files: &[EmbeddedFile]) -> String {
    let mut sorted: Vec<&EmbeddedFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut buf = Vec::new();
    for file in sorted {
        buf.extend_from_slice(file.path.as_bytes());
        buf.push(0);
        buf.extend_from_slice(&(file.contents.len() as u64).to_le_bytes());
        buf.extend_from_slice(file.contents);
    }
    sha256_hex_of(&buf)
}

/// Grava o registry embutido em `reg_dir/embedded-<hash>/` (idempotente e atômico:
/// escreve num diretório temporário e renomeia, então processos concorrentes não
/// enxergam uma extração pela metade).
pub fn materialize_embedded(files: &[EmbeddedFile], reg_dir: &Path) -> Result<PathBuf> {
    let hash = embedded_hash(files);
    let name = format!("embedded-{}", &hash[..16]);
    let target = reg_dir.join(&name);
    if target.join(COMPLETE_MARKER).is_file() {
        return Ok(target);
    }

    fs::create_dir_all(reg_dir).map_err(io_err(reg_dir))?;
    let tmp = reg_dir.join(format!(".embedded-tmp-{}", std::process::id()));
    remove_any(&tmp);
    fs::create_dir_all(&tmp).map_err(io_err(&tmp))?;

    for file in files {
        if !is_safe_relative(&file.path) {
            remove_any(&tmp);
            return Err(Error::Unsupported(format!(
                "caminho embutido inseguro: {}",
                file.path
            )));
        }
        let path = tmp.join(&file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        fs::write(&path, file.contents).map_err(io_err(&path))?;
    }
    fs::write(tmp.join(COMPLETE_MARKER), b"").map_err(io_err(&tmp))?;

    // Versões antigas do embutido (de binários anteriores) já não servem.
    if let Ok(entries) = fs::read_dir(reg_dir) {
        for entry in entries.flatten() {
            let entry_name = entry.file_name().to_string_lossy().to_string();
            if entry_name.starts_with("embedded-") && entry_name != name {
                remove_any(&entry.path());
            }
        }
    }
    remove_any(&target); // resto de uma extração interrompida, sem marcador

    if let Err(source) = fs::rename(&tmp, &target) {
        remove_any(&tmp);
        // Outro processo pode ter terminado primeiro.
        if target.join(COMPLETE_MARKER).is_file() {
            return Ok(target);
        }
        return Err(Error::Io {
            path: target,
            source,
        });
    }
    Ok(target)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Embedded,
    Remote,
}

#[derive(Debug, Clone)]
pub struct ActiveRegistry {
    /// Contém `registry.toml` e `modules/`.
    pub root: PathBuf,
    pub version: u64,
    pub source: Source,
    /// Por que o cache remoto foi ignorado, se foi.
    pub note: Option<String>,
}

impl ActiveRegistry {
    pub fn modules_dir(&self) -> PathBuf {
        self.root.join("modules")
    }
}

pub fn resolve_active(files: &[EmbeddedFile], state_dir: &Path) -> Result<ActiveRegistry> {
    let reg_dir = state_dir.join("registry");
    let embedded_root = materialize_embedded(files, &reg_dir)?;
    let embedded_meta = read_meta(&embedded_root)?;

    let mut note = None;
    let remote_root = reg_dir.join("remote");
    if remote_root.join(META_FILE).is_file() {
        let checked = read_meta(&remote_root)
            .and_then(|meta| registry::load_dir(&remote_root.join("modules")).map(|_| meta));
        match checked {
            Ok(meta) if meta.version > embedded_meta.version => {
                return Ok(ActiveRegistry {
                    root: remote_root,
                    version: meta.version,
                    source: Source::Remote,
                    note: None,
                });
            }
            Ok(_) => {}
            Err(e) => note = Some(format!("cache remoto ignorado: {e}")),
        }
    }

    Ok(ActiveRegistry {
        root: embedded_root,
        version: embedded_meta.version,
        source: Source::Embedded,
        note,
    })
}

pub trait Fetcher {
    fn get(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>>;
}

pub struct UreqFetcher;

impl Fetcher for UreqFetcher {
    fn get(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>> {
        let fail = |reason: String| Error::Download {
            url: url.to_string(),
            reason,
        };
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(60))
            .build();
        let response = agent.get(url).call().map_err(|e| fail(e.to_string()))?;
        let mut buf = Vec::new();
        response
            .into_reader()
            .take(max_bytes as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| fail(e.to_string()))?;
        if buf.len() > max_bytes {
            return Err(fail(format!("resposta maior que {max_bytes} bytes")));
        }
        Ok(buf)
    }
}

/// Extrai um `.tar.gz` aceitando só arquivos e diretórios, com caminhos seguros e
/// limites de tamanho e de quantidade (defesa extra, mesmo com assinatura válida).
fn safe_unpack(gz: &[u8], dest: &Path) -> Result<()> {
    let archive_err = |e: std::io::Error| Error::Archive(e.to_string());
    fs::create_dir_all(dest).map_err(io_err(dest))?;

    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(gz));
    let mut total = 0u64;
    let mut count = 0usize;
    for entry in archive.entries().map_err(archive_err)? {
        let mut entry = entry.map_err(archive_err)?;
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            return Err(Error::Archive(
                "o registry só pode conter arquivos e diretórios".to_string(),
            ));
        }
        let path = entry.path().map_err(archive_err)?.into_owned();
        if !path
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(Error::Archive(format!(
                "caminho inseguro no registry: {}",
                path.display()
            )));
        }
        count += 1;
        total += entry.size();
        if count > MAX_ENTRIES || total > MAX_UNPACKED_BYTES {
            return Err(Error::Archive(
                "registry excede os limites de tamanho".to_string(),
            ));
        }
        entry.unpack_in(dest).map_err(archive_err)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    UpToDate { version: u64 },
    Updated { from: u64, to: u64 },
}

/// Baixa, verifica e ativa o registry remoto. Ordem: assinatura -> extração segura ->
/// versão (sem regressão) -> validação de todos os manifests -> troca atômica.
pub fn update_remote(
    url: &str,
    fetcher: &dyn Fetcher,
    verifier: &dyn SignatureVerifier,
    embedded: &[EmbeddedFile],
    state_dir: &Path,
) -> Result<UpdateOutcome> {
    if !url.starts_with("https://") {
        return Err(Error::Unsupported(
            "a URL do registry deve começar com https://".to_string(),
        ));
    }
    let current = resolve_active(embedded, state_dir)?.version;

    let data = fetcher.get(url, MAX_ARCHIVE_BYTES)?;
    let sig_bytes = fetcher.get(&format!("{url}.minisig"), MAX_SIGNATURE_BYTES)?;
    let signature = String::from_utf8(sig_bytes)
        .map_err(|_| Error::Signature("arquivo de assinatura não é texto".to_string()))?;
    verifier.verify(&data, &signature)?;

    let reg_dir = state_dir.join("registry");
    fs::create_dir_all(&reg_dir).map_err(io_err(&reg_dir))?;
    let staging = reg_dir.join(format!(".staging-{}", std::process::id()));
    remove_any(&staging);

    let outcome = (|| -> Result<UpdateOutcome> {
        safe_unpack(&data, &staging)?;
        let meta = read_meta(&staging)?;
        if meta.version < current {
            return Err(Error::Unsupported(format!(
                "versão {} do registry é mais antiga que a atual ({current}); recusado",
                meta.version
            )));
        }
        if meta.version == current {
            return Ok(UpdateOutcome::UpToDate { version: current });
        }
        // Qualquer manifest inválido derruba a atualização inteira.
        registry::load_dir(&staging.join("modules"))?;

        let remote = reg_dir.join("remote");
        let old = reg_dir.join("remote.old");
        remove_any(&old);
        if remote.exists() {
            fs::rename(&remote, &old).map_err(io_err(&remote))?;
        }
        if let Err(source) = fs::rename(&staging, &remote) {
            if old.exists() {
                let _ = fs::rename(&old, &remote);
            }
            return Err(Error::Io {
                path: remote,
                source,
            });
        }
        remove_any(&old);
        Ok(UpdateOutcome::Updated {
            from: current,
            to: meta.version,
        })
    })();

    remove_any(&staging);
    outcome
}

#[cfg(test)]
mod tests {
    #[test]
    fn key_file_keeps_only_minisign_key_lines() {
        let text = "# comentário\nuntrusted comment: minisign public key ABC\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3\n\n  RWSoutra  \n";
        assert_eq!(super::parse_key_lines(text), vec!["RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3", "RWSoutra"]);
        assert!(super::parse_key_lines(include_str!("../../../keys/registry.pub")).len() <= 4);
    }

    use super::*;
    use std::collections::HashMap;

    const DEMO_MODULE: &str = "schema = 1\nid = \"demo\"\nname = \"demo\"\n";

    fn embedded(version: u64) -> Vec<EmbeddedFile<'static>> {
        let meta: &'static str = Box::leak(format!("schema = 1\nversion = {version}\n").into_boxed_str());
        vec![
            EmbeddedFile {
                path: "registry.toml".to_string(),
                contents: meta.as_bytes(),
            },
            EmbeddedFile {
                path: "modules/old/module.toml".to_string(),
                contents: b"schema = 1\nid = \"old\"\nname = \"old\"\n",
            },
        ]
    }

    fn tar_gz(entries: &[(&str, &str)]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for (name, text) in entries {
            let data = text.as_bytes();
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn registry_tar(version: u64, module: &str) -> Vec<u8> {
        let meta = format!("schema = 1\nversion = {version}\n");
        tar_gz(&[("registry.toml", meta.as_str()), ("modules/demo/module.toml", module)])
    }

    struct FakeFetcher(HashMap<String, Vec<u8>>);

    impl FakeFetcher {
        fn serving(url: &str, archive: Vec<u8>) -> FakeFetcher {
            let mut map = HashMap::new();
            map.insert(url.to_string(), archive);
            map.insert(format!("{url}.minisig"), b"assinatura".to_vec());
            FakeFetcher(map)
        }
    }

    impl Fetcher for FakeFetcher {
        fn get(&self, url: &str, _max: usize) -> Result<Vec<u8>> {
            self.0.get(url).cloned().ok_or_else(|| Error::Download {
                url: url.to_string(),
                reason: "não encontrado".to_string(),
            })
        }
    }

    struct FakeVerifier(bool);

    impl SignatureVerifier for FakeVerifier {
        fn verify(&self, _data: &[u8], _signature: &str) -> Result<()> {
            if self.0 {
                Ok(())
            } else {
                Err(Error::Signature("rejeitada pelo teste".to_string()))
            }
        }
    }

    const URL: &str = "https://example.com/registry.tar.gz";

    fn leftovers(state_dir: &Path) -> Vec<String> {
        fs::read_dir(state_dir.join("registry"))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|n| n.starts_with(".staging") || n == "remote.old")
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn shipped_embedded_registry_is_valid() {
        let dir = tempfile::tempdir().unwrap();
        let active = resolve_active(&embedded_files(), dir.path()).unwrap();
        assert_eq!(active.source, Source::Embedded);
        assert!(active.version >= 1);
        let manifests = registry::load_dir(&active.modules_dir()).unwrap();
        let ids: Vec<&str> = manifests.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"kitty") && ids.contains(&"tmux"), "ids: {ids:?}");
    }

    #[test]
    fn materialize_is_idempotent_and_replaces_stale_versions() {
        let dir = tempfile::tempdir().unwrap();
        let reg = dir.path().join("registry");
        let a = materialize_embedded(&embedded(1), &reg).unwrap();
        let again = materialize_embedded(&embedded(1), &reg).unwrap();
        assert_eq!(a, again);
        assert!(a.join("modules/old/module.toml").is_file());

        let b = materialize_embedded(&embedded(2), &reg).unwrap();
        assert_ne!(a, b);
        assert!(!a.exists(), "versão antiga do embutido deve ser removida");
        assert_eq!(read_meta(&b).unwrap().version, 2);
    }

    #[test]
    fn rejects_unsafe_embedded_paths() {
        let dir = tempfile::tempdir().unwrap();
        let files = vec![EmbeddedFile {
            path: "../fora.txt".to_string(),
            contents: b"x",
        }];
        assert!(materialize_embedded(&files, dir.path()).is_err());
    }

    #[test]
    fn valid_signed_registry_is_activated() {
        let dir = tempfile::tempdir().unwrap();
        let fetcher = FakeFetcher::serving(URL, registry_tar(5, DEMO_MODULE));
        let outcome = update_remote(URL, &fetcher, &FakeVerifier(true), &embedded(1), dir.path()).unwrap();
        assert_eq!(outcome, UpdateOutcome::Updated { from: 1, to: 5 });

        let active = resolve_active(&embedded(1), dir.path()).unwrap();
        assert_eq!(active.source, Source::Remote);
        assert_eq!(active.version, 5);
        assert!(active.modules_dir().join("demo/module.toml").is_file());
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn bad_signature_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let fetcher = FakeFetcher::serving(URL, registry_tar(5, DEMO_MODULE));
        let err = update_remote(URL, &fetcher, &FakeVerifier(false), &embedded(1), dir.path()).unwrap_err();
        assert!(matches!(err, Error::Signature(_)));
        assert!(!dir.path().join("registry/remote").exists());
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn missing_signature_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut map = HashMap::new();
        map.insert(URL.to_string(), registry_tar(5, DEMO_MODULE));
        let err = update_remote(URL, &FakeFetcher(map), &FakeVerifier(true), &embedded(1), dir.path()).unwrap_err();
        assert!(matches!(err, Error::Download { .. }));
        assert!(!dir.path().join("registry/remote").exists());
    }

    #[test]
    fn downgrade_is_refused_and_same_version_is_up_to_date() {
        let dir = tempfile::tempdir().unwrap();
        let v5 = FakeFetcher::serving(URL, registry_tar(5, DEMO_MODULE));
        update_remote(URL, &v5, &FakeVerifier(true), &embedded(1), dir.path()).unwrap();

        let v3 = FakeFetcher::serving(URL, registry_tar(3, DEMO_MODULE));
        assert!(update_remote(URL, &v3, &FakeVerifier(true), &embedded(1), dir.path()).is_err());
        assert_eq!(resolve_active(&embedded(1), dir.path()).unwrap().version, 5);

        let same = update_remote(URL, &v5, &FakeVerifier(true), &embedded(1), dir.path()).unwrap();
        assert_eq!(same, UpdateOutcome::UpToDate { version: 5 });
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn invalid_manifest_in_signed_archive_keeps_previous_remote() {
        let dir = tempfile::tempdir().unwrap();
        let good = FakeFetcher::serving(URL, registry_tar(5, DEMO_MODULE));
        update_remote(URL, &good, &FakeVerifier(true), &embedded(1), dir.path()).unwrap();

        let broken = FakeFetcher::serving(URL, registry_tar(6, "schema = 1\nid = \"Demo!\"\nname = \"x\"\n"));
        assert!(update_remote(URL, &broken, &FakeVerifier(true), &embedded(1), dir.path()).is_err());
        assert_eq!(resolve_active(&embedded(1), dir.path()).unwrap().version, 5);
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn symlink_entries_are_rejected() {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_link_name("/etc/passwd").unwrap();
        header.set_cksum();
        builder.append_data(&mut header, "registry.toml", &b""[..]).unwrap();
        let archive = builder.into_inner().unwrap().finish().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let fetcher = FakeFetcher::serving(URL, archive);
        let err = update_remote(URL, &fetcher, &FakeVerifier(true), &embedded(1), dir.path()).unwrap_err();
        assert!(matches!(err, Error::Archive(_)));
        assert!(!dir.path().join("registry/remote").exists());
    }

    #[test]
    fn non_https_url_is_refused_before_any_request() {
        let dir = tempfile::tempdir().unwrap();
        let fetcher = FakeFetcher(HashMap::new());
        let err = update_remote("http://example.com/r.tar.gz", &fetcher, &FakeVerifier(true), &embedded(1), dir.path()).unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
    }

    #[test]
    fn embedded_wins_when_it_is_newer_than_the_cached_remote() {
        let dir = tempfile::tempdir().unwrap();
        let fetcher = FakeFetcher::serving(URL, registry_tar(5, DEMO_MODULE));
        update_remote(URL, &fetcher, &FakeVerifier(true), &embedded(1), dir.path()).unwrap();
        // um binário novo traz um registry embutido mais novo que o cache remoto
        let active = resolve_active(&embedded(9), dir.path()).unwrap();
        assert_eq!(active.source, Source::Embedded);
        assert_eq!(active.version, 9);
    }

    #[test]
    fn corrupted_remote_cache_falls_back_to_embedded_with_a_note() {
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("registry/remote");
        fs::create_dir_all(&remote).unwrap();
        fs::write(remote.join("registry.toml"), "isto = = quebrado").unwrap();
        let active = resolve_active(&embedded(1), dir.path()).unwrap();
        assert_eq!(active.source, Source::Embedded);
        assert!(active.note.is_some());
    }
}
