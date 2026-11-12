//! Turns sketch data into parameters and equations (`SketchSystem` constructor and
//! `buildEquations` in TypeScript). Parameters and equations are created in the same order
//! as the TypeScript solver, so parameter vectors can be compared index by index.
//!
//! Deviation, fail-soft: a relation or dimension referring to a point that does not exist is
//! skipped. (TypeScript would throw when evaluating it.)

use crate::equations::{signed_offset, EqKind, Measure, OffsetPair, Pt, Rad, RadOf, Size, Values};
use crate::model::{Dimension, DimensionKind, Id, Relation, SketchData, SketchEntity, ORIGIN_ID};
use indexmap::IndexMap;
use std::collections::HashSet;

/// Who a parameter belongs to.
#[derive(Clone, Debug, PartialEq)]
pub enum ParamOwner {
    /// Coordinate `axis` (0 = x, 1 = y) of a point.
    Point { id: Id, axis: u8 },
    /// Radius of a circle.
    Radius { id: Id },
}

/// Whose equation it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// Arc ends equidistant from the centre, ellipse axes perpendicular.
    Internal,
    /// Index into `Built::owner_ids`.
    Item(usize),
}

#[derive(Clone, Debug)]
pub struct Equation {
    pub owner: Owner,
    pub kind: EqKind,
    /// Free parameters read, sorted.
    pub vars: Vec<usize>,
}

/// An entity with its references resolved.
#[derive(Clone, Debug)]
pub enum Ent {
    Line { p1: Pt, p2: Pt },
    Circle { c: Pt, r: Rad },
    Arc { c: Pt, p1: Pt, p2: Pt },
    Point { p: Pt },
    Ellipse { c: Pt, a: Pt, q: Pt },
    Spline { pts: Vec<Pt> },
}

pub struct Built {
    pub x: Vec<f64>,
    pub owners: Vec<ParamOwner>,
    pub pts: IndexMap<Id, Pt>,
    pub radii: IndexMap<Id, Rad>,
    pub eqs: Vec<Equation>,
    pub owner_ids: Vec<Id>,
}

pub fn build(sk: &SketchData, extra_fixed: &HashSet<Id>) -> Built {
    let mut fixed: HashSet<&str> = extra_fixed.iter().map(String::as_str).collect();
    fixed.insert(ORIGIN_ID);
    let mut fixed_radius: HashSet<&str> = HashSet::new();
    for r in sk.relations.values() {
        let (Relation::Fix { target, .. } | Relation::OnEdge { target, .. }) = r else {
            continue;
        };
        if sk.points.contains_key(target) {
            fixed.insert(target);
        } else if let Some(e) = sk.entities.get(target) {
            fixed.extend(e.points());
            if matches!(e, SketchEntity::Circle { .. }) {
                fixed_radius.insert(target);
            }
        }
    }
    // Geometry standing for model references is placed by the rebuild, never by the solver.
    for (id, e) in &sk.entities {
        if let SketchEntity::Circle { ext: Some(_), .. } = e {
            fixed_radius.insert(id);
        }
    }
    let mut b = Built {
        x: Vec::new(),
        owners: Vec::new(),
        pts: IndexMap::new(),
        radii: IndexMap::new(),
        eqs: Vec::new(),
        owner_ids: Vec::new(),
    };
    for (id, p) in &sk.points {
        if fixed.contains(id.as_str()) || p.ext.is_some() {
            b.pts.insert(id.clone(), Pt::Fixed([p.x, p.y]));
        } else {
            let i = b.x.len();
            b.x.extend([p.x, p.y]);
            b.owners.push(ParamOwner::Point {
                id: id.clone(),
                axis: 0,
            });
            b.owners.push(ParamOwner::Point {
                id: id.clone(),
                axis: 1,
            });
            b.pts.insert(id.clone(), Pt::Var(i));
        }
    }
    for (id, e) in &sk.entities {
        let SketchEntity::Circle { r, .. } = e else {
            continue;
        };
        if fixed_radius.contains(id.as_str()) {
            b.radii.insert(id.clone(), Rad::Fixed(*r));
        } else {
            b.radii.insert(id.clone(), Rad::Var(b.x.len()));
            b.x.push(*r);
            b.owners.push(ParamOwner::Radius { id: id.clone() });
        }
    }
    let r = Resolver {
        sk,
        pts: &b.pts,
        radii: &b.radii,
    };
    let mut eb = EqBuilder {
        r,
        x: &b.x,
        pending: Vec::new(),
        eqs: Vec::new(),
        owner_ids: Vec::new(),
    };
    eb.build_all();
    let EqBuilder { eqs, owner_ids, .. } = eb;
    b.eqs = eqs;
    b.owner_ids = owner_ids;
    b
}

/// Resolution failed: skip the item.
pub struct Missing;
type R<T> = Result<T, Missing>;

/// Resolves ids to parameters, shared by equation building and [`Resolver::measure`].
pub struct Resolver<'a> {
    pub sk: &'a SketchData,
    pub pts: &'a IndexMap<Id, Pt>,
    pub radii: &'a IndexMap<Id, Rad>,
}

impl Resolver<'_> {
    fn p(&self, id: &str) -> R<Pt> {
        self.pts.get(id).copied().ok_or(Missing)
    }

    fn opt_p(&self, id: Option<&String>) -> R<Pt> {
        id.map_or(Err(Missing), |id| self.p(id))
    }

    /// `Ok(None)`: no such entity (the item is ignored, as in TypeScript).
    fn ent(&self, id: &str) -> R<Option<Ent>> {
        let p = |s: &String| self.p(s);
        Ok(Some(match self.sk.entities.get(id) {
            Some(SketchEntity::Line { p1, p2, .. }) => Ent::Line {
                p1: p(p1)?,
                p2: p(p2)?,
            },
            Some(SketchEntity::Circle { c, .. }) => Ent::Circle {
                c: p(c)?,
                r: self.radii[id],
            },
            Some(SketchEntity::Arc { c, p1, p2, .. }) => Ent::Arc {
                c: p(c)?,
                p1: p(p1)?,
                p2: p(p2)?,
            },
            Some(SketchEntity::Point { p: q, .. }) => Ent::Point { p: p(q)? },
            Some(SketchEntity::Ellipse { c, a, q, .. }) => Ent::Ellipse {
                c: p(c)?,
                a: p(a)?,
                q: p(q)?,
            },
            Some(SketchEntity::Spline { pts, .. }) => Ent::Spline {
                pts: pts.iter().map(p).collect::<R<_>>()?,
            },
            Some(SketchEntity::Other) | None => return Ok(None),
        }))
    }

    fn opt_ent(&self, id: Option<&String>) -> R<Option<Ent>> {
        id.map_or(Ok(None), |id| self.ent(id))
    }

    fn line(&self, id: Option<&String>) -> R<Option<(Pt, Pt)>> {
        Ok(match self.opt_ent(id)? {
            Some(Ent::Line { p1, p2 }) => Some((p1, p2)),
            _ => None,
        })
    }

    /// What a dimension measures (`measureFn`). `Ok(None)` when its references are of the
    /// wrong kind (TypeScript returns `null`), `Err` when a point is missing.
    pub fn measure(&self, d: &Dimension) -> R<Option<Measure>> {
        let (r0, r1) = (d.refs.first(), d.refs.get(1));
        let sign = d.sign.unwrap_or(1.0);
        Ok(Some(match d.kind {
            DimensionKind::Distance => Measure::Distance(self.opt_p(r0)?, self.opt_p(r1)?),
            DimensionKind::Horizontal | DimensionKind::Vertical => {
                let axis = usize::from(d.kind == DimensionKind::Vertical);
                Measure::Delta {
                    sign,
                    axis,
                    p: self.opt_p(r0)?,
                    q: self.opt_p(r1)?,
                }
            }
            DimensionKind::PointLine => {
                let Some((a, b)) = self.line(r1)? else {
                    return Ok(None);
                };
                Measure::PointLine {
                    p: self.opt_p(r0)?,
                    a,
                    b,
                }
            }
            DimensionKind::LineLine | DimensionKind::Angle => {
                let (Some((a1, a2)), Some((b1, b2))) = (self.line(r0)?, self.line(r1)?) else {
                    return Ok(None);
                };
                if d.kind == DimensionKind::Angle {
                    Measure::Angle {
                        sign,
                        quadrant: d.quadrant.unwrap_or(0),
                        a1,
                        a2,
                        b1,
                        b2,
                    }
                } else {
                    Measure::LineLine { a1, a2, b1, b2 }
                }
            }
            DimensionKind::Radius | DimensionKind::Diameter => {
                let Some(e) = self.opt_ent(r0)? else {
                    return Ok(None);
                };
                let k = if d.kind == DimensionKind::Diameter {
                    2.0
                } else {
                    1.0
                };
                Measure::Radius { k, r: rad_of(&e) }
            }
            DimensionKind::Offset => {
                let (Some(a), Some(b)) = (self.opt_ent(r0)?, self.opt_ent(r1)?) else {
                    return Ok(None);
                };
                Measure::Offset(offset_pair(&a, &b))
            }
            DimensionKind::ArcLength => {
                let Some(Ent::Arc { c, p1, p2 }) = self.opt_ent(r0)? else {
                    return Ok(None);
                };
                Measure::ArcLength { c, p1, p2 }
            }
            DimensionKind::Other => return Ok(None),
        }))
    }
}

struct EqBuilder<'a> {
    r: Resolver<'a>,
    x: &'a [f64],
    /// Equations of the item being built; committed only if every reference resolved.
    pending: Vec<EqKind>,
    eqs: Vec<Equation>,
    owner_ids: Vec<Id>,
}

impl EqBuilder<'_> {
    fn build_all(&mut self) {
        let sk = self.r.sk;
        for e in sk.entities.values() {
            self.item(Owner::Internal, |s| s.internal(e));
        }
        for rel in sk.relations.values() {
            let Some(id) = rel.id() else { continue };
            let owner = self.owner_index(id);
            self.item(owner, |s| s.relation(rel));
        }
        for d in sk.dimensions.values() {
            if d.driven == Some(true) {
                continue;
            }
            let owner = self.owner_index(&d.id);
            self.item(owner, |s| {
                if let Some(m) = s.r.measure(d)? {
                    s.push(EqKind::Dim {
                        measure: m,
                        value: d.value,
                        angle: d.kind == DimensionKind::Angle,
                    });
                }
                Ok(())
            });
        }
    }

    fn owner_index(&mut self, id: &str) -> Owner {
        self.owner_ids.push(id.to_string());
        Owner::Item(self.owner_ids.len() - 1)
    }

    fn item(&mut self, owner: Owner, f: impl FnOnce(&mut Self) -> R<()>) {
        self.pending.clear();
        if f(self).is_ok() {
            for kind in std::mem::take(&mut self.pending) {
                let vars = kind.vars();
                self.eqs.push(Equation { owner, kind, vars });
            }
        }
    }

    fn push(&mut self, k: EqKind) {
        self.pending.push(k);
    }

    fn internal(&mut self, e: &SketchEntity) -> R<()> {
        let r = &self.r;
        let k = match e {
            SketchEntity::Ellipse { c, a, q, .. } => EqKind::EllipseAxes {
                c: r.p(c)?,
                a: r.p(a)?,
                q: r.p(q)?,
            },
            SketchEntity::Arc { c, p1, p2, .. } => EqKind::EqualDist {
                c: r.p(c)?,
                a: r.p(p1)?,
                b: r.p(p2)?,
            },
            _ => return Ok(()),
        };
        self.push(k);
        Ok(())
    }

    fn point_on(&mut self, p: Pt, e: &Ent) {
        match e {
            Ent::Line { p1, p2 } => self.push(EqKind::PointOnLine { p, a: *p1, b: *p2 }),
            Ent::Circle { .. } | Ent::Arc { .. } => {
                let (c, r) = center_rad(e).expect("circle or arc");
                self.push(EqKind::PointOnCircle { p, c, r });
            }
            Ent::Point { p: q } => {
                self.push(EqKind::Diff {
                    axis: 0,
                    a: p,
                    b: *q,
                });
                self.push(EqKind::Diff {
                    axis: 1,
                    a: p,
                    b: *q,
                });
            }
            Ent::Ellipse { c, a, q } => self.push(EqKind::PointOnEllipse {
                p,
                c: *c,
                a: *a,
                q: *q,
            }),
            Ent::Spline { pts } => self.push(EqKind::PointOnSpline {
                p,
                pts: pts.clone(),
            }),
        }
    }

    fn relation(&mut self, rel: &Relation) -> R<()> {
        use Relation::*;
        match rel {
            Horizontal { e, .. } | Vertical { e, .. } => {
                let Some(Ent::Line { p1, p2 }) = self.r.ent(e)? else {
                    return Ok(());
                };
                let axis = usize::from(matches!(rel, Horizontal { .. }));
                self.push(EqKind::Diff { axis, a: p1, b: p2 });
            }
            HorizontalPoints { p1, p2, .. } | VerticalPoints { p1, p2, .. } => {
                let axis = usize::from(matches!(rel, HorizontalPoints { .. }));
                let k = EqKind::Diff {
                    axis,
                    a: self.r.p(p1)?,
                    b: self.r.p(p2)?,
                };
                self.push(k);
            }
            Coincident { p, e, .. } => {
                if let Some(ent) = self.r.ent(e)? {
                    let p = self.r.p(p)?;
                    self.point_on(p, &ent);
                } else if self.r.sk.points.contains_key(e) {
                    let (a, b) = (self.r.p(p)?, self.r.p(e)?);
                    self.push(EqKind::Diff { axis: 0, a, b });
                    self.push(EqKind::Diff { axis: 1, a, b });
                }
            }
            Midpoint { p, e, .. } => match self.r.ent(e)? {
                Some(Ent::Line { p1, p2 }) => {
                    let p = self.r.p(p)?;
                    for axis in 0..2 {
                        self.push(EqKind::Midpoint {
                            axis,
                            p,
                            a: p1,
                            b: p2,
                        });
                    }
                }
                Some(arc @ Ent::Arc { p1, p2, .. }) => {
                    let p = self.r.p(p)?;
                    self.point_on(p, &arc);
                    self.push(EqKind::EqualDist { c: p, a: p1, b: p2 });
                }
                _ => {}
            },
            Parallel { e1, e2, .. } | Perpendicular { e1, e2, .. } | Collinear { e1, e2, .. } => {
                let (Some(Ent::Line { p1: a1, p2: a2 }), Some(Ent::Line { p1: b1, p2: b2 })) =
                    (self.r.ent(e1)?, self.r.ent(e2)?)
                else {
                    return Ok(());
                };
                if matches!(rel, Collinear { .. }) {
                    self.push(EqKind::PointOnLine {
                        p: b1,
                        a: a1,
                        b: a2,
                    });
                    self.push(EqKind::PointOnLine {
                        p: b2,
                        a: a1,
                        b: a2,
                    });
                } else {
                    self.push(EqKind::Angle {
                        cross: matches!(rel, Parallel { .. }),
                        a1,
                        a2,
                        b1,
                        b2,
                    });
                }
            }
            Equal { e1, e2, .. } => {
                let (Some(a), Some(b)) = (self.r.ent(e1)?, self.r.ent(e2)?) else {
                    return Ok(());
                };
                self.push(EqKind::Equal {
                    a: size_of(&a),
                    b: size_of(&b),
                });
            }
            Tangent { e1, e2, inner, .. } => self.tangent(e1, e2, *inner == Some(true))?,
            Concentric { e1, e2, .. } | Coradial { e1, e2, .. } => {
                let (Some(a), Some(b)) = (self.r.ent(e1)?, self.r.ent(e2)?) else {
                    return Ok(());
                };
                let (Some((ca, ra)), Some((cb, rb))) = (center_rad(&a), center_rad(&b)) else {
                    return Ok(());
                };
                self.push(EqKind::Diff {
                    axis: 0,
                    a: ca,
                    b: cb,
                });
                self.push(EqKind::Diff {
                    axis: 1,
                    a: ca,
                    b: cb,
                });
                if matches!(rel, Coradial { .. }) {
                    self.push(EqKind::RadDiff(ra, rb));
                }
            }
            Symmetric { p1, p2, axis, .. } => {
                let Some(Ent::Line { p1: a, p2: b }) = self.r.ent(axis)? else {
                    return Ok(());
                };
                let (p, q) = (self.r.p(p1)?, self.r.p(p2)?);
                self.push(EqKind::SymmetricMid { p, q, a, b });
                self.push(EqKind::SymmetricAlong { p, q, a, b });
            }
            Intersection { p, e1, e2, .. } => {
                let (a, b) = (self.r.ent(e1)?, self.r.ent(e2)?);
                for ent in [a, b].into_iter().flatten() {
                    let p = self.r.p(p)?;
                    self.point_on(p, &ent);
                }
            }
            Equidistant { c, p1, p2, .. } => {
                let k = EqKind::EqualDist {
                    c: self.r.p(c)?,
                    a: self.r.p(p1)?,
                    b: self.r.p(p2)?,
                };
                self.push(k);
            }
            Offset { e1, e2, m1, m2, .. } => {
                let ents = (
                    self.r.ent(e1)?,
                    self.r.ent(e2)?,
                    self.r.ent(m1)?,
                    self.r.ent(m2)?,
                );
                let (Some(a), Some(b), Some(ma), Some(mb)) = ents else {
                    return Ok(());
                };
                let (ab, m) = (offset_pair(&a, &b), offset_pair(&ma, &mb));
                // Keep the sign seen at creation so the copy never flips sides
                // (`Math.sign(v) || 1`).
                let sign = |o: OffsetPair| {
                    let s = signed_offset(&Values(self.x), o);
                    if s == 0.0 || s.is_nan() {
                        1.0
                    } else {
                        s.signum()
                    }
                };
                let (s0, sm) = (sign(ab), sign(m));
                self.push(EqKind::Offset { s0, sm, ab, m });
            }
            Fix { .. } | OnEdge { .. } | Other => {} // fixes are handled by removing parameters
        }
        Ok(())
    }

    fn tangent(&mut self, e1: &str, e2: &str, inner: bool) -> R<()> {
        let (Some(mut a), Some(mut b)) = (self.r.ent(e1)?, self.r.ent(e2)?) else {
            return Ok(());
        };
        let (mut ida, mut idb) = (e1, e2);
        if matches!(b, Ent::Line { .. }) {
            std::mem::swap(&mut a, &mut b);
            std::mem::swap(&mut ida, &mut idb);
        }
        let sk = self.r.sk;
        let ends = |id: &str| match sk.entities.get(id) {
            Some(SketchEntity::Arc { p1, p2, .. }) => vec![p1.as_str(), p2.as_str()],
            _ => vec![],
        };
        match (&a, &b) {
            (Ent::Line { p1: l1, p2: l2 }, Ent::Arc { .. } | Ent::Circle { .. }) => {
                let (c, r) = center_rad(&b).expect("arc or circle");
                let Some(SketchEntity::Line { p1, p2, .. }) = sk.entities.get(ida) else {
                    unreachable!("resolved as a line")
                };
                // When the arc ends on the line (fillets, slots, tangent arcs) the distance form
                // is stationary at the solution, so use "radius at the shared point ⟂ line".
                if let Some(shared) = ends(idb).into_iter().find(|&p| p == p1 || p == p2) {
                    let s = self.r.p(shared)?;
                    self.push(EqKind::TangentLineShared {
                        l1: *l1,
                        l2: *l2,
                        c,
                        s,
                    });
                } else {
                    self.push(EqKind::TangentLineCircle {
                        l1: *l1,
                        l2: *l2,
                        c,
                        r,
                    });
                }
            }
            (Ent::Arc { .. } | Ent::Circle { .. }, Ent::Arc { .. } | Ent::Circle { .. }) => {
                let ((c1, r1), (c2, r2)) =
                    (center_rad(&a).expect("arc"), center_rad(&b).expect("arc"));
                // Two arcs meeting at a shared end point are tangent when both centres line up
                // with it.
                let eb = ends(idb);
                if let Some(shared) = ends(ida).into_iter().find(|p| eb.contains(p)) {
                    let s = self.r.p(shared)?;
                    self.push(EqKind::TangentArcsShared { s, c1, c2 });
                } else {
                    self.push(EqKind::TangentCircles {
                        c1,
                        r1,
                        c2,
                        r2,
                        inner,
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn center_rad(e: &Ent) -> Option<(Pt, RadOf)> {
    match *e {
        Ent::Circle { c, r } => Some((c, RadOf::Circle(r))),
        Ent::Arc { c, p1, .. } => Some((c, RadOf::Arc { c, p1 })),
        _ => None,
    }
}

fn rad_of(e: &Ent) -> RadOf {
    center_rad(e).map_or(RadOf::Zero, |(_, r)| r)
}

fn size_of(e: &Ent) -> Size {
    match *e {
        Ent::Line { p1, p2 } => Size::Len(p1, p2),
        _ => Size::Rad(rad_of(e)),
    }
}

fn offset_pair(x: &Ent, y: &Ent) -> OffsetPair {
    match (x, y) {
        (Ent::Line { p1: x1, p2: x2 }, Ent::Line { p1: y1, p2: y2 }) => OffsetPair::Lines {
            x1: *x1,
            x2: *x2,
            y1: *y1,
            y2: *y2,
        },
        _ => OffsetPair::Radii {
            x: rad_of(x),
            y: rad_of(y),
        },
    }
}
