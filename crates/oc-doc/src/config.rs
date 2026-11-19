//! Configurations and display states (G1e): named variants of a part or an assembly, as
//! SOLIDWORKS' ConfigurationManager keeps them.
//!
//! The document's features (instances, mates) always hold the **active** configuration's
//! values, so everything that reads a document (the regeneration, the UI, the TypeScript
//! engine) reads one plain document. A [`Configuration`] stores only what is *configured*:
//!
//! - `suppressed`: the suppression state of configured features (parts), or of instances
//!   and mates (assemblies), by id;
//! - `values`: configured dimension values by key `"<id>/<path>"`, where `<path>` is a
//!   dotted path into that feature's (or mate's) JSON: `f3/end1.depth`,
//!   `f1/sketch.dimensions.d1.value`, `m2/value`;
//! - `sketches`: parts, the solved points and circle radii of sketches with configured
//!   dimensions as this configuration last had them, so activating it again gives the
//!   same document bit for bit (and the regeneration cache hits);
//! - `components`: assemblies, the configuration each configured instance references;
//! - `display_state`: assemblies, the display state linked to it.
//!
//! Invariant (kept by the operations that configure something): every configured key has
//! an entry in every configuration. A key with no entry in a configuration takes the value
//! the document has. Activating configuration `b` writes `b`'s entries into the document
//! ([`apply_part_values`] here; the sketch re-solve is `oc_graph::config`). The active
//! configuration's own entries are refreshed from the document by [`capture_part`] before
//! anything copies or leaves them.
//!
//! Absent `configurations` (every document before schema 3) is one configuration,
//! `Default`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{DisplayMode, Id};

/// The name SOLIDWORKS gives the first configuration.
pub const DEFAULT_CONFIGURATION: &str = "Default";

/// The solved geometry of one sketch in a configuration.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SketchState {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub points: BTreeMap<Id, [f64; 2]>,
    /// Circle radii (circles carry their radius as a solved value).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub radii: BTreeMap<Id, f64>,
}

/// One configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Configuration {
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub suppressed: BTreeMap<Id, bool>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sketches: BTreeMap<Id, SketchState>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<Id, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_state: Option<Id>,
}

impl Configuration {
    pub fn new(id: &str, name: &str) -> Self {
        Configuration {
            id: Id::new(id),
            name: name.into(),
            description: None,
            suppressed: BTreeMap::new(),
            values: BTreeMap::new(),
            sketches: BTreeMap::new(),
            components: BTreeMap::new(),
            display_state: None,
        }
    }
}

/// A document's configurations and which one is active.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Configurations {
    pub active: Id,
    pub list: Vec<Configuration>,
}

impl Configurations {
    pub fn get(&self, id: &str) -> Option<&Configuration> {
        self.list.iter().find(|c| c.id == *id)
    }

    /// By id, else by name (assemblies reference part configurations by name, as
    /// SOLIDWORKS does).
    pub fn find(&self, id_or_name: &str) -> Option<&Configuration> {
        self.get(id_or_name).or_else(|| self.list.iter().find(|c| c.name == id_or_name))
    }

    pub fn active(&self) -> Option<&Configuration> {
        self.get(&self.active)
    }

    /// Structural problems: unknown active id, repeated ids or names, values whose key
    /// has no `/`.
    pub fn validate(&self) -> Vec<String> {
        let mut issues = vec![];
        if self.active().is_none() {
            issues.push(format!("the active configuration {} does not exist", self.active));
        }
        for (i, c) in self.list.iter().enumerate() {
            if self.list[..i].iter().any(|d| d.id == c.id) {
                issues.push(format!("configuration id {} is used twice", c.id));
            }
            if self.list[..i].iter().any(|d| d.name == c.name) {
                issues.push(format!("configuration name {:?} is used twice", c.name));
            }
            for k in c.values.keys() {
                if split_key(k).is_none() {
                    issues.push(format!("configuration {}: value key {k:?} is not <id>/<path>", c.name));
                }
            }
        }
        issues
    }
}

/// One occurrence's look in a display state.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Look {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<DisplayMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transparent: Option<bool>,
}

impl Look {
    pub fn is_default(&self) -> bool {
        *self == Look::default()
    }
}

/// A named display state of an assembly: hide/show, colour, display mode and transparency
/// of every occurrence. The active one's looks live in the instances.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DisplayState {
    pub id: Id,
    pub name: String,
    /// Occurrences that differ from the default look, by instance id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub instances: BTreeMap<Id, Look>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DisplayStates {
    pub active: Id,
    pub list: Vec<DisplayState>,
}

impl DisplayStates {
    pub fn get(&self, id: &str) -> Option<&DisplayState> {
        self.list.iter().find(|c| c.id == *id)
    }
}

/// `"<id>/<path>"` → (id, path segments).
pub fn split_key(key: &str) -> Option<(&str, Vec<&str>)> {
    let (id, path) = key.split_once('/')?;
    let parts: Vec<&str> = path.split('.').collect();
    (!id.is_empty() && parts.iter().all(|p| !p.is_empty())).then_some((id, parts))
}

/// The number at `path` in `v`.
pub fn get_path(v: &Value, path: &[&str]) -> Option<f64> {
    path.iter().try_fold(v, |o, k| o.get(*k))?.as_f64()
}

/// Sets the number at `path` in `v` (its parents must exist). Returns whether it did.
pub fn set_path(v: &mut Value, path: &[&str], x: f64) -> bool {
    let Some((last, parents)) = path.split_last() else { return false };
    let mut o = v;
    for k in parents {
        match o.get_mut(*k) {
            Some(next) => o = next,
            None => return false,
        }
    }
    match o.as_object_mut() {
        Some(m) if m.contains_key(*last) => {
            m.insert((*last).into(), serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number));
            true
        }
        _ => false,
    }
}

/// Every key configured in any configuration (values), and every id whose suppression is.
pub fn configured(c: &Configurations) -> (Vec<String>, Vec<Id>) {
    let mut values: Vec<String> = c.list.iter().flat_map(|x| x.values.keys().cloned()).collect();
    values.sort();
    values.dedup();
    let mut ids: Vec<Id> = c.list.iter().flat_map(|x| x.suppressed.keys().cloned()).collect();
    ids.sort();
    ids.dedup();
    (values, ids)
}

/// Sketch features whose solution a configuration keeps: those with a configured value.
pub fn configured_sketches(doc: &crate::PartDocument, c: &Configurations) -> Vec<Id> {
    let (values, _) = configured(c);
    let mut out: Vec<Id> = values
        .iter()
        .filter_map(|k| split_key(k))
        .filter(|(id, _)| doc.feature(id).is_some_and(|f| f.sketch().is_some()))
        .map(|(id, _)| Id::new(id))
        .collect();
    out.dedup();
    out
}

fn sketch_state(s: &crate::SketchData) -> SketchState {
    SketchState {
        points: s.points.iter().map(|(k, p)| (k.clone(), [p.x, p.y])).collect(),
        radii: s
            .entities
            .iter()
            .filter_map(|(k, e)| match e {
                crate::SketchEntity::Circle { r, .. } => Some((k.clone(), *r)),
                _ => None,
            })
            .collect(),
    }
}

/// Refreshes the active configuration's entries from the document: configured values,
/// configured suppression and configured sketches' solutions. Call before copying the
/// active configuration or leaving it (activation, saving).
pub fn capture_part(doc: &mut crate::PartDocument) {
    let Some(cfgs) = doc.configurations.as_deref() else { return };
    let (values, ids) = configured(cfgs);
    let sketches = configured_sketches(doc, cfgs);
    let mut cfgs = cfgs.clone();
    let active = cfgs.active.clone();
    let Some(a) = cfgs.list.iter_mut().find(|c| c.id == active) else { return };
    for k in values {
        let Some((id, path)) = split_key(&k) else { continue };
        let Some(f) = doc.feature(id) else { continue };
        let Ok(v) = serde_json::to_value(f) else { continue };
        if let Some(x) = get_path(&v, &path) {
            a.values.insert(k.clone(), x);
        }
    }
    for id in ids {
        if let Some(f) = doc.feature(&id) {
            a.suppressed.insert(id.clone(), f.is_suppressed());
        }
    }
    a.sketches.clear();
    for id in sketches {
        if let Some(s) = doc.feature(&id).and_then(|f| f.sketch()) {
            a.sketches.insert(id.clone(), sketch_state(s));
        }
    }
    doc.configurations = Some(std::sync::Arc::new(cfgs));
}

/// Writes configuration `id`'s values, suppression and sketch solutions into the
/// document's features and makes it the active one. Returns the sketch features whose
/// dimensions or geometry were written (the caller re-solves them: `oc_graph::config`).
/// The previous active configuration should have been captured ([`capture_part`]).
pub fn apply_part_values(doc: &mut crate::PartDocument, id: &str) -> Result<Vec<Id>, String> {
    let cfgs = doc.configurations.as_deref().ok_or("the document has no configurations")?.clone();
    let target = cfgs.find(id).ok_or_else(|| format!("no configuration {id}"))?.clone();
    let mut touched: Vec<Id> = vec![];
    let mut by_feature: BTreeMap<&str, Vec<(Vec<&str>, f64)>> = BTreeMap::new();
    for (k, x) in &target.values {
        if let Some((fid, path)) = split_key(k) {
            by_feature.entry(fid).or_default().push((path, *x));
        }
    }
    for (fid, sets) in by_feature {
        let Some(f) = doc.feature(fid) else { continue };
        let mut v = serde_json::to_value(f).map_err(|e| e.to_string())?;
        let mut changed = false;
        for (path, x) in sets {
            if get_path(&v, &path) != Some(x) && set_path(&mut v, &path, x) {
                changed = true;
            }
        }
        if changed {
            let nf: crate::Feature = serde_json::from_value(v).map_err(|e| format!("{fid}: {e}"))?;
            if nf.sketch().is_some() {
                touched.push(Id::new(fid));
            }
            *doc.feature_mut(fid).expect("found above") = nf;
        }
    }
    for (fid, s) in &target.suppressed {
        if let Some(f) = doc.feature(fid) {
            if f.is_suppressed() != *s {
                doc.feature_mut(fid).expect("found above").suppressed = s.then_some(true);
            }
        }
    }
    for (fid, st) in &target.sketches {
        let Some(sk) = doc.feature(fid).and_then(|f| f.sketch()) else { continue };
        let same = st.points.iter().all(|(k, p)| sk.points.get(k).is_none_or(|q| [q.x, q.y] == *p))
            && st.radii.iter().all(|(k, r)| match sk.entities.get(k) {
                Some(crate::SketchEntity::Circle { r: now, .. }) => now == r,
                _ => true,
            });
        if same {
            continue;
        }
        let sk = doc.feature_mut(fid).and_then(|f| f.sketch_mut()).expect("found above");
        for (k, p) in &st.points {
            if let Some(q) = sk.points.get_mut(k) {
                q.x = p[0];
                q.y = p[1];
            }
        }
        for (k, r) in &st.radii {
            if let Some(crate::SketchEntity::Circle { r: now, .. }) = sk.entities.get_mut(k) {
                *now = *r;
            }
        }
        if !touched.contains(fid) {
            touched.push(fid.clone());
        }
    }
    let mut cfgs = cfgs;
    cfgs.active = target.id.clone();
    doc.configurations = Some(std::sync::Arc::new(cfgs));
    Ok(touched)
}

/// The instance looks of an assembly as a display state stores them.
pub fn looks_of(doc: &crate::AssemblyDocument) -> BTreeMap<Id, Look> {
    doc.instances
        .iter()
        .map(|i| (i.id.clone(), Look { hidden: i.hidden, color: i.color.clone(), display: i.display, transparent: i.transparent }))
        .filter(|(_, l)| !l.is_default())
        .collect()
}

/// Refreshes the active configuration's and the active display state's entries from an
/// assembly document (configured instance and mate suppression, mate values, referenced
/// configurations, every instance's look).
pub fn capture_assembly(doc: &mut crate::AssemblyDocument) {
    if let Some(ds) = &mut doc.display_states {
        let looks = looks_of_instances(&doc.instances);
        let active = ds.active.clone();
        if let Some(s) = ds.list.iter_mut().find(|s| s.id == active) {
            s.instances = looks;
        }
    }
    let Some(cfgs) = doc.configurations.as_ref() else { return };
    let (values, ids) = configured(cfgs);
    let configured_components: Vec<Id> = {
        let mut v: Vec<Id> = cfgs.list.iter().flat_map(|c| c.components.keys().cloned()).collect();
        v.sort();
        v.dedup();
        v
    };
    let mut cfgs = cfgs.clone();
    let active = cfgs.active.clone();
    let Some(a) = cfgs.list.iter_mut().find(|c| c.id == active) else { return };
    for k in values {
        let Some((id, path)) = split_key(&k) else { continue };
        let Some(m) = doc.mates.iter().find(|m| m.id == *id) else { continue };
        let Ok(v) = serde_json::to_value(m) else { continue };
        if let Some(x) = get_path(&v, &path) {
            a.values.insert(k.clone(), x);
        }
    }
    for id in ids {
        if let Some(i) = doc.instances.iter().find(|i| i.id == id) {
            a.suppressed.insert(id.clone(), i.is_suppressed());
        } else if let Some(m) = doc.mates.iter().find(|m| m.id == id) {
            a.suppressed.insert(id.clone(), m.is_suppressed());
        }
    }
    for id in configured_components {
        if let Some(i) = doc.instances.iter().find(|i| i.id == id) {
            let cfg = i.config.clone().or_else(|| doc.component(&i.component).map(|c| c.config.clone())).unwrap_or_else(|| DEFAULT_CONFIGURATION.into());
            a.components.insert(id.clone(), cfg);
        }
    }
    if let Some(ds) = &doc.display_states {
        a.display_state = Some(ds.active.clone());
    }
    doc.configurations = Some(cfgs);
}

fn looks_of_instances(instances: &imbl::Vector<crate::Instance>) -> BTreeMap<Id, Look> {
    instances
        .iter()
        .map(|i| (i.id.clone(), Look { hidden: i.hidden, color: i.color.clone(), display: i.display, transparent: i.transparent }))
        .filter(|(_, l)| !l.is_default())
        .collect()
}

/// Writes display state `id`'s looks into the instances and makes it the active one.
pub fn apply_display_state(doc: &mut crate::AssemblyDocument, id: &str) -> Result<(), String> {
    let ds = doc.display_states.as_mut().ok_or("the assembly has no display states")?;
    let s = ds.get(id).ok_or_else(|| format!("no display state {id}"))?.clone();
    ds.active = s.id.clone();
    for i in doc.instances.iter_mut() {
        let l = s.instances.get(&i.id).cloned().unwrap_or_default();
        i.hidden = l.hidden;
        i.color = l.color;
        i.display = l.display;
        i.transparent = l.transparent;
    }
    Ok(())
}

/// Writes assembly configuration `id` into the document (instance and mate suppression,
/// mate values, the configuration each instance references, its linked display state) and
/// makes it the active one. The active configuration should have been captured first
/// ([`capture_assembly`]).
pub fn apply_assembly(doc: &mut crate::AssemblyDocument, id: &str) -> Result<(), String> {
    let cfgs = doc.configurations.clone().ok_or("the assembly has no configurations")?;
    let target = cfgs.find(id).ok_or_else(|| format!("no configuration {id}"))?.clone();
    let comp_config: BTreeMap<Id, String> = doc.components.iter().map(|c| (c.id.clone(), c.config.clone())).collect();
    for i in doc.instances.iter_mut() {
        if let Some(s) = target.suppressed.get(&i.id) {
            i.suppressed = s.then_some(true);
        }
        if let Some(c) = target.components.get(&i.id) {
            // The component's own configuration is written as no override.
            i.config = (comp_config.get(&i.component) != Some(c)).then(|| c.clone());
        }
    }
    for m in doc.mates.iter_mut() {
        if let Some(s) = target.suppressed.get(&m.id) {
            m.suppressed = s.then_some(true);
        }
        let sets: Vec<(Vec<&str>, f64)> = target.values.iter().filter_map(|(k, x)| split_key(k).filter(|(mid, _)| *mid == m.id.as_str()).map(|(_, p)| (p, *x))).collect();
        if sets.is_empty() {
            continue;
        }
        let mut v = serde_json::to_value(&*m).map_err(|e| e.to_string())?;
        for (path, x) in sets {
            set_path(&mut v, &path, x);
        }
        *m = serde_json::from_value(v).map_err(|e| format!("{}: {e}", m.id))?;
    }
    if let Some(ds) = target.display_state.as_deref() {
        if doc.display_states.as_ref().is_some_and(|d| d.get(ds).is_some()) {
            apply_display_state(doc, ds)?;
        }
    }
    let mut cfgs = cfgs;
    cfgs.active = target.id.clone();
    doc.configurations = Some(cfgs);
    Ok(())
}

/// The configuration an instance of a component references: its own (configured per
/// instance), else the component's.
pub fn referenced_configuration<'a>(doc: &'a crate::AssemblyDocument, i: &'a crate::Instance) -> &'a str {
    i.config.as_deref().or_else(|| doc.component(&i.component).map(|c| c.config.as_str())).unwrap_or(DEFAULT_CONFIGURATION)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_and_paths() {
        assert_eq!(split_key("f3/end1.depth"), Some(("f3", vec!["end1", "depth"])));
        assert_eq!(split_key("f1/sketch.dimensions.d1.value"), Some(("f1", vec!["sketch", "dimensions", "d1", "value"])));
        assert_eq!(split_key("nodot"), None);
        assert_eq!(split_key("/x"), None);
        assert_eq!(split_key("f1/a..b"), None);
        let mut v = json!({ "end1": { "depth": 10.0 }, "radius": 2 });
        assert_eq!(get_path(&v, &["end1", "depth"]), Some(10.0));
        assert!(set_path(&mut v, &["end1", "depth"], 25.0));
        assert_eq!(v["end1"]["depth"], 25.0);
        // Only existing numbers are set: a configuration never invents a field.
        assert!(!set_path(&mut v, &["end2", "depth"], 1.0));
        assert!(!set_path(&mut v, &["end1", "draft"], 1.0));
        assert_eq!(get_path(&v, &["radius"]), Some(2.0));
    }
}
