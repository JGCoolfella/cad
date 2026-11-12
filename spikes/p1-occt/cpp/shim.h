#pragma once
// P1 spike: the smallest C++ surface over OCCT that the Rust side needs to prove
// make-box / cut / fillet with operation history. Shapes are opaque to Rust.
#include <memory>
#include <stdexcept>
#include <Standard_Failure.hxx>
#include <TopoDS_Shape.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include "rust/cxx.h"

// OCCT exceptions do not always derive from std::exception, so tell cxx how to turn
// any of them into a Rust Err instead of aborting the process.
namespace rust {
namespace behavior {
template <typename Try, typename Fail>
static void trycatch(Try &&func, Fail &&fail) noexcept try {
  func();
} catch (const Standard_Failure &e) {
  fail(e.GetMessageString() ? e.GetMessageString() : "OCCT failure");
} catch (const std::exception &e) {
  fail(e.what());
} catch (...) {
  fail("unknown C++ exception");
}
} // namespace behavior
} // namespace rust

namespace oc {
struct FaceInfo;
struct Fate;
struct Metrics;
struct MeshOut;

struct Shape {
  TopoDS_Shape s;
  TopTools_IndexedMapOfShape faces, edges; // 1-based, filled on construction
  explicit Shape(const TopoDS_Shape &shape);
};

// A finished modelling operation: its result plus the history of its inputs.
struct Op {
  std::unique_ptr<Shape> result;
  rust::Vec<Fate> history;
};

std::unique_ptr<Shape> make_box(double dx, double dy, double dz);
std::unique_ptr<Shape> make_cylinder(double x, double y, double z, double r, double h);
std::unique_ptr<Shape> make_sphere(double r);
// Triangulates every face (linear deflection, 0.5 rad angular) into one indexed mesh.
MeshOut tessellate(const Shape &s, double deflection);
std::unique_ptr<Op> cut(const Shape &a, const Shape &b);
std::unique_ptr<Op> fillet(const Shape &a, rust::Slice<const uint32_t> edges, double r);

std::unique_ptr<Shape> op_result(const Op &op);
rust::Vec<Fate> op_history(const Op &op);

Metrics metrics(const Shape &s);
rust::Vec<FaceInfo> faces(const Shape &s);
// Indices (0-based) of the edges shared by faces fa and fb.
rust::Vec<uint32_t> edges_between(const Shape &s, uint32_t fa, uint32_t fb);
} // namespace oc
