/**
 * The Move Component / Rotate Component PropertyManager (G1b), laid out as SOLIDWORKS':
 * SmartMates, Move (Free Drag, Along Assembly XYZ, Along Entity, By Delta XYZ, To XYZ
 * Position), Rotate (Free Drag, About Entity, By Delta XYZ) and Options (Standard Drag;
 * Collision Detection and Physical Dynamics are not implemented yet). Dragging a component
 * moves it as the active group says, always within the freedom its mates leave.
 */
import type { App } from "../app/app";
import type { Vec3 } from "../core/types";
import { Group } from "../ui/pmWidgets";
import { Icon } from "../ui/icons";
import type { MovePM as MoveState } from "./session";

const MOVES: [MoveState["move"], string][] = [
  ["free", "Free Drag"],
  ["xyz", "Along Assembly XYZ"],
  ["entity", "Along Entity"],
  ["delta", "By Delta XYZ"],
  ["toXYZ", "To XYZ Position"],
];
const ROTS: [MoveState["rot"], string][] = [
  ["free", "Free Drag"],
  ["entity", "About Entity"],
  ["delta", "By Delta XYZ"],
];

/** Components selected for By Delta XYZ / To XYZ Position (tree rows or faces). */
export function selectedInstances(app: App): string[] {
  const s = app.asm;
  if (!s) return [];
  const ids = app.selection.flatMap((x) => (x.kind === "feature" ? [x.id] : "body" in x && x.body !== undefined ? [s.instanceOfOccurrence(x.body) ?? ""] : []));
  return [...new Set(ids)].filter((id) => s.doc.instances.some((i) => i.id === id));
}

export function MovePM({ app }: { app: App }) {
  const s = app.asm!;
  const pm = s.movePM!;
  const set = (f: () => void) => (f(), app.changed());
  const values = (labels: [string, string, string], unit: string) => (
    <div className="pm-xyz">
      {labels.map((l, k) => (
        <label key={k} className="pm-row">
          <span>{l}</span>
          <input
            type="number"
            data-move-value={k}
            value={pm.values[k]}
            onChange={(e) => set(() => (pm.values = pm.values.map((v, j) => (j === k ? Number(e.target.value) || 0 : v)) as Vec3))}
          />
          <span className="muted">{unit}</span>
        </label>
      ))}
      <button className="btn" data-move-apply="" disabled={!selectedInstances(app).length} onClick={() => void s.applyMove(selectedInstances(app))}>
        Apply
      </button>
    </div>
  );
  const entity = (what: string) => (
    <div
      className={`pm-field ${pm.pickingEntity ? "active" : ""}`}
      data-move-entity=""
      title={`Select ${what}`}
      onClick={() => set(() => (pm.pickingEntity = true))}
    >
      {pm.entity ? <div className="pm-item">{pm.entity.label}</div> : <div className="muted">Select {what}.</div>}
    </div>
  );
  const sel = selectedInstances(app).map((id) => s.doc.instances.find((i) => i.id === id)?.name);
  return (
    <div className="pm" data-move-pm="">
      <div className="pm-head">
        <Icon name={pm.rotate ? "rotateEntities" : "move"} size={18} />
        <span className="pm-title">{pm.rotate ? "Rotate Component" : "Move Component"}</span>
      </div>
      <div className="pm-buttons">
        <button title="OK" onClick={() => s.closeMove()}>
          <Icon name="check" size={18} />
        </button>
      </div>
      <Group title="Move">
        <div className="pm-row muted">SmartMates: Alt-drag a component onto a face or edge of another.</div>
        <select
          data-move-mode=""
          value={pm.move}
          onChange={(e) => set(() => ((pm.move = e.target.value as MoveState["move"]), (pm.rotate = false), (pm.pickingEntity = pm.move === "entity" && !pm.entity)))}
          onFocus={() => set(() => (pm.rotate = false))}
        >
          {MOVES.map(([k, l]) => (
            <option key={k} value={k}>
              {l}
            </option>
          ))}
        </select>
        {!pm.rotate && pm.move === "entity" && entity("a linear edge, an axis or a face")}
        {!pm.rotate && pm.move === "delta" && values(["ΔX", "ΔY", "ΔZ"], "mm")}
        {!pm.rotate && pm.move === "toXYZ" && values(["X", "Y", "Z"], "mm")}
      </Group>
      <Group title="Rotate">
        <select
          data-rotate-mode=""
          value={pm.rot}
          onChange={(e) => set(() => ((pm.rot = e.target.value as MoveState["rot"]), (pm.rotate = true), (pm.pickingEntity = pm.rot === "entity" && !pm.entity)))}
          onFocus={() => set(() => (pm.rotate = true))}
        >
          {ROTS.map(([k, l]) => (
            <option key={k} value={k}>
              {l}
            </option>
          ))}
        </select>
        {pm.rotate && pm.rot === "entity" && entity("an axis, a linear edge or a face")}
        {pm.rotate && pm.rot === "delta" && values(["ΔX", "ΔY", "ΔZ"], "deg")}
      </Group>
      <Group title="Options">
        <label className="pm-row">
          <input type="radio" checked readOnly /> Standard drag
        </label>
        <label className="pm-row muted" title="Not yet implemented">
          <input type="radio" disabled /> Collision Detection
        </label>
        <label className="pm-row muted" title="Not yet implemented">
          <input type="radio" disabled /> Physical Dynamics
        </label>
      </Group>
      <div className="pm-row muted">{sel.length ? `Selected: ${sel.join(", ")}` : "Drag a component, or select components for Apply."}</div>
    </div>
  );
}
