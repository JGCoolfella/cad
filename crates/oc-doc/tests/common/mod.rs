//! Shared test helpers: the golden corpus and the torture edit scripts.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

pub fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

pub fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

/// A document of the corpus: its label and its JSON text as stored.
pub struct CorpusDoc {
    pub label: String,
    pub text: String,
}

fn sorted_files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    v.sort();
    v
}

/// Every document in the corpus: the `.ocpart` parts, and for each torture script its
/// initial document and the document after each of its edits (replayed as the
/// TypeScript `applyEdits` replays them). `corpus/hard/` holds results, not documents.
pub fn corpus_documents() -> Vec<CorpusDoc> {
    let dir = corpus_dir();
    let mut out = Vec::new();
    for p in sorted_files(&dir, "ocpart") {
        let label = p.file_name().unwrap().to_string_lossy().into_owned();
        out.push(CorpusDoc {
            label,
            text: fs::read_to_string(&p).unwrap(),
        });
    }
    for p in sorted_files(&dir.join("torture"), "json") {
        let name = p.file_stem().unwrap().to_string_lossy().into_owned();
        let script: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
        let mut doc = script["doc"].clone();
        out.push(CorpusDoc {
            label: format!("torture/{name} (initial)"),
            text: doc.to_string(),
        });
        for (i, edit) in script["edits"].as_array().unwrap().iter().enumerate() {
            apply_edit(&mut doc, edit);
            out.push(CorpusDoc {
                label: format!("torture/{name} (after edit {})", i + 1),
                text: doc.to_string(),
            });
        }
    }
    out
}

/// One torture edit: `{op: set|remove|insert, path, value}` on a JSON document.
pub fn apply_edit(doc: &mut Value, edit: &Value) {
    let op = edit["op"].as_str().unwrap();
    let path = edit["path"].as_array().unwrap();
    let (last, parents) = path.split_last().unwrap();
    let mut target = doc;
    for k in parents {
        target = match k {
            Value::String(s) => &mut target[s.as_str()],
            Value::Number(n) => &mut target[n.as_u64().unwrap() as usize],
            _ => panic!("bad path {path:?}"),
        };
    }
    match (target, last) {
        (Value::Array(a), Value::Number(n)) => {
            let i = n.as_u64().unwrap() as usize;
            match op {
                "remove" => {
                    a.remove(i);
                }
                "insert" => a.insert(i, edit["value"].clone()),
                "set" => a[i] = edit["value"].clone(),
                _ => panic!("bad op {op}"),
            }
        }
        (Value::Object(m), Value::String(k)) => match op {
            "remove" => {
                // JavaScript's `delete` keeps the other members' order.
                m.shift_remove(k);
            }
            "set" | "insert" => {
                m.insert(k.clone(), edit["value"].clone());
            }
            _ => panic!("bad op {op}"),
        },
        (t, k) => panic!("cannot apply {op} at {k} of {t}"),
    }
}
