import { useEffect, useState, type ReactNode } from "react";
import type { App } from "../app/app";
import type { Ctx } from "../commands/commands";
import type { EndCondition, ExtrudeFeature, Id, NewRelation, RelationType, SketchData } from "../core/types";
import { formatLength, parseAngle, parseLength, UNIT_INFO } from "../core/units";
import { uid } from "../core/ids";
import { Icon } from "./icons";
import { relationRefs } from "../sketch/solver";

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
    case "sketchTool": {
      title = toolTitle(pm.tool);
      icon = pm.tool;
      body = (
        <>
          <Group title="Message">
            <div className="msg">{toolHelp(pm.tool)}</div>
          </Group>
          {(pm.tool === "line" || pm.tool === "centerline") && (
            <Group title="Options">
              <label className="chk">
                <input
                  type="checkbox"
                  checked={pm.tool === "centerline"}
                  onChange={(e) => app.editor?.setTool(e.target.checked ? "centerline" : "line")}
                />{" "}
                For construction
              </label>
            </Group>
          )}
          <Group title="Settings">
            <label className="chk">
              <input type="checkbox" checked={app.options.sketch.automaticRelations} onChange={() => app.setOptions((o) => (o.sketch.automaticRelations = !o.sketch.automaticRelations))} /> Add relations automatically
            </label>
            <label className="chk">
              <input type="checkbox" checked={app.options.sketch.inputDimensionValue} onChange={() => app.setOptions((o) => (o.sketch.inputDimensionValue = !o.sketch.inputDimensionValue))} /> Input dimension value
            </label>
          </Group>
        </>
      );
      break;
    }
    case "addRelations":
    case "smartDimension":
      title = "Display/Delete Relations";
      icon = "relations";
      body = <RelationsPanel app={app} all />;
      break;
  }
  return (
    <div className="pm">
      <div className="pm-head">
        <Icon name={icon} size={18} />
        <span className="pm-title">{title}</span>
      </div>
      <div className="pm-buttons">
        {"feature" in pm || pm.kind === "addRelations" ? (
          <>
            <button title="OK (Enter)" onClick={() => app.acceptPM()}>
              <Icon name="check" size={18} />
            </button>
            <button title="Cancel (Esc)" onClick={() => app.cancelPM()}>
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

/** PM for sketch selections: existing relations + Add Relations buttons. */
export function SketchSelectionPM({ app }: { app: App }) {
  return (
    <div className="pm">
      <div className="pm-head">
        <Icon name="relations" size={18} />
        <span className="pm-title">Properties</span>
      </div>
      <div className="pm-body">
        <RelationsPanel app={app} />
      </div>
    </div>
  );
}

function RelationsPanel({ app, all }: { app: App; all?: boolean }) {
  const sk = app.sketch?.data;
  if (!sk) return null;
  const sel = app.selection.filter((s) => s.kind === "skEntity" || s.kind === "skPoint").map((s) => (s as { id: Id }).id);
  const selSet = new Set(sel);
  const rels = Object.values(sk.relations).filter((r) => all || relationRefs(r).some((x) => selSet.has(x)));
  const dims = Object.values(sk.dimensions).filter((d) => all || d.refs.some((x) => selSet.has(x)));
  const options = possibleRelations(sk, sel);
  const add = (type: RelationType) => {
    const r = buildRelation(sk, type, sel);
    if (!r) return;
    app.sketchCheckpoint();
    const id = uid("r");
    sk.relations[id] = { ...(r as object), id } as never;
    app.resolveSketch();
    if (app.sketch!.analysis.bad.has(id)) app.flash("Adding this relation over defines the sketch.", "warning");
  };
  const bad = app.sketch!.analysis.bad;
  return (
    <>
      {sel.length > 0 && (
        <Group title="Selected Entities">
          <div className="sel-box">
            {sel.map((id) => (
              <div key={id}>{describe(sk, id)}</div>
            ))}
          </div>
        </Group>
      )}
      <Group title="Existing Relations">
        <div className="sel-box rels">
          {rels.map((r) => (
            <div key={r.id} className={`rel ${bad.has(r.id) ? "bad" : ""}`}>
              <span>{relationLabel(r.type)}</span>
              <button
                title="Delete relation"
                onClick={() => {
                  app.sketchCheckpoint();
                  delete sk.relations[r.id];
                  app.resolveSketch();
                }}
              >
                ✕
              </button>
            </div>
          ))}
          {!rels.length && <div className="muted">None</div>}
        </div>
        <div className="muted small">{app.sketch!.analysis.status === "full" ? "Fully Defined" : app.sketch!.analysis.status === "over" ? "Over Defined" : `Under Defined (${app.sketch!.analysis.dof} DOF)`}</div>
      </Group>
      {dims.length > 0 && (
        <Group title="Dimensions">
          <div className="sel-box rels">
            {dims.map((d) => (
              <div key={d.id} className={`rel ${bad.has(d.id) ? "bad" : ""}`}>
                <span>
                  {d.name} = {app.editor?.dimText(d)}
                </span>
                <button
                  title={d.driven ? "Make driving" : "Make driven"}
                  onClick={() => {
                    app.sketchCheckpoint();
                    d.driven = !d.driven;
                    app.resolveSketch();
                  }}
                >
                  {d.driven ? "⇢" : "⇠"}
                </button>
              </div>
            ))}
          </div>
        </Group>
      )}
      {sel.length > 0 && (
        <Group title="Add Relations">
          {options.length ? (
            options.map((t) => (
              <button key={t} className="relbtn" onClick={() => add(t)}>
                {relationLabel(t)}
              </button>
            ))
          ) : (
            <div className="muted">No relations available for this selection.</div>
          )}
        </Group>
      )}
      {sel.some((id) => sk.entities[id] && sk.entities[id].type !== "point") && (
        <Group title="Options">
          <label className="chk">
            <input
              type="checkbox"
              checked={sel.every((id) => (sk.entities[id] as { construction?: boolean } | undefined)?.construction)}
              onChange={(e) => {
                app.sketchCheckpoint();
                for (const id of sel) {
                  const en = sk.entities[id];
                  if (en && en.type !== "point") (en as { construction?: boolean }).construction = e.target.checked;
                }
                app.resolveSketch();
              }}
            />{" "}
            For construction
          </label>
        </Group>
      )}
    </>
  );
}

function describe(sk: SketchData, id: Id) {
  if (sk.points[id]) return id === "origin" ? "Point1@Origin" : `Point`;
  const e = sk.entities[id];
  if (!e) return id;
  return e.type === "line" ? ((e as { construction?: boolean }).construction ? "Centerline" : "Line") : e.type === "circle" ? "Arc (circle)" : e.type === "arc" ? "Arc" : "Point";
}

const REL_LABEL: Record<RelationType, string> = {
  horizontal: "Horizontal",
  vertical: "Vertical",
  horizontalPoints: "Horizontal",
  verticalPoints: "Vertical",
  coincident: "Coincident",
  midpoint: "Midpoint",
  parallel: "Parallel",
  perpendicular: "Perpendicular",
  collinear: "Collinear",
  equal: "Equal",
  tangent: "Tangent",
  concentric: "Concentric",
  coradial: "Coradial",
  symmetric: "Symmetric",
  intersection: "Intersection",
  fix: "Fix",
};
function relationLabel(t: RelationType) {
  return REL_LABEL[t];
}

/** Which relations can be added for the selection (as in Add Relations). */
function possibleRelations(sk: SketchData, sel: Id[]): RelationType[] {
  const pts = sel.filter((id) => sk.points[id]);
  const ents = sel.map((id) => sk.entities[id]).filter(Boolean);
  const lines = ents.filter((e) => e.type === "line");
  const curves = ents.filter((e) => e.type === "circle" || e.type === "arc");
  const out: RelationType[] = [];
  if (sel.length === 1) {
    if (lines.length === 1) out.push("horizontal", "vertical");
    out.push("fix");
    return out;
  }
  if (pts.length === sel.length && pts.length >= 2) out.push("horizontal", "vertical");
  if (pts.length === 2 && sel.length === 2) out.push("coincident");
  if (pts.length === 1 && ents.length === 1) {
    out.push("coincident");
    if (lines.length === 1 || curves.some((c) => c.type === "arc")) out.push("midpoint");
  }
  if (pts.length === 2 && lines.length === 1 && sel.length === 3) out.push("symmetric");
  if (lines.length === sel.length && lines.length >= 2) {
    out.push("horizontal", "vertical", "collinear", "parallel", "equal");
    if (lines.length === 2) out.push("perpendicular");
  }
  if (lines.length === 1 && curves.length === 1 && sel.length === 2) out.push("tangent");
  if (curves.length === sel.length && curves.length >= 2) out.push("tangent", "concentric", "coradial", "equal");
  if (pts.length === 1 && lines.length === 2 && sel.length === 3) out.push("intersection");
  out.push("fix");
  return [...new Set(out)];
}

function buildRelation(sk: SketchData, type: RelationType, sel: Id[]): NewRelation | null {
  const pts = sel.filter((id) => sk.points[id]);
  const ents = sel.filter((id) => sk.entities[id]);
  switch (type) {
    case "horizontal":
    case "vertical":
      if (ents.length >= 1 && pts.length === 0) {
        if (ents.length === 1) return { type, e: ents[0] };
        // multiple lines: chain as parallel to the first + H/V on first
        for (let i = 1; i < ents.length; i++) sk.relations[uid("r")] = { id: "", type: type, e: ents[i] } as never;
        fixIds(sk);
        return { type, e: ents[0] };
      }
      if (pts.length >= 2) {
        for (let i = 2; i < pts.length; i++) sk.relations[uid("r")] = { id: "", type: type === "horizontal" ? "horizontalPoints" : "verticalPoints", p1: pts[0], p2: pts[i] } as never;
        fixIds(sk);
        return { type: type === "horizontal" ? "horizontalPoints" : "verticalPoints", p1: pts[0], p2: pts[1] };
      }
      return null;
    case "coincident":
      if (pts.length === 2) return { type, p: pts[1], e: pts[0] };
      return { type, p: pts[0], e: ents[0] };
    case "midpoint":
      return { type, p: pts[0], e: ents[0] };
    case "symmetric":
      return { type, p1: pts[0], p2: pts[1], axis: ents[0] };
    case "intersection":
      return { type, p: pts[0], e1: ents[0], e2: ents[1] };
    case "fix":
      return { type, target: sel[0] };
    case "tangent": {
      const a = sk.entities[ents[0]], b = sk.entities[ents[1]];
      let inner = false;
      if ((a.type === "circle" || a.type === "arc") && (b.type === "circle" || b.type === "arc")) {
        const ca = sk.points[a.c], cb = sk.points[b.c];
        const d = Math.hypot(ca.x - cb.x, ca.y - cb.y);
        const ra = a.type === "circle" ? a.r : Math.hypot(sk.points[a.p1].x - ca.x, sk.points[a.p1].y - ca.y);
        const rb = b.type === "circle" ? b.r : Math.hypot(sk.points[b.p1].x - cb.x, sk.points[b.p1].y - cb.y);
        inner = Math.abs(d - Math.abs(ra - rb)) < Math.abs(d - (ra + rb));
      }
      return { type, e1: ents[0], e2: ents[1], inner };
    }
    default:
      return { type, e1: ents[0], e2: ents[1] } as NewRelation;
  }
}

function fixIds(sk: SketchData) {
  for (const [k, r] of Object.entries(sk.relations)) if (!r.id) r.id = k;
}

function toolTitle(t: string) {
  return (
    {
      line: "Insert Line",
      centerline: "Insert Line",
      rectangle: "Rectangle",
      centerRectangle: "Rectangle",
      circle: "Circle",
      arc: "Arc",
      threePointArc: "Arc",
      tangentArc: "Arc",
      point: "Point",
      smartDimension: "Dimension",
      trim: "Trim",
    } as Record<string, string>
  )[t] ?? t;
}

function toolHelp(t: string) {
  return (
    {
      line: "Click to start the line, then click to place each endpoint. Click-drag draws a single line. Double-click or Esc ends the chain. Move back to the endpoint and away (or press A) for a tangent arc. Hold Ctrl to suppress automatic relations.",
      centerline: "Sketch a construction centerline. Same controls as Line.",
      rectangle: "Click two opposite corners.",
      centerRectangle: "Click the center, then a corner.",
      circle: "Click the center, then the radius.",
      arc: "Click the center, the start point, then the end point.",
      threePointArc: "Click the start point, end point, then a point on the arc.",
      tangentArc: "Click the endpoint of a line or arc, then the end of the arc.",
      point: "Click to place points.",
      smartDimension: "Select one or two entities, then click to place the dimension.",
      trim: "Power trim: drag across entities to trim them, or click a segment to trim it to the closest intersections.",
    } as Record<string, string>
  )[t] ?? "";
}

// ---------------------------------------------------------------- widgets

function Group({ title, children, checkable, checked, onCheck, collapsed, disabled }: { title: string; children?: ReactNode; checkable?: boolean; checked?: boolean; onCheck?: (on: boolean) => void; collapsed?: boolean; disabled?: boolean }) {
  const [open, setOpen] = useState(!collapsed);
  const isOpen = checkable ? !!checked : open;
  return (
    <div className={`pm-group ${disabled ? "disabled" : ""}`}>
      <div className="pm-group-head" onClick={() => !checkable && setOpen(!open)}>
        {checkable ? (
          <input type="checkbox" checked={!!checked} disabled={disabled} onChange={(e) => onCheck?.(e.target.checked)} />
        ) : null}
        <span>{title}</span>
        {!checkable && <span className="caret">{open ? "⌃" : "⌄"}</span>}
      </div>
      {isOpen && children && <div className="pm-group-body">{children}</div>}
    </div>
  );
}

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

/** Numeric box: accepts expressions and units; mouse wheel spins the value. */
export function NumField({ label, value, onChange, app, angle }: { label: string; value: number; onChange: (v: number) => void; app: App; angle?: boolean }) {
  const fmt = (v: number) => (angle ? `${+((v * 180) / Math.PI).toFixed(2)}deg` : `${formatLength(v, app.doc.units)}${UNIT_INFO[app.doc.units].length}`);
  const [text, setText] = useState(fmt(value));
  useEffect(() => setText(fmt(value)), [value, app.doc.units]);
  const commit = (t: string) => {
    const v = angle ? parseAngle(t) : parseLength(t, app.doc.units);
    if (v === null) {
      setText(fmt(value));
      return;
    }
    onChange(v);
  };
  const step = angle ? Math.PI / 180 : 1 / UNIT_INFO[app.doc.units].perMm;
  return (
    <div className="numfield" title={label}>
      <span className="nf-label">{label}</span>
      <input
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={(e) => commit(e.target.value)}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") commit((e.target as HTMLInputElement).value);
        }}
        onWheel={(e) => {
          const v = value + (e.deltaY < 0 ? step : -step);
          onChange(v);
        }}
      />
      <span className="spin">
        <button onClick={() => onChange(value + step)}>▴</button>
        <button onClick={() => onChange(value - step)}>▾</button>
      </span>
    </div>
  );
}
