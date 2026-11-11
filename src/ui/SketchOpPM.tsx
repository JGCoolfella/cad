/**
 * PropertyManagers of the sketch operation tools (Offset, Mirror, Move/Copy/Rotate/Scale,
 * Linear/Circular Sketch Pattern, Extend, Sketch Chamfer), laid out like SOLIDWORKS.
 */
import type { ReactNode } from "react";
import type { App } from "../app/app";
import type { Id } from "../core/types";
import { describeEntity } from "../sketch/relations";
import type { OpField } from "../sketch/editor";
import { Group, NumField } from "./pmWidgets";
import { Icon } from "./icons";

/** Selection box; the active one (pink in SOLIDWORKS, light blue here) receives graphics picks. */
function SelBox({ app, field, items, hint, icon }: { app: App; field: OpField; items: Id[]; hint: string; icon: string }) {
  const ed = app.editor;
  const active = ed?.st.opField === field;
  const sk = app.sketch?.data;
  return (
    <div className="sel-row">
      <Icon name={icon} size={18} />
      <div className={`sel-box ${active ? "active" : ""}`} onClick={() => ed?.setOpField(field)} title={hint}>
        {items.length && sk ? items.map((id) => <div key={id}>{describeEntity(sk, id)}</div>) : <span className="muted">{hint}</span>}
      </div>
    </div>
  );
}

function PointBox({ app, field, label }: { app: App; field: OpField; label: string }) {
  const ed = app.editor;
  const active = ed?.st.opField === field;
  const i = field === "point2" ? 1 : 0;
  const p = ed?.st.opPts?.[i];
  return (
    <div className="sel-row">
      <Icon name="point" size={18} />
      <div className={`sel-box ${active ? "active" : ""}`} onClick={() => ed?.setOpField(field)}>
        {p ? `${label}: ${p[0].toFixed(2)}, ${p[1].toFixed(2)}` : <span className="muted">{label}</span>}
      </div>
    </div>
  );
}

function IntField({ label, value, onChange, min = 1 }: { label: string; value: number; onChange: (v: number) => void; min?: number }) {
  return (
    <div className="numfield" title={label}>
      <span className="nf-label">{label}</span>
      <input type="number" value={value} min={min} step={1} onChange={(e) => onChange(Math.max(min, Math.round(Number(e.target.value) || min)))} onKeyDown={(e) => e.stopPropagation()} />
    </div>
  );
}

function Chk({ label, on, set }: { label: string; on: boolean; set: (v: boolean) => void }) {
  return (
    <label className="chk">
      <input type="checkbox" checked={on} onChange={(e) => set(e.target.checked)} /> {label}
    </label>
  );
}

/** Returns the PM spec for an operation tool, or null if `tool` is not one. */
export function sketchOpPM(app: App, tool: string): { title: string; icon: string; body: ReactNode } | null {
  const o = app.toolOpts;
  const st = app.editor?.st;
  const sel = st?.opSel ?? [];
  const set = (f: () => void) => {
    f();
    app.changed();
  };
  const err = app.editor?.opPreview()?.error;
  const Msg = (text: string) => (
    <Group title="Message">
      <div className="msg">{text}</div>
    </Group>
  );
  const ents = (hint = "Entities to move") => (
    <Group title={tool === "offset" ? "Parameters" : "Entities to " + (tool === "linearPattern" || tool === "circularPattern" ? "Pattern" : tool === "mirror" ? "Mirror" : tool[0].toUpperCase() + tool.slice(1))}>
      <SelBox app={app} field="entities" items={sel} hint={hint} icon="boxSelect" />
    </Group>
  );
  switch (tool) {
    case "offset":
      return {
        title: "Offset Entities",
        icon: "offset",
        body: (
          <>
            {Msg(err && sel.length ? err : "Select the sketch entities, model edges or faces to offset. Set the distance, then click ✓.")}
            <Group title="Parameters">
              <NumField label="Offset distance" value={o.offsetDistance} app={app} onChange={(v) => set(() => (o.offsetDistance = Math.max(1e-4, v)))} />
              <Chk label="Add dimensions" on={o.offsetAddDimensions} set={(v) => set(() => (o.offsetAddDimensions = v))} />
              <Chk label="Reverse" on={o.offsetReverse} set={(v) => set(() => (o.offsetReverse = v))} />
              <Chk label="Select chain" on={o.offsetSelectChain} set={(v) => set(() => (o.offsetSelectChain = v))} />
              <Chk label="Bi-directional" on={o.offsetBidirectional} set={(v) => set(() => (o.offsetBidirectional = v))} />
              <Chk label="Cap ends (Arcs)" on={o.offsetCapEnds} set={(v) => set(() => (o.offsetCapEnds = v))} />
              <div className="pm-sub">Construction geometry:</div>
              <Chk label="Base geometry" on={o.offsetBaseConstruction} set={(v) => set(() => (o.offsetBaseConstruction = v))} />
              <Chk label="Offset geometry" on={o.offsetConstruction} set={(v) => set(() => (o.offsetConstruction = v))} />
              <SelBox app={app} field="entities" items={sel} hint="Sketch entities to offset" icon="boxSelect" />
            </Group>
          </>
        ),
      };
    case "mirror":
      return {
        title: "Mirror",
        icon: "sketchMirror",
        body: (
          <>
            {Msg("Select entities to mirror and a sketch line or linear model edge to mirror about.")}
            <Group title="Options">
              <SelBox app={app} field="entities" items={sel} hint="Entities to mirror" icon="boxSelect" />
              <Chk label="Copy" on={o.mirrorCopy} set={(v) => set(() => (o.mirrorCopy = v))} />
              <SelBox app={app} field="axis" items={st?.opAxis ? [st.opAxis] : []} hint="Mirror about" icon="centerline" />
            </Group>
          </>
        ),
      };
    case "move":
    case "copy":
      return {
        title: tool === "move" ? "Move" : "Copy",
        icon: "move",
        body: (
          <>
            {Msg("Select sketch items, then pick a base point and the destination, or type X and Y.")}
            {ents(tool === "move" ? "Sketch items to move" : "Sketch items to copy")}
            <Group title="Parameters">
              <label className="chk">
                <input type="radio" checked={o.moveMode === "fromTo"} onChange={() => set(() => (o.moveMode = "fromTo"))} /> From/To
              </label>
              <label className="chk">
                <input type="radio" checked={o.moveMode === "xy"} onChange={() => set(() => (o.moveMode = "xy"))} /> X/Y
              </label>
              {o.moveMode === "fromTo" ? (
                <>
                  <PointBox app={app} field="point1" label="Base point" />
                  <PointBox app={app} field="point2" label="To point" />
                </>
              ) : (
                <>
                  <NumField label="ΔX" value={o.moveDX} app={app} onChange={(v) => set(() => (o.moveDX = v))} />
                  <NumField label="ΔY" value={o.moveDY} app={app} onChange={(v) => set(() => (o.moveDY = v))} />
                </>
              )}
              {tool === "move" && <Chk label="Keep relations" on={o.keepRelations} set={(v) => set(() => (o.keepRelations = v))} />}
            </Group>
          </>
        ),
      };
    case "rotate":
      return {
        title: "Rotate",
        icon: "rotateEntities",
        body: (
          <>
            {Msg("Select sketch items, pick the center of rotation and set the angle.")}
            {ents("Sketch items to rotate")}
            <Group title="Parameters">
              <PointBox app={app} field="point1" label="Center of rotation (default: origin)" />
              <NumField label="Angle" angle value={o.rotateAngle} app={app} onChange={(v) => set(() => (o.rotateAngle = v))} />
              <Chk label="Keep relations" on={o.keepRelations} set={(v) => set(() => (o.keepRelations = v))} />
            </Group>
          </>
        ),
      };
    case "scale":
      return {
        title: "Scale",
        icon: "scaleEntities",
        body: (
          <>
            {Msg("Select sketch items, pick the scale point and set the factor.")}
            {ents("Sketch items to scale")}
            <Group title="Parameters">
              <PointBox app={app} field="point1" label="Scale about (default: origin)" />
              <NumField label="Scale factor" value={o.scaleFactor} app={app} onChange={(v) => set(() => (o.scaleFactor = Math.max(1e-4, v)))} />
              <Chk label="Copy" on={o.scaleCopy} set={(v) => set(() => (o.scaleCopy = v))} />
            </Group>
          </>
        ),
      };
    case "linearPattern":
      return {
        title: "Linear Pattern",
        icon: "sketchPattern",
        body: (
          <>
            {Msg("Select the entities to pattern and set the spacing and number of instances.")}
            <Group title="Direction 1">
              <NumField label="Spacing" value={o.lpSpacing1} app={app} onChange={(v) => set(() => (o.lpSpacing1 = v))} />
              <IntField label="Number of instances" value={o.lpCount1} onChange={(v) => set(() => (o.lpCount1 = v))} />
              <NumField label="Angle" angle value={o.lpAngle1} app={app} onChange={(v) => set(() => (o.lpAngle1 = v))} />
            </Group>
            <Group title="Direction 2" checkable checked={o.lpCount2 > 1} onCheck={(on) => set(() => (o.lpCount2 = on ? 2 : 1))}>
              <NumField label="Spacing" value={o.lpSpacing2} app={app} onChange={(v) => set(() => (o.lpSpacing2 = v))} />
              <IntField label="Number of instances" value={o.lpCount2} onChange={(v) => set(() => (o.lpCount2 = v))} />
              <NumField label="Angle" angle value={o.lpAngle2} app={app} onChange={(v) => set(() => (o.lpAngle2 = v))} />
            </Group>
            {ents("Entities to pattern")}
          </>
        ),
      };
    case "circularPattern":
      return {
        title: "Circular Pattern",
        icon: "circularSketchPattern",
        body: (
          <>
            {Msg("Select the entities to pattern, the center point, and set the number of instances.")}
            <Group title="Parameters">
              <PointBox app={app} field="point1" label="Center (default: origin)" />
              <Chk label="Reverse direction" on={o.cpReverse} set={(v) => set(() => (o.cpReverse = v))} />
              <NumField label="Angle" angle value={o.cpAngle} app={app} onChange={(v) => set(() => (o.cpAngle = v))} />
              <Chk label="Equal spacing" on={o.cpEqual} set={(v) => set(() => (o.cpEqual = v))} />
              <IntField label="Number of instances" value={o.cpCount} onChange={(v) => set(() => (o.cpCount = v))} />
            </Group>
            {ents("Entities to pattern")}
          </>
        ),
      };
    case "extend":
      return { title: "Extend", icon: "extend", body: Msg("Click a line or arc near the end to extend it to the next entity.") };
    case "sketchChamfer":
      return {
        title: "Sketch Chamfer",
        icon: "sketchChamfer",
        body: (
          <>
            {Msg("Select a corner where two lines meet, or select two lines.")}
            <Group title="Chamfer Parameters">
              <label className="chk">
                <input type="radio" checked={o.chamferType === "angleDistance"} onChange={() => set(() => (o.chamferType = "angleDistance"))} /> Angle-distance
              </label>
              <label className="chk">
                <input type="radio" checked={o.chamferType === "distanceDistance"} onChange={() => set(() => (o.chamferType = "distanceDistance"))} /> Distance-distance
              </label>
              {o.chamferType === "distanceDistance" && <Chk label="Equal distance" on={o.chamferEqual} set={(v) => set(() => (o.chamferEqual = v))} />}
              <NumField label="Distance 1" value={o.chamferD1} app={app} onChange={(v) => set(() => (o.chamferD1 = Math.max(1e-4, v)))} />
              {o.chamferType === "distanceDistance" && !o.chamferEqual && <NumField label="Distance 2" value={o.chamferD2} app={app} onChange={(v) => set(() => (o.chamferD2 = Math.max(1e-4, v)))} />}
              {o.chamferType === "angleDistance" && <NumField label="Direction 1 angle" angle value={o.chamferAngle} app={app} onChange={(v) => set(() => (o.chamferAngle = v))} />}
            </Group>
          </>
        ),
      };
  }
  return null;
}
