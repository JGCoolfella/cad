#include "p1-occt/cpp/shim.h"
#include "p1-occt/src/lib.rs.h"

#include <BRepAlgoAPI_Cut.hxx>
#include <BRepBuilderAPI_MakeShape.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepGProp.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakeSphere.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRep_Tool.hxx>
#include <Poly_Triangulation.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <GProp_GProps.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopTools_IndexedDataMapOfShapeListOfShape.hxx>
#include <TopTools_ListOfShape.hxx>
#include <TopoDS.hxx>
#include <gp_Ax2.hxx>

namespace oc {

Shape::Shape(const TopoDS_Shape &shape) : s(shape) {
  TopExp::MapShapes(s, TopAbs_FACE, faces);
  TopExp::MapShapes(s, TopAbs_EDGE, edges);
}

std::unique_ptr<Shape> make_box(double dx, double dy, double dz) {
  return std::make_unique<Shape>(BRepPrimAPI_MakeBox(dx, dy, dz).Shape());
}

std::unique_ptr<Shape> make_cylinder(double x, double y, double z, double r, double h) {
  gp_Ax2 ax(gp_Pnt(x, y, z), gp::DZ());
  return std::make_unique<Shape>(BRepPrimAPI_MakeCylinder(ax, r, h).Shape());
}

std::unique_ptr<Shape> make_sphere(double r) {
  return std::make_unique<Shape>(BRepPrimAPI_MakeSphere(r).Shape());
}

MeshOut tessellate(const Shape &s, double deflection) {
  BRepMesh_IncrementalMesh mesher(s.s, deflection, Standard_False, 0.5, Standard_True);
  MeshOut out{};
  for (int i = 1; i <= s.faces.Extent(); ++i) {
    const TopoDS_Face &f = TopoDS::Face(s.faces(i));
    TopLoc_Location loc;
    Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(f, loc);
    if (tri.IsNull()) continue;
    if (!tri->HasNormals()) tri->ComputeNormals();
    const gp_Trsf tr = loc.Transformation();
    const uint32_t base = static_cast<uint32_t>(out.positions.size() / 3);
    for (int n = 1; n <= tri->NbNodes(); ++n) {
      gp_Pnt p = tri->Node(n).Transformed(tr);
      gp_Dir d = tri->Normal(n).Transformed(tr);
      if (f.Orientation() == TopAbs_REVERSED) d.Reverse();
      out.positions.push_back(static_cast<float>(p.X()));
      out.positions.push_back(static_cast<float>(p.Y()));
      out.positions.push_back(static_cast<float>(p.Z()));
      out.normals.push_back(static_cast<float>(d.X()));
      out.normals.push_back(static_cast<float>(d.Y()));
      out.normals.push_back(static_cast<float>(d.Z()));
    }
    const bool rev = f.Orientation() == TopAbs_REVERSED;
    for (int t = 1; t <= tri->NbTriangles(); ++t) {
      int a, b, c;
      tri->Triangle(t).Get(a, b, c);
      if (rev) std::swap(b, c);
      out.indices.push_back(base + a - 1);
      out.indices.push_back(base + b - 1);
      out.indices.push_back(base + c - 1);
    }
    out.face_ends.push_back(static_cast<uint32_t>(out.indices.size()));
  }
  return out;
}

// Fate of every face of `input` (tagged `which`) through `op`, as indices into `out`.
static void record(BRepBuilderAPI_MakeShape &op, uint8_t which, const Shape &input,
                   const Shape &out, rust::Vec<Fate> &hist, int kind) {
  const TopTools_IndexedMapOfShape &items = kind == 0 ? input.faces : input.edges;
  for (int i = 1; i <= items.Extent(); ++i) {
    const TopoDS_Shape &f = items(i);
    Fate fate{};
    fate.input = which;
    fate.kind = static_cast<uint8_t>(kind);
    fate.index = static_cast<uint32_t>(i - 1);
    fate.deleted = op.IsDeleted(f);
    auto add = [&](const TopTools_ListOfShape &list, rust::Vec<uint32_t> &dst) {
      for (TopTools_ListIteratorOfListOfShape it(list); it.More(); it.Next()) {
        if (it.Value().ShapeType() != TopAbs_FACE) continue;
        int k = out.faces.FindIndex(it.Value());
        if (k > 0) dst.push_back(static_cast<uint32_t>(k - 1));
      }
    };
    add(op.Modified(f), fate.modified);
    add(op.Generated(f), fate.generated);
    if (kind == 0 && !fate.deleted && fate.modified.empty()) {
      int k = out.faces.FindIndex(f);
      fate.kept = k > 0 ? k - 1 : -1;
    } else {
      fate.kept = -1;
    }
    hist.push_back(std::move(fate));
  }
}

std::unique_ptr<Op> cut(const Shape &a, const Shape &b) {
  BRepAlgoAPI_Cut op(a.s, b.s);
  if (!op.IsDone() || op.HasErrors()) throw std::runtime_error("cut failed");
  auto r = std::make_unique<Op>();
  r->result = std::make_unique<Shape>(op.Shape());
  record(op, 0, a, *r->result, r->history, 0);
  record(op, 1, b, *r->result, r->history, 0);
  return r;
}

std::unique_ptr<Op> fillet(const Shape &a, rust::Slice<const uint32_t> edges, double radius) {
  BRepFilletAPI_MakeFillet op(a.s);
  for (uint32_t e : edges) op.Add(radius, TopoDS::Edge(a.edges(static_cast<int>(e) + 1)));
  op.Build();
  if (!op.IsDone()) throw std::runtime_error("fillet failed");
  auto r = std::make_unique<Op>();
  r->result = std::make_unique<Shape>(op.Shape());
  record(op, 0, a, *r->result, r->history, 0);
  record(op, 0, a, *r->result, r->history, 1);
  return r;
}

std::unique_ptr<Shape> op_result(const Op &op) { return std::make_unique<Shape>(op.result->s); }

rust::Vec<Fate> op_history(const Op &op) {
  rust::Vec<Fate> out;
  for (const Fate &f : op.history) {
    Fate c{};
    c.input = f.input;
    c.kind = f.kind;
    c.index = f.index;
    c.deleted = f.deleted;
    c.kept = f.kept;
    for (auto v : f.modified) c.modified.push_back(v);
    for (auto v : f.generated) c.generated.push_back(v);
    out.push_back(std::move(c));
  }
  return out;
}

Metrics metrics(const Shape &s) {
  GProp_GProps vp, sp;
  BRepGProp::VolumeProperties(s.s, vp);
  BRepGProp::SurfaceProperties(s.s, sp);
  BRepCheck_Analyzer check(s.s);
  return Metrics{vp.Mass(), sp.Mass(), static_cast<uint32_t>(s.faces.Extent()),
                 static_cast<uint32_t>(s.edges.Extent()), check.IsValid() == Standard_True};
}

rust::Vec<FaceInfo> faces(const Shape &s) {
  rust::Vec<FaceInfo> out;
  for (int i = 1; i <= s.faces.Extent(); ++i) {
    const TopoDS_Face &f = TopoDS::Face(s.faces(i));
    GProp_GProps p;
    BRepGProp::SurfaceProperties(f, p);
    gp_Pnt c = p.CentreOfMass();
    BRepAdaptor_Surface ad(f);
    out.push_back(FaceInfo{static_cast<int32_t>(ad.GetType()), p.Mass(), c.X(), c.Y(), c.Z()});
  }
  return out;
}

rust::Vec<uint32_t> edges_between(const Shape &s, uint32_t fa, uint32_t fb) {
  TopTools_IndexedDataMapOfShapeListOfShape ef;
  TopExp::MapShapesAndAncestors(s.s, TopAbs_EDGE, TopAbs_FACE, ef);
  const TopoDS_Shape &A = s.faces(static_cast<int>(fa) + 1);
  const TopoDS_Shape &B = s.faces(static_cast<int>(fb) + 1);
  rust::Vec<uint32_t> out;
  for (int i = 1; i <= ef.Extent(); ++i) {
    bool hasA = false, hasB = false;
    for (TopTools_ListIteratorOfListOfShape it(ef(i)); it.More(); it.Next()) {
      hasA |= it.Value().IsSame(A);
      hasB |= it.Value().IsSame(B);
    }
    if (hasA && hasB) {
      int k = s.edges.FindIndex(ef.FindKey(i));
      if (k > 0) out.push_back(static_cast<uint32_t>(k - 1));
    }
  }
  return out;
}

} // namespace oc
