//! Writes `tests/fixtures/server-frames.json`, the encoder's output that the web
//! client's tests replay. Run from anywhere: `cargo run --bin export-frames`.

fn main() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tests/fixtures/server-frames.json");
    std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tests/fixtures"))
        .expect("tests/ is writable");
    std::fs::write(path, fsw_server::encode::frames_fixture()).expect("tests/fixtures is writable");
    println!("wrote {path}");
}
