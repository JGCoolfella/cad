import { useEffect, useState, type ReactNode } from "react";
import type { App } from "../app/app";
import type { Ctx } from "../commands/commands";
import type { EndCondition, ExtrudeFeature, GeomRef, Id, NewRelation, RelationType, SketchData } from "../core/types";
import { formatLength, parseAngle, parseLength, UNIT_INFO } from "../core/units";
import { uid } from "../core/ids";
import { Icon } from "./icons";
import { Group, NumField } from "./pmWidgets";
import { RelationsPanel, sketchToolPM } from "./SketchPM";
import { OP_TOOLS } from "../sketch/editor";
import { patternPM } from "./PatternPM";
import { refName } from "./refs";
import { refGeomPM } from "./RefGeomPM";

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
      const from = f.from ?? { type: "sketchPlane" as const };
      body = (
        <>
          <Group title="From">
            <select
              value={from.type}
              onChange={(e) => {
                const t = e.target.value as "sketchPlane" | "offset" | "surface";
                f.from = t === "sketchPlane" ? undefined : { type: t, offset: from.offset ?? 10, ref: from.ref ?? null };
                if (t === "surface") app.pmField = "from";
                touch();
              }}
            >
              <option value="sketchPlane">Sketch Plane</option>
              <option value="surface">Surface/Face/Plane</option>
              <option value="offset">Offset</option>
            </select>
            {from.type === "surface" && (
              <FieldBox app={app} field="from" hint="Select a face or plane to start from">
                {from.ref ? refName(app, from.ref) : null}
              </FieldBox>
            )}
            {from.type === "offset" && (
              <div className="pm-row">
                <button className={`tbtn ${from.reverse ? "on" : ""}`} title="Reverse Direction" onClick={() => ((f.from = { ...from, reverse: !from.reverse }), touch())}>
                  ⇅
                </button>
                <NumField label="Offset" value={from.offset ?? 10} app={app} onChange={(v) => ((f.from = { ...from, offset: v }), touch())} />
              </div>
            )}
          </Group>
          <Group title="Direction 1">
            <EndCond app={app} field="end1" value={f.end1} cut={f.cut} reverse={f.reverse} onReverse={() => ((f.reverse = !f.reverse), touch())} onChange={(e) => ((f.end1 = e), touch())} />
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
            {f.dir2 && <EndCond app={app} field="dir2" value={f.dir2} cut={f.cut} noMid onChange={(e) => ((f.dir2 = e), touch())} />}
          </Group>
          <Group
            title="Thin Feature"
            checkable
            checked={!!f.thin}
            onCheck={(on) => {
              f.thin = on ? { type: "oneDirection", thickness: 1 } : undefined;
              touch();
            }}
          >
            {f.thin && <ThinBox app={app} thin={f.thin} touch={touch} />}
          </Group>
          <Group title="Selected Contours" collapsed={!f.contours?.length}>
            <FieldBox app={app} field="contours" hint="Click regions of the sketch to extrude only those (empty = all)">
              {f.contours?.length ? f.contours.map((_, i) => <div key={i}>{`${app.featureById(f.sketchId)?.name ?? "Sketch"}-Region<${i + 1}>`}</div>) : null}
            </FieldBox>
          </Group>
          {!f.cut && (
            <Group title="Options">
              <label className="chk">
                <input type="checkbox" checked={f.merge} onChange={(e) => ((f.merge = e.target.checked), touch())} /> Merge result
              </label>
            </Group>
          )}
          <Group title="Sketch" collapsed={!!f.sketchId}>
            <FieldBox app={app} field="sketch" hint="Select a sketch in the tree or graphics area">
              {app.featureById(f.sketchId)?.name ?? null}
            </FieldBox>
          </Group>
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
            <FieldBox app={app} field="axis" hint="Pick an edge or axis, or choose a sketch line below">
              {f.axisRef ? refName(app, f.axisRef) : null}
            </FieldBox>
            <select value={f.axisRef ? "" : f.axis} onChange={(e) => ((f.axis = e.target.value), (f.axisRef = null), touch())}>
              <option value="">(sketch line)</option>
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
          {f.revType !== "midPlane" && (
            <Group title="Direction 2" checkable checked={!!f.angle2} onCheck={(on) => ((f.angle2 = on ? Math.PI / 2 : undefined), touch())}>
              {!!f.angle2 && <NumField label="Angle" angle value={f.angle2} app={app} onChange={(v) => ((f.angle2 = Math.max(1e-3, Math.min(2 * Math.PI - f.angle, v))), touch())} />}
            </Group>
          )}
          <Group title="Thin Feature" checkable checked={!!f.thin} onCheck={(on) => ((f.thin = on ? { type: "oneDirection", thickness: 1 } : undefined), touch())}>
            {f.thin && <ThinBox app={app} thin={f.thin} touch={touch} />}
          </Group>
          {!f.cut && (
            <Group title="Options">
              <label className="chk">
                <input type="checkbox" checked={f.merge} onChange={(e) => ((f.merge = e.target.checked), touch())} /> Merge result
              </label>
            </Group>
          )}
          <Group title="Selected Contours" collapsed={!f.contours?.length}>
            <FieldBox app={app} field="contours" hint="Click regions of the sketch to revolve only those (empty = all)">
              {f.contours?.length ? f.contours.map((_, i) => <div key={i}>{`${app.featureById(f.sketchId)?.name ?? "Sketch"}-Region<${i + 1}>`}</div>) : null}
            </FieldBox>
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
              <button className={`tbtn ${f.radius2 === undefined ? "on" : ""}`} title="Constant Size Fillet" onClick={() => ((f.radius2 = undefined), touch())}>
                ◜
              </button>
              <button className={`tbtn ${f.radius2 !== undefined ? "on" : ""}`} title="Variable Size Fillet" onClick={() => ((f.radius2 = f.radius2 ?? f.radius * 2), touch())}>
                ◝
              </button>
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
            <NumField label={f.radius2 !== undefined ? "Start radius" : "Radius"} value={f.radius} app={app} onChange={(v) => ((f.radius = Math.max(1e-4, v)), touch())} />
            {f.radius2 !== undefined && <NumField label="End radius" value={f.radius2} app={app} onChange={(v) => ((f.radius2 = Math.max(1e-4, v)), touch())} />}
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
          <Group title="Chamfer Type">
            <select value={f.chamferType === "angleDistance" ? "angleDistance" : "distanceDistance"} onChange={(e) => ((f.chamferType = e.target.value === "angleDistance" ? "angleDistance" : "equal"), touch())}>
              <option value="angleDistance">Angle Distance</option>
              <option value="distanceDistance">Distance Distance</option>
              <option value="vertex" disabled>
                Vertex
              </option>
              <option value="offsetFace" disabled>
                Offset Face
              </option>
              <option value="faceFace" disabled>
                Face Face
              </option>
            </select>
          </Group>
          <Group title="Chamfer Parameters">
            {f.chamferType !== "angleDistance" && (
              <label className="chk">
                <input type="checkbox" checked={(f.chamferType ?? "equal") === "equal"} onChange={(e) => ((f.chamferType = e.target.checked ? "equal" : "distanceDistance"), (f.distance2 = f.distance2 ?? f.distance), touch())} /> Equal distance
              </label>
            )}
            <NumField label={f.chamferType === "distanceDistance" ? "Distance 1" : "Distance"} value={f.distance} app={app} onChange={(v) => ((f.distance = Math.max(1e-4, v)), touch())} />
            {f.chamferType === "distanceDistance" && <NumField label="Distance 2" value={f.distance2 ?? f.distance} app={app} onChange={(v) => ((f.distance2 = Math.max(1e-4, v)), touch())} />}
            {f.chamferType === "angleDistance" && <NumField label="Angle" angle value={f.angle ?? Math.PI / 4} app={app} onChange={(v) => ((f.angle = Math.max(0.01, Math.min(Math.PI / 2 - 0.01, v))), touch())} />}
            {f.chamferType && f.chamferType !== "equal" && (
              <label className="chk">
                <input type="checkbox" checked={!!f.flip} onChange={(e) => ((f.flip = e.target.checked), touch())} /> Flip direction
              </label>
            )}
          </Group>
        </>
      );
      break;
    }
    case "refplane":
    case "refaxis":
    case "refpoint":
    case "coordsys": {
      const spec = refGeomPM(app, pm)!;
      title = spec.title;
      icon = spec.icon;
      body = spec.body;
      break;
    }
    case "linearPattern":
    case "circularPattern":
    case "mirror":
    case "sweep":
    case "loft":
    case "hole":
    case "draft":
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
    case "section": {
      const s = app.section;
      title = "Section View";
      icon = "section";
      const set = (f: () => void) => {
        f();
        app.changed();
      };
      const std = s?.ref.kind === "plane" && s.ref.plane.kind === "std" ? s.ref.plane.plane : null;
      body = s ? (
        <>
          <Group title="Section Method">
            <select value="planar" disabled>
              <option value="planar">Planar</option>
            </select>
          </Group>
          <Group title="Section 1">
            <div className="pm-row seg">
              {(["front", "top", "right"] as const).map((pl) => (
                <button key={pl} className={`tbtn ${std === pl ? "on" : ""}`} title={`${pl[0].toUpperCase()}${pl.slice(1)}`} onClick={() => set(() => ((s.ref = { kind: "plane", plane: { kind: "std", plane: pl } }), (s.offset = 0)))}>
                  {pl[0].toUpperCase()}
                </button>
              ))}
            </div>
            <div className="sel-box active">{refName(app, s.ref)}</div>
            <div className="pm-row">
              <button className={`tbtn ${s.flip ? "on" : ""}`} title="Flip the side to view" onClick={() => set(() => (s.flip = !s.flip))}>
                ⇄
              </button>
              <NumField label="Offset Distance" value={s.offset} app={app} onChange={(v) => set(() => (s.offset = v))} />
            </div>
          </Group>
        </>
      ) : null;
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
        {"feature" in pm || pm.kind === "addRelations" || pm.kind === "section" || (pm.kind === "sketchTool" && OP_TOOLS.has(pm.tool)) ? (
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

/** Selection box that receives graphics/tree picks while active (highlighted). */
function FieldBox({ app, field, hint, children }: { app: App; field: string; hint: string; children: ReactNode }) {
  const active = app.pmField === field;
  return (
    <div
      className={`sel-box ${active ? "active" : ""}`}
      title={hint}
      onClick={() => {
        app.pmField = field;
        app.changed();
      }}
    >
      {children || <span className="muted">{hint}</span>}
    </div>
  );
}


const END_LABEL: Record<EndCondition["type"], string> = {
  blind: "Blind",
  throughAll: "Through All",
  throughAllBoth: "Through All - Both",
  upToNext: "Up To Next",
  upToVertex: "Up To Vertex",
  upToSurface: "Up To Surface",
  offsetFromSurface: "Offset From Surface",
  upToBody: "Up To Body",
  midPlane: "Mid Plane",
};

function EndCond({ app, field, value, onChange, reverse, onReverse, cut, noMid }: { app: App; field: string; value: EndCondition; onChange: (e: EndCondition) => void; reverse?: boolean; onReverse?: () => void; cut: boolean; noMid?: boolean }) {
  const types: EndCondition["type"][] = ["blind", "throughAll", ...(cut && !noMid ? (["throughAllBoth"] as const) : []), "upToNext", "upToVertex", "upToSurface", "offsetFromSurface", "upToBody", ...(!noMid ? (["midPlane"] as const) : [])];
  const t = value.type;
  const needsRef = t === "upToSurface" || t === "offsetFromSurface";
  return (
    <>
      <div className="pm-row">
        {onReverse && (
          <button className={`tbtn ${reverse ? "on" : ""}`} title="Reverse Direction" onClick={onReverse}>
            ⇅
          </button>
        )}
        <select
          value={t}
          onChange={(e) => {
            const nt = e.target.value as EndCondition["type"];
            if (nt === "upToSurface" || nt === "offsetFromSurface" || nt === "upToVertex") app.pmField = field;
            onChange({ ...value, type: nt });
          }}
        >
          {types.map((x) => (
            <option key={x} value={x}>
              {END_LABEL[x]}
            </option>
          ))}
        </select>
      </div>
      {needsRef && (
        <FieldBox app={app} field={field} hint="Select a face or plane">
          {value.ref ? refName(app, value.ref) : null}
        </FieldBox>
      )}
      {t === "upToVertex" && (
        <FieldBox app={app} field={field} hint="Select a vertex">
          {value.vertex ? "Vertex<1>" : null}
        </FieldBox>
      )}
      {(t === "blind" || t === "midPlane" || t === "offsetFromSurface") && (
        <NumField label={t === "offsetFromSurface" ? "Offset Distance" : "Depth"} value={value.depth} app={app} onChange={(v) => onChange({ ...value, depth: Math.max(t === "offsetFromSurface" ? 0 : 1e-4, v) })} />
      )}
      {t === "offsetFromSurface" && (
        <label className="chk">
          <input type="checkbox" checked={!!value.reverseOffset} onChange={(e) => onChange({ ...value, reverseOffset: e.target.checked })} /> Reverse offset
        </label>
      )}
      <div className="pm-row">
        <button className={`tbtn ${value.draft ? "on" : ""}`} title="Draft On/Off" onClick={() => onChange({ ...value, draft: value.draft ? 0 : (2 * Math.PI) / 180 })}>
          ◿
        </button>
        {value.draft ? <NumField label="Draft Angle" angle value={value.draft} app={app} onChange={(v) => onChange({ ...value, draft: Math.max(0, Math.min(Math.PI / 2 - 0.01, v)) })} /> : <span className="muted small">Draft</span>}
      </div>
      {!!value.draft && (
        <label className="chk">
          <input type="checkbox" checked={!!value.draftOutward} onChange={(e) => onChange({ ...value, draftOutward: e.target.checked })} /> Draft outward
        </label>
      )}
      {cut && <div className="muted small">Cuts default into the material.</div>}
    </>
  );
}

function ThinBox({ app, thin, touch }: { app: App; thin: NonNullable<ExtrudeFeature["thin"]>; touch: () => void }) {
  return (
    <>
      <div className="pm-row">
        <button className={`tbtn ${thin.reverse ? "on" : ""}`} title="Reverse Direction" onClick={() => ((thin.reverse = !thin.reverse), touch())}>
          ⇄
        </button>
        <select value={thin.type} onChange={(e) => ((thin.type = e.target.value as typeof thin.type), touch())}>
          <option value="oneDirection">One-Direction</option>
          <option value="midPlane">Mid-Plane</option>
          <option value="twoDirection">Two-Direction</option>
        </select>
      </div>
      <NumField label="Thickness" value={thin.thickness} app={app} onChange={(v) => ((thin.thickness = Math.max(1e-3, v)), touch())} />
      {thin.type === "twoDirection" && <NumField label="Direction 2 Thickness" value={thin.thickness2 ?? thin.thickness} app={app} onChange={(v) => ((thin.thickness2 = Math.max(1e-3, v)), touch())} />}
    </>
  );
}

function SketchPick({ app, feature }: { app: App; feature: ExtrudeFeature | { sketchId: Id } }) {
  const s = app.featureById(feature.sketchId);
  return <div className="sel-box">{s ? s.name : <span className="muted">Select a sketch in the tree or graphics area</span>}</div>;
}

