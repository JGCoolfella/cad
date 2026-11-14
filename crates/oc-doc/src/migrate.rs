//! Schema versions and migrations.
//!
//! A saved document carries `version`. Loading runs, on the untyped JSON, every
//! migration from the file's version up to [`CURRENT_VERSION`], one step per version, and
//! only then reads the result into the typed model. A migration therefore never sees
//! typed data from another schema, and the typed model only ever describes the current
//! schema.
//!
//! To change the schema: bump [`CURRENT_VERSION`], change the types, and append a
//! [`Migration`] with `from` = the previous version to [`MIGRATIONS`]. The
//! `builtin_registry_is_a_chain` test checks the registry.

use serde_json::{Map, Value};

/// Schema version written by this build.
///
/// - 1: the TypeScript app's original format. References are geometric signatures.
/// - 2: references may carry persistent names (`name` on face and edge signatures and on
///   vertex references, `vertexName` on end conditions; `docs/OVERHAUL_PLAN.md`
///   section 3.3), and the part origin is its own kind of reference
///   (`{"kind": "origin"}`) instead of a vertex at (0, 0, 0).
pub const CURRENT_VERSION: u32 = 2;

/// One schema step: rewrites a version `from` document (the JSON root object) into a
/// version `from + 1` document. It need not touch `version`; the runner sets it.
#[derive(Clone, Copy, Debug)]
pub struct Migration {
    pub from: u32,
    pub description: &'static str,
    pub apply: fn(&mut Map<String, Value>) -> Result<(), String>,
}

/// An ordered chain of migrations ending at `current`.
#[derive(Clone, Copy, Debug)]
pub struct Migrations<'a> {
    pub current: u32,
    pub steps: &'a [Migration],
}

/// 1 → 2. Names need no rewriting: they are optional, and a version 1 reference is a
/// version 2 reference without one, which the first rebuild finds by signature and names
/// (a legacy reference, section 3.3 step 4).
///
/// The origin does: version 1 wrote a picked origin as `{"kind": "vertex", "p": [0, 0, 0]}`,
/// the same as a model vertex there. Version 2 has `{"kind": "origin"}`, so a vertex is
/// always model topology and can be followed by name. Every picked reference that is
/// exactly that object becomes the origin. (Sketch references to vertices carry an `id`
/// and are always model vertices; Up To Vertex stores a bare position and only ever takes
/// a model vertex. Neither is touched.)
fn origin_gets_its_own_kind(root: &mut Map<String, Value>) -> Result<(), String> {
    fn walk(v: &mut Value) {
        match v {
            Value::Object(m) => {
                let at_origin = |p: &Value| p.as_array().is_some_and(|a| a.len() == 3 && a.iter().all(|x| x.as_f64() == Some(0.0)));
                if m.len() == 2 && m.get("kind").and_then(Value::as_str) == Some("vertex") && m.get("p").is_some_and(at_origin) {
                    m.clear();
                    m.insert("kind".into(), Value::from("origin"));
                } else {
                    m.values_mut().for_each(walk);
                }
            }
            Value::Array(a) => a.iter_mut().for_each(walk),
            _ => {}
        }
    }
    if let Some(features) = root.get_mut("features") {
        walk(features);
    }
    Ok(())
}

/// The migrations of this build.
pub static MIGRATIONS: Migrations<'static> = Migrations {
    current: CURRENT_VERSION,
    steps: &[Migration {
        from: 1,
        description: "references may carry persistent names; the origin is its own kind of reference",
        apply: origin_gets_its_own_kind,
    }],
};

/// Why a document's version cannot be brought to the current one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MigrationError {
    /// Saved by a newer build.
    TooNew { found: u32, newest: u32 },
    /// Older than the oldest version a migration starts from.
    TooOld { found: u32, oldest: u32 },
    /// A migration step failed.
    Failed {
        from: u32,
        description: &'static str,
        message: String,
    },
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationError::TooNew { found, newest } => write!(
                f,
                "the document was saved by a newer OpenCAD (schema version {found}; this build reads up to {newest})"
            ),
            MigrationError::TooOld { found, oldest } => {
                write!(f, "schema version {found} is not supported (oldest readable: {oldest})")
            }
            MigrationError::Failed { from, description, message } => {
                write!(f, "upgrading from schema version {from} ({description}) failed: {message}")
            }
        }
    }
}

impl std::error::Error for MigrationError {}

impl Migrations<'_> {
    /// Oldest version this registry can read.
    pub fn oldest(&self) -> u32 {
        self.steps.first().map_or(self.current, |s| s.from)
    }

    /// Checks that the steps are exactly `oldest, oldest + 1, ..., current - 1`.
    pub fn check(&self) -> Result<(), String> {
        let oldest = self.oldest();
        if oldest > self.current {
            return Err(format!(
                "oldest version {oldest} is after the current version {}",
                self.current
            ));
        }
        for (i, s) in self.steps.iter().enumerate() {
            let want = oldest + i as u32;
            if s.from != want {
                return Err(format!(
                    "step {i} migrates from {}, expected {want}",
                    s.from
                ));
            }
        }
        if oldest + self.steps.len() as u32 != self.current {
            return Err(format!(
                "steps end at {}, not at {}",
                oldest + self.steps.len() as u32,
                self.current
            ));
        }
        Ok(())
    }

    /// Brings `root`, a document at schema version `found`, to `self.current`, and sets
    /// its `version`. Returns the number of steps run.
    pub fn upgrade(
        &self,
        root: &mut Map<String, Value>,
        found: u32,
    ) -> Result<usize, MigrationError> {
        if found > self.current {
            return Err(MigrationError::TooNew {
                found,
                newest: self.current,
            });
        }
        let oldest = self.oldest();
        if found < oldest {
            return Err(MigrationError::TooOld { found, oldest });
        }
        let todo = &self.steps[(found - oldest) as usize..];
        for step in todo {
            (step.apply)(root).map_err(|message| MigrationError::Failed {
                from: step.from,
                description: step.description,
                message,
            })?;
            root.insert("version".into(), Value::from(step.from + 1));
        }
        Ok(todo.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builtin_registry_is_a_chain() {
        MIGRATIONS.check().unwrap();
        assert_eq!(MIGRATIONS.current, CURRENT_VERSION);
    }

    fn add_a(m: &mut Map<String, Value>) -> Result<(), String> {
        m.insert("a".into(), json!(1));
        Ok(())
    }
    fn a_to_b(m: &mut Map<String, Value>) -> Result<(), String> {
        let a = m.remove("a").ok_or("no a")?;
        m.insert("b".into(), a);
        Ok(())
    }

    const STEPS: [Migration; 2] = [
        Migration {
            from: 3,
            description: "add a",
            apply: add_a,
        },
        Migration {
            from: 4,
            description: "rename a to b",
            apply: a_to_b,
        },
    ];

    #[test]
    fn runs_the_steps_from_the_found_version_in_order() {
        let reg = Migrations {
            current: 5,
            steps: &STEPS,
        };
        reg.check().unwrap();
        let mut m = Map::new();
        assert_eq!(reg.upgrade(&mut m, 3), Ok(2));
        assert_eq!(Value::Object(m), json!({"b": 1, "version": 5}));

        let mut m = json!({"a": 7}).as_object().unwrap().clone();
        assert_eq!(reg.upgrade(&mut m, 4), Ok(1));
        assert_eq!(Value::Object(m), json!({"b": 7, "version": 5}));

        let mut m = Map::new();
        assert_eq!(reg.upgrade(&mut m, 5), Ok(0));
        assert!(m.is_empty());
    }

    #[test]
    fn rejects_versions_it_cannot_reach_and_reports_failing_steps() {
        let reg = Migrations {
            current: 5,
            steps: &STEPS,
        };
        assert_eq!(
            reg.upgrade(&mut Map::new(), 6),
            Err(MigrationError::TooNew {
                found: 6,
                newest: 5
            })
        );
        assert_eq!(
            reg.upgrade(&mut Map::new(), 2),
            Err(MigrationError::TooOld {
                found: 2,
                oldest: 3
            })
        );
        let err = reg.upgrade(&mut Map::new(), 4).unwrap_err();
        assert_eq!(
            err,
            MigrationError::Failed {
                from: 4,
                description: "rename a to b",
                message: "no a".into()
            }
        );
    }

    #[test]
    fn check_rejects_gaps_and_short_chains() {
        let gap = [
            STEPS[0],
            Migration {
                from: 5,
                ..STEPS[1]
            },
        ];
        assert!(Migrations {
            current: 6,
            steps: &gap
        }
        .check()
        .is_err());
        assert!(Migrations {
            current: 6,
            steps: &STEPS
        }
        .check()
        .is_err());
        assert!(Migrations {
            current: 1,
            steps: &[]
        }
        .check()
        .is_ok());
    }
}
