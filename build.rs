//! Embeds the built tracker (`tracker/dist/t.js`). When it has not been built (plain
//! `cargo test` without node) a stub is embedded so the crate still compiles.
use std::{env, fs, path::Path};

fn main() {
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("t.js");
    let src = Path::new("tracker/dist/t.js");
    println!("cargo:rerun-if-changed=tracker/dist/t.js");
    match fs::read(src) {
        Ok(b) => fs::write(out, b).unwrap(),
        Err(_) => fs::write(
            out,
            b"/* greprum tracker not built: run `make tracker` */\n",
        )
        .unwrap(),
    }
}
