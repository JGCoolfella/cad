// Data exchange (STEP).
#include "oc-kernel-occt/cpp/internal.h"

#include <sstream>

#include <IFSelect_ReturnStatus.hxx>
#include <Interface_Static.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_PrinterOStream.hxx>
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

} // namespace occ
