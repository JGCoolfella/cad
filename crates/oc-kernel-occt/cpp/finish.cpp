// History recording and composition, result validation and guarded healing.
#include "oc-kernel-occt/cpp/internal.h"

#include <algorithm>
#include <cmath>
#include <set>
#include <sstream>

#include <BRepCheck.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <NCollection_List.hxx>
#include <BRepCheck_Result.hxx>
#include <BRepGProp.hxx>
#include <BRep_Builder.hxx>
#include <GProp_GProps.hxx>
#include <ShapeBuild_ReShape.hxx>
#include <ShapeFix_Shape.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS_Compound.hxx>

namespace occ {

void fail(const char *category, const std::string &msg) { throw std::runtime_error(std::string(category) + "|" + msg); }

gp_Dir dir(rust::Slice<const double> v, size_t i, const char *what) {
  gp_Vec d = vec(v, i);
  if (d.Magnitude() < 1e-12) fail("input", std::string(what) + " is a zero vector");
  return gp_Dir(d);
}

const TopoDS_Face &face_at(const Body &b, uint32_t i) {
  if (i >= static_cast<uint32_t>(b.faces.Extent())) fail("input", "face " + std::to_string(i) + " does not exist");
  return TopoDS::Face(b.faces(static_cast<int>(i) + 1));
}

const TopoDS_Edge &edge_at(const Body &b, uint32_t i) {
  if (i >= static_cast<uint32_t>(b.edges.Extent())) fail("input", "edge " + std::to_string(i) + " does not exist");
  return TopoDS::Edge(b.edges(static_cast<int>(i) + 1));
}

TopoDS_Shape make_compound(const std::vector<TopoDS_Shape> &shapes) {
  if (shapes.size() == 1) return shapes[0];
  BRep_Builder bb;
  TopoDS_Compound c;
  bb.MakeCompound(c);
  for (const auto &s : shapes) bb.Add(c, s);
  return c;
}

// ------------------------------------------------------------------ recording

static void append_of_type(const ShapeList &list, TopAbs_ShapeEnum type, std::vector<TopoDS_Shape> &dst) {
  for (ShapeList::Iterator it(list); it.More(); it.Next())
    if (it.Value().ShapeType() == type) dst.push_back(it.Value());
}

void record_body(BRepBuilderAPI_MakeShape &op, const Body &in, uint32_t arg, Hist &h) {
  for (int i = 1; i <= in.faces.Extent(); ++i) {
    const TopoDS_Shape &f = in.faces(i);
    FateS fate;
    fate.src = SrcFace;
    fate.arg = arg;
    fate.a = static_cast<uint32_t>(i - 1);
    fate.deleted = op.IsDeleted(f);
    if (!fate.deleted) {
      append_of_type(op.Modified(f), TopAbs_FACE, fate.modified);
      append_of_type(op.Generated(f), TopAbs_FACE, fate.generated);
      if (fate.modified.empty()) fate.same.push_back(f);
    }
    h.push_back(std::move(fate));
  }
  // Edges and vertices matter only where they create faces (fillets, chamfers, corners).
  const ShapeMap *subs[2] = {&in.edges, &in.vertices};
  for (int k = 0; k < 2; ++k) {
    for (int i = 1; i <= subs[k]->Extent(); ++i) {
      const TopoDS_Shape &s = (*subs[k])(i);
      FateS fate;
      append_of_type(op.Generated(s), TopAbs_FACE, fate.generated);
      if (fate.generated.empty()) continue;
      fate.src = k == 0 ? SrcEdge : SrcVertex;
      fate.arg = arg;
      fate.a = static_cast<uint32_t>(i - 1);
      h.push_back(std::move(fate));
    }
  }
}

void record_same(const Body &in, uint32_t arg, Hist &h) {
  for (int i = 1; i <= in.faces.Extent(); ++i) {
    FateS fate;
    fate.src = SrcFace;
    fate.arg = arg;
    fate.a = static_cast<uint32_t>(i - 1);
    fate.same.push_back(in.faces(i));
    h.push_back(std::move(fate));
  }
}

void record_generated(Hist &h, uint8_t src, uint32_t a, uint32_t b, uint32_t c, const ShapeList &gen) {
  FateS fate;
  fate.src = src;
  fate.a = a;
  fate.b = b;
  fate.c = c;
  append_of_type(gen, TopAbs_FACE, fate.generated);
  if (!fate.generated.empty()) h.push_back(std::move(fate));
}

void record_cap(Hist &h, uint32_t region, uint32_t end, const TopoDS_Shape &cap) {
  if (cap.IsNull()) return;
  FateS fate;
  fate.src = SrcCap;
  fate.a = region;
  fate.b = end;
  for (TopExp_Explorer ex(cap, TopAbs_FACE); ex.More(); ex.Next()) fate.generated.push_back(ex.Current());
  if (!fate.generated.empty()) h.push_back(std::move(fate));
}

// ------------------------------------------------------------------ composition

// An image is replaced by what the later operation modified it into, or generated from
// it as the same kind of shape (OCCT's DraftAngle reports a drafted face as generated).
template <typename Deleted, typename Modified, typename Generated>
static void compose_with(Hist &h, Deleted &&deleted, Modified &&modified, Generated &&generated) {
  auto carry = [&](std::vector<TopoDS_Shape> &list, std::vector<TopoDS_Shape> *moved_to) {
    std::vector<TopoDS_Shape> keep;
    for (const auto &s : list) {
      if (deleted(s)) continue;
      std::vector<TopoDS_Shape> mods;
      append_of_type(modified(s), s.ShapeType(), mods);
      if (mods.empty()) append_of_type(generated(s), s.ShapeType(), mods);
      if (mods.empty()) {
        keep.push_back(s);
      } else {
        auto &dst = moved_to ? *moved_to : keep;
        dst.insert(dst.end(), mods.begin(), mods.end());
      }
    }
    list.swap(keep);
  };
  for (auto &f : h) {
    std::vector<TopoDS_Shape> moved;
    carry(f.same, &moved);
    carry(f.modified, nullptr);
    carry(f.generated, nullptr);
    f.modified.insert(f.modified.end(), moved.begin(), moved.end());
  }
}

void compose(Hist &h, BRepBuilderAPI_MakeShape &op) {
  compose_with(
      h, [&](const TopoDS_Shape &s) { return op.IsDeleted(s); },
      [&](const TopoDS_Shape &s) -> const ShapeList & { return op.Modified(s); },
      [&](const TopoDS_Shape &s) -> const ShapeList & { return op.Generated(s); });
}

void compose(Hist &h, const Handle(BRepTools_History) &hist) {
  if (hist.IsNull()) return;
  compose_with(
      h, [&](const TopoDS_Shape &s) { return BRepTools_History::IsSupportedType(s) && hist->IsRemoved(s); },
      [&](const TopoDS_Shape &s) -> const ShapeList & { return hist->Modified(s); },
      [&](const TopoDS_Shape &s) -> const ShapeList & { return hist->Generated(s); });
}

// ------------------------------------------------------------------ validation

static std::string status_text(BRepCheck_Status st) {
  std::ostringstream os;
  BRepCheck::Print(st, os);
  std::string s = os.str();
  while (!s.empty() && (s.back() == '\n' || s.back() == '\r' || s.back() == ' ')) s.pop_back();
  return s;
}

CheckOut check_shape(const TopoDS_Shape &s) {
  CheckOut out{};
  out.valid = true;
  if (s.IsNull()) {
    out.valid = false;
    out.problems.push_back(ProblemOut{3, 0, "NullShape"});
    return out;
  }
  Body body(s);
  BRepCheck_Analyzer an(s, true);
  if (!an.IsValid()) {
    out.valid = false;
    std::set<std::pair<std::pair<int, uint32_t>, std::string>> seen;
    auto report = [&](const TopoDS_Shape &sub, const NCollection_List<BRepCheck_Status> &list) {
      uint8_t kind = 3;
      uint32_t idx = 0;
      if (int k = body.faces.FindIndex(sub)) kind = 0, idx = k - 1;
      else if (int k = body.edges.FindIndex(sub)) kind = 1, idx = k - 1;
      else if (int k = body.vertices.FindIndex(sub)) kind = 2, idx = k - 1;
      for (NCollection_List<BRepCheck_Status>::Iterator it(list); it.More(); it.Next()) {
        if (it.Value() == BRepCheck_NoError) continue;
        std::string code = status_text(it.Value());
        if (seen.insert({{kind, idx}, code}).second) out.problems.push_back(ProblemOut{kind, idx, rust::String(code)});
      }
    };
    const TopAbs_ShapeEnum types[] = {TopAbs_SOLID, TopAbs_SHELL, TopAbs_FACE, TopAbs_WIRE, TopAbs_EDGE, TopAbs_VERTEX};
    for (TopAbs_ShapeEnum t : types) {
      ShapeMap subs;
      TopExp::MapShapes(s, t, subs);
      for (int i = 1; i <= subs.Extent(); ++i) {
        const Handle(BRepCheck_Result) &res = an.Result(subs(i));
        if (res.IsNull()) continue;
        report(subs(i), res->Status());
        for (res->InitContextIterator(); res->MoreShapeInContext(); res->NextShapeInContext())
          report(subs(i), res->StatusOnShape());
      }
    }
    if (out.problems.empty()) out.problems.push_back(ProblemOut{3, 0, "BRepCheck_Invalid"});
  }
  // An inside-out solid passes BRepCheck; its volume is negative.
  int solids = 0;
  for (TopExp_Explorer ex(s, TopAbs_SOLID); ex.More(); ex.Next(), ++solids) {
    GProp_GProps g;
    BRepGProp::VolumeProperties(ex.Current(), g);
    if (!(g.Mass() > 0)) {
      out.valid = false;
      out.problems.push_back(ProblemOut{3, 0, "NegativeVolume"});
      break;
    }
  }
  if (solids == 0 && body.faces.Extent() > 0) {
    out.valid = false;
    out.problems.push_back(ProblemOut{3, 0, "NoSolid"});
  }
  return out;
}

static std::string problems_text(const CheckOut &c) {
  std::string s;
  for (const auto &p : c.problems) {
    if (!s.empty()) s += ", ";
    s += std::string(p.code);
  }
  return s;
}

static void volume_area(const TopoDS_Shape &s, double &v, double &a) {
  GProp_GProps gv, ga;
  BRepGProp::VolumeProperties(s, gv);
  BRepGProp::SurfaceProperties(s, ga);
  v = gv.Mass();
  a = ga.Mass();
}

static bool within(double x, double y, double rel) { return std::abs(x - y) <= rel * std::max(std::abs(x), std::abs(y)); }

// ------------------------------------------------------------------ packaging

static rust::Vec<uint32_t> indices(const std::vector<TopoDS_Shape> &list, const ShapeMap &map) {
  std::set<uint32_t> idx;
  for (const auto &s : list)
    if (int k = map.FindIndex(s)) idx.insert(static_cast<uint32_t>(k - 1));
  rust::Vec<uint32_t> out;
  for (uint32_t i : idx) out.push_back(i);
  return out;
}

OpOut finish(const Tol &tol, const TopoDS_Shape &raw, Hist h, uint8_t image_kind) {
  if (raw.IsNull()) fail("empty", "the operation produced nothing");
  TopoDS_Shape result = raw;
  bool healed = false;
  if (image_kind == 0) {
    ShapeMap faces;
    TopExp::MapShapes(result, TopAbs_FACE, faces);
    if (faces.Extent() == 0) fail("empty", "the result has no faces");
    CheckOut c = check_shape(result);
    if (!c.valid) {
      // Heal only if the part provably stays the same: valid afterwards, same face
      // count, same volume and area (a zero-thickness lid adds area).
      double v0, a0, v1, a1;
      volume_area(result, v0, a0);
      ShapeFix_Shape fix(result);
      fix.SetPrecision(tol.heal);
      fix.Perform();
      TopoDS_Shape fixed = fix.Shape();
      CheckOut c2 = check_shape(fixed);
      ShapeMap faces2;
      if (!fixed.IsNull()) TopExp::MapShapes(fixed, TopAbs_FACE, faces2);
      bool keep = c2.valid && faces2.Extent() == faces.Extent();
      if (keep) {
        volume_area(fixed, v1, a1);
        keep = within(v0, v1, tol.heal_acceptance) && within(a0, a1, tol.heal_acceptance);
      }
      if (!keep) fail("invalid", problems_text(c));
      compose(h, fix.Context()->History());
      result = fixed;
      healed = true;
    }
  } else {
    ShapeMap edges;
    TopExp::MapShapes(result, TopAbs_EDGE, edges);
    if (edges.Extent() == 0) fail("empty", "the result has no edges");
  }
  OpOut out{};
  out.body = std::make_unique<Body>(result);
  const ShapeMap &map = image_kind == 0 ? out.body->faces : out.body->edges;
  for (const FateS &f : h) {
    FateOut o{};
    o.src = f.src;
    o.arg = f.arg;
    o.a = f.a;
    o.b = f.b;
    o.c = f.c;
    o.deleted = f.deleted;
    o.same = indices(f.same, map);
    o.modified = indices(f.modified, map);
    o.generated = indices(f.generated, map);
    // A face neither kept, modified, generated nor reported deleted vanished: deleted.
    if (f.src == SrcFace && o.same.empty() && o.modified.empty() && o.generated.empty()) o.deleted = true;
    if (f.src != SrcFace && o.same.empty() && o.modified.empty() && o.generated.empty()) continue;
    out.fates.push_back(std::move(o));
  }
  out.image_kind = image_kind;
  out.healed = healed;
  return out;
}

} // namespace occ
