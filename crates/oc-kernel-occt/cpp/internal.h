#pragma once
// Helpers shared by the shim's implementation files. Not seen by the bridge.
#include "oc-kernel-occt/src/ffi.rs.h"

#include <BRepBuilderAPI_MakeShape.hxx>
#include <BRepTools_History.hxx>
#include <NCollection_IndexedDataMap.hxx>
#include <NCollection_List.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Edge.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Wire.hxx>
#include <gp_Dir.hxx>
#include <gp_Pnt.hxx>
#include <gp_Vec.hxx>

namespace occ {

using ShapeList = NCollection_List<TopoDS_Shape>;
using ShapeListMap = NCollection_IndexedDataMap<TopoDS_Shape, ShapeList, TopTools_ShapeMapHasher>;

// Throw a categorised error; the Rust side maps the category onto KernelError.
// Categories: "input", "failed", "invalid", "empty", "io".
[[noreturn]] void fail(const char *category, const std::string &msg);

inline gp_Pnt pnt(rust::Slice<const double> v, size_t i) { return gp_Pnt(v[i], v[i + 1], v[i + 2]); }
inline gp_Vec vec(rust::Slice<const double> v, size_t i) { return gp_Vec(v[i], v[i + 1], v[i + 2]); }
// Unit direction from three numbers; "input" error for a zero vector.
gp_Dir dir(rust::Slice<const double> v, size_t i, const char *what);
inline gp_Pnt pnt(const rust::Vec<double> &v, size_t i) { return gp_Pnt(v[i], v[i + 1], v[i + 2]); }

// Sub-shapes of a body by index, with range checks ("input" errors).
const TopoDS_Face &face_at(const Body &b, uint32_t i);
const TopoDS_Edge &edge_at(const Body &b, uint32_t i);

// ---- history, kept as shapes until the final body is known (finish.cpp)

enum Src : uint8_t { SrcFace = 0, SrcEdge = 1, SrcVertex = 2, SrcSegment = 3, SrcCap = 4 };

struct FateS {
  uint8_t src = 0;
  uint32_t arg = 0, a = 0, b = 0, c = 0;
  bool deleted = false;
  std::vector<TopoDS_Shape> same, modified, generated;
};
using Hist = std::vector<FateS>;

// Fates of every face of `in` through `op`; edges and vertices that generate faces.
void record_body(BRepBuilderAPI_MakeShape &op, const Body &in, uint32_t arg, Hist &h);
// Every face of `in` passes through unchanged (compound, heal input).
void record_same(const Body &in, uint32_t arg, Hist &h);
// Push a fate generated from `src` with the faces `op.Generated(s)` (profile segments).
void record_generated(Hist &h, uint8_t src, uint32_t a, uint32_t b, uint32_t c, const ShapeList &gen);
// Push a cap fate (`end` 0 start, 1 end) when `cap` is a face.
void record_cap(Hist &h, uint32_t region, uint32_t end, const TopoDS_Shape &cap);
// Carry images through a later operation (images it deletes are dropped, images it
// modifies are replaced).
void compose(Hist &h, BRepBuilderAPI_MakeShape &op);
void compose(Hist &h, const Handle(BRepTools_History) &hist);

// Validate `result` (faces: BRepCheck + positive solid volumes), heal it if that provably
// keeps the part, index the history against it and package it. `image_kind` 0 = faces,
// 1 = edges (sections: no solid checks).
OpOut finish(const Tol &tol, const TopoDS_Shape &result, Hist h, uint8_t image_kind);
// The same check `check()` reports, on a raw shape.
CheckOut check_shape(const TopoDS_Shape &s);

TopoDS_Shape make_compound(const std::vector<TopoDS_Shape> &shapes);

// ---- profiles (profile.cpp)

struct BuiltWire {
  TopoDS_Wire wire;
  std::vector<TopoDS_Edge> edges; // one per segment, as they are in the wire
};
BuiltWire build_wire(const Tol &tol, const WireIn &w);

struct BuiltRegion {
  TopoDS_Face face;
  std::vector<BuiltWire> wires; // 0 = outer, then holes
  gp_Dir normal;                // face normal including orientation
};
BuiltRegion build_region(const Tol &tol, const RegionIn &r);
// Record, for every segment of `reg` (region `r`), the faces `gen(edge)` returns.
template <typename Gen> void record_segments(Hist &h, uint32_t r, const BuiltRegion &reg, Gen &&gen) {
  for (size_t w = 0; w < reg.wires.size(); ++w)
    for (size_t s = 0; s < reg.wires[w].edges.size(); ++s)
      record_generated(h, SrcSegment, r, static_cast<uint32_t>(w), static_cast<uint32_t>(s),
                       gen(reg.wires[w].edges[s]));
}

} // namespace occ
