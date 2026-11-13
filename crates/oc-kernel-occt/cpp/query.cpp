// Queries: topology with geometry, bounding box, mass properties, distance, validity.
#include "oc-kernel-occt/cpp/internal.h"

#include <set>

#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepBndLib.hxx>
#include <BRepExtrema_DistShapeShape.hxx>
#include <BRepGProp.hxx>
#include <BRepGProp_Face.hxx>
#include <BRep_Tool.hxx>
#include <Bnd_Box.hxx>
#include <GCPnts_AbscissaPoint.hxx>
#include <GProp_GProps.hxx>
#include <GProp_PrincipalProps.hxx>
#include <ShapeAnalysis_Surface.hxx>
#include <TopExp.hxx>
#include <gp_Circ.hxx>
#include <gp_Cone.hxx>
#include <gp_Cylinder.hxx>
#include <gp_Elips.hxx>
#include <gp_Pln.hxx>
#include <gp_Sphere.hxx>
#include <gp_Torus.hxx>

namespace occ {

static void push3(rust::Vec<double> &v, const gp_XYZ &p) {
  v.push_back(p.X());
  v.push_back(p.Y());
  v.push_back(p.Z());
}

static void push_box(rust::Vec<double> &v, const Bnd_Box &b) {
  if (b.IsVoid()) {
    for (int i = 0; i < 6; ++i) v.push_back(0);
    return;
  }
  double x0, y0, z0, x1, y1, z1;
  b.Get(x0, y0, z0, x1, y1, z1);
  for (double x : {x0, y0, z0, x1, y1, z1}) v.push_back(x);
}

static FaceOut face_out(const Body &body, const TopoDS_Face &f) {
  FaceOut o{};
  GProp_GProps g;
  BRepGProp::SurfaceProperties(f, g);
  o.area = g.Mass();
  gp_Pnt c = g.CentreOfMass();
  push3(o.centroid, c.XYZ());
  // Normal of the (oriented) surface at the point nearest the centroid.
  gp_Vec n(0, 0, 1);
  try {
    Handle(ShapeAnalysis_Surface) sas = new ShapeAnalysis_Surface(BRep_Tool::Surface(f));
    TopLoc_Location loc;
    BRep_Tool::Surface(f, loc);
    gp_Pnt local = c.Transformed(loc.Transformation().Inverted());
    gp_Pnt2d uv = sas->ValueOfUV(local, 1e-7);
    BRepGProp_Face bf(f);
    gp_Pnt p;
    bf.Normal(uv.X(), uv.Y(), p, n);
    if (n.Magnitude() > 0) n.Normalize();
  } catch (const Standard_Failure &) {
    // degenerate surface point: keep the default
  }
  push3(o.normal, n.XYZ());
  BRepAdaptor_Surface ad(f);
  rust::Vec<double> &p = o.params;
  switch (ad.GetType()) {
  case GeomAbs_Plane:
    o.surface = 0;
    push3(p, ad.Plane().Location().XYZ());
    {
      gp_Dir d = ad.Plane().Axis().Direction();
      if (f.Orientation() == TopAbs_REVERSED) d.Reverse();
      push3(p, d.XYZ());
    }
    break;
  case GeomAbs_Cylinder:
    o.surface = 1;
    push3(p, ad.Cylinder().Location().XYZ());
    push3(p, ad.Cylinder().Axis().Direction().XYZ());
    p.push_back(ad.Cylinder().Radius());
    break;
  case GeomAbs_Cone:
    o.surface = 2;
    push3(p, ad.Cone().Location().XYZ());
    push3(p, ad.Cone().Axis().Direction().XYZ());
    p.push_back(ad.Cone().RefRadius());
    p.push_back(ad.Cone().SemiAngle());
    break;
  case GeomAbs_Sphere:
    o.surface = 3;
    push3(p, ad.Sphere().Location().XYZ());
    p.push_back(ad.Sphere().Radius());
    break;
  case GeomAbs_Torus:
    o.surface = 4;
    push3(p, ad.Torus().Location().XYZ());
    push3(p, ad.Torus().Axis().Direction().XYZ());
    p.push_back(ad.Torus().MajorRadius());
    p.push_back(ad.Torus().MinorRadius());
    break;
  case GeomAbs_BSplineSurface: o.surface = 5; break;
  case GeomAbs_BezierSurface: o.surface = 6; break;
  case GeomAbs_SurfaceOfRevolution: o.surface = 7; break;
  case GeomAbs_SurfaceOfExtrusion: o.surface = 8; break;
  case GeomAbs_OffsetSurface: o.surface = 9; break;
  default: o.surface = 10; break;
  }
  // Exact box for planar faces; for curved faces OCCT's fast box, which contains the face
  // but may be looser (the optimal one costs ~8 ms per torus face).
  Bnd_Box box;
  if (o.surface == 0) BRepBndLib::AddOptimal(f, box, false, false);
  else BRepBndLib::Add(f, box, false);
  push_box(o.bbox, box);
  ShapeMap fe;
  TopExp::MapShapes(f, TopAbs_EDGE, fe);
  std::set<uint32_t> idx;
  for (int i = 1; i <= fe.Extent(); ++i)
    if (int k = body.edges.FindIndex(fe(i))) idx.insert(static_cast<uint32_t>(k - 1));
  for (uint32_t i : idx) o.edges.push_back(i);
  return o;
}

static EdgeOut edge_out(const Body &body, const TopoDS_Edge &e, const ShapeListMap &ef) {
  EdgeOut o{};
  o.curve = 5;
  if (BRep_Tool::Degenerated(e)) {
    gp_Pnt p = BRep_Tool::Pnt(TopExp::FirstVertex(e));
    for (int i = 0; i < 3; ++i) push3(o.pts, p.XYZ());
  } else {
    BRepAdaptor_Curve c(e);
    double f = c.FirstParameter(), l = c.LastParameter();
    gp_Pnt a = c.Value(f), b = c.Value(l), m = c.Value(0.5 * (f + l));
    if (e.Orientation() == TopAbs_REVERSED) std::swap(a, b);
    push3(o.pts, a.XYZ());
    push3(o.pts, b.XYZ());
    push3(o.pts, m.XYZ());
    o.length = GCPnts_AbscissaPoint::Length(c);
    rust::Vec<double> &p = o.params;
    switch (c.GetType()) {
    case GeomAbs_Line: o.curve = 0; break;
    case GeomAbs_Circle:
      o.curve = 1;
      push3(p, c.Circle().Location().XYZ());
      push3(p, c.Circle().Axis().Direction().XYZ());
      p.push_back(c.Circle().Radius());
      break;
    case GeomAbs_Ellipse:
      o.curve = 2;
      push3(p, c.Ellipse().Location().XYZ());
      push3(p, c.Ellipse().Axis().Direction().XYZ());
      push3(p, c.Ellipse().XAxis().Direction().XYZ());
      p.push_back(c.Ellipse().MajorRadius());
      p.push_back(c.Ellipse().MinorRadius());
      break;
    case GeomAbs_BSplineCurve: o.curve = 3; break;
    case GeomAbs_BezierCurve: o.curve = 4; break;
    default: break;
    }
  }
  std::set<uint32_t> faces;
  if (const ShapeList *adj = ef.Seek(e)) {
    for (ShapeList::Iterator it(*adj); it.More(); it.Next()) {
      if (int k = body.faces.FindIndex(it.Value())) faces.insert(static_cast<uint32_t>(k - 1));
      if (BRep_Tool::IsClosed(e, TopoDS::Face(it.Value()))) o.seam = true;
    }
  }
  for (uint32_t i : faces) o.faces.push_back(i);
  TopoDS_Vertex v1, v2;
  TopExp::Vertices(e, v1, v2, true);
  std::vector<uint32_t> vs;
  for (const TopoDS_Vertex &v : {v1, v2})
    if (!v.IsNull())
      if (int k = body.vertices.FindIndex(v))
        if (vs.empty() || vs.back() != static_cast<uint32_t>(k - 1)) vs.push_back(static_cast<uint32_t>(k - 1));
  for (uint32_t i : vs) o.vertices.push_back(i);
  return o;
}

TopoOut topology(const Body &body) {
  TopoOut out{};
  ShapeMap solids, shells;
  TopExp::MapShapes(body.s, TopAbs_SOLID, solids);
  TopExp::MapShapes(body.s, TopAbs_SHELL, shells);
  out.solids = static_cast<uint32_t>(solids.Extent());
  out.shells = static_cast<uint32_t>(shells.Extent());
  for (int i = 1; i <= body.faces.Extent(); ++i) out.faces.push_back(face_out(body, TopoDS::Face(body.faces(i))));
  ShapeListMap ef;
  TopExp::MapShapesAndAncestors(body.s, TopAbs_EDGE, TopAbs_FACE, ef);
  for (int i = 1; i <= body.edges.Extent(); ++i) out.edges.push_back(edge_out(body, TopoDS::Edge(body.edges(i)), ef));
  for (int i = 1; i <= body.vertices.Extent(); ++i)
    push3(out.vertices, BRep_Tool::Pnt(TopoDS::Vertex(body.vertices(i))).XYZ());
  return out;
}

std::array<double, 6> bbox(const Body &body) {
  Bnd_Box box;
  BRepBndLib::AddOptimal(body.s, box, false, false);
  if (box.IsVoid()) fail("empty", "the body has no extent");
  std::array<double, 6> out{};
  box.Get(out[0], out[1], out[2], out[3], out[4], out[5]);
  return out;
}

MassOut mass(const Body &body) {
  GProp_GProps v, s;
  BRepGProp::VolumeProperties(body.s, v);
  BRepGProp::SurfaceProperties(body.s, s);
  MassOut out{};
  out.volume = v.Mass();
  out.area = s.Mass();
  push3(out.center, v.CentreOfMass().XYZ());
  gp_Mat m = v.MatrixOfInertia();
  for (int i = 1; i <= 3; ++i)
    for (int j = 1; j <= 3; ++j) out.inertia.push_back(m.Value(i, j));
  GProp_PrincipalProps pp = v.PrincipalProperties();
  double i1, i2, i3;
  pp.Moments(i1, i2, i3);
  for (double x : {i1, i2, i3}) out.moments.push_back(x);
  for (const gp_Vec &a : {pp.FirstAxisOfInertia(), pp.SecondAxisOfInertia(), pp.ThirdAxisOfInertia()}) push3(out.axes, a.XYZ());
  return out;
}

static TopoDS_Shape target(const Body &b, uint8_t kind, uint32_t i) {
  switch (kind) {
  case 0: return face_at(b, i);
  case 1: return edge_at(b, i);
  case 2:
    if (i >= static_cast<uint32_t>(b.vertices.Extent())) fail("input", "vertex " + std::to_string(i) + " does not exist");
    return b.vertices(static_cast<int>(i) + 1);
  default: return b.s;
  }
}

DistOut distance(const Body &a, uint8_t ka, uint32_t ia, const Body &b, uint8_t kb, uint32_t ib) {
  BRepExtrema_DistShapeShape d(target(a, ka, ia), target(b, kb, ib), Extrema_ExtFlag_MIN);
  if (!d.IsDone() || d.NbSolution() < 1) fail("failed", "the distance could not be computed");
  DistOut out{};
  out.distance = d.Value();
  push3(out.points, d.PointOnShape1(1).XYZ());
  push3(out.points, d.PointOnShape2(1).XYZ());
  return out;
}

CheckOut check(const Tol &, const Body &body) { return check_shape(body.s); }

} // namespace occ
