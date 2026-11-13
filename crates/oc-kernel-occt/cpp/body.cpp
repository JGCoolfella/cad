// Bodies and body lists, primitives.
#include "oc-kernel-occt/cpp/internal.h"

#include <algorithm>
#include <tuple>

#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeVertex.hxx>
#include <BRepTools.hxx>
#include <BRep_Tool.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakeSphere.hxx>
#include <Precision.hxx>
#include <TopExp.hxx>
#include <gp_Ax2.hxx>

namespace occ {

// ------------------------------------------------------------------ canonical order
//
// FaceId/EdgeId/VertexId must not depend on memory layout. OCCT's traversal order does for
// some algorithms (the offset behind shell iterates address-keyed maps), so entities are
// numbered by a geometric key instead: kind of surface/curve, then points on the entity.
// Ties (coincident keys) keep traversal order; exact ties need coincident geometry.

using Key = std::array<double, 10>;

// `forward`: store the entity in its own orientation instead of the orientation of its
// first occurrence (an edge occurs once per face, reversed in one of them).
static void order_by(const ShapeMap &raw, const std::vector<Key> &keys, ShapeMap &out, bool forward) {
  std::vector<int> idx(keys.size());
  for (size_t i = 0; i < idx.size(); ++i) idx[i] = static_cast<int>(i);
  std::stable_sort(idx.begin(), idx.end(), [&](int a, int b) { return keys[a] < keys[b]; });
  for (int i : idx) out.Add(forward ? raw(i + 1).Oriented(TopAbs_FORWARD) : raw(i + 1));
}

static void push_pnt(Key &k, size_t at, const gp_Pnt &p) {
  k[at] = p.X();
  k[at + 1] = p.Y();
  k[at + 2] = p.Z();
}

static Key face_key(const TopoDS_Face &f) {
  Key k{};
  BRepAdaptor_Surface ad(f);
  double u0, u1, v0, v1;
  BRepTools::UVBounds(f, u0, u1, v0, v1);
  k[0] = static_cast<double>(ad.GetType());
  push_pnt(k, 1, ad.Value(0.5 * (u0 + u1), 0.5 * (v0 + v1)));
  push_pnt(k, 4, ad.Value(u0, v0));
  push_pnt(k, 7, ad.Value(u1, v1));
  return k;
}

static Key edge_key(const TopoDS_Edge &e) {
  Key k{};
  if (BRep_Tool::Degenerated(e)) {
    k[0] = -1;
    push_pnt(k, 1, BRep_Tool::Pnt(TopExp::FirstVertex(e)));
    return k;
  }
  BRepAdaptor_Curve c(e);
  gp_Pnt a = c.Value(c.FirstParameter()), b = c.Value(c.LastParameter());
  if (std::make_tuple(b.X(), b.Y(), b.Z()) < std::make_tuple(a.X(), a.Y(), a.Z())) std::swap(a, b);
  k[0] = static_cast<double>(c.GetType());
  push_pnt(k, 1, c.Value(0.5 * (c.FirstParameter() + c.LastParameter())));
  push_pnt(k, 4, a);
  push_pnt(k, 7, b);
  return k;
}

Body::Body(const TopoDS_Shape &shape) : s(shape) {
  ShapeMap raw;
  std::vector<Key> keys;
  TopExp::MapShapes(s, TopAbs_FACE, raw);
  for (int i = 1; i <= raw.Extent(); ++i) keys.push_back(face_key(TopoDS::Face(raw(i))));
  order_by(raw, keys, faces, false);
  raw.Clear();
  keys.clear();
  TopExp::MapShapes(s, TopAbs_EDGE, raw);
  for (int i = 1; i <= raw.Extent(); ++i) keys.push_back(edge_key(TopoDS::Edge(raw(i))));
  order_by(raw, keys, edges, true);
  raw.Clear();
  keys.clear();
  TopExp::MapShapes(s, TopAbs_VERTEX, raw);
  for (int i = 1; i <= raw.Extent(); ++i) {
    Key k{};
    push_pnt(k, 0, BRep_Tool::Pnt(TopoDS::Vertex(raw(i))));
    keys.push_back(k);
  }
  order_by(raw, keys, vertices, true);
}

std::unique_ptr<BodyList> body_list() { return std::make_unique<BodyList>(); }

void body_list_push(BodyList &list, const Body &body) { list.items.push_back(body); }

std::unique_ptr<Body> clone_body(const Body &body) { return std::make_unique<Body>(body); }

std::unique_ptr<Body> make_point(double x, double y, double z) {
  return std::make_unique<Body>(BRepBuilderAPI_MakeVertex(gp_Pnt(x, y, z)).Vertex());
}

std::unique_ptr<Body> make_segment(rust::Slice<const double> ab) {
  if (ab.size() < 6) fail("input", "a segment needs six numbers");
  gp_Pnt a = pnt(ab, 0), b = pnt(ab, 3);
  if (a.Distance(b) <= Precision::Confusion()) return std::make_unique<Body>(BRepBuilderAPI_MakeVertex(a).Vertex());
  return std::make_unique<Body>(BRepBuilderAPI_MakeEdge(a, b).Edge());
}

// Solids of a body, ordered by the lowest canonical index of their faces (so the order
// is as deterministic as the face order).
static std::vector<TopoDS_Shape> ordered_solids(const Body &body) {
  ShapeMap map;
  TopExp::MapShapes(body.s, TopAbs_SOLID, map);
  std::vector<std::pair<int, TopoDS_Shape>> keyed;
  for (int i = 1; i <= map.Extent(); ++i) {
    int lowest = body.faces.Extent() + i;
    ShapeMap faces;
    TopExp::MapShapes(map(i), TopAbs_FACE, faces);
    for (int f = 1; f <= faces.Extent(); ++f) {
      int k = body.faces.FindIndex(faces(f));
      if (k > 0 && k < lowest) lowest = k;
    }
    keyed.emplace_back(lowest, map(i));
  }
  std::stable_sort(keyed.begin(), keyed.end(), [](const auto &a, const auto &b) { return a.first < b.first; });
  std::vector<TopoDS_Shape> out;
  for (auto &k : keyed) out.push_back(k.second);
  return out;
}

uint32_t solid_count(const Body &body) { return static_cast<uint32_t>(ordered_solids(body).size()); }

OpOut solid(const Tol &tol, const Body &body, uint32_t i) {
  std::vector<TopoDS_Shape> solids = ordered_solids(body);
  if (i >= solids.size()) fail("input", "solid " + std::to_string(i) + " does not exist");
  ShapeMap own;
  TopExp::MapShapes(solids[i], TopAbs_FACE, own);
  Hist h;
  for (int f = 1; f <= body.faces.Extent(); ++f) {
    FateS fate;
    fate.src = SrcFace;
    fate.a = static_cast<uint32_t>(f - 1);
    if (own.Contains(body.faces(f)))
      fate.same.push_back(body.faces(f));
    else
      fate.deleted = true;
    h.push_back(std::move(fate));
  }
  return finish(tol, solids[i], std::move(h), 0);
}

std::array<uint32_t, 3> body_counts(const Body &body) {
  return {static_cast<uint32_t>(body.faces.Extent()), static_cast<uint32_t>(body.edges.Extent()),
          static_cast<uint32_t>(body.vertices.Extent())};
}

OpOut primitive(const Tol &tol, uint8_t kind, rust::Slice<const double> v) {
  TopoDS_Shape s;
  switch (kind) {
  case 0:
    if (v.size() < 6) fail("input", "a block needs six numbers");
    s = BRepPrimAPI_MakeBox(pnt(v, 0), v[3], v[4], v[5]).Shape();
    break;
  case 1:
    if (v.size() < 8) fail("input", "a cylinder needs eight numbers");
    s = BRepPrimAPI_MakeCylinder(gp_Ax2(pnt(v, 0), dir(v, 3, "the cylinder axis")), v[6], v[7]).Shape();
    break;
  case 2:
    if (v.size() < 4) fail("input", "a sphere needs four numbers");
    s = BRepPrimAPI_MakeSphere(pnt(v, 0), v[3]).Shape();
    break;
  default:
    fail("input", "unknown primitive");
  }
  return finish(tol, s, {}, 0);
}

} // namespace occ
