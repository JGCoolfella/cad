//! The engine behind one entry (W1b): commands by name with the webview's arguments, as the
//! browser's engine worker sends them.
mod common;

use common::*;
use oc_engine::request::{request, Answer, COMMANDS};
use serde_json::{json, Value};

/// Every corpus part through `engine_rebuild` and `engine_mass_props`, as the app's `--corpus`
/// check runs them: faces, edges, volume and area as the oracle has them, no errors.
#[test]
fn corpus_through_requests() {
    let mut e = engine();
    let mut diffs = vec![];
    for (name, doc, oracle) in parts() {
        let features = doc["features"].clone();
        let b = bytes(&mut e, "engine_rebuild", json!({ "features": features, "preview": null, "quality": 3, "config": null }));
        let h = header(&b);
        let mp = json(&mut e, "engine_mass_props", json!({ "features": features }));
        let w = expected(&name, &oracle);
        for k in ["faces", "edges"] {
            let got = h["mesh"][k].as_array().map(|a| a.len() as u64);
            if got != w[k].as_u64() {
                diffs.push(format!("{name}: {k} {got:?} vs oracle {}", w[k]));
            }
        }
        for k in ["volume", "area"] {
            let (a, b) = (mp[k].as_f64().unwrap_or(f64::NAN), w[k].as_f64().unwrap());
            if (a - b).abs() > w["slack"][k].as_f64().unwrap() {
                diffs.push(format!("{name}: {k} {a} vs oracle {b}"));
            }
        }
        let bad: Vec<&Value> = h["statuses"].as_array().unwrap().iter().filter(|s| s.get("error").is_some() || s.get("warning").is_some()).collect();
        if !bad.is_empty() {
            diffs.push(format!("{name}: statuses {bad:?}"));
        }
    }
    assert!(diffs.is_empty(), "{diffs:#?}");
}

/// A rebuild reply carries its mesh as typed arrays: positions and indices consistent.
#[test]
fn rebuild_reply_layout() {
    let (_, doc, _) = parts().into_iter().find(|p| p.0 == "block-hole").unwrap();
    let mut e = engine();
    let b = bytes(&mut e, "engine_rebuild", json!({ "features": doc["features"], "quality": 3 }));
    let h = header(&b);
    let m = &h["mesh"];
    assert!(m["handle"].as_u64().unwrap() > 0);
    let a = arrays(&b);
    let n = m["positions"]["n"].as_u64().unwrap() as usize;
    assert_eq!(m["positions"]["t"], "f32");
    assert!(n > 0 && n % 3 == 0);
    let at = m["indices"]["$b"].as_u64().unwrap() as usize;
    let count = m["indices"]["n"].as_u64().unwrap() as usize;
    let max = (0..count).map(|k| u32::from_le_bytes(a[at + 4 * k..at + 4 * k + 4].try_into().unwrap())).max().unwrap();
    assert!((max as usize) < n / 3);
}

/// The other part commands: export, measure, convert, the cache.
#[test]
fn part_commands() {
    let (_, doc, _) = parts().into_iter().find(|p| p.0 == "block-hole").unwrap();
    let f = doc["features"].clone();
    let mut e = engine();
    let step = bytes(&mut e, "engine_export", json!({ "features": f, "format": "step", "quality": 3 }));
    assert!(String::from_utf8_lossy(&step[..64]).contains("ISO-10303-21"));
    let stl = bytes(&mut e, "engine_export", json!({ "features": f, "format": "stl", "quality": 3 }));
    assert_eq!(&stl[..14], b"OpenCAD binary");
    let h = header(&bytes(&mut e, "engine_rebuild", json!({ "features": f, "quality": 3 })));
    let faces = h["mesh"]["faces"].as_array().unwrap();
    let (a, b) = (&faces[0]["sig"], &faces[1]["sig"]);
    let d = json(&mut e, "engine_measure", json!({ "features": f, "refs": [{ "kind": "face", "sig": a }, { "kind": "face", "sig": b }] }));
    assert!(d["distance"].as_f64().is_some(), "{d}");
    let frame = json!({ "origin": [0, 0, 0], "x": [1, 0, 0], "y": [0, 1, 0], "normal": [0, 0, 1] });
    let c = json(&mut e, "engine_convert", json!({ "features": f, "frame": frame, "edges": [], "faces": [a] }));
    assert!(!c.as_array().unwrap().is_empty());
    assert_eq!(json(&mut e, "engine_clear_cache", Value::Null), Value::Null);
    assert!(json(&mut e, "engine_mass_props", json!({ "features": f }))["volume"].as_f64().unwrap() > 0.0);
}

/// Text files and exports by path in the engine's file system.
#[test]
fn files() {
    let dir = std::env::temp_dir().join(format!("oc-engine-files-{}", std::process::id()));
    let p = dir.join("sub/part.ocpart");
    let ps = p.to_string_lossy().to_string();
    let mut e = engine();
    assert_eq!(json(&mut e, "app_write_text", json!({ "path": ps, "text": "hello" })), Value::Null);
    assert_eq!(json(&mut e, "app_read_text", json!({ "path": ps })), json!("hello"));
    let (_, doc, _) = parts().into_iter().find(|p| p.0 == "block-hole").unwrap();
    let out = dir.join("sub/part.stl").to_string_lossy().to_string();
    let n = json(&mut e, "engine_export_to", json!({ "features": doc["features"], "format": "stl", "quality": 3, "path": out }));
    assert_eq!(n.as_u64(), Some(std::fs::metadata(&out).unwrap().len()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Unknown commands and bad arguments are errors, not panics; every command is known.
#[test]
fn errors() {
    let mut e = engine();
    assert!(request(&mut e, "engine_nothing", Value::Null).unwrap_err().contains("unknown command"));
    assert!(request(&mut e, "engine_rebuild", json!({ "features": 3 })).unwrap_err().contains("invalid arguments"));
    assert!(request(&mut e, "asm_drag", json!({ "instance": "i1", "grab": [0, 0, 0], "target": [0, 0, 0], "lever": 1 })).unwrap_err().contains("no assembly"));
    assert!(request(&mut e, "app_read_text", json!({ "path": "/no/such/file" })).is_err());
    for c in COMMANDS {
        let r = request(&mut e, c, json!({}));
        assert!(!matches!(&r, Err(m) if m.contains("unknown command")), "{c}");
        assert!(!matches!(r, Ok(Answer::Deferred)), "{c}");
    }
}
