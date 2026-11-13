// Data exchange (STEP) and tessellation.
#include "oc-kernel-occt/cpp/internal.h"

#include <sstream>

#include <BRepMesh_IncrementalMesh.hxx>
#include <BRep_Tool.hxx>
#include <GCPnts_TangentialDeflection.hxx>
#include <BRepAdaptor_Curve.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <IMeshTools_Parameters.hxx>
#include <Interface_Static.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_PrinterOStream.hxx>
#include <Poly_Polygon3D.hxx>
#include <Poly_PolygonOnTriangulation.hxx>
#include <Poly_Triangulation.hxx>
#include <STEPControl_Reader.hxx>
#include <STEPControl_Writer.hxx>
#include <ShapeBuild_ReShape.hxx>
#include <ShapeFix_Shape.hxx>
#include <TopExp.hxx>

namespace occ {

// OCCT's data exchange reports progress on stdout through the default messenger. A
// library must not write to the host's console: drop those printers (once per process).
static void quiet() {
  static const bool done = [] {
    Message::DefaultMessenger()->RemovePrinters(STANDARD_TYPE(Message_PrinterOStream));
    return true;
  }();
  (void)done;
}

OpOut read_step(const Tol &tol, rust::Slice<const uint8_t> data) {
  quiet();
  std::string text(reinterpret_cast<const char *>(data.data()), data.size());
  std::istringstream in(text);
  STEPControl_Reader reader;
  if (reader.ReadStream("model.step", in) != IFSelect_RetDone) fail("io", "the file is not readable STEP");
  if (reader.TransferRoots() == 0 || reader.NbShapes() == 0) fail("io", "the STEP file contains no shapes");
  TopoDS_Shape shape = reader.OneShape();
  if (shape.IsNull()) fail("io", "the STEP file contains no shapes");
  // Imported geometry is always healed (tolerances, orientation, small gaps).
  ShapeFix_Shape fix(shape);
  fix.SetPrecision(tol.heal);
  fix.Perform();
  return finish(tol, fix.Shape(), {}, 0);
}

rust::Vec<uint8_t> write_step(const BodyList &bodies) {
  if (bodies.items.empty()) fail("input", "nothing to export");
  quiet();
  // Interface_Static is process-global; the kernel host runs one kernel per process.
  Interface_Static::SetCVal("write.step.unit", "MM");
  Interface_Static::SetCVal("write.step.schema", "AP214IS");
  STEPControl_Writer writer;
  for (const Body &b : bodies.items)
    if (writer.Transfer(b.s, STEPControl_AsIs) != IFSelect_RetDone) fail("io", "a body could not be converted to STEP");
  std::ostringstream os;
  if (writer.WriteStream(os) != IFSelect_RetDone) fail("io", "the STEP file could not be written");
  std::string s = os.str();
  rust::Vec<uint8_t> out;
  out.reserve(s.size());
  for (char c : s) out.push_back(static_cast<uint8_t>(c));
  return out;
}

MeshOut tessellate(const Body &body, double linear, double angular) {
  if (!(linear > 0) || !(angular > 0)) fail("input", "tessellation tolerances must be positive");
  IMeshTools_Parameters p;
  p.Deflection = linear;
  p.Angle = angular;
  p.Relative = false;
  p.InParallel = true;
  // Re-mesh at exactly the requested quality even if a finer mesh is stored on the
  // (shared) shape, so the output depends only on the parameters.
  p.AllowQualityDecrease = true;
  BRepMesh_IncrementalMesh mesher(body.s, p);
  if (!mesher.IsDone()) fail("failed", "the body could not be tessellated");
  MeshOut out{};
  for (int i = 1; i <= body.faces.Extent(); ++i) {
    const TopoDS_Face &f = TopoDS::Face(body.faces(i));
    TopLoc_Location loc;
    Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(f, loc);
    if (!tri.IsNull()) {
      if (!tri->HasNormals()) tri->ComputeNormals();
      const gp_Trsf tr = loc.Transformation();
      const bool rev = f.Orientation() == TopAbs_REVERSED;
      const uint32_t base = static_cast<uint32_t>(out.positions.size() / 3);
      for (int n = 1; n <= tri->NbNodes(); ++n) {
        gp_Pnt q = tri->Node(n).Transformed(tr);
        gp_Dir d = tri->Normal(n).Transformed(tr);
        if (rev) d.Reverse();
        out.positions.push_back(q.X());
        out.positions.push_back(q.Y());
        out.positions.push_back(q.Z());
        out.normals.push_back(static_cast<float>(d.X()));
        out.normals.push_back(static_cast<float>(d.Y()));
        out.normals.push_back(static_cast<float>(d.Z()));
      }
      for (int t = 1; t <= tri->NbTriangles(); ++t) {
        int a, b, c;
        tri->Triangle(t).Get(a, b, c);
        if (rev) std::swap(b, c);
        out.triangles.push_back(base + a - 1);
        out.triangles.push_back(base + b - 1);
        out.triangles.push_back(base + c - 1);
      }
    }
    out.face_ends.push_back(static_cast<uint32_t>(out.triangles.size() / 3));
  }
  ShapeListMap ef;
  TopExp::MapShapesAndAncestors(body.s, TopAbs_EDGE, TopAbs_FACE, ef);
  auto push = [&](const gp_Pnt &q) {
    out.edge_points.push_back(q.X());
    out.edge_points.push_back(q.Y());
    out.edge_points.push_back(q.Z());
  };
  for (int i = 1; i <= body.edges.Extent(); ++i) {
    const TopoDS_Edge &e = TopoDS::Edge(body.edges(i));
    bool done = BRep_Tool::Degenerated(e);
    // The edge's nodes in the triangulation of a face it bounds (shared with the faces).
    if (!done)
      if (const ShapeList *adj = ef.Seek(e))
        for (ShapeList::Iterator it(*adj); it.More() && !done; it.Next()) {
          TopLoc_Location loc;
          Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(TopoDS::Face(it.Value()), loc);
          if (tri.IsNull()) continue;
          Handle(Poly_PolygonOnTriangulation) pol = BRep_Tool::PolygonOnTriangulation(e, tri, loc);
          if (pol.IsNull()) continue;
          const gp_Trsf tr = loc.Transformation();
          for (int k = 1; k <= pol->NbNodes(); ++k) push(tri->Node(pol->Node(k)).Transformed(tr));
          done = true;
        }
    if (!done && !BRep_Tool::Degenerated(e)) {
      BRepAdaptor_Curve c(e);
      GCPnts_TangentialDeflection td(c, angular, linear);
      for (int k = 1; k <= td.NbPoints(); ++k) push(td.Value(k));
    }
    out.edge_ends.push_back(static_cast<uint32_t>(out.edge_points.size() / 3));
  }
  return out;
}

} // namespace occ
