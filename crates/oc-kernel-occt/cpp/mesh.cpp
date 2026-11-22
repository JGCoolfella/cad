// Tessellation (Kernel::tessellate) and the kernel's thread count.
#include "oc-kernel-occt/cpp/internal.h"

#include <vector>

#include <BRepAdaptor_Curve.hxx>
#include <BRepLib_ToolTriangulatedShape.hxx>
#include <BRepMesh_Context.hxx>
#include <BRepMesh_DelabellaMeshAlgoFactory.hxx>
#include <BRepMesh_FaceDiscret.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepMesh_MeshAlgoFactory.hxx>
#include <IMeshTools_MeshAlgoFactory.hxx>
#include <BRepTools.hxx>
#include <BRep_Tool.hxx>
#include <GCPnts_TangentialDeflection.hxx>
#include <IMeshTools_Parameters.hxx>
#include <OSD_Parallel.hxx>
#include <OSD_ThreadPool.hxx>
#include <Standard_Failure.hxx>
#include <Poly_Connect.hxx>
#include <Poly_PolygonOnTriangulation.hxx>
#include <Poly_Triangulation.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS.hxx>

namespace occ {

namespace {

// Edge `e`'s polygon in face `f`'s triangulation (null if none). For a seam, `e`'s
// orientation picks which of its two polygons on the face.
Handle(Poly_PolygonOnTriangulation) polygon_on(const TopoDS_Edge &e, const TopoDS_Face &f) {
  TopLoc_Location loc;
  Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(f, loc);
  if (tri.IsNull()) return nullptr;
  return BRep_Tool::PolygonOnTriangulation(e, tri, loc);
}

// The canonical points of every edge: the nodes of its polygon on the first face (in the
// body's order) that has one, with each end replaced by its vertex's point. Every face
// bounded by the edge then uses exactly these coordinates for its boundary nodes, so
// faces share their boundary bit for bit (each face's own nodes differ in the last bits
// where it evaluates the vertex through its own parametrisation).
std::vector<std::vector<gp_Pnt>> edge_points(const Body &body, double linear, double angular) {
  ShapeListMap ef;
  TopExp::MapShapesAndAncestors(body.s, TopAbs_EDGE, TopAbs_FACE, ef);
  std::vector<std::vector<gp_Pnt>> out(body.edges.Extent());
  for (int i = 1; i <= body.edges.Extent(); ++i) {
    const TopoDS_Edge &e = TopoDS::Edge(body.edges(i));
    if (BRep_Tool::Degenerated(e)) continue;
    std::vector<gp_Pnt> &pts = out[i - 1];
    // The adjacent face that comes first in the body's order.
    int first = 0;
    if (const ShapeList *adj = ef.Seek(e))
      for (ShapeList::Iterator it(*adj); it.More(); it.Next()) {
        const int f = body.faces.FindIndex(it.Value());
        if (f > 0 && (first == 0 || f < first) && !polygon_on(e, TopoDS::Face(body.faces(f))).IsNull()) first = f;
      }
    if (first > 0) {
      const TopoDS_Face &face = TopoDS::Face(body.faces(first));
      Handle(Poly_PolygonOnTriangulation) pol = polygon_on(e, face);
      TopLoc_Location loc;
      Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(face, loc);
      const gp_Trsf tr = loc.Transformation();
      for (int k = 1; k <= pol->NbNodes(); ++k) pts.push_back(tri->Node(pol->Node(k)).Transformed(tr));
    }
    if (pts.empty()) {
      // No face carries it (a wire-only body): discretise the curve itself.
      BRepAdaptor_Curve c(e);
      GCPnts_TangentialDeflection td(c, angular, linear);
      for (int k = 1; k <= td.NbPoints(); ++k) pts.push_back(td.Value(k));
    }
    // Ends onto the vertex points (the vertex nearest each end).
    TopoDS_Vertex v1, v2;
    TopExp::Vertices(e, v1, v2);
    for (gp_Pnt *end : {&pts.front(), &pts.back()}) {
      gp_Pnt best = *end;
      double d = RealLast();
      for (const TopoDS_Vertex &v : {v1, v2}) {
        if (v.IsNull()) continue;
        const gp_Pnt q = BRep_Tool::Pnt(v);
        if (q.Distance(*end) < d) {
          d = q.Distance(*end);
          best = q;
        }
      }
      *end = best;
    }
  }
  return out;
}

// Delabella's Delaunay triangulation for planar faces, OCCT's default (Watson) for the
// rest. Watson's time grows about as n^1.5 on one face with many holes (a plate with 432
// holes: 3.3 s for its top face alone, 0.29 s with Delabella), but Delabella fails on
// some curved faces (an elliptic extrusion side) and, rarely, on a plane.
class PlanarDelabella : public IMeshTools_MeshAlgoFactory {
public:
  Handle(IMeshTools_MeshAlgo) GetAlgo(const GeomAbs_SurfaceType type, const IMeshTools_Parameters &p) const override {
    return type == GeomAbs_Plane ? delabella->GetAlgo(type, p) : watson->GetAlgo(type, p);
  }

private:
  Handle(IMeshTools_MeshAlgoFactory) delabella = new BRepMesh_DelabellaMeshAlgoFactory();
  Handle(IMeshTools_MeshAlgoFactory) watson = new BRepMesh_MeshAlgoFactory();
};

// Mesh `body` from scratch; true if every face got triangles.
bool mesh(const Body &body, const IMeshTools_Parameters &p, bool fast) {
  BRepTools::Clean(body.s);
  Handle(BRepMesh_Context) ctx = new BRepMesh_Context(IMeshTools_MeshAlgoType_Watson);
  if (fast) ctx->SetFaceDiscret(new BRepMesh_FaceDiscret(new PlanarDelabella()));
  BRepMesh_IncrementalMesh mesher;
  mesher.SetShape(body.s);
  mesher.ChangeParameters() = p;
  mesher.Perform(ctx);
  if (!mesher.IsDone()) return false;
  for (int i = 1; i <= body.faces.Extent(); ++i) {
    TopLoc_Location loc;
    Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(TopoDS::Face(body.faces(i)), loc);
    if (tri.IsNull() || tri->NbTriangles() == 0) return false;
  }
  return true;
}

} // namespace

MeshOut tessellate(const Body &body, double linear, double angular) {
  if (!(linear > 0) || !(angular > 0)) fail("input", "tessellation tolerances must be positive");
  IMeshTools_Parameters p;
  p.Deflection = linear;
  p.Angle = angular;
  p.Relative = false;
  p.InParallel = true; // edges, then faces, each in parallel (OSD_Parallel)
  p.AllowQualityDecrease = true;
  // Explicit, because the default reads the CSF_MeshAlgo environment variable.
  p.MeshAlgo = IMeshTools_MeshAlgoType_Watson;
  // OCCT stores triangulations on the faces and edges, which bodies share, and reuses a
  // stored one within 10 % of the requested deflection, meshed in whatever context it was
  // made. Start from none (mesh() cleans first), so the mesh depends only on this body and
  // the parameters (determinism, plan section 2), and remove them again afterwards so
  // cached bodies do not keep meshes alive.
  struct Cleanup {
    const TopoDS_Shape &s;
    ~Cleanup() { BRepTools::Clean(s); }
  } cleanup{body.s};
  // Fast path first; if it leaves any face unmeshed, the whole body again with Watson.
  // The choice depends only on the result, so it is deterministic.
  if (!mesh(body, p, true) && !mesh(body, p, false)) fail("failed", "the body could not be tessellated");

  const std::vector<std::vector<gp_Pnt>> edges = edge_points(body, linear, angular);
  MeshOut out{};
  Poly_Connect connect;
  std::vector<gp_Pnt> nodes;
  for (int i = 1; i <= body.faces.Extent(); ++i) {
    const TopoDS_Face &f = TopoDS::Face(body.faces(i));
    TopLoc_Location loc;
    Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(f, loc);
    if (!tri.IsNull()) {
      // Exact surface normals at the nodes' (u, v); facet averages only where the surface
      // normal is undefined (apex, pole).
      BRepLib_ToolTriangulatedShape::ComputeNormals(f, tri, connect);
      const gp_Trsf tr = loc.Transformation();
      const bool rev = f.Orientation() == TopAbs_REVERSED;
      const uint32_t base = static_cast<uint32_t>(out.positions.size() / 3);
      nodes.assign(tri->NbNodes(), gp_Pnt());
      for (int n = 1; n <= tri->NbNodes(); ++n) nodes[n - 1] = tri->Node(n).Transformed(tr);
      // Boundary nodes take their edge's canonical points (both sides of a seam).
      for (TopExp_Explorer x(f, TopAbs_EDGE); x.More(); x.Next()) {
        const TopoDS_Edge &e = TopoDS::Edge(x.Current());
        const int ei = body.edges.FindIndex(e);
        if (ei == 0 || BRep_Tool::Degenerated(e)) continue;
        Handle(Poly_PolygonOnTriangulation) pol = BRep_Tool::PolygonOnTriangulation(e, tri, loc);
        const std::vector<gp_Pnt> &pts = edges[ei - 1];
        if (pol.IsNull() || pol->NbNodes() != static_cast<int>(pts.size())) continue;
        for (int k = 1; k <= pol->NbNodes(); ++k) nodes[pol->Node(k) - 1] = pts[k - 1];
      }
      for (int n = 1; n <= tri->NbNodes(); ++n) {
        const gp_Pnt &q = nodes[n - 1];
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
  for (const std::vector<gp_Pnt> &pts : edges) {
    for (const gp_Pnt &q : pts) {
      out.edge_points.push_back(q.X());
      out.edge_points.push_back(q.Y());
      out.edge_points.push_back(q.Z());
    }
    out.edge_ends.push_back(static_cast<uint32_t>(out.edge_points.size() / 3));
  }
  return out;
}

// OSD_Parallel::For (booleans, meshing) caps a job at the pool's size and ignores
// NbDefaultThreadsToLaunch, so the pool itself is resized. OCCT refuses while a job runs.
void set_threads(uint32_t n) {
  const Handle(OSD_ThreadPool) &pool = OSD_ThreadPool::DefaultPool();
#if defined(__EMSCRIPTEN__) && !defined(__EMSCRIPTEN_PTHREADS__)
  // WebAssembly without threads (W1a): a pool of more than one waits forever for threads
  // that cannot start, so it stays at one, whatever is asked (results never depend on it).
  (void)n;
  const int want = 1;
#else
  const int want = n == 0 ? OSD_Parallel::NbLogicalProcessors() : static_cast<int>(std::min<uint32_t>(n, 1024));
#endif
  if (pool->NbThreads() == want) return;
  try {
    pool->Init(want);
  } catch (const Standard_Failure &e) {
    fail("busy", std::string("the thread count cannot change while the kernel is working: ") + e.what());
  }
  pool->SetNbDefaultThreadsToLaunch(want);
}

} // namespace occ
