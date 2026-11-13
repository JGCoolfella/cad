// Profiles (segments → edges → wires → faces) and the operations that sweep them:
// prism (optionally tapered), revolve, sweep, loft. Each records which faces every
// profile segment generated and which faces are the caps.
#include "oc-kernel-occt/cpp/internal.h"

#include <cmath>

#include <BRepAdaptor_Curve.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepOffsetAPI_MakePipeShell.hxx>
#include <BRepOffsetAPI_ThruSections.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRep_Tool.hxx>
#include <BRepGProp.hxx>
#include <GC_MakeArcOfCircle.hxx>
#include <GProp_GProps.hxx>
#include <GeomAPI_PointsToBSpline.hxx>
#include <Geom_BSplineCurve.hxx>
#include <Geom_Plane.hxx>
#include <Geom_TrimmedCurve.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepOffsetAPI_DraftAngle.hxx>
#include <BRepOffsetAPI_MakeOffset.hxx>
#include <gp_Pln.hxx>
#include <ShapeFix_Face.hxx>
#include <NCollection_Array1.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Circ.hxx>
#include <gp_Elips.hxx>

namespace occ {

static TopoDS_Edge make_edge(const Tol &tol, const SegIn &s) {
  const rust::Vec<double> &v = s.v;
  auto need = [&](size_t n) {
    if (v.size() < n) fail("input", "a profile segment has too few numbers");
  };
  switch (s.kind) {
  case 0: {
    need(6);
    BRepBuilderAPI_MakeEdge e(pnt(v, 0), pnt(v, 3));
    if (!e.IsDone()) fail("input", "a line segment could not be built");
    return e.Edge();
  }
  case 1: {
    need(9);
    GC_MakeArcOfCircle arc(pnt(v, 0), pnt(v, 3), pnt(v, 6));
    if (!arc.IsDone()) fail("input", "an arc could not be built from its three points");
    return BRepBuilderAPI_MakeEdge(arc.Value()).Edge();
  }
  case 2: {
    need(7);
    gp_Ax2 ax(pnt(v, 0), gp_Dir(v[3], v[4], v[5]));
    return BRepBuilderAPI_MakeEdge(gp_Circ(ax, v[6])).Edge();
  }
  case 3: {
    need(11);
    gp_Dir n(v[3], v[4], v[5]);
    gp_Vec x(v[6], v[7], v[8]);
    x -= gp_Vec(n) * x.Dot(gp_Vec(n)); // the major axis must be perpendicular to the normal
    gp_Ax2 ax(pnt(v, 0), n, gp_Dir(x));
    return BRepBuilderAPI_MakeEdge(gp_Elips(ax, v[9], v[10])).Edge();
  }
  case 4: {
    size_t n = v.size() / 3;
    if (n < 2) fail("input", "a spline needs at least two points");
    NCollection_Array1<gp_Pnt> pts(1, static_cast<int>(n));
    for (size_t i = 0; i < n; ++i) pts.SetValue(static_cast<int>(i) + 1, pnt(v, 3 * i));
    int deg = static_cast<int>(std::min<size_t>(3, n - 1));
    GeomAPI_PointsToBSpline fit(pts, deg, 3, GeomAbs_C2, tol.spline_fit);
    if (!fit.IsDone()) fail("input", "a spline could not be fitted to its points");
    return BRepBuilderAPI_MakeEdge(fit.Curve()).Edge();
  }
  }
  fail("input", "unknown segment kind");
}

BuiltWire build_wire(const Tol &tol, const WireIn &w) {
  BuiltWire out;
  BRepBuilderAPI_MakeWire mw;
  for (const SegIn &s : w.segs) {
    mw.Add(make_edge(tol, s));
    if (!mw.IsDone()) fail("input", "the segments of a wire do not connect");
    out.edges.push_back(mw.Edge()); // the edge as stored in the wire (vertices merged)
  }
  if (out.edges.empty()) fail("input", "a wire has no segments");
  out.wire = mw.Wire();
  return out;
}

static gp_Dir face_normal(const TopoDS_Face &f) {
  Handle(Geom_Plane) pl = Handle(Geom_Plane)::DownCast(BRep_Tool::Surface(f));
  if (pl.IsNull()) fail("input", "a profile is not planar");
  gp_Dir n = pl->Axis().Direction();
  if (f.Orientation() == TopAbs_REVERSED) n.Reverse();
  return n;
}

BuiltRegion build_region(const Tol &tol, const RegionIn &r) {
  BuiltRegion out;
  out.wires.push_back(build_wire(tol, r.outer));
  BRepBuilderAPI_MakeFace mf(out.wires[0].wire, true);
  if (!mf.IsDone()) fail("input", "a profile contour is not planar or not closed");
  for (const WireIn &h : r.holes) {
    out.wires.push_back(build_wire(tol, h));
    mf.Add(out.wires.back().wire);
  }
  TopoDS_Face face = mf.Face();
  if (!r.holes.empty()) {
    // Holes must run against the outer boundary; the fix reverses wires, keeping edges.
    ShapeFix_Face fix(face);
    fix.FixOrientation();
    face = fix.Face();
  }
  out.face = face;
  out.normal = face_normal(face);
  return out;
}

static void orient(BuiltRegion &reg, const gp_Vec &along) {
  if (gp_Vec(reg.normal).Dot(along) < 0) {
    reg.face.Reverse();
    reg.normal.Reverse();
  }
}

// solids[0] minus the rest (the bodies swept from a region's holes), history composed.
static TopoDS_Shape cut_holes(const Tol &tol, const std::vector<TopoDS_Shape> &solids, Hist &h) {
  if (solids.size() == 1) return solids[0];
  ShapeList args, tools;
  args.Append(solids[0]);
  for (size_t k = 1; k < solids.size(); ++k) tools.Append(solids[k]);
  BRepAlgoAPI_Cut cut;
  cut.SetArguments(args);
  cut.SetTools(tools);
  cut.SetNonDestructive(true);
  cut.SetRunParallel(tol.parallel);
  cut.Build();
  if (!cut.IsDone() || cut.HasErrors()) fail("failed", "the holes of the profile could not be cut");
  compose(h, cut);
  return cut.Shape();
}

// Area enclosed by a closed planar wire.
static double wire_area(const TopoDS_Wire &w) {
  BRepBuilderAPI_MakeFace mf(w, true);
  if (!mf.IsDone()) return 0;
  GProp_GProps g;
  BRepGProp::SurfaceProperties(mf.Face(), g);
  return std::abs(g.Mass());
}

// `w` offset in its plane with sharp (extended and intersected) corners, so that its area
// grows when `grow` > 0 and shrinks when `grow` < 0.
static TopoDS_Wire offset_wire(const TopoDS_Wire &w, double grow) {
  const double a0 = wire_area(w);
  for (double sign : {1.0, -1.0}) {
    BRepOffsetAPI_MakeOffset mk(w, GeomAbs_Intersection, false);
    mk.Perform(sign * grow);
    if (!mk.IsDone()) continue;
    TopExp_Explorer ex(mk.Shape(), TopAbs_WIRE);
    if (!ex.More()) continue;
    TopoDS_Wire out = TopoDS::Wire(ex.Current());
    if ((wire_area(out) > a0) == (grow > 0)) return out;
  }
  fail("failed", "the tapered profile could not be offset (the taper is too large for the profile)");
}

// Tapered extrusion as a ruled loft from each wire to its offset copy at the far end
// (the oracle's construction): sharp corners and exact volume, but OCCT makes the ruled
// sides B-spline surfaces. Holes taper the opposite way and are cut out.
static TopoDS_Shape tapered_loft(const Tol &tol, const BuiltRegion &reg, uint32_t ri, const gp_Vec &dv, double d,
                                 Hist &h) {
  gp_Trsf move;
  move.SetTranslation(dv);
  std::vector<TopoDS_Shape> solids;
  for (size_t w = 0; w < reg.wires.size(); ++w) {
    const BuiltWire &bw = reg.wires[w];
    TopoDS_Wire far = offset_wire(bw.wire, w == 0 ? d : -d);
    far = TopoDS::Wire(BRepBuilderAPI_Transform(far, move, true).Shape());
    BRepOffsetAPI_ThruSections mk(true, true, tol.loft);
    mk.AddWire(bw.wire);
    mk.AddWire(far);
    mk.Build();
    if (!mk.IsDone()) fail("failed", "the tapered extrusion could not be built");
    for (size_t k = 0; k < bw.edges.size(); ++k)
      record_generated(h, SrcSegment, ri, static_cast<uint32_t>(w), static_cast<uint32_t>(k), mk.Generated(bw.edges[k]));
    if (w == 0) {
      record_cap(h, ri, 0, mk.FirstShape());
      record_cap(h, ri, 1, mk.LastShape());
    }
    solids.push_back(mk.Shape());
  }
  return cut_holes(tol, solids, h);
}

// Tapered extrusion. Polygon profiles: a straight prism whose planar sides are then drafted
// about the profile plane, giving exact planes with sharp corners (SOLIDWORKS' result).
// Inward (the default) tilts every side towards the material, so the far end is smaller
// and holes widen. Profiles with curved segments use the ruled loft (exact cones for arcs,
// B-spline sides for lines): OCCT 8.0.1's DraftAngle crashes the process (access
// violation) on the cylindrical side of a circle prism, so it is never given one.
static TopoDS_Shape tapered(const Tol &tol, const BuiltRegion &reg, uint32_t ri, const gp_Vec &dv, double angle,
                            bool outward, Hist &h) {
  bool polygon = true;
  for (const BuiltWire &w : reg.wires)
    for (const TopoDS_Edge &e : w.edges) polygon = polygon && BRepAdaptor_Curve(e).GetType() == GeomAbs_Line;
  BRepPrimAPI_MakePrism prism(reg.face, dv, false, true);
  if (polygon && prism.IsDone()) {
    Hist h2;
    record_segments(h2, ri, reg, [&](const TopoDS_Edge &e) -> const ShapeList & { return prism.Generated(e); });
    record_cap(h2, ri, 0, prism.FirstShape());
    record_cap(h2, ri, 1, prism.LastShape());
    gp_Dir pull(dv);
    gp_Pln neutral(BRep_Tool::Pnt(TopExp::FirstVertex(reg.wires[0].edges[0])), pull);
    BRepOffsetAPI_DraftAngle draft(prism.Shape());
    bool ok = true;
    for (const FateS &f : h2) {
      if (f.src != SrcSegment) continue;
      for (const TopoDS_Shape &side : f.generated) {
        draft.Add(TopoDS::Face(side), pull, outward ? -angle : angle, neutral, false);
        ok = ok && draft.AddDone();
        if (!ok) break;
      }
      if (!ok) break;
    }
    if (ok) {
      draft.Build();
      if (draft.IsDone() && check_shape(draft.Shape()).valid) {
        compose(h2, draft);
        h.insert(h.end(), h2.begin(), h2.end());
        return draft.Shape();
      }
    }
  }
  const double height = std::abs(gp_Vec(reg.normal).Dot(dv));
  return tapered_loft(tol, reg, ri, dv, height * std::tan(angle) * (outward ? 1 : -1), h);
}

OpOut prism(const Tol &tol, rust::Slice<const RegionIn> regions, rust::Slice<const double> v, double taper,
            bool outward) {
  if (v.size() < 3) fail("input", "the extrusion vector needs three numbers");
  gp_Vec dv = vec(v, 0);
  if (dv.Magnitude() <= tol.confusion) fail("input", "the extrusion vector is zero");
  Hist h;
  std::vector<TopoDS_Shape> parts;
  for (size_t r = 0; r < regions.size(); ++r) {
    BuiltRegion reg = build_region(tol, regions[r]);
    if (std::abs(gp_Vec(reg.normal).Dot(dv)) <= tol.angular * dv.Magnitude())
      fail("input", "the extrusion direction lies in the profile plane");
    orient(reg, dv);
    const uint32_t ri = static_cast<uint32_t>(r);
    if (taper == 0) {
      BRepPrimAPI_MakePrism mk(reg.face, dv, false, true);
      if (!mk.IsDone()) fail("failed", "the extrusion could not be built");
      record_segments(h, ri, reg, [&](const TopoDS_Edge &e) -> const ShapeList & { return mk.Generated(e); });
      record_cap(h, ri, 0, mk.FirstShape());
      record_cap(h, ri, 1, mk.LastShape());
      parts.push_back(mk.Shape());
    } else {
      parts.push_back(tapered(tol, reg, ri, dv, taper, outward, h));
    }
  }
  return finish(tol, make_compound(parts), std::move(h), 0);
}

OpOut revolve(const Tol &tol, rust::Slice<const RegionIn> regions, rust::Slice<const double> axis, double angle) {
  if (axis.size() < 6) fail("input", "the axis needs six numbers");
  gp_Ax1 ax(pnt(axis, 0), dir(axis, 3, "the axis direction"));
  const bool full = angle >= 2 * std::acos(-1.0) - tol.angular;
  Hist h;
  std::vector<TopoDS_Shape> parts;
  for (size_t r = 0; r < regions.size(); ++r) {
    BuiltRegion reg = build_region(tol, regions[r]);
    // Like a prism, the face must look the way it moves: d × (centroid − axis point).
    GProp_GProps g;
    BRepGProp::SurfaceProperties(reg.face, g);
    gp_Vec motion = gp_Vec(ax.Direction()).Crossed(gp_Vec(ax.Location(), g.CentreOfMass()));
    if (motion.Magnitude() <= tol.confusion) fail("input", "the profile's centre lies on the axis");
    orient(reg, motion);
    const uint32_t ri = static_cast<uint32_t>(r);
    std::unique_ptr<BRepPrimAPI_MakeRevol> mkp(full ? new BRepPrimAPI_MakeRevol(reg.face, ax, false)
                                                    : new BRepPrimAPI_MakeRevol(reg.face, ax, angle, false));
    BRepPrimAPI_MakeRevol &mk = *mkp;
    if (!mk.IsDone()) fail("failed", "the revolution could not be built");
    // MakeRevol::Generated omits faces of a full revolution that it rebuilt (the annuli
    // swept by segments perpendicular to the axis); the sweep itself still knows them.
    ShapeList fallback;
    record_segments(h, ri, reg, [&](const TopoDS_Edge &e) -> const ShapeList & {
      const ShapeList &g = mk.Generated(e);
      if (!g.IsEmpty()) return g;
      fallback.Clear();
      // Shape() is a non-const lookup on the sweep that mk owns (mk itself is non-const).
      TopoDS_Shape swept = const_cast<BRepSweep_Revol &>(mk.Revol()).Shape(e);
      if (!swept.IsNull()) fallback.Append(swept);
      return fallback;
    });
    if (!full) {
      record_cap(h, ri, 0, mk.FirstShape());
      record_cap(h, ri, 1, mk.LastShape());
    }
    parts.push_back(mk.Shape());
  }
  return finish(tol, make_compound(parts), std::move(h), 0);
}

// Faces of `s` whose edges all belong to `wire` (the cap a pipe closes a profile with).
static TopoDS_Shape faces_on_wire(const TopoDS_Shape &s, const TopoDS_Shape &wire) {
  ShapeMap we;
  TopExp::MapShapes(wire, TopAbs_EDGE, we);
  std::vector<TopoDS_Shape> found;
  for (TopExp_Explorer ex(s, TopAbs_FACE); ex.More(); ex.Next()) {
    bool all = true, any = false;
    for (TopExp_Explorer ee(ex.Current(), TopAbs_EDGE); ee.More() && all; ee.Next()) {
      all = we.Contains(ee.Current());
      any = true;
    }
    if (all && any) found.push_back(ex.Current());
  }
  return found.empty() ? TopoDS_Shape() : make_compound(found);
}

OpOut sweep(const Tol &tol, rust::Slice<const RegionIn> regions, const WireIn &path, bool frenet) {
  BuiltWire spine = build_wire(tol, path);
  Hist h;
  std::vector<TopoDS_Shape> parts;
  for (size_t r = 0; r < regions.size(); ++r) {
    BuiltRegion reg = build_region(tol, regions[r]);
    const uint32_t ri = static_cast<uint32_t>(r);
    std::vector<TopoDS_Shape> solids; // outer, then one per hole
    for (size_t w = 0; w < reg.wires.size(); ++w) {
      BRepOffsetAPI_MakePipeShell mk(spine.wire);
      mk.SetMode(frenet); // false: corrected Frenet (OCCT's default, as the oracle)
      mk.SetTransitionMode(BRepBuilderAPI_RightCorner);
      mk.Add(reg.wires[w].wire, false, false);
      mk.Build();
      if (!mk.IsDone()) fail("failed", "the sweep could not be built");
      if (!mk.MakeSolid()) fail("failed", "the sweep could not be closed into a solid");
      TopoDS_Shape s = mk.Shape();
      for (size_t k = 0; k < reg.wires[w].edges.size(); ++k)
        record_generated(h, SrcSegment, ri, static_cast<uint32_t>(w), static_cast<uint32_t>(k),
                         mk.Generated(reg.wires[w].edges[k]));
      if (w == 0) {
        record_cap(h, ri, 0, faces_on_wire(s, mk.FirstShape()));
        record_cap(h, ri, 1, faces_on_wire(s, mk.LastShape()));
      }
      solids.push_back(s);
    }
    parts.push_back(cut_holes(tol, solids, h));
  }
  return finish(tol, make_compound(parts), std::move(h), 0);
}

OpOut loft(const Tol &tol, rust::Slice<const WireIn> sections, bool ruled) {
  if (sections.size() < 2) fail("input", "a loft needs at least two sections");
  BRepOffsetAPI_ThruSections mk(true, ruled, tol.loft);
  std::vector<BuiltWire> wires;
  for (const WireIn &w : sections) {
    wires.push_back(build_wire(tol, w));
    mk.AddWire(wires.back().wire);
  }
  mk.Build();
  if (!mk.IsDone()) fail("failed", "the loft could not be built");
  Hist h;
  for (size_t k = 0; k < wires.size(); ++k)
    for (size_t s = 0; s < wires[k].edges.size(); ++s)
      record_generated(h, SrcSegment, static_cast<uint32_t>(k), 0, static_cast<uint32_t>(s),
                       mk.Generated(wires[k].edges[s]));
  record_cap(h, 0, 0, mk.FirstShape());
  record_cap(h, 0, 1, mk.LastShape());
  return finish(tol, mk.Shape(), std::move(h), 0);
}

} // namespace occ
