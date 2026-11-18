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
