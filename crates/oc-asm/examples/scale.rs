//! Writes the generated scale assembly (G3): `cargo run --release -p oc-asm --example scale --
//! <dir> [parts] [instances]` (defaults 100 and 10 000, G2's benchmark). The `.ocasm` has no
//! caches, so its first open regenerates every part; saving it from the app adds them.
fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().unwrap_or_else(|| "target/scale".into());
    let parts = args.next().map_or(100, |s| s.parse().expect("parts"));
    let instances = args.next().map_or(10_000, |s| s.parse().expect("instances"));
    match oc_asm::scale::write(std::path::Path::new(&dir), parts, instances) {
        Ok(p) => println!("{}", p.display()),
        Err(e) => {
            eprintln!("{dir}: {e}");
            std::process::exit(1);
        }
    }
}
