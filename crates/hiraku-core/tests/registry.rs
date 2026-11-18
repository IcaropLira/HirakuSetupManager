//! Valida os manifests reais de `registry/modules` contra o schema.

use std::path::PathBuf;

use hiraku_core::registry;

fn modules_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/modules")
}

#[test]
fn shipped_registry_is_valid() {
    let manifests = registry::load_dir(&modules_dir()).expect("registry deve carregar");
    let ids: Vec<&str> = manifests.iter().map(|m| m.id.as_str()).collect();
    assert!(ids.contains(&"kitty"), "ids: {ids:?}");
    assert!(ids.contains(&"tmux"), "ids: {ids:?}");
    for m in &manifests {
        assert!(
            !m.install.is_empty() || !m.config.is_empty(),
            "módulo '{}' não faz nada",
            m.id
        );
    }
}

/// O hiraku.json do vim e do nvim, renderizado com os padrões, é JSON válido e traz
/// todas as opções do manifest (o editor depende disso para aplicar).
#[test]
fn editor_option_files_render_to_valid_json() {
    use std::collections::BTreeMap;
    let manifests = registry::load_dir(&modules_dir()).unwrap();
    for id in ["vim", "nvim"] {
        let m = manifests.iter().find(|m| m.id == id).unwrap();
        let vars = hiraku_core::configs::resolve_options(m, &BTreeMap::new()).unwrap();
        let src = std::fs::read_to_string(modules_dir().join(id).join("files/hiraku.json")).unwrap();
        let out = hiraku_core::configs::render_template(&src, &vars).unwrap();
        let json: serde_json::Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{id}: {e}\n{out}"));
        for key in m.options.keys() {
            assert!(json.get(key).is_some(), "{id}: falta '{key}' no hiraku.json");
        }
        assert_eq!(json["keymaps"], serde_json::json!([]));
        assert_eq!(json["colorscheme"], "auto");

        // Valores do usuário com aspas, quebras de linha e atalhos.
        let mut choices = BTreeMap::new();
        choices.insert("extra".to_string(), "echo \"oi\"\nlet x = '\\\\'".to_string());
        choices.insert("keymaps".to_string(), r#"[{"mode":"n","lhs":"dd","rhs":"\"_dd","desc":"apagar"}]"#.to_string());
        let vars = hiraku_core::configs::resolve_options(m, &choices).unwrap();
        let out = hiraku_core::configs::render_template(&src, &vars).unwrap();
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(json["extra"], "echo \"oi\"\nlet x = '\\\\'");
        assert_eq!(json["keymaps"][0]["rhs"], "\"_dd");

        // Template de C++ padrão = o arquivo do módulo; teclas com os padrões.
        let tpl = std::fs::read_to_string(modules_dir().join("vim/files/templates/cpp.cpp")).unwrap();
        let vars = hiraku_core::configs::resolve_options(m, &BTreeMap::new()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&hiraku_core::configs::render_template(&src, &vars).unwrap()).unwrap();
        assert_eq!(json["template_cpp"], tpl.as_str(), "{id}");
        assert!(tpl.contains("solve_tc"));
        assert_eq!(json["key_compile"], "<F5>");
        assert_eq!(json["key_cheatsheet"], "<F12>");
        // Template com chaves duplas e placeholders passa intacto.
        let mut choices = BTreeMap::new();
        choices.insert("template_cpp".to_string(), "int v[] = {{1}};\n__CURSOR__\n".to_string());
        let vars = hiraku_core::configs::resolve_options(m, &choices).unwrap();
        let json: serde_json::Value = serde_json::from_str(&hiraku_core::configs::render_template(&src, &vars).unwrap()).unwrap();
        assert_eq!(json["template_cpp"], "int v[] = {{1}};\n__CURSOR__\n");
    }
}
