// Operations on existing bodies: booleans, fillet, chamfer, shell, draft, offset,
// transform, compound, section, heal. Inputs are never modified (non-destructive
// booleans, no in-place edits).
#include "oc-kernel-occt/cpp/internal.h"

#include <sstream>

#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepAlgoAPI_Section.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepFilletAPI_MakeChamfer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepOffsetAPI_DraftAngle.hxx>
#include <BRepOffsetAPI_MakeOffsetShape.hxx>
#include <BRepOffsetAPI_MakeThickSolid.hxx>
#include <Message_Report.hxx>
#include <ShapeBuild_ReShape.hxx>
#include <ShapeFix_Shape.hxx>
#include <BRep_Tool.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <gp_Pln.hxx>
#include <gp_Trsf.hxx>

namespace occ {

static std::string bop_errors(const BRepAlgoAPI_BooleanOperation &op) {
  std::ostringstream os;
  op.DumpErrors(os);
  std::string s = os.str();
  for (char &c : s)
    if (c == '\n' || c == '\r') c = ' ';
  return s.empty() ? std::string("the boolean operation failed") : s;
}

OpOut boolean(const Tol &tol, uint8_t kind, const Body &target, const BodyList &tools) {
  if (tools.items.empty()) fail("input", "a boolean needs at least one tool body");
  std::unique_ptr<BRepAlgoAPI_BooleanOperation> op;
  switch (kind) {
  case 0: op = std::make_unique<BRepAlgoAPI_Fuse>(); break;
  case 1: op = std::make_unique<BRepAlgoAPI_Cut>(); break;
  case 2: op = std::make_unique<BRepAlgoAPI_Common>(); break;
  default: fail("input", "unknown boolean operation");
  }
  ShapeList args, tl;
  args.Append(target.s);
  for (const Body &t : tools.items) tl.Append(t.s);
  op->SetArguments(args);
  op->SetTools(tl);
  op->SetNonDestructive(true);
  op->SetRunParallel(tol.parallel);
  if (tol.fuzzy > 0) op->SetFuzzyValue(tol.fuzzy);
  op->Build();
  if (!op->IsDone() || op->HasErrors()) fail("failed", bop_errors(*op));
  op->SimplifyResult(true, true, tol.unify_angular);
  Hist h;
  record_body(*op, target, 0, h);
  for (size_t k = 0; k < tools.items.size(); ++k) record_body(*op, tools.items[k], static_cast<uint32_t>(k + 1), h);
  return finish(tol, op->Shape(), std::move(h), 0);
}

OpOut fillet(const Tol &tol, const Body &body, rust::Slice<const uint32_t> edges, rust::Slice<const double> r1,
             rust::Slice<const double> r2) {
  if (edges.empty()) fail("input", "no edges to fillet");
  if (r1.size() != edges.size() || r2.size() != edges.size()) fail("input", "one radius pair per edge");
  BRepFilletAPI_MakeFillet mk(body.s, ChFi3d_Rational);
  for (size_t i = 0; i < edges.size(); ++i) {
    const TopoDS_Edge &e = edge_at(body, edges[i]);
    if (r1[i] == r2[i]) mk.Add(r1[i], e);
    else mk.Add(r1[i], r2[i], e);
  }
  mk.Build();
  if (!mk.IsDone()) {
    std::string msg = "the fillet could not be built";
    if (mk.NbFaultyContours() > 0) msg += " (" + std::to_string(mk.NbFaultyContours()) + " failing edge chains)";
    fail("failed", msg);
  }
  Hist h;
  record_body(mk, body, 0, h);
  return finish(tol, mk.Shape(), std::move(h), 0);
}

OpOut chamfer(const Tol &tol, const Body &body, rust::Slice<const uint32_t> edges, rust::Slice<const uint8_t> kinds,
              rust::Slice<const double> d1, rust::Slice<const double> d2, rust::Slice<const uint32_t> faces) {
  const size_t n = edges.size();
  if (n == 0) fail("input", "no edges to chamfer");
  if (kinds.size() != n || d1.size() != n || d2.size() != n || faces.size() != n)
    fail("input", "one chamfer size per edge");
  ShapeListMap ef;
  TopExp::MapShapesAndAncestors(body.s, TopAbs_EDGE, TopAbs_FACE, ef);
  BRepFilletAPI_MakeChamfer mk(body.s);
  for (size_t i = 0; i < n; ++i) {
    const TopoDS_Edge &e = edge_at(body, edges[i]);
    if (kinds[i] == 0) {
      mk.Add(d1[i], e);
      continue;
    }
    const TopoDS_Face &f = face_at(body, faces[i]);
    bool adjacent = false;
    if (const ShapeList *adj = ef.Seek(e))
      for (ShapeList::Iterator it(*adj); it.More(); it.Next()) adjacent |= it.Value().IsSame(f);
    if (!adjacent) fail("input", "a chamfer's reference face does not touch its edge");
    if (kinds[i] == 1) mk.Add(d1[i], d2[i], e, f);
    else mk.AddDA(d1[i], d2[i], e, f);
  }
  mk.Build();
  if (!mk.IsDone()) fail("failed", "the chamfer could not be built");
  Hist h;
  record_body(mk, body, 0, h);
  return finish(tol, mk.Shape(), std::move(h), 0);
}

static ShapeList face_list(const Body &body, rust::Slice<const uint32_t> faces) {
  ShapeList out;
  for (uint32_t f : faces) out.Append(face_at(body, f));
  return out;
}

OpOut shell(const Tol &tol, const Body &body, rust::Slice<const uint32_t> faces, double off) {
  BRepOffsetAPI_MakeThickSolid mk;
  mk.MakeThickSolidByJoin(body.s, face_list(body, faces), off, tol.offset, BRepOffset_Skin, false, false, GeomAbs_Arc,
                          false);
  if (!mk.IsDone()) fail("failed", "the shell could not be built (offset error " + std::to_string(mk.MakeOffset().Error()) + ")");
  Hist h;
  record_body(mk, body, 0, h);
  return finish(tol, mk.Shape(), std::move(h), 0);
}

OpOut offset(const Tol &tol, const Body &body, double distance) {
  BRepOffsetAPI_MakeOffsetShape mk;
  mk.PerformByJoin(body.s, distance, tol.offset, BRepOffset_Skin, false, false, GeomAbs_Arc, false);
  if (!mk.IsDone()) fail("failed", "the offset could not be built (offset error " + std::to_string(mk.MakeOffset().Error()) + ")");
  Hist h;
  record_body(mk, body, 0, h);
  return finish(tol, mk.Shape(), std::move(h), 0);
}

OpOut draft(const Tol &tol, const Body &body, rust::Slice<const uint32_t> faces, rust::Slice<const double> plane,
            double angle) {
  if (faces.empty()) fail("input", "no faces to draft");
  if (plane.size() < 6) fail("input", "the neutral plane needs six numbers");
  gp_Dir n = dir(plane, 3, "the neutral plane normal");
  gp_Pln pln(pnt(plane, 0), n);
  // OCCT 8.0.1's DraftAngle crashes the process (access violation) on a face that closes on
  // itself through a seam edge, such as a full cylinder (failure inventory H14). Refuse
  // those cleanly; faces bounded by other faces (fillets between planes, H15) work.
  for (uint32_t f : faces) {
    const TopoDS_Face &face = face_at(body, f);
    for (TopExp_Explorer ex(face, TopAbs_EDGE); ex.More(); ex.Next())
      if (BRep_Tool::IsClosed(TopoDS::Edge(ex.Current()), face))
        fail("failed", "face " + std::to_string(f) + " closes on itself (a full cylinder or cone): OCCT cannot draft it");
  }
  BRepOffsetAPI_DraftAngle mk(body.s);
  for (uint32_t f : faces) {
    mk.Add(face_at(body, f), n, angle, pln, false);
    if (!mk.AddDone()) fail("failed", "face " + std::to_string(f) + " cannot be drafted");
  }
  mk.Build();
  if (!mk.IsDone()) fail("failed", "the draft could not be built");
  Hist h;
  record_body(mk, body, 0, h);
  return finish(tol, mk.Shape(), std::move(h), 0);
}

OpOut transform(const Tol &tol, const Body &body, rust::Slice<const double> m) {
  if (m.size() < 12) fail("input", "a transform needs twelve numbers");
  gp_Trsf t;
  t.SetValues(m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8], m[9], m[10], m[11]);
  // A reflection cannot be a location (OCCT forbids mirrored locations): copy geometry.
  BRepBuilderAPI_Transform mk(body.s, t, t.IsNegative());
  if (!mk.IsDone()) fail("failed", "the transform could not be applied");
  Hist h;
  record_body(mk, body, 0, h);
  return finish(tol, mk.Shape(), std::move(h), 0);
}

OpOut compound(const Tol &tol, const BodyList &bodies) {
  if (bodies.items.empty()) fail("input", "nothing to group");
  std::vector<TopoDS_Shape> shapes;
  Hist h;
  for (size_t k = 0; k < bodies.items.size(); ++k) {
    shapes.push_back(bodies.items[k].s);
    record_same(bodies.items[k], static_cast<uint32_t>(k), h);
  }
  return finish(tol, make_compound(shapes), std::move(h), 0);
}

OpOut section(const Tol &tol, const Body &body, rust::Slice<const double> plane) {
  if (plane.size() < 6) fail("input", "the section plane needs six numbers");
  gp_Pln pln(pnt(plane, 0), dir(plane, 3, "the section plane normal"));
  BRepAlgoAPI_Section mk(body.s, pln, false);
  mk.ComputePCurveOn1(true);
  mk.Approximation(true);
  mk.SetRunParallel(tol.parallel);
  mk.SetNonDestructive(true);
  mk.Build();
  if (!mk.IsDone() || mk.HasErrors()) fail("failed", bop_errors(mk));
  Hist h;
  for (int i = 1; i <= body.faces.Extent(); ++i) {
    FateS f;
    f.src = SrcFace;
    f.a = static_cast<uint32_t>(i - 1);
    for (ShapeList::Iterator it(mk.Generated(body.faces(i))); it.More(); it.Next())
      if (it.Value().ShapeType() == TopAbs_EDGE) f.generated.push_back(it.Value());
    if (!f.generated.empty()) h.push_back(std::move(f));
  }
  return finish(tol, mk.Shape(), std::move(h), 1);
}

OpOut heal(const Tol &tol, const Body &body) {
  ShapeFix_Shape fix(body.s);
  fix.SetPrecision(tol.heal);
  fix.Perform();
  Hist h;
  record_same(body, 0, h);
  compose(h, fix.Context()->History());
  OpOut out = finish(tol, fix.Shape(), std::move(h), 0);
  out.healed = out.healed || !fix.Shape().IsSame(body.s);
  return out;
}

} // namespace occ
