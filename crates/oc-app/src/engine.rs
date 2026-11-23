//! The engine thread: in `oc-engine` since W1b (shared with the browser's engine worker).
//! The app starts it with its stuck-state diagnostics (`crate::diag`).
pub use oc_engine::engine::*;

/// Starts the engine thread (see [`oc_engine::engine::spawn`]).
pub fn spawn(store: std::sync::Arc<MeshStore>) -> std::sync::mpsc::Sender<Job> {
    oc_engine::engine::spawn(store, crate::diag::engine)
}

#[cfg(test)]
mod tests {
    /// The browser's engine worker serves the same commands as this app (`oc_engine::request`):
    /// every engine, assembly and text file command registered in `main.rs`, and no other.
    #[test]
    fn the_browser_serves_the_same_commands() {
        let src = include_str!("main.rs");
        let list = &src[src.find("generate_handler![").expect("handler list")..];
        let list = &list[..list.find(']').expect("end of the list")];
        let mut ours: Vec<&str> = list
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|n| n.starts_with("engine_") || n.starts_with("asm_") || *n == "app_read_text" || *n == "app_write_text")
            .collect();
        let mut theirs = oc_engine::request::COMMANDS.to_vec();
        ours.sort();
        theirs.sort();
        assert_eq!(ours, theirs);
    }
}
