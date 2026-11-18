// `include_dir!` não rastreia mudanças nos arquivos embutidos; sem isto, editar um
// manifest em `registry/` não recompilaria o core.
fn main() {
    println!("cargo:rerun-if-changed=../../registry");
}
