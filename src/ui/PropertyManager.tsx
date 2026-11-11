import { useEffect, useState, type ReactNode } from "react";
import type { App } from "../app/app";
import type { Ctx } from "../commands/commands";
import type { EndCondition, ExtrudeFeature, Id, NewRelation, RelationType, SketchData } from "../core/types";
import { formatLength, parseAngle, parseLength, UNIT_INFO } from "../core/units";
import { uid } from "../core/ids";
import { Icon } from "./icons";
import { Group, NumField } from "./pmWidgets";
import { RelationsPanel, sketchToolPM } from "./SketchPM";
import { OP_TOOLS } from "../sketch/editor";
import { patternPM } from "./PatternPM";

/** Left-pane PropertyManager. */
export function PropertyManager({ app, ctx }: { app: App; ctx: Ctx }) {
  const pm = app.pm!;
  void ctx;
  let title = "";
  let icon = "generic";
  let body: ReactNode = null;
  const touch = () => {
    app.schedulePreview();
    app.changed();
  };
  switch (pm.kind) {
    case "extrude": {
      const f = pm.feature;
      title = f.cut ? "Cut-Extrude" : "Boss-Extrude";
      icon = f.cut ? "cutExtrude" : "extrude";
      body = (
        <>
          <Group title="From">
            <select value="sketchPlane" disabled>
              <option value="sketchPlane">Sketch Plane</option>
            </select>
          </Group>
          <Group title="Direction 1">
            <EndCond app={app} value={f.end1} cut={f.cut} reverse={f.reverse} onReverse={() => ((f.reverse = !f.reverse), touch())} onChange={(e) => ((f.end1 = e), touch())} />
          </Group>
          <Group
            title="Direction 2"
            checkable
            checked={!!f.dir2}
            onCheck={(on) => {
              f.dir2 = on ? { type: "blind", depth: 10 } : undefined;
              touch();
            }}
          >
            {f.dir2 && <EndCond app={app} value={f.dir2} cut={f.cut} noMid onChange={(e) => ((f.dir2 = e), touch())} />}
          </Group>
          {!f.cut && (
            <Group title="Options">
              <label className="chk">
                <input type="checkbox" checked={f.merge} onChange={(e) => ((f.merge = e.target.checked), touch())} /> Merge result
              </label>
            </Group>
          )}
          <Group title="Selected Contours" collapsed>
            <SketchPick app={app} feature={f} />
          </Group>
          <Group title="Thin Feature" checkable checked={false} disabled onCheck={() => {}} />
        </>
      );
      break;
    }
    case "revolve": {
      const f = pm.feature;
      title = f.cut ? "Cut-Revolve" : "Revolve";
      icon = f.cut ? "cutRevolve" : "revolve";
      const sk = app.featureById(f.sketchId);
      const lines = sk?.type === "sketch" ? Object.values(sk.sketch.entities).filter((e) => e.type === "line") : [];
      body = (
        <>
          <Group title="Axis of Revolution">
            <select value={f.axis} onChange={(e) => ((f.axis = e.target.value), touch())}>
              <option value="">(select an axis)</option>
              {lines.map((l, i) => (
                <option key={l.id} value={l.id}>
                  {(l as { construction?: boolean }).construction ? `Centerline ${i + 1}` : `Line ${i + 1}`}
                </option>
              ))}
            </select>
          </Group>
          <Group title="Direction 1">
            <div className="pm-row">
              <button className={`tbtn ${f.reverse ? "on" : ""}`} title="Reverse Direction" onClick={() => ((f.reverse = !f.reverse), touch())}>
                ⇄
              </button>
              <select value={f.revType} onChange={(e) => ((f.revType = e.target.value as typeof f.revType), touch())}>
                <option value="oneDirection">Blind</option>
                <option value="midPlane">Mid-Plane</option>
              </select>
            </div>
            <NumField label="Angle" angle value={f.angle} app={app} onChange={(v) => ((f.angle = Math.max(1e-3, Math.min(2 * Math.PI, v))), touch())} />
          </Group>
          {!f.cut && (
            <Group title="Options">
              <label className="chk">
                <input type="checkbox" checked={f.merge} onChange={(e) => ((f.merge = e.target.checked), touch())} /> Merge result
              </label>
            </Group>
          )}
          <Group title="Selected Contours" collapsed>
            <SketchPick app={app} feature={f} />
          </Group>
        </>
      );
      break;
    }
    case "fillet": {
      const f = pm.feature;
      title = "Fillet";
      icon = "fillet";
      body = (
        <>
          <Group title="Fillet Type">
            <div className="pm-row seg">
              <button className="tbtn on" title="Constant Size Fillet">◜</button>
              <button className="tbtn" disabled title="Variable Size Fillet (not yet implemented)">◝</button>
              <button className="tbtn" disabled title="Face Fillet (not yet implemented)">◞</button>
              <button className="tbtn" disabled title="Full Round Fillet (not yet implemented)">◟</button>
            </div>
          </Group>
          <Group title="Items To Fillet">
            <div className="sel-box">
              {f.edges.map((_, i) => (
                <div key={"e" + i}>Edge&lt;{i + 1}&gt;</div>
              ))}
              {f.faces.map((_, i) => (
                <div key={"f" + i}>Face&lt;{i + 1}&gt;</div>
              ))}
              {!f.edges.length && !f.faces.length && <div className="muted">Select edges or faces in the graphics area</div>}
            </div>
            <label className="chk">
              <input type="checkbox" checked readOnly /> Tangent propagation
            </label>
          </Group>
          <Group title="Fillet Parameters">
            <NumField label="Radius" value={f.radius} app={app} onChange={(v) => ((f.radius = Math.max(1e-4, v)), touch())} />
          </Group>
        </>
      );
      break;
    }
    case "chamfer": {
      const f = pm.feature;
      title = "Chamfer";
      icon = "chamfer";
      body = (
        <>
          <Group title="Items To Chamfer">
            <div className="sel-box">
              {f.edges.map((_, i) => (
                <div key={i}>Edge&lt;{i + 1}&gt;</div>
              ))}
              {!f.edges.length && <div className="muted">Select edges in the graphics area</div>}
            </div>
          </Group>
          <Group title="Chamfer Parameters">
            <select value="equal" disabled>
              <option value="equal">Equal distance</option>
            </select>
            <NumField label="Distance" value={f.distance} app={app} onChange={(v) => ((f.distance = Math.max(1e-4, v)), touch())} />
          </Group>
        </>
      );
      break;
    }
    case "refplane": {
      const f = pm.feature;
      title = "Plane";
      icon = "plane";
      const baseLabel = f.base.kind === "std" ? `${f.base.plane[0].toUpperCase()}${f.base.plane.slice(1)} Plane` : f.base.kind === "face" ? "Face<1>" : app.featureById(f.base.featureId)?.name;
      body = (
        <>
          <Group title="First Reference">
            <div className="sel-box">{baseLabel}</div>
            <button
              className="btn small"
              onClick={() => {
                const p = app.planeRefFromSelection();
                if (p) (f.base = p.ref), touch();
                else app.flash("Select a plane or planar face, then click Use Selection.", "info");
              }}
            >
              Use Selection
            </button>
            <NumField label="Offset distance" value={f.offset} app={app} onChange={(v) => ((f.offset = v), touch())} />
            <label className="chk">
              <input type="checkbox" checked={f.flip} onChange={(e) => ((f.flip = e.target.checked), touch())} /> Flip offset
            </label>
          </Group>
        </>
      );
      break;
    }
    case "linearPattern":
    case "circularPattern":
    case "mirror":
    case "shell": {
      const spec = patternPM(app, pm)!;
      title = spec.title;
      icon = spec.icon;
      body = spec.body;
      break;
    }
    case "sketchTool": {
      const spec = sketchToolPM(app, ctx, pm.tool);
      title = spec.title;
      icon = spec.icon;
      body = spec.body;
      break;
    }
    case "addRelations":
    case "smartDimension":
      title = "Display/Delete Relations";
      icon = "relations";
      body = <RelationsPanel app={app} />;
      break;
  }
  return (
    <div className="pm">
      <div className="pm-head">
        <Icon name={icon} size={18} />
        <span className="pm-title">{title}</span>
      </div>
      <div className="pm-buttons">
        {"feature" in pm || pm.kind === "addRelations" || (pm.kind === "sketchTool" && OP_TOOLS.has(pm.tool)) ? (
          <>
            <button title="OK (Enter)" onClick={() => app.acceptPM()}>
              <Icon name="check" size={18} />
            </button>
            <button title="Cancel (Esc)" onClick={() => (pm.kind === "sketchTool" && app.editor ? app.editor.setTool(null) : app.cancelPM())}>
              <Icon name="cancel" size={18} />
            </button>
          </>
        ) : (
          <button title="Close" onClick={() => (app.editor ? app.editor.setTool(null) : app.cancelPM())}>
            <Icon name="check" size={18} />
          </button>
        )}
      </div>
      <div className="pm-body">{body}</div>
    </div>
  );
}

// ---------------------------------------------------------------- widgets

function EndCond({ app, value, onChange, reverse, onReverse, cut, noMid }: { app: App; value: EndCondition; onChange: (e: EndCondition) => void; reverse?: boolean; onReverse?: () => void; cut: boolean; noMid?: boolean }) {
  return (
    <>
      <div className="pm-row">
        {onReverse && (
          <button className={`tbtn ${reverse ? "on" : ""}`} title="Reverse Direction" onClick={onReverse}>
            ⇅
          </button>
        )}
        <select value={value.type} onChange={(e) => onChange({ ...value, type: e.target.value as EndCondition["type"] })}>
          <option value="blind">Blind</option>
          <option value="throughAll">Through All</option>
          {!noMid && <option value="midPlane">Mid Plane</option>}
          <option value="upToNext" disabled>
            Up To Next
          </option>
        </select>
      </div>
      {value.type !== "throughAll" && <NumField label="D1" value={value.depth} app={app} onChange={(v) => onChange({ ...value, depth: Math.max(1e-4, v) })} />}
      <label className="chk muted" title="Draft is not yet implemented">
        <input type="checkbox" disabled /> Draft outward
      </label>
      {cut && <div className="muted small">Cuts default into the material.</div>}
    </>
  );
}

function SketchPick({ app, feature }: { app: App; feature: ExtrudeFeature | { sketchId: Id } }) {
  const s = app.featureById(feature.sketchId);
  return <div className="sel-box">{s ? s.name : <span className="muted">Select a sketch in the tree or graphics area</span>}</div>;
}

