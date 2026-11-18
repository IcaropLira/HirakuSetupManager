//! Catálogo de módulos e opções visto pela TUI, já sem os detalhes de instalação.
//! Tudo aqui é puro (sem I/O): valida, avança e formata valores de opções para as
//! telas de Opções e do assistente.

use std::path::PathBuf;

use hiraku_core::env::Os;
use hiraku_core::manifest::{Manifest, OptionDef};
use hiraku_core::settings::Settings;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "lowercase")]
pub enum OptKind {
    Choice(Vec<String>),
    Bool,
    Number { min: i64, max: i64, divisor: i64 },
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OptSpec {
    pub module: String,
    pub key: String,
    pub kind: OptKind,
    pub default: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModuleInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub supported: bool,
    pub requires: Vec<String>,
    pub options: Vec<OptSpec>,
}

pub fn build_catalog(manifests: &[(Manifest, PathBuf)], os: Os) -> Vec<ModuleInfo> {
    let mut out: Vec<ModuleInfo> = manifests
        .iter()
        .map(|(m, _)| ModuleInfo {
            id: m.id.clone(),
            name: m.name.clone(),
            description: m.description.clone(),
            supported: m.supports(os),
            requires: m.requires.clone(),
            options: m
                .options
                .iter()
                .map(|(key, def)| OptSpec::from_def(&m.id, key, def))
                .collect(),
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

impl OptSpec {
    pub fn from_def(module: &str, key: &str, def: &OptionDef) -> OptSpec {
        let (kind, default, description) = match def {
            OptionDef::Choice {
                values,
                default,
                description,
            } => (OptKind::Choice(values.clone()), default.clone(), description.clone()),
            OptionDef::Bool {
                default,
                description,
            } => (OptKind::Bool, default.to_string(), description.clone()),
            OptionDef::Number {
                min,
                max,
                default,
                divisor,
                description,
            } => (
                OptKind::Number {
                    min: *min,
                    max: *max,
                    divisor: *divisor,
                },
                default.to_string(),
                description.clone(),
            ),
            OptionDef::Text {
                default,
                description,
            } => (OptKind::Text, default.clone(), description.clone()),
        };
        OptSpec {
            module: module.to_string(),
            key: key.to_string(),
            kind,
            default,
            description,
        }
    }

    /// Valor efetivo e se ele foi escolhido pelo usuário (`true`) ou é o padrão.
    pub fn value_in(&self, settings: &Settings) -> (String, bool) {
        match settings.options.get(&self.module).and_then(|o| o.get(&self.key)) {
            Some(v) => (v.clone(), v != &self.default),
            None => (self.default.clone(), false),
        }
    }

    /// Confere e normaliza um valor digitado. Mesmas regras do core
    /// (`configs::resolve_options`), com mensagens curtas para a barra de status.
    pub fn validate(&self, raw: &str) -> Result<String, String> {
        let v = raw.trim();
        match &self.kind {
            OptKind::Choice(values) => {
                if values.iter().any(|x| x == v) {
                    Ok(v.to_string())
                } else {
                    Err(format!("'{v}' não é uma das opções"))
                }
            }
            OptKind::Bool => match v {
                "true" | "sim" | "s" | "1" | "on" => Ok("true".to_string()),
                "false" | "não" | "nao" | "n" | "0" | "off" => Ok("false".to_string()),
                _ => Err("use sim ou não".to_string()),
            },
            OptKind::Number { min, max, .. } => {
                let n: i64 = v
                    .parse()
                    .map_err(|_| format!("'{v}' não é um número inteiro"))?;
                if n < *min || n > *max {
                    Err(format!("{n} fora do intervalo {min} a {max}"))
                } else {
                    Ok(n.to_string())
                }
            }
            OptKind::Text => Ok(raw.to_string()),
        }
    }

    /// Próximo valor ao apertar ← / →: escolha anterior/seguinte (circular), número
    /// ± `delta` (limitado ao intervalo) e bool invertido. Texto não muda.
    pub fn step(&self, current: &str, delta: i64) -> String {
        match &self.kind {
            OptKind::Choice(values) if !values.is_empty() => {
                let len = values.len() as i64;
                let at = values.iter().position(|v| v == current).unwrap_or(0) as i64;
                let dir = delta.signum();
                values[(at + dir).rem_euclid(len) as usize].clone()
            }
            OptKind::Bool => {
                if current == "true" {
                    "false".to_string()
                } else {
                    "true".to_string()
                }
            }
            OptKind::Number { min, max, .. } => {
                let n: i64 = current.parse().unwrap_or(*min);
                (n + delta).clamp(*min, *max).to_string()
            }
            _ => current.to_string(),
        }
    }

    /// Como o valor aparece na tela.
    pub fn display(&self, value: &str) -> String {
        match &self.kind {
            OptKind::Bool => if value == "true" { "sim" } else { "não" }.to_string(),
            OptKind::Number { divisor: 100, .. } => format!("{value}%"),
            OptKind::Text if value.is_empty() => "(vazio)".to_string(),
            _ => value.to_string(),
        }
    }

    /// Texto curto do tipo/intervalo, para a dica de rodapé.
    pub fn hint(&self) -> String {
        match &self.kind {
            OptKind::Choice(values) => format!("{} opções", values.len()),
            OptKind::Bool => "sim / não".to_string(),
            OptKind::Number { min, max, .. } => format!("{min} a {max}"),
            OptKind::Text => "texto livre".to_string(),
        }
    }

    /// Barra de 0 a `width` para números (opacidade, blur…).
    pub fn gauge(&self, value: &str, width: usize) -> Option<String> {
        let OptKind::Number { min, max, .. } = self.kind else {
            return None;
        };
        if max <= min || width == 0 {
            return None;
        }
        let n: i64 = value.parse().ok()?;
        let filled = (((n - min) as f64 / (max - min) as f64) * width as f64).round() as usize;
        let filled = filled.min(width);
        Some(format!("{}{}", "█".repeat(filled), "░".repeat(width - filled)))
    }
}

/// Módulos habilitados mais as dependências (transitivas), na ordem do catálogo.
pub fn with_dependencies(catalog: &[ModuleInfo], enabled: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<String> = enabled.to_vec();
    while let Some(id) = stack.pop() {
        if out.contains(&id) {
            continue;
        }
        if let Some(info) = catalog.iter().find(|m| m.id == id) {
            stack.extend(info.requires.iter().cloned());
        }
        out.push(id);
    }
    catalog
        .iter()
        .filter(|m| out.contains(&m.id))
        .map(|m| m.id.clone())
        .collect()
}

/// Opções em ordem de exibição: por módulo; escolhas primeiro (paleta antes de blur).
pub fn sorted_specs<'a>(modules: impl Iterator<Item = &'a ModuleInfo>) -> Vec<OptSpec> {
    let rank = |s: &OptSpec| match s.kind {
        OptKind::Choice(_) => 0,
        OptKind::Number { .. } => 1,
        OptKind::Bool => 2,
        OptKind::Text => 3,
    };
    let mut out = Vec::new();
    for m in modules {
        let mut specs = m.options.clone();
        specs.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.key.cmp(&b.key)));
        out.extend(specs);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: OptKind, default: &str) -> OptSpec {
        OptSpec {
            module: "kitty".to_string(),
            key: "k".to_string(),
            kind,
            default: default.to_string(),
            description: String::new(),
        }
    }

    fn choice() -> OptSpec {
        spec(
            OptKind::Choice(vec!["a".into(), "b".into(), "c".into()]),
            "a",
        )
    }

    fn number() -> OptSpec {
        spec(
            OptKind::Number {
                min: 30,
                max: 100,
                divisor: 100,
            },
            "80",
        )
    }

    #[test]
    fn choice_steps_wrap_around() {
        let s = choice();
        assert_eq!(s.step("a", 1), "b");
        assert_eq!(s.step("c", 1), "a");
        assert_eq!(s.step("a", -1), "c");
        assert_eq!(s.step("desconhecido", 1), "b", "valor inválido recomeça do início");
    }

    #[test]
    fn number_steps_clamp() {
        let s = number();
        assert_eq!(s.step("80", 1), "81");
        assert_eq!(s.step("98", 10), "100");
        assert_eq!(s.step("35", -10), "30");
        assert_eq!(s.display("85"), "85%");
    }

    #[test]
    fn bool_toggles_and_validates_portuguese() {
        let s = spec(OptKind::Bool, "false");
        assert_eq!(s.step("false", 1), "true");
        assert_eq!(s.validate("sim").unwrap(), "true");
        assert_eq!(s.validate("não").unwrap(), "false");
        assert!(s.validate("talvez").is_err());
        assert_eq!(s.display("true"), "sim");
    }

    #[test]
    fn validation_matches_kind() {
        assert!(choice().validate("b").is_ok());
        assert!(choice().validate("z").is_err());
        assert_eq!(number().validate(" 90 ").unwrap(), "90");
        assert!(number().validate("101").unwrap_err().contains("fora"));
        assert!(number().validate("x").is_err());
        let t = spec(OptKind::Text, "@33");
        assert_eq!(t.validate(" @34").unwrap(), " @34", "texto é guardado como veio");
    }

    #[test]
    fn value_in_reads_settings_and_flags_custom_values() {
        let s = choice();
        let mut settings = Settings::default();
        assert_eq!(s.value_in(&settings), ("a".to_string(), false));
        settings.set_option("kitty", "k", "c");
        assert_eq!(s.value_in(&settings), ("c".to_string(), true));
        settings.set_option("kitty", "k", "a");
        assert_eq!(s.value_in(&settings), ("a".to_string(), false));
    }

    #[test]
    fn gauge_fills_proportionally() {
        let s = number();
        assert_eq!(s.gauge("30", 10).unwrap(), "░".repeat(10));
        assert_eq!(s.gauge("100", 10).unwrap(), "█".repeat(10));
        assert!(choice().gauge("a", 10).is_none());
    }

    #[test]
    fn dependencies_are_closed_over() {
        let info = |id: &str, req: &[&str]| ModuleInfo {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            supported: true,
            requires: req.iter().map(|s| s.to_string()).collect(),
            options: vec![],
        };
        let cat = vec![
            info("font", &[]),
            info("kitty", &["font"]),
            info("themes", &[]),
            info("vim", &["themes", "font"]),
        ];
        assert_eq!(
            with_dependencies(&cat, &["vim".to_string()]),
            vec!["font", "themes", "vim"]
        );
        assert_eq!(with_dependencies(&cat, &[]), Vec::<String>::new());
    }
}
