//! Instalação por release: download com checksum, extração segura e links de binários.
//!
//! Ordem de segurança: baixa -> confere sha256 -> só então extrai. Nada é extraído
//! sem checksum válido, e a extração acontece num diretório temporário que só
//! substitui o destino ao final.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::error::{io_err, Error, Result};
use crate::event::{Event, EventSink};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    TarGz,
    TarXz,
    /// Binário único, sem empacotamento.
    Raw,
}

impl ArchiveKind {
    pub fn from_name(name: &str) -> Result<ArchiveKind> {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
            Ok(ArchiveKind::TarGz)
        } else if lower.ends_with(".tar.xz") || lower.ends_with(".txz") {
            Ok(ArchiveKind::TarXz)
        } else if [".zip", ".7z", ".tar.bz2", ".tar.zst", ".tar"]
            .iter()
            .any(|ext| lower.ends_with(ext))
        {
            Err(Error::Unsupported(format!(
                "formato de arquivo ainda não suportado: {name}"
            )))
        } else {
            Ok(ArchiveKind::Raw)
        }
    }
}

pub(crate) fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex_of(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    to_hex(&hasher.finalize())
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(io_err(path))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(io_err(path))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(to_hex(&hasher.finalize()))
}

/// Baixa `url` para `dest_file`, calculando o sha256 durante o download.
/// Se não bater com `expected_sha256`, apaga o arquivo e devolve erro.
pub fn download(
    module: &str,
    url: &str,
    dest_file: &Path,
    expected_sha256: &str,
    sink: &mut dyn EventSink,
) -> Result<()> {
    let download_err = |reason: String| Error::Download {
        url: url.to_string(),
        reason,
    };

    if let Some(parent) = dest_file.parent() {
        fs::create_dir_all(parent).map_err(io_err(parent))?;
    }

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| download_err(e.to_string()))?;
    let total = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    let mut reader = response.into_reader();

    let mut file = File::create(dest_file).map_err(io_err(dest_file))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut done = 0u64;
    let mut last_report = 0u64;

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| download_err(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(io_err(dest_file))?;
        done += n as u64;
        if done - last_report >= 256 * 1024 {
            last_report = done;
            sink.emit(Event::Progress {
                module: module.to_string(),
                done,
                total,
            });
        }
    }
    file.flush().map_err(io_err(dest_file))?;
    sink.emit(Event::Progress {
        module: module.to_string(),
        done,
        total: Some(done),
    });

    let actual = to_hex(&hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        let _ = fs::remove_file(dest_file);
        return Err(Error::ChecksumMismatch {
            expected: expected_sha256.to_ascii_lowercase(),
            actual,
        });
    }
    Ok(())
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

fn remove_path(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            let result = if meta.is_dir() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            };
            result.map_err(io_err(path))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Remove os `strip` primeiros componentes de `path`. `None` = a entrada some
/// (era a própria pasta raiz). Caminhos absolutos ou com `..` são erro.
fn strip_path(path: &Path, strip: usize) -> Result<Option<PathBuf>> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_os_string()),
            Component::CurDir => {}
            _ => {
                return Err(Error::Archive(format!(
                    "caminho inseguro no arquivo: {}",
                    path.display()
                )))
            }
        }
    }
    if parts.len() <= strip {
        return Ok(None);
    }
    Ok(Some(parts[strip..].iter().collect()))
}

fn unpack_tar<R: Read>(reader: R, dest: &Path, strip: u32) -> Result<()> {
    let archive_err = |e: std::io::Error| Error::Archive(e.to_string());
    let mut archive = tar::Archive::new(reader);
    if strip == 0 {
        // O `tar` já ignora entradas que tentariam escapar do diretório.
        return archive.unpack(dest).map_err(archive_err);
    }

    fs::create_dir_all(dest).map_err(io_err(dest))?;
    for entry in archive.entries().map_err(archive_err)? {
        let mut entry = entry.map_err(archive_err)?;
        let path = entry.path().map_err(archive_err)?.into_owned();
        let Some(relative) = strip_path(&path, strip as usize)? else {
            continue;
        };
        let target = dest.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        if entry.header().entry_type().is_hard_link() {
            let link = entry
                .link_name()
                .map_err(archive_err)?
                .map(|l| l.into_owned())
                .ok_or_else(|| Error::Archive("hardlink sem destino".to_string()))?;
            let Some(link_relative) = strip_path(&link, strip as usize)? else {
                continue;
            };
            fs::hard_link(dest.join(link_relative), &target).map_err(io_err(&target))?;
        } else {
            entry.unpack(&target).map_err(archive_err)?;
        }
    }
    Ok(())
}

fn untar_gz(archive: &Path, dest: &Path, strip: u32) -> Result<()> {
    let file = File::open(archive).map_err(io_err(archive))?;
    unpack_tar(flate2::read::GzDecoder::new(file), dest, strip)
}

fn untar_xz(archive: &Path, dest: &Path, strip: u32) -> Result<()> {
    let tar_path = archive.with_extension("tar");
    {
        let input = File::open(archive).map_err(io_err(archive))?;
        let output = File::create(&tar_path).map_err(io_err(&tar_path))?;
        let mut reader = BufReader::new(input);
        let mut writer = BufWriter::new(output);
        lzma_rs::xz_decompress(&mut reader, &mut writer)
            .map_err(|e| Error::Archive(format!("xz: {e}")))?;
        writer.flush().map_err(io_err(&tar_path))?;
    }
    let result = File::open(&tar_path)
        .map_err(io_err(&tar_path))
        .and_then(|f| unpack_tar(f, dest, strip));
    let _ = fs::remove_file(&tar_path);
    result
}

fn copy_raw(archive: &Path, file_name: &str, dest: &Path) -> Result<()> {
    let target = dest.join(file_name);
    fs::copy(archive, &target).map_err(io_err(&target))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).map_err(io_err(&target))?;
    }
    Ok(())
}

/// Extrai `archive` num diretório temporário ao lado de `dest` e o troca por `dest`.
/// Se algo falhar, o `dest` anterior é preservado. A extração do `tar` ignora
/// entradas que tentariam escapar do diretório (caminhos absolutos ou `..`).
pub fn install_archive(
    archive: &Path,
    file_name: &str,
    dest: &Path,
    strip_components: u32,
) -> Result<()> {
    let kind = ArchiveKind::from_name(file_name)?;
    let parent = dest.parent().ok_or_else(|| {
        Error::Unsupported(format!("destino sem diretório pai: {}", dest.display()))
    })?;
    fs::create_dir_all(parent).map_err(io_err(parent))?;

    let staging = sibling(dest, ".hiraku-new");
    let backup = sibling(dest, ".hiraku-old");
    remove_path(&staging)?;
    remove_path(&backup)?;
    fs::create_dir_all(&staging).map_err(io_err(&staging))?;

    let extracted = match kind {
        ArchiveKind::TarGz => untar_gz(archive, &staging, strip_components),
        ArchiveKind::TarXz => untar_xz(archive, &staging, strip_components),
        ArchiveKind::Raw => copy_raw(archive, file_name, &staging),
    };
    if let Err(e) = extracted {
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }

    if fs::symlink_metadata(dest).is_ok() {
        fs::rename(dest, &backup).map_err(io_err(dest))?;
    }
    if let Err(source) = fs::rename(&staging, dest) {
        if backup.exists() {
            let _ = fs::rename(&backup, dest);
        }
        return Err(Error::Io {
            path: dest.to_path_buf(),
            source,
        });
    }
    let _ = remove_path(&backup);
    Ok(())
}

#[cfg(unix)]
pub(crate) fn make_link(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Sem symlinks (Windows): copia o binário e grava ao lado um marcador com a origem,
/// para reconhecer a cópia como do hiraku na próxima atualização.
#[cfg(not(unix))]
pub(crate) fn make_link(target: &Path, link: &Path) -> std::io::Result<()> {
    fs::copy(target, link)?;
    fs::write(copy_marker(link), target.to_string_lossy().as_bytes())
}

fn copy_marker(link: &Path) -> PathBuf {
    let mut name = link.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".hiraku-link");
    link.with_file_name(name)
}

/// O arquivo comum em `link` é uma cópia feita pelo hiraku a partir de `target`?
fn is_our_copy(link: &Path, target: &Path) -> bool {
    fs::read_to_string(copy_marker(link)).is_ok_and(|origin| Path::new(origin.trim()) == target)
}

/// Cria os links dos binários (`link` -> `dest/src`). Só substitui um link que já
/// aponta para o mesmo alvo; nunca sobrescreve arquivos ou links de outra origem.
pub fn link_bins(dest: &Path, bins: &[(String, PathBuf)]) -> Result<Vec<PathBuf>> {
    let mut created = Vec::new();
    for (src_rel, link) in bins {
        let target = dest.join(src_rel);
        if !target.exists() {
            return Err(Error::Archive(format!(
                "binário '{src_rel}' não existe em {}",
                dest.display()
            )));
        }
        if let Some(parent) = link.parent() {
            fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        match fs::symlink_metadata(link) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let current = fs::read_link(link).map_err(io_err(link))?;
                if current == target {
                    fs::remove_file(link).map_err(io_err(link))?;
                } else {
                    return Err(Error::Unsupported(format!(
                        "{} já é um link para {}; remova-o ou mova-o",
                        link.display(),
                        current.display()
                    )));
                }
            }
            Ok(_) if is_our_copy(link, &target) => {
                fs::remove_file(link).map_err(io_err(link))?;
            }
            Ok(_) => {
                return Err(Error::Unsupported(format!(
                    "{} já existe e não é um link do hiraku; remova-o ou mova-o",
                    link.display()
                )))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(Error::Io {
                    path: link.clone(),
                    source,
                })
            }
        }
        make_link(&target, link).map_err(io_err(link))?;
        created.push(link.clone());
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tar_gz(path: &Path, files: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for (name, data) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, name, *data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex_of(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_file_matches_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        fs::write(&path, b"abc").unwrap();
        assert_eq!(sha256_file(&path).unwrap(), sha256_hex_of(b"abc"));
    }

    #[test]
    fn archive_kind_detection() {
        assert_eq!(ArchiveKind::from_name("k.tar.gz").unwrap(), ArchiveKind::TarGz);
        assert_eq!(ArchiveKind::from_name("K.TGZ").unwrap(), ArchiveKind::TarGz);
        assert_eq!(ArchiveKind::from_name("k.txz").unwrap(), ArchiveKind::TarXz);
        assert_eq!(ArchiveKind::from_name("tmux").unwrap(), ArchiveKind::Raw);
        assert!(ArchiveKind::from_name("k.zip").is_err());
    }

    #[test]
    fn copies_are_recognized_only_with_a_matching_marker() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("opt/tool");
        let link = dir.path().join("bin/tool");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "novo").unwrap();
        fs::write(&link, "antigo").unwrap();
        assert!(!is_our_copy(&link, &target), "sem marcador: arquivo de outra origem");
        fs::write(copy_marker(&link), target.to_string_lossy().as_bytes()).unwrap();
        assert!(is_our_copy(&link, &target));
        assert!(!is_our_copy(&link, &dir.path().join("outro")), "marcador de outra origem");
        assert_eq!(copy_marker(&link).file_name().unwrap(), "tool.hiraku-link");
    }

    #[test]
    fn extracts_tar_gz_and_links_bins() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("app.tar.gz");
        make_tar_gz(&archive, &[("bin/tool", &b"#!/bin/sh\necho hi\n"[..])]);

        let dest = dir.path().join("opt/app");
        install_archive(&archive, "app.tar.gz", &dest, 0).unwrap();
        assert!(dest.join("bin/tool").is_file());
        assert!(!sibling(&dest, ".hiraku-new").exists());
        assert!(!sibling(&dest, ".hiraku-old").exists());

        let link = dir.path().join("bin/tool");
        let created = link_bins(&dest, &[("bin/tool".to_string(), link.clone())]).unwrap();
        assert_eq!(created, vec![link.clone()]);
        assert!(link.exists());

        // reinstalar substitui o destino e refaz o link sem erro (idempotente)
        install_archive(&archive, "app.tar.gz", &dest, 0).unwrap();
        link_bins(&dest, &[("bin/tool".to_string(), link)]).unwrap();
    }

    #[test]
    fn strip_components_removes_the_versioned_top_directory() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("node.tar.gz");
        make_tar_gz(
            &archive,
            &[
                ("node-v22.1.0-linux-x64/bin/node", &b"binario"[..]),
                ("node-v22.1.0-linux-x64/lib/x.js", &b"js"[..]),
                ("LEIAME", &b"raso demais"[..]),
            ],
        );
        let dest = dir.path().join("node");
        install_archive(&archive, "node.tar.gz", &dest, 1).unwrap();
        assert_eq!(fs::read(dest.join("bin/node")).unwrap(), b"binario");
        assert_eq!(fs::read(dest.join("lib/x.js")).unwrap(), b"js");
        assert!(!dest.join("node-v22.1.0-linux-x64").exists());
        assert!(!dest.join("LEIAME").exists(), "entradas mais rasas que o strip são descartadas");
    }

    #[cfg(unix)]
    #[test]
    fn extraction_preserves_the_executable_bit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("app.tar.gz");
        make_tar_gz(&archive, &[("top/bin/tool", &b"#!/bin/sh\n"[..])]);
        for strip in [0u32, 1] {
            let dest = dir.path().join(format!("out{strip}"));
            install_archive(&archive, "app.tar.gz", &dest, strip).unwrap();
            let tool = if strip == 0 { dest.join("top/bin/tool") } else { dest.join("bin/tool") };
            let mode = fs::metadata(&tool).unwrap().permissions().mode();
            assert!(mode & 0o111 != 0, "strip={strip}: o binário perdeu o bit de execução");
        }
    }

    #[test]
    fn strip_path_rules() {
        assert_eq!(
            strip_path(Path::new("a/b/c"), 1).unwrap(),
            Some(PathBuf::from("b/c"))
        );
        assert_eq!(strip_path(Path::new("./a/b"), 1).unwrap(), Some(PathBuf::from("b")));
        assert_eq!(strip_path(Path::new("a"), 1).unwrap(), None);
        assert_eq!(strip_path(Path::new("a/b"), 2).unwrap(), None);
        assert!(strip_path(Path::new("/etc/passwd"), 1).is_err());
        assert!(strip_path(Path::new("a/../../x"), 1).is_err());
    }

    #[test]
    fn reinstall_replaces_old_contents() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("app");
        let v1 = dir.path().join("v1.tar.gz");
        let v2 = dir.path().join("v2.tar.gz");
        make_tar_gz(&v1, &[("old.txt", &b"1"[..])]);
        make_tar_gz(&v2, &[("new.txt", &b"2"[..])]);
        install_archive(&v1, "v1.tar.gz", &dest, 0).unwrap();
        install_archive(&v2, "v2.tar.gz", &dest, 0).unwrap();
        assert!(dest.join("new.txt").exists());
        assert!(!dest.join("old.txt").exists());
    }

    #[test]
    fn corrupt_archive_keeps_previous_install() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("app");
        let good = dir.path().join("good.tar.gz");
        make_tar_gz(&good, &[("keep.txt", &b"1"[..])]);
        install_archive(&good, "good.tar.gz", &dest, 0).unwrap();

        let bad = dir.path().join("bad.tar.gz");
        fs::write(&bad, b"isto nao e gzip").unwrap();
        assert!(install_archive(&bad, "bad.tar.gz", &dest, 0).is_err());
        assert!(dest.join("keep.txt").exists());
        assert!(!sibling(&dest, ".hiraku-new").exists());
    }

    #[test]
    fn raw_binary_is_copied() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("dl");
        fs::write(&src, b"binario").unwrap();
        let dest = dir.path().join("out");
        install_archive(&src, "tmux", &dest, 0).unwrap();
        assert_eq!(fs::read(dest.join("tmux")).unwrap(), b"binario");
    }

    #[test]
    fn link_bins_refuses_to_overwrite_regular_files() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("app");
        fs::create_dir_all(dest.join("bin")).unwrap();
        fs::write(dest.join("bin/tool"), b"x").unwrap();
        let link = dir.path().join("tool");
        fs::write(&link, b"do usuario").unwrap();
        assert!(link_bins(&dest, &[("bin/tool".to_string(), link.clone())]).is_err());
        assert_eq!(fs::read(&link).unwrap(), b"do usuario");
    }

    #[test]
    fn link_bins_errors_when_binary_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("app");
        fs::create_dir_all(&dest).unwrap();
        let link = dir.path().join("tool");
        assert!(link_bins(&dest, &[("bin/tool".to_string(), link)]).is_err());
    }
}
