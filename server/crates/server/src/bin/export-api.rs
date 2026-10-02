//! Writes the wire types to `src/generated/api.ts`. Run from anywhere: `cargo run --bin export-api`.

fn main() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../src/generated/api.ts");
    std::fs::write(path, fsw_server::api::wire::typescript()).expect("src/generated is writable");
    println!("wrote {path}");
}
