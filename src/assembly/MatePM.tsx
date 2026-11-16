/**
 * The Mate PropertyManager (G1a): Entities to Mate (two picks in the graphics area), the
 * Standard Mates, the distance or angle, and Mate Alignment. The components move as the
 * mate is set up (the assembly is solved with it); OK adds it. SOLIDWORKS also shows a
 * pop-up Mate toolbar next to the pointer; that, Advanced and Mechanical mates are G1b.
 */
import type { App } from "../app/app";
import { Group, NumField } from "../ui/pmWidgets";
import { Icon } from "../ui/icons";
import { MATE_LABEL, type MateType } from "./types";

const STANDARD: [MateType, string][] = [
  ["coincident", "rel:coincident"],
  ["parallel", "rel:parallel"],
  ["perpendicular", "rel:perpendicular"],
  ["tangent", "rel:tangent"],
  ["concentric", "rel:concentric"],
  ["lock", "rel:fix"],
  ["distance", "dimH"],
  ["angle", "smartDimension"],
];

export function MatePM({ app }: { app: App }) {
  const s = app.asm!;
  const pm = s.matePM!;
  const touch = () => {
    void s.preview();
    app.changed();
  };
  const ready = pm.picks.length === 2;
  return (
    <div
      className="pm"
      onKeyDown={(e) => {
        if (e.key === "Enter" && ready && (e.target as HTMLElement).tagName !== "BUTTON") {
          e.preventDefault();
          void s.acceptMate().then(() => app.changed());
        }
      }}
    >
      <div className="pm-head">
        <Icon name="mate" size={18} />
        <span className="pm-title">{pm.editing ? s.doc.mates.find((m) => m.id === pm.editing)?.name ?? "Mate" : "Mate"}</span>
      </div>
      <div className="pm-buttons">
        <button title="OK (Enter)" disabled={!ready} onClick={() => void s.acceptMate().then(() => app.changed())}>
          <Icon name="check" size={18} />
        </button>
        <button title="Cancel (Esc)" onClick={() => (s.cancelMate(), app.changed())}>
          <Icon name="cancel" size={18} />
        </button>
      </div>
      <Group title="Mate Selections">
        <div className="pm-field active" data-mate-picks="">
          {pm.picks.length ? (
            pm.picks.map((p, i) => (
              <div key={i} className="pm-item">
                {p.label}
              </div>
            ))
          ) : (
            <div className="muted">Select two faces, edges or vertices on different components.</div>
          )}
          {pm.busy && <div className="muted">…</div>}
        </div>
      </Group>
      <Group title="Standard Mates">
        {STANDARD.map(([t, icon]) => (
          <label key={t} className="pm-row" data-mate-type={t}>
            <input type="radio" name="mate-type" checked={pm.type === t} onChange={() => ((pm.type = t), touch())} />
            <Icon name={icon} size={16} />
            <span>{MATE_LABEL[t]}</span>
          </label>
        ))}
        {(pm.type === "distance" || pm.type === "angle") && (
          <div className="pm-row">
            <NumField label={pm.type === "angle" ? "Angle" : "Distance"} angle={pm.type === "angle"} value={pm.value} app={app} onChange={(v) => ((pm.value = Math.max(0, v)), touch())} />
            <label title="Flip Dimension">
              <input type="checkbox" checked={pm.flip} onChange={(e) => ((pm.flip = e.target.checked), touch())} /> Flip dimension
            </label>
          </div>
        )}
        {pm.type !== "lock" && pm.type !== "perpendicular" && (
          <div className="pm-row seg" title="Mate alignment">
            <button className={`tbtn ${pm.align === "aligned" ? "on" : ""}`} data-align="aligned" title="Aligned" onClick={() => ((pm.align = "aligned"), touch())}>
              ⇈
            </button>
            <button className={`tbtn ${pm.align === "antiAligned" ? "on" : ""}`} data-align="antiAligned" title="Anti-Aligned" onClick={() => ((pm.align = "antiAligned"), touch())}>
              ⇅
            </button>
          </div>
        )}
      </Group>
      <Group title="Advanced Mates" collapsed disabled />
      <Group title="Mechanical Mates" collapsed disabled />
    </div>
  );
}
