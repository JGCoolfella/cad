#pragma once
// C++ side of the oc-kernel-occt bridge (src/ffi.rs). Included by the generated bridge
// header, so it only forward-declares the shared structs; implementation files include
// internal.h for the full definitions and helpers.
#include <array>
#include <cstdint>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

#include <Standard_Failure.hxx>
#include <NCollection_IndexedMap.hxx>
#include <TopTools_ShapeMapHasher.hxx>
#include <TopoDS_Shape.hxx>

#include "rust/cxx.h"

// Turn every C++ exception into a Rust Err. OCCT failures (Standard_Failure, a
// std::exception since OCCT 8.0) are tagged "failed|" with the exception type; the shim's
// own errors (std::runtime_error) already carry their category.
namespace rust {
namespace behavior {
template <typename Try, typename Fail>
static void trycatch(Try &&func, Fail &&fail) noexcept try {
  func();
} catch (const Standard_Failure &e) {
  std::string msg = "failed|";
  msg += e.ExceptionType();
  if (e.what() && *e.what()) {
    msg += ": ";
    msg += e.what();
  }
  fail(msg);
} catch (const std::exception &e) {
  fail(e.what());
} catch (...) {
  fail("failed|unknown C++ exception");
}
} // namespace behavior
} // namespace rust

namespace occ {
using ShapeMap = NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher>;

struct Tol;
struct SegIn;
struct WireIn;
struct RegionIn;
struct FateOut;
struct OpOut;
struct TopoOut;
struct MassOut;
struct DistOut;
struct CheckOut;
struct MeshOut;

// An immutable shape plus its face/edge/vertex enumeration (the indices the Rust side
// sees). Built once; TopExp::MapShapes order is deterministic.
struct Body {
  TopoDS_Shape s;
  ShapeMap faces, edges, vertices;
  explicit Body(const TopoDS_Shape &shape);
};

struct BodyList {
  std::vector<Body> items;
};

std::unique_ptr<BodyList> body_list();
void body_list_push(BodyList &list, const Body &body);
std::unique_ptr<Body> clone_body(const Body &body);
std::unique_ptr<Body> make_point(double x, double y, double z);
std::unique_ptr<Body> make_segment(rust::Slice<const double> ab);
std::array<uint32_t, 3> body_counts(const Body &body);
uint32_t solid_count(const Body &body);
OpOut solid(const Tol &tol, const Body &body, uint32_t i);

OpOut primitive(const Tol &tol, uint8_t kind, rust::Slice<const double> v);
OpOut prism(const Tol &tol, rust::Slice<const RegionIn> regions, rust::Slice<const double> vec, double taper,
            bool outward);
OpOut revolve(const Tol &tol, rust::Slice<const RegionIn> regions, rust::Slice<const double> axis, double angle);
OpOut sweep(const Tol &tol, rust::Slice<const RegionIn> regions, const WireIn &path, bool frenet);
OpOut loft(const Tol &tol, rust::Slice<const WireIn> sections, bool ruled);

OpOut boolean(const Tol &tol, uint8_t op, const Body &target, const BodyList &tools);
OpOut fillet(const Tol &tol, const Body &body, rust::Slice<const uint32_t> edges, rust::Slice<const double> r1,
             rust::Slice<const double> r2);
OpOut chamfer(const Tol &tol, const Body &body, rust::Slice<const uint32_t> edges, rust::Slice<const uint8_t> kinds,
              rust::Slice<const double> d1, rust::Slice<const double> d2, rust::Slice<const uint32_t> faces);
OpOut shell(const Tol &tol, const Body &body, rust::Slice<const uint32_t> faces, double offset);
OpOut draft(const Tol &tol, const Body &body, rust::Slice<const uint32_t> faces, rust::Slice<const double> plane,
            double angle);
OpOut offset(const Tol &tol, const Body &body, double distance);
OpOut transform(const Tol &tol, const Body &body, rust::Slice<const double> m);
OpOut compound(const Tol &tol, const BodyList &bodies);
OpOut section(const Tol &tol, const Body &body, rust::Slice<const double> plane);
OpOut heal(const Tol &tol, const Body &body);

TopoOut topology(const Body &body);
std::array<double, 6> bbox(const Body &body);
MassOut mass(const Body &body);
DistOut distance(const Body &a, uint8_t ka, uint32_t ia, const Body &b, uint8_t kb, uint32_t ib);
CheckOut check(const Tol &tol, const Body &body);
MeshOut tessellate(const Body &body, double linear, double angular);
void set_threads(uint32_t n);

OpOut read_step(const Tol &tol, rust::Slice<const uint8_t> data);
rust::Vec<uint8_t> write_step(const BodyList &bodies);

rust::Vec<uint8_t> export_native(const Body &body);
std::unique_ptr<Body> import_native(rust::Slice<const uint8_t> data);
} // namespace occ
