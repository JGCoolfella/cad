/**
 * Sketch PropertyManagers, laid out like SOLIDWORKS:
 *  - tool PMs (Insert Line, Rectangle, Circle, Arc, Slot, Polygon, Sketch Fillet...)
 *    with type buttons that switch between the tools of the same family;
 *  - entity PMs (Line Properties, Circle, Arc, Point) with Existing Relations,
 *    Add Relations, Options, Parameters and Additional Parameters;
 *  - Display/Delete Relations.
 */
import { useState, type ReactNode } from "react";
import type { App } from "../app/app";
import { CMD, runCommand, type Ctx } from "../commands/commands";
import type { Id, RelationType, SketchData, SketchEntity } from "../core/types";
import { REL_LABEL, describeEntity, possibleRelations } from "../sketch/relations";
import { relationRefs } from "../sketch/solver";
import { arcGeom } from "../sketch/model";
import { SHAPES } from "../sketch/shapes";
import { Icon } from "./icons";
import { Group, NumField } from "./pmWidgets";

const MSG: Record<string, string> = {
  line: "Edit the settings of the next new line or sketch a new line.",
  centerline: "Edit the settings of the next new line or sketch a new line.",
  rectangle: "Click to set the first corner, then click to set the opposite corner.",
  centerRectangle: "Click to set the center, then click to set a corner.",
  circle: "Click to set the center, then click to set the radius.",
  arc: "Click to set the center, then the start point, then the end point of the arc.",
  threePointArc: "Click to set the start point, then the end point, then a point on the arc.",
  tangentArc: "Click the endpoint of a line, arc, ellipse or spline, then click to set the end of the arc.",
  point: "Click to place a point.",
  smartDimension: "Select one or two edges/vertices and then a text location.",
  trim: "Select an entity or drag your cursor across an entity to trim it. Hold Shift and drag to extend.",
  sketchFillet: "Select a sketch vertex or entities to fillet.",
};

const FAMILY: Record<string, { title: string; label: string; ids: string[] }> = {
  rectangle: { title: "Rectangle", label: "Rectangle Type", ids: ["sketch.rectangle", "sketch.centerRectangle", "sketch.3ptRectangle", "sketch.3ptCenterRectangle", "sketch.parallelogram"] },
  circle: { title: "Circle", label: "Circle Type", ids: ["sketch.circle", "sketch.perimeterCircle"] },
  arc: { title: "Arc", label: "Arc Type", ids: ["sketch.arc", "sketch.tangentArc", "sketch.threePointArc"] },
  slot: { title: "Slot", label: "Slot Types", ids: ["sketch.slot", "sketch.centerSlot", "sketch.arcSlot3", "sketch.arcSlotCenter"] },
};
const TOOL_FAMILY: Record<string, string> = {
  rectangle: "rectangle",
  centerRectangle: "rectangle",
  threePointRectangle: "rectangle",
  threePointCenterRectangle: "rectangle",
  parallelogram: "rectangle",
  circle: "circle",
  perimeterCircle: "circle",
  arc: "arc",
  tangentArc: "arc",
  threePointArc: "arc",
  straightSlot: "slot",
  centerSlot: "slot",
};

function TypeButtons({ ctx, ids }: { ctx: Ctx; ids: string[] }) {
  return (
    <div className="pm-types">
      {ids.map((id) => {
        const c = CMD[id];
        return (
          <button key={id} className={`pm-type ${c.checked?.(ctx) ? "on" : ""}`} disabled={c.todo} data-tip={c.label} data-tip-desc={c.todo ? "Not yet implemented in OpenCAD." : c.tip} onClick={() => runCommand(id, ctx)}>
            <Icon name={c.icon ?? "generic"} size={26} />
          </button>
        );
      })}
    </div>
  );
}

function DisabledParams({ labels }: { labels: string[] }) {
  return (
    <Group title="Parameters">
      {labels.map((l, i) => (
        <div key={i} className="numfield disabled">
          <span className="nf-label">{l}</span>
          <input value="0.00" disabled readOnly />
        </div>
      ))}
    </Group>
  );
}

/** PropertyManager shown while a sketch tool is active. */
export function sketchToolPM(app: App, ctx: Ctx, tool: string): { title: string; icon: string; body: ReactNode } {
  const o = app.toolOpts;
  const set = (f: () => void) => {
    f();
    app.changed();
  };
  const shapeStep = SHAPES[tool] ? SHAPES[tool].prompts[Math.min(app.editor?.st.pts.length ?? 0, SHAPES[tool].prompts.length - 1)] : null;
  const message = shapeStep ?? MSG[tool] ?? "";
  const Msg = (
    <Group title="Message">
      <div className="msg">{message}</div>
    </Group>
  );
  const construction = (
    <label className="chk">
      <input type="checkbox" checked={o.construction} onChange={(e) => set(() => (o.construction = e.target.checked))} /> For construction
    </label>
  );

  if (tool === "line" || tool === "centerline" || tool === "midpointLine") {
    const forConstruction = tool === "centerline" || (tool === "midpointLine" && o.construction);
    return {
      title: "Insert Line",
      icon: "line",
      body: (
        <>
          {Msg}
          <Group title="Orientation">
            {(
              [
                ["asSketched", "As sketched"],
                ["horizontal", "Horizontal"],
                ["vertical", "Vertical"],
                ["angle", "Angle"],
              ] as const
            ).map(([v, l]) => (
              <label key={v} className="chk">
                <input type="radio" checked={o.lineOrientation === v} onChange={() => set(() => (o.lineOrientation = v))} /> {l}
              </label>
            ))}
          </Group>
          <Group title="Options">
            <label className="chk">
              <input
                type="checkbox"
                checked={forConstruction}
                onChange={(e) => {
                  if (tool === "midpointLine") set(() => (o.construction = e.target.checked));
                  else runCommand(e.target.checked ? "sketch.centerline" : "sketch.line", ctx);
                }}
              />{" "}
              For construction
            </label>
            <label className="chk disabled" data-tip="Infinite length" data-tip-desc="Not yet implemented in OpenCAD.">
              <input type="checkbox" disabled /> Infinite length
            </label>
            <label className="chk">
              <input type="checkbox" checked={tool === "midpointLine"} onChange={(e) => runCommand(e.target.checked ? "sketch.midpointLine" : "sketch.line", ctx)} /> Midpoint line
            </label>
          </Group>
          {o.lineOrientation === "angle" && (
            <Group title="Parameters">
              <NumField label="Angle" angle app={app} value={(o.lineAngle * Math.PI) / 180} onChange={(v) => set(() => (o.lineAngle = (v * 180) / Math.PI))} />
            </Group>
          )}
        </>
      ),
    };
  }

  const fam = FAMILY[TOOL_FAMILY[tool]];
  if (fam) {
    const isRect = TOOL_FAMILY[tool] === "rectangle";
    const centerType = tool === "centerRectangle" || tool === "threePointCenterRectangle";
    return {
      title: fam.title,
      icon: CMD[fam.ids.find((id) => CMD[id].checked?.(ctx)) ?? fam.ids[0]].icon ?? "generic",
      body: (
        <>
          {SHAPES[tool] && Msg}
          <Group title={fam.label}>
            <TypeButtons ctx={ctx} ids={fam.ids} />
            {isRect && (
              <>
                <label className={`chk ${centerType ? "disabled" : ""}`}>
                  <input type="checkbox" checked={centerType || o.rectConstruction} disabled={centerType} onChange={(e) => set(() => (o.rectConstruction = e.target.checked))} /> Add construction lines
                </label>
                {(
                  [
                    [false, "From Corners"],
                    [true, "From Midpoints"],
                  ] as const
                ).map(([v, l]) => (
                  <label key={l} className="chk indent">
                    <input type="radio" disabled={!(centerType || o.rectConstruction)} checked={o.rectFromMidpoints === v} onChange={() => set(() => (o.rectFromMidpoints = v))} /> {l}
                  </label>
                ))}
              </>
            )}
            {TOOL_FAMILY[tool] === "slot" && (
              <label className="chk disabled" data-tip="Add dimensions" data-tip-desc="Not yet implemented in OpenCAD.">
                <input type="checkbox" disabled /> Add dimensions
              </label>
            )}
          </Group>
          {TOOL_FAMILY[tool] === "slot" && <Group title="Options">{construction}</Group>}
          <DisabledParams labels={isRect ? ["X", "Y", "X", "Y", "X", "Y", "X", "Y"] : TOOL_FAMILY[tool] === "circle" ? ["X", "Y", "R"] : TOOL_FAMILY[tool] === "arc" ? ["X", "Y", "X", "Y", "X", "Y", "R", "A"] : ["X", "Y", "W", "L"]} />
        </>
      ),
    };
  }

  if (tool === "polygon") {
    return {
      title: "Polygon",
      icon: "polygon",
      body: (
        <>
          {Msg}
          <Group title="Options">{construction}</Group>
          <Group title="Parameters">
            <div className="numfield" data-tip="Number of Sides">
              <span className="nf-label">Sides</span>
              <input type="number" min={3} max={40} value={o.polygonSides} onKeyDown={(e) => e.stopPropagation()} onChange={(e) => set(() => (o.polygonSides = Math.max(3, Math.min(40, Math.round(+e.target.value) || 6))))} />
            </div>
            <label className="chk">
              <input type="radio" checked={o.polygonInscribed} onChange={() => set(() => (o.polygonInscribed = true))} /> Inscribed circle
            </label>
            <label className="chk">
              <input type="radio" checked={!o.polygonInscribed} onChange={() => set(() => (o.polygonInscribed = false))} /> Circumscribed circle
            </label>
          </Group>
        </>
      ),
    };
  }

  if (tool === "sketchFillet") {
    return {
      title: "Sketch Fillet",
      icon: "sketchFillet",
      body: (
        <>
          {Msg}
          <Group title="Entities to Fillet">
            <div className="sel-box">{app.editor?.st.filletFirst ? "Line" : <span className="muted">&nbsp;</span>}</div>
          </Group>
          <Group title="Fillet Parameters">
            <NumField label="Radius" value={o.filletRadius} app={app} onChange={(v) => set(() => (o.filletRadius = Math.max(1e-4, v)))} />
            <label className="chk">
              <input type="checkbox" checked={o.filletKeepCorners} onChange={(e) => set(() => (o.filletKeepCorners = e.target.checked))} /> Keep constrained corners
            </label>
            <label className="chk">
              <input type="checkbox" checked={o.filletDimensionEach} onChange={(e) => set(() => (o.filletDimensionEach = e.target.checked))} /> Dimension each fillet
            </label>
          </Group>
        </>
      ),
    };
  }

  if (tool === "trim") {
    return {
      title: "Trim",
      icon: "trim",
      body: (
        <>
          {Msg}
          <Group title="Options">
            {["Power trim", "Corner", "Trim away inside", "Trim away outside", "Trim to closest"].map((l, i) => (
              <button key={l} className={`relbtn ${i === 0 ? "on" : ""}`} disabled={i > 0 && i < 4} data-tip={l} data-tip-desc={i === 0 ? "Drag across entities to trim them." : i === 4 ? "Click a segment to trim it to the closest intersections (always active)." : "Not yet implemented in OpenCAD."}>
                {l}
              </button>
            ))}
          </Group>
        </>
      ),
    };
  }

  if (tool === "point") return { title: "Point", icon: "point", body: <>{Msg}<DisabledParams labels={["X", "Y"]} /></> };
  if (tool === "smartDimension") return { title: "Dimension", icon: "smartDimension", body: Msg };
  return { title: CMD[`sketch.${tool}`]?.label ?? tool, icon: CMD[`sketch.${tool}`]?.icon ?? "generic", body: Msg };
}

// ---------------------------------------------------------------- entity properties

function selectedIds(app: App): Id[] {
  return app.selection.filter((s) => s.kind === "skEntity" || s.kind === "skPoint").map((s) => (s as { id: Id }).id);
}

function RelationList({ app, ids }: { app: App; ids: Id[] }) {
  const sk = app.sketch!.data;
  const set = new Set(ids);
  const rels = Object.values(sk.relations).filter((r) => relationRefs(r).some((x) => set.has(x)));
  const bad = app.sketch!.analysis.bad;
  const [sel, setSel] = useState<Id | null>(null);
  return (
    <div
      className="sel-box rels"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "Delete" && sel && sk.relations[sel]) {
          e.stopPropagation();
          app.sketchCheckpoint();
          delete sk.relations[sel];
          app.resolveSketch();
        }
      }}
    >
      {rels.map((r, i) => (
        <div key={r.id} className={`rel ${bad.has(r.id) ? "bad" : ""} ${sel === r.id ? "on" : ""}`} onClick={() => setSel(r.id)}>
          <span>
            <Icon name={`rel:${r.type}`} size={14} /> {REL_LABEL[r.type]}
            {i}
          </span>
        </div>
      ))}
    </div>
  );
}

function stateLabel(app: App, ids: Id[]) {
  const a = app.sketch!.analysis;
  const states = ids.map((id) => a.entityState[id] ?? a.pointState[id]);
  return states.includes("over") ? "Over Defined" : states.every((s) => s === "full") ? "Fully Defined" : "Under Defined";
}

/** PropertyManager for selected sketch entities ("Line Properties", "Circle", ...). */
export function SketchSelectionPM({ app, ctx }: { app: App; ctx: Ctx }) {
  const sk = app.sketch?.data;
  if (!sk) return null;
  const ids = selectedIds(app);
  const single = ids.length === 1 ? ids[0] : null;
  const ent = single ? sk.entities[single] : undefined;
  const title = !single ? "Properties" : ent?.type === "line" ? "Line Properties" : ent?.type === "circle" ? "Circle" : ent?.type === "arc" ? "Arc" : "Point";
  const icon = ent?.type === "line" ? "line" : ent?.type === "circle" ? "circle" : ent?.type === "arc" ? "arc" : "point";
  const options = possibleRelations(sk, ids).filter((t) => CMD[`rel.${t}`]);
  const ents = ids.map((id) => sk.entities[id]).filter((e): e is SketchEntity => !!e && e.type !== "point");
  return (
    <div className="pm">
      <div className="pm-head">
        <Icon name={icon} size={18} />
        <span className="pm-title">{title}</span>
      </div>
      <div className="pm-buttons">
        <button data-tip="OK" onClick={() => app.clearSelection()}>
          <Icon name="check" size={18} />
        </button>
      </div>
      <div className="pm-body">
        {!single && (
          <Group title="Selected Entities">
            <div className="sel-box">
              {ids.map((id) => (
                <div key={id}>{describeEntity(sk, id)}</div>
              ))}
            </div>
          </Group>
        )}
        <Group title="Existing Relations">
          <RelationList app={app} ids={ids} />
          <div className="rel-state">
            <span className="info-i">i</span> {stateLabel(app, ids)}
          </div>
        </Group>
        <Group title="Add Relations">
          {options.map((t: RelationType) => (
            <button key={t} className="relbtn" onClick={() => runCommand(`rel.${t}`, ctx)}>
              <Icon name={`rel:${t}`} size={16} /> {REL_LABEL[t]}
            </button>
          ))}
        </Group>
        {ents.length > 0 && (
          <Group title="Options">
            <label className="chk">
              <input type="checkbox" checked={ents.every((e) => (e as { construction?: boolean }).construction)} onChange={() => runCommand("sketch.toggleConstruction", ctx)} /> For construction
            </label>
            {ent?.type === "line" && (
              <label className="chk disabled">
                <input type="checkbox" disabled /> Infinite length
              </label>
            )}
          </Group>
        )}
        {single && <Parameters app={app} sk={sk} id={single} />}
      </div>
    </div>
  );
}

function Parameters({ app, sk, id }: { app: App; sk: SketchData; id: Id }) {
  const e = sk.entities[id];
  const p = sk.points[id];
  const setp = (key: string) => (v: number) => app.editor?.setEntityParam(id, key, v);
  if (p || e?.type === "point") {
    const q = p ?? sk.points[(e as { p: Id }).p];
    return (
      <Group title="Parameters">
        <NumField label="X" value={q.x} app={app} onChange={setp("x")} />
        <NumField label="Y" value={q.y} app={app} onChange={setp("y")} />
      </Group>
    );
  }
  if (!e) return null;
  if (e.type === "line") {
    const a = sk.points[e.p1], b = sk.points[e.p2];
    const len = Math.hypot(b.x - a.x, b.y - a.y);
    const ang = Math.atan2(b.y - a.y, b.x - a.x);
    return (
      <>
        <Group title="Parameters">
          <NumField label="Length" value={len} app={app} onChange={setp("length")} />
          <NumField label="Angle" angle value={ang} app={app} onChange={setp("angle")} />
        </Group>
        <Group title="Additional Parameters" collapsed>
          <NumField label="Start X" value={a.x} app={app} onChange={setp("x1")} />
          <NumField label="Start Y" value={a.y} app={app} onChange={setp("y1")} />
          <NumField label="End X" value={b.x} app={app} onChange={setp("x2")} />
          <NumField label="End Y" value={b.y} app={app} onChange={setp("y2")} />
          <NumField label="Delta X" value={b.x - a.x} app={app} onChange={setp("dx")} />
          <NumField label="Delta Y" value={b.y - a.y} app={app} onChange={setp("dy")} />
        </Group>
      </>
    );
  }
  const c = sk.points[e.c];
  const r = e.type === "circle" ? e.r : arcGeom(sk, e).r;
  return (
    <Group title="Parameters">
      <NumField label="Center X" value={c.x} app={app} onChange={setp("cx")} />
      <NumField label="Center Y" value={c.y} app={app} onChange={setp("cy")} />
      <NumField label="Radius" value={r} app={app} onChange={setp("r")} />
      {e.type === "arc" && <NumField label="Angle" angle value={arcGeom(sk, e).sweep} app={app} onChange={() => {}} />}
    </Group>
  );
}

// ---------------------------------------------------------------- Display/Delete Relations

export function RelationsPanel({ app }: { app: App }) {
  const sk = app.sketch?.data;
  const [filter, setFilter] = useState<"all" | "selected" | "bad">("all");
  const [sel, setSel] = useState<Id | null>(null);
  if (!sk) return null;
  const ids = new Set(selectedIds(app));
  const bad = app.sketch!.analysis.bad;
  const rels = Object.values(sk.relations).filter((r) => (filter === "all" ? true : filter === "bad" ? bad.has(r.id) : relationRefs(r).some((x) => ids.has(x))));
  const current = sel ? sk.relations[sel] : undefined;
  const del = (list: Id[]) => {
    if (!list.length) return;
    app.sketchCheckpoint();
    for (const id of list) delete sk.relations[id];
    setSel(null);
    app.resolveSketch();
  };
  const a = app.sketch!.analysis;
  return (
    <>
      <Group title="Relations">
        <select value={filter} onChange={(e) => setFilter(e.target.value as typeof filter)}>
          <option value="all">All in this sketch</option>
          <option value="selected">Selected Entities</option>
          <option value="bad">Overdefining/Not Solved</option>
        </select>
        <div className="sel-box rels tall">
          {rels.map((r, i) => (
            <div
              key={r.id}
              className={`rel ${bad.has(r.id) ? "bad" : ""} ${sel === r.id ? "on" : ""}`}
              onClick={() => {
                setSel(r.id);
                // Highlight the entities the relation refers to.
                app.selection = relationRefs(r).map((x) => (sk.entities[x] ? { kind: "skEntity" as const, id: x } : { kind: "skPoint" as const, id: x }));
                app.changed();
              }}
            >
              <span>
                <Icon name={`rel:${r.type}`} size={14} /> {REL_LABEL[r.type]}
                {i}
              </span>
            </div>
          ))}
          {!rels.length && <div className="muted">&nbsp;</div>}
        </div>
        <div className="rel-state">
          <span className="info-i">i</span> {current ? (bad.has(current.id) ? "Over Defining" : "Satisfied") : a.status === "full" ? "Fully Defined" : a.status === "over" ? "Over Defined" : "Under Defined"}
        </div>
        <label className="chk disabled">
          <input type="checkbox" disabled /> Suppressed
        </label>
        <div className="pm-row">
          <button className="btn small" disabled={!sel} onClick={() => sel && del([sel])}>
            Delete
          </button>
          <button className="btn small" disabled={!rels.length} onClick={() => del(rels.map((r) => r.id))}>
            Delete All
          </button>
        </div>
      </Group>
      <Group title="Entities">
        <div className="sel-box">
          {current ? (
            relationRefs(current).map((x) => (
              <div key={x} className="ent-row">
                <span>{describeEntity(sk, x)}</span>
                <span className="muted">{a.entityState[x] === "full" || a.pointState[x] === "full" ? "Fully Defined" : "Under Defined"}</span>
                <span className="muted">Current Sketch</span>
              </div>
            ))
          ) : (
            <span className="muted">&nbsp;</span>
          )}
        </div>
      </Group>
    </>
  );
}
