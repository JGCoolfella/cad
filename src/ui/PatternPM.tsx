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
