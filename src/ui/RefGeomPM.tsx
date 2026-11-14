/**
 * Reference Geometry PropertyManagers (Plane, Axis, Point, Coordinate System). As in
 * SOLIDWORKS, the type follows from the references picked into the boxes.
 */
import type { ReactNode } from "react";
import type { App, PMState } from "../app/app";
import type { GeomRef } from "../core/types";
import { Group, NumField } from "./pmWidgets";
import { refName } from "./refs";

function RefBox({ app, field, title, value, hint }: { app: App; field: string; title: string; value: GeomRef | null | undefined; hint: string }) {
  const active = app.pmField === field;
  return (
    <Group title={title}>
      <div
        className={`sel-box ${active ? "active" : ""}`}
        title={hint}
        onClick={() => {
          app.pmField = field;
          app.changed();
        }}
      >
        {refName(app, value) ?? <span className="muted">{hint}</span>}
      </div>
    </Group>
  );
}

const isPlane = (r: GeomRef) => r.kind === "plane" || (r.kind === "face" && r.sig.surface === "PLANE");
const isLine = (r: GeomRef) => (r.kind === "edge" && r.sig.curve === "LINE") || r.kind === "axis" || r.kind === "sketchLine";
const isPoint = (r: GeomRef) => r.kind === "vertex" || r.kind === "origin" || r.kind === "point";

/** Plane type implied by the references (mirrors the kernel's rules). */
export function planeMode(refs: GeomRef[]): "offset" | "angle" | "midPlane" | "threePoints" | "parallelAtPoint" | "normalToCurve" | null {
  const planes = refs.filter(isPlane).length, lines = refs.filter(isLine).length, pts = refs.filter(isPoint).length;
  const curves = refs.filter((r) => r.kind === "edge" && r.sig.curve !== "LINE").length;
  if (pts >= 3) return "threePoints";
  if (planes >= 2) return "midPlane";
  if (planes === 1 && lines >= 1) return "angle";
  if (planes === 1 && pts >= 1) return "parallelAtPoint";
  if ((curves || lines) && pts) return "normalToCurve";
  if (planes === 1) return "offset";
  return null;
}

const MODE_LABEL: Record<string, string> = {
  offset: "Offset distance",
  angle: "At angle",
  midPlane: "Mid Plane",
  threePoints: "Through three points",
  parallelAtPoint: "Parallel at point",
  normalToCurve: "Normal to curve",
};

export function refGeomPM(app: App, pm: PMState): { title: string; icon: string; body: ReactNode } | null {
  const touch = () => {
    app.schedulePreview();
    app.changed();
  };
  const msg = (text: string) => (
    <Group title="Message">
      <div className="msg">{text}</div>
    </Group>
  );
  switch (pm.kind) {
    case "refplane": {
      const f = pm.feature;
      const refs = f.refs ?? [];
      const mode = planeMode(refs);
      return {
        title: "Plane",
        icon: "plane",
        body: (
          <>
            {msg(mode ? `Fully defined: ${MODE_LABEL[mode]}.` : "Select references and constraints.")}
            <RefBox app={app} field="ref0" title="First Reference" value={refs[0]} hint="Select a plane, face, edge or point" />
            {mode === "offset" && (
              <Group title="Offset">
                <NumField label="Offset distance" value={f.offset} app={app} onChange={(v) => ((f.offset = v), touch())} />
              </Group>
            )}
            {mode === "angle" && (
              <Group title="Angle">
                <NumField label="Angle" angle value={f.angle ?? 0} app={app} onChange={(v) => ((f.angle = v), touch())} />
              </Group>
            )}
            <RefBox app={app} field="ref1" title="Second Reference" value={refs[1]} hint="Optional second reference" />
            <RefBox app={app} field="ref2" title="Third Reference" value={refs[2]} hint="Optional third reference" />
            <Group title="Options">
              <label className="chk">
                <input type="checkbox" checked={f.flip} onChange={(e) => ((f.flip = e.target.checked), touch())} /> Flip offset
              </label>
              <button className="btn small" onClick={() => ((f.refs = []), (app.pmField = "ref0"), touch())}>
                Clear references
              </button>
            </Group>
          </>
        ),
      };
    }
    case "refaxis": {
      const f = pm.feature;
      return {
        title: "Axis",
        icon: "refAxis",
        body: (
          <>
            {msg("Select a line or edge, two planes, two points, a cylindrical face, or a point and a plane.")}
            <RefBox app={app} field="ref0" title="Selections" value={f.refs[0]} hint="First reference" />
            <RefBox app={app} field="ref1" title="Second Selection" value={f.refs[1]} hint="Optional second reference" />
            <button className="btn small" onClick={() => ((f.refs = []), (app.pmField = "ref0"), touch())}>
              Clear references
            </button>
          </>
        ),
      };
    }
    case "refpoint": {
      const f = pm.feature;
      const xyz = f.xyz ?? [0, 0, 0];
      return {
        title: "Point",
        icon: "refPoint",
        body: (
          <>
            {msg("Select an arc or circle (centre), a face (centre) or a vertex, or type coordinates.")}
            <RefBox app={app} field="ref0" title="Selections" value={f.refs[0]} hint="Arc, face or vertex" />
            {!f.refs.length && (
              <Group title="Coordinates">
                {(["X", "Y", "Z"] as const).map((l, i) => (
                  <NumField key={l} label={l} value={xyz[i]} app={app} onChange={(v) => ((f.xyz = xyz.map((x, k) => (k === i ? v : x)) as [number, number, number]), touch())} />
                ))}
              </Group>
            )}
            {!!f.refs.length && (
              <button className="btn small" onClick={() => ((f.refs = []), touch())}>
                Use coordinates instead
              </button>
            )}
          </>
        ),
      };
    }
    case "coordsys": {
      const f = pm.feature;
      return {
        title: "Coordinate System",
        icon: "coordSys",
        body: (
          <>
            {msg("Select the origin, then edges or axes for X and Y (defaults: part origin and axes).")}
            <RefBox app={app} field="origin" title="Origin" value={f.origin} hint="Origin (vertex or point)" />
            <RefBox app={app} field="xAxis" title="X Axis Direction Reference" value={f.xAxis} hint="Edge, axis or face" />
            <RefBox app={app} field="yAxis" title="Y Axis Direction Reference" value={f.yAxis} hint="Edge, axis or face" />
          </>
        ),
      };
    }
  }
  return null;
}
