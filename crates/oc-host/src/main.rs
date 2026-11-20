//! A geometry host on its own (tests; the app runs itself with `--geometry-host` instead).
fn main() {
    std::process::exit(oc_host::serve());
}
