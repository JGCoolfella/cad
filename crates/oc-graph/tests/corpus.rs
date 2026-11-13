//! Every corpus part (`corpus/*.ocpart`) rebuilt by oc-graph on OCCT and compared with
//! the phase-0 oracle (`corpus/*.json`): volume, area, bounding box and reference
//! geometry to the oracle's six significant figures, solid/face/edge counts exactly,
//! statuses equal. Deviations recorded in `docs/OVERHAUL_PLAN.md` section 8 are listed in
//! `EXPECTED` with their reason; anything else fails.
mod common;

use common::*;

#[test]
fn corpus_matches_the_oracle() {
    let mut all = vec![];
    let mut boundary = 0;
    for part in PARTS {
        let doc = load(part);
        let mut r = regen();
        let reb = rebuild(&mut r, &doc);
        let (d, b) = diff_part(r.kernel(), part, &reb);
        boundary += b;
        for s in &reb.steps {
            eprintln!("{part:>18} {:<5} {:>8.2} ms {:?} {:?}", s.status.id, s.time.as_secs_f64() * 1e3, s.status.error, s.status.warning);
        }
        all.extend(d);
    }
    eprintln!("{boundary} values agree within 6e-6 but round differently in the sixth figure");
    assert!(all.is_empty(), "{} differences:\n{}", all.len(), all.join("\n"));
}
