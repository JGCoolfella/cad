/**
 * Linear Pattern, Circular Pattern, Mirror and Shell PropertyManagers. Selection boxes
 * work as in SOLIDWORKS: click a box to make it active (highlighted), then pick in the
 * graphics area or the FeatureManager tree.
 */
import type { ReactNode } from "react";
import type { App, PMState } from "../app/app";
import type { GeomRef, Id, PatternDirection } from "../core/types";
import { Group, NumField } from "./pmWidgets";
import { Icon } from "./icons";
import { refName } from "./refs";
import { HOLE_FASTENER, HOLE_TYPE_LABEL, ISO_SIZES, holeDims, type HoleType } from "../core/holes";

const refLabel = (app: App, r: GeomRef | null) => refName(app, r);

function Box({ app, field, icon, children, hint }: { app: App; field: string; icon: string; children: ReactNode; hint: string }) {
  const active = app.pmField === field;
  return (
    <div className="sel-row">
      <Icon name={icon} size={18} />
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
    </div>
  );
}

function Seeds({ app, ids }: { app: App; ids: Id[] }) {
  return ids.length ? <>{ids.map((id) => <div key={id}>{app.featureById(id)?.name ?? id}</div>)}</> : null;
}

function IntField({ label, value, onChange }: { label: string; value: number; onChange: (v: number) => void }) {
  return (
    <div className="numfield" title={label}>
      <span className="nf-label">{label}</span>
      <input type="number" min={1} step={1} value={value} onChange={(e) => onChange(Math.max(1, Math.round(Number(e.target.value) || 1)))} onKeyDown={(e) => e.stopPropagation()} />
    </div>
  );
}

function Direction({ app, d, field, touch }: { app: App; d: PatternDirection; field: string; touch: () => void }) {
  return (
    <>
      <div className="pm-row">
        <button className={`tbtn ${d.reverse ? "on" : ""}`} title="Reverse Direction" onClick={() => ((d.reverse = !d.reverse), touch())}>
          ⇄
        </button>
        <Box app={app} field={field} icon="linearPattern" hint="Pattern direction (edge, face or plane)">
          {refLabel(app, d.ref)}
        </Box>
      </div>
      <NumField label="Spacing" value={d.spacing} app={app} onChange={(v) => ((d.spacing = Math.max(1e-4, v)), touch())} />
      <IntField label="Number of instances" value={d.count} onChange={(v) => ((d.count = v), touch())} />
    </>
  );
}

export function patternPM(app: App, pm: PMState): { title: string; icon: string; body: ReactNode } | null {
  const touch = () => {
    app.schedulePreview();
    app.changed();
  };
  switch (pm.kind) {
    case "linearPattern": {
      const f = pm.feature;
      return {
        title: "Linear Pattern",
        icon: "linearPattern",
        body: (
          <>
            <Group title="Direction 1">
              <Direction app={app} d={f.dir1} field="dir1" touch={touch} />
            </Group>
            <Group
              title="Direction 2"
              checkable
              checked={!!f.dir2}
              onCheck={(on) => {
                f.dir2 = on ? { ref: null, reverse: false, spacing: 10, count: 2 } : undefined;
                app.pmField = on ? "dir2" : "seeds";
                touch();
              }}
            >
              {f.dir2 && <Direction app={app} d={f.dir2} field="dir2" touch={touch} />}
            </Group>
            <Group title="Features and Faces">
              <Box app={app} field="seeds" icon="extrude" hint="Features to Pattern">
                <Seeds app={app} ids={f.seeds} />
              </Box>
            </Group>
          </>
        ),
      };
    }
    case "circularPattern": {
      const f = pm.feature;
      return {
        title: "CirPattern",
        icon: "circularPattern",
        body: (
          <>
            <Group title="Direction 1">
              <div className="pm-row">
                <button className={`tbtn ${f.reverse ? "on" : ""}`} title="Reverse Direction" onClick={() => ((f.reverse = !f.reverse), touch())}>
                  ⇄
                </button>
                <Box app={app} field="axis" icon="circularPattern" hint="Pattern axis (circular edge, cylindrical face, straight edge)">
                  {refLabel(app, f.axis)}
                </Box>
              </div>
              <label className="chk">
                <input type="radio" checked={f.equalSpacing} onChange={() => ((f.equalSpacing = true), (f.angle = 2 * Math.PI), touch())} /> Equal spacing
              </label>
              <label className="chk">
                <input type="radio" checked={!f.equalSpacing} onChange={() => ((f.equalSpacing = false), (f.angle = (2 * Math.PI) / Math.max(1, f.count)), touch())} /> Instance spacing
              </label>
              <NumField label="Angle" angle value={f.angle} app={app} onChange={(v) => ((f.angle = v), touch())} />
              <IntField label="Number of instances" value={f.count} onChange={(v) => ((f.count = v), touch())} />
            </Group>
            <Group title="Features and Faces">
              <Box app={app} field="seeds" icon="extrude" hint="Features to Pattern">
                <Seeds app={app} ids={f.seeds} />
              </Box>
            </Group>
          </>
        ),
      };
    }
    case "mirror": {
      const f = pm.feature;
      return {
        title: "Mirror",
        icon: "mirror",
        body: (
          <>
            <Group title="Mirror Face/Plane">
              <Box app={app} field="plane" icon="plane" hint="Mirror face or plane">
                {refLabel(app, f.plane)}
              </Box>
            </Group>
            <Group title="Features to Mirror">
              <Box app={app} field="seeds" icon="extrude" hint="Features to Mirror">
                <Seeds app={app} ids={f.seeds} />
              </Box>
            </Group>
            <Group title="Bodies to Mirror" checkable checked={!!f.body} onCheck={(on) => ((f.body = on), touch())}>
              <div className="muted">The whole body is mirrored and merged.</div>
            </Group>
          </>
        ),
      };
    }
    case "sweep": {
      const f = pm.feature;
      const name = (id: Id) => app.featureById(id)?.name ?? null;
      return {
        title: f.cut ? "Cut-Sweep" : "Sweep",
        icon: f.cut ? "sweptCut" : "swept",
        body: (
          <>
            <Group title="Profile and Path">
              <label className="chk">
                <input type="radio" checked={f.profileType === "sketch"} onChange={() => ((f.profileType = "sketch"), (app.pmField = "profile"), touch())} /> Sketch Profile
              </label>
              <label className="chk">
                <input type="radio" checked={f.profileType === "circular"} onChange={() => ((f.profileType = "circular"), (app.pmField = "path"), touch())} /> Circular Profile
              </label>
              {f.profileType === "sketch" && (
                <Box app={app} field="profile" icon="sketch" hint="Profile (closed sketch)">
                  {name(f.profile)}
                </Box>
              )}
              <Box app={app} field="path" icon="spline" hint="Path (sketch)">
                {name(f.path)}
              </Box>
              {f.profileType === "circular" && <NumField label="Diameter" value={f.diameter} app={app} onChange={(v) => ((f.diameter = Math.max(1e-3, v)), touch())} />}
            </Group>
            <Group title="Options">
              <select value={f.orientation} onChange={(e) => ((f.orientation = e.target.value as typeof f.orientation), touch())}>
                <option value="followPath">Follow Path</option>
                <option value="keepNormal">Keep Normal Constant</option>
              </select>
              {!f.cut && (
                <label className="chk">
                  <input type="checkbox" checked={f.merge} onChange={(e) => ((f.merge = e.target.checked), touch())} /> Merge result
                </label>
              )}
            </Group>
          </>
        ),
      };
    }
    case "loft": {
      const f = pm.feature;
      const move = (i: number, d: number) => {
        const j = i + d;
        if (j < 0 || j >= f.profiles.length) return;
        const list = [...f.profiles];
        [list[i], list[j]] = [list[j], list[i]];
        f.profiles = list;
        touch();
      };
      return {
        title: f.cut ? "Cut-Loft" : "Loft",
        icon: f.cut ? "loftedCut" : "lofted",
        body: (
          <>
            <Group title="Profiles">
              <Box app={app} field="profiles" icon="lofted" hint="Select profile sketches in order">
                {f.profiles.length
                  ? f.profiles.map((id, i) => (
                      <div key={id} className="pm-row">
                        <span style={{ flex: 1 }}>{app.featureById(id)?.name ?? id}</span>
                        <button className="tbtn" title="Move Up" onClick={(e) => (e.stopPropagation(), move(i, -1))}>
                          ▴
                        </button>
                        <button className="tbtn" title="Move Down" onClick={(e) => (e.stopPropagation(), move(i, 1))}>
                          ▾
                        </button>
                      </div>
                    ))
                  : null}
              </Box>
            </Group>
            {!f.cut && (
              <Group title="Options">
                <label className="chk">
                  <input type="checkbox" checked={f.merge} onChange={(e) => ((f.merge = e.target.checked), touch())} /> Merge result
                </label>
              </Group>
            )}
          </>
        ),
      };
    }
    case "hole": {
      const f = pm.feature;
      const d = holeDims(f.holeType, f.size, f.fit);
      const c = (f.custom ??= {});
      const rename = () => {
        if (!pm.editing) f.name = app.holeName({ ...f, name: "" });
      };
      const types: HoleType[] = ["cbore", "csk", "hole", "tap"];
      const count = f.sketchId ? Object.values((app.featureById(f.sketchId) as { sketch?: { entities: Record<string, { type: string }> } })?.sketch?.entities ?? {}).filter((e) => e.type === "point").length : (f.positions?.length ?? 0);
      return {
        title: "Hole Specification",
        icon: "hole",
        body: (
          <>
            <Group title="Hole Type">
              <div className="pm-row seg">
                {types.map((t) => (
                  <button key={t} className={`tbtn ${f.holeType === t ? "on" : ""}`} title={HOLE_TYPE_LABEL[t]} onClick={() => ((f.holeType = t), (f.custom = {}), rename(), touch())}>
                    {t === "cbore" ? "⊔" : t === "csk" ? "∨" : t === "hole" ? "│" : "⫼"}
                  </button>
                ))}
              </div>
              <div className="pm-row">
                <span className="nf-label">Standard:</span>
                <select value="ISO" disabled>
                  <option>ISO</option>
                </select>
              </div>
              <div className="muted small">{HOLE_FASTENER[f.holeType]}</div>
            </Group>
            <Group title="Hole Specifications">
              <div className="pm-row">
                <span className="nf-label">Size:</span>
                <select value={f.size} onChange={(e) => ((f.size = e.target.value), (f.custom = {}), rename(), touch())}>
                  {ISO_SIZES.map((s) => (
                    <option key={s.size}>{s.size}</option>
                  ))}
                </select>
              </div>
              {(f.holeType === "cbore" || f.holeType === "csk") && (
                <div className="pm-row">
                  <span className="nf-label">Fit:</span>
                  <select value={f.fit} onChange={(e) => ((f.fit = e.target.value as typeof f.fit), (f.custom = {}), touch())}>
                    <option value="close">Close</option>
                    <option value="normal">Normal</option>
                    <option value="loose">Loose</option>
                  </select>
                </div>
              )}
              <Group title="Show custom sizing" checkable checked={Object.keys(c).length > 0} onCheck={(on) => ((f.custom = on ? { dia: d.dia } : {}), touch())}>
                <NumField label="Through Hole Diameter" value={c.dia ?? d.dia} app={app} onChange={(v) => ((c.dia = Math.max(0.01, v)), touch())} />
                {f.holeType === "cbore" && (
                  <>
                    <NumField label="Counterbore Diameter" value={c.cboreDia ?? d.cboreDia} app={app} onChange={(v) => ((c.cboreDia = v), touch())} />
                    <NumField label="Counterbore Depth" value={c.cboreDepth ?? d.cboreDepth} app={app} onChange={(v) => ((c.cboreDepth = v), touch())} />
                  </>
                )}
                {f.holeType === "csk" && (
                  <>
                    <NumField label="Countersink Diameter" value={c.cskDia ?? d.cskDia} app={app} onChange={(v) => ((c.cskDia = v), touch())} />
                    <NumField label="Countersink Angle" angle value={c.cskAngle ?? d.cskAngle} app={app} onChange={(v) => ((c.cskAngle = v), touch())} />
                  </>
                )}
              </Group>
            </Group>
            <Group title="End Condition">
              <select value={f.end.type} onChange={(e) => ((f.end.type = e.target.value as typeof f.end.type), touch())}>
                <option value="blind">Blind</option>
                <option value="throughAll">Through All</option>
                <option value="upToNext">Up To Next</option>
              </select>
              {f.end.type === "blind" && <NumField label="Blind Hole Depth" value={f.end.depth} app={app} onChange={(v) => ((f.end.depth = Math.max(0.01, v)), touch())} />}
            </Group>
            <Group title="Positions">
              <div className={`sel-box ${app.pmField === "positions" ? "active" : ""}`}>
                {f.face || f.sketchId ? `${count} hole position${count === 1 ? "" : "s"}` : <span className="muted">Select a planar face, then click to place holes</span>}
              </div>
              {!pm.editing && !!f.positions?.length && (
                <button className="btn small" onClick={() => ((f.positions = f.positions!.slice(0, -1)), touch())}>
                  Remove last position
                </button>
              )}
              {pm.editing && <div className="muted small">Edit the positions sketch to move or dimension the holes.</div>}
            </Group>
          </>
        ),
      };
    }
    case "draft": {
      const f = pm.feature;
      return {
        title: "Draft",
        icon: "draft",
        body: (
          <>
            <Group title="Type of Draft">
              <select value="neutral" disabled>
                <option value="neutral">Neutral Plane</option>
              </select>
            </Group>
            <Group title="Draft Angle">
              <NumField label="Draft Angle" angle value={f.angle} app={app} onChange={(v) => ((f.angle = Math.max(0, Math.min(Math.PI / 2 - 0.01, v))), touch())} />
            </Group>
            <Group title="Neutral Plane">
              <div className="pm-row">
                <button className={`tbtn ${f.reverse ? "on" : ""}`} title="Reverse Direction" onClick={() => ((f.reverse = !f.reverse), touch())}>
                  ⇅
                </button>
                <Box app={app} field="neutral" icon="plane" hint="Neutral plane (face or plane)">
                  {refLabel(app, f.neutral)}
                </Box>
              </div>
            </Group>
            <Group title="Faces to Draft">
              <Box app={app} field="faces" icon="draft" hint="Faces to draft">
                {f.faces.length ? f.faces.map((_, i) => <div key={i}>Face&lt;{i + 1}&gt;</div>) : null}
              </Box>
            </Group>
          </>
        ),
      };
    }
    case "shell": {
      const f = pm.feature;
      return {
        title: "Shell",
        icon: "shell",
        body: (
          <Group title="Parameters">
            <NumField label="Thickness" value={f.thickness} app={app} onChange={(v) => ((f.thickness = Math.max(1e-3, v)), touch())} />
            <Box app={app} field="faces" icon="shell" hint="Faces to Remove">
              {f.faces.length ? f.faces.map((_, i) => <div key={i}>Face&lt;{i + 1}&gt;</div>) : null}
            </Box>
            <label className="chk">
              <input type="checkbox" checked={f.outward} onChange={(e) => ((f.outward = e.target.checked), touch())} /> Shell outward
            </label>
          </Group>
        ),
      };
    }
  }
  return null;
}
