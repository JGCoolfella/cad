/**
 * The Mate PropertyManager (G1a, G1b): Mate Selections (picks in the graphics area or in
 * the fly-out FeatureManager), the Standard Mates, the distance or angle, and Mate
 * Alignment. The components move as the mate is set up (the assembly is solved with it);
 * OK adds it. Editing a mate lists a reference that no longer resolves as **Missing**: the
 * next pick replaces it, and clicking an entry re-picks it (as SOLIDWORKS).
 *
 * `MatePopup` is SOLIDWORKS' Mate pop-up toolbar: once two entities are picked it opens
 * next to the pointer with the proposed mate, the other mates the pair allows (the rest
 * greyed), Flip Mate Alignment, the distance or angle, OK and Cancel.
 */
import type { App } from "../app/app";
import { Group, NumField } from "../ui/pmWidgets";
import { Icon } from "../ui/icons";
import { MATE_LABEL, type MateType } from "./types";

export const MATE_ICONS: [MateType, string][] = [
  ["coincident", "rel:coincident"],
  ["parallel", "rel:parallel"],
  ["perpendicular", "rel:perpendicular"],
  ["tangent", "rel:tangent"],
  ["concentric", "rel:concentric"],
  ["lock", "rel:fix"],
  ["distance", "dimH"],
  ["angle", "smartDimension"],
];

const accept = (app: App) => void app.asm!.acceptMate().then(() => app.changed());

export function MatePM({ app }: { app: App }) {
  const s = app.asm!;
  const pm = s.matePM!;
  const touch = () => {
    void s.preview();
    app.changed();
  };
  const ready = pm.picks.length === 2 && !pm.picks.some((p) => p.missing) && !pm.problem;
  const valid = s.validTypes();
  return (
    <div
      className="pm"
      onKeyDown={(e) => {
        if (e.key === "Enter" && ready && (e.target as HTMLElement).tagName !== "BUTTON") {
          e.preventDefault();
          accept(app);
        }
      }}
    >
      <div className="pm-head">
        <Icon name="mate" size={18} />
        <span className="pm-title">{pm.editing ? (s.doc.mates.find((m) => m.id === pm.editing)?.name ?? "Mate") : "Mate"}</span>
      </div>
      <div className="pm-buttons">
        <button title="OK (Enter)" data-mate-ok="" disabled={!ready} onClick={() => accept(app)}>
          <Icon name="check" size={18} />
        </button>
        <button title="Cancel (Esc)" onClick={() => (s.cancelMate(), app.changed())}>
          <Icon name="cancel" size={18} />
        </button>
      </div>
      {pm.problem && (
        <div className="pm-message error" data-mate-problem="">
          <Icon name="error" size={14} /> {pm.problem}
        </div>
      )}
      {pm.picks.some((p) => p.missing) && (
        <div className="pm-message" data-mate-missing="">
          <Icon name="warning" size={14} /> A reference of this mate is missing. Select an entity to replace it.
        </div>
      )}
      <Group title="Mate Selections">
        <div className="pm-field active" data-mate-picks="">
          {pm.picks.length ? (
            pm.picks.map((p, i) => (
              <div
                key={i}
                className={`pm-item ${pm.active === i ? "sel" : ""} ${p.missing ? "missing" : ""}`}
                data-mate-pick={i}
                title="Click, then select an entity to replace this one"
                onClick={() => s.setActivePick(pm.active === i ? null : i)}
              >
                {p.label}
                <span className="pm-item-x" title="Delete" onClick={(e) => (e.stopPropagation(), s.removePick(i))}>
                  ×
                </span>
              </div>
            ))
          ) : (
            <div className="muted">Select two faces, edges, vertices or planes on different components.</div>
          )}
          {pm.busy && <div className="muted">…</div>}
        </div>
      </Group>
      <Group title="Standard Mates">
        {MATE_ICONS.map(([t, icon]) => (
          <label key={t} className={`pm-row ${valid.length && !valid.includes(t) ? "muted" : ""}`} data-mate-type={t}>
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

/** The Mate pop-up toolbar, at graphics-area position (`x`, `y`). */
export function MatePopup({ app, x, y }: { app: App; x: number; y: number }) {
  const s = app.asm!;
  const pm = s.matePM!;
  const valid = s.validTypes();
  const touch = () => {
    void s.preview();
    app.changed();
  };
  const ready = !pm.problem && !pm.busy;
  const hasAlign = pm.type !== "lock" && pm.type !== "perpendicular";
  return (
    <div className="mate-popup" style={{ left: x + 16, top: y + 16 }} data-mate-popup="" onPointerDown={(e) => e.stopPropagation()}>
      {MATE_ICONS.map(([t, icon]) => {
        const ok = valid.includes(t);
        return (
          <button
            key={t}
            className={`ctb ${pm.type === t ? "on" : ""} ${ok ? "" : "disabled"}`}
            data-popup-type={t}
            data-tip={MATE_LABEL[t]}
            title={MATE_LABEL[t]}
            disabled={!ok}
            onClick={() => ((pm.type = t), touch())}
          >
            <Icon name={icon} size={18} />
          </button>
        );
      })}
      {(pm.type === "distance" || pm.type === "angle") && (
        <span className="mate-popup-value">
          <NumField label="" angle={pm.type === "angle"} value={pm.value} app={app} onChange={(v) => ((pm.value = Math.max(0, v)), touch())} />
        </span>
      )}
      {hasAlign && (
        <button
          className="ctb"
          data-popup-flip=""
          title="Flip Mate Alignment"
          onClick={() => ((pm.align = pm.align === "aligned" ? "antiAligned" : "aligned"), touch())}
        >
          {pm.align === "aligned" ? "⇈" : "⇅"}
        </button>
      )}
      <span className="mate-popup-sep" />
      <button className={`ctb ${ready ? "" : "disabled"}`} data-popup-ok="" title="Add/Finish Mate" disabled={!ready} onClick={() => accept(app)}>
        <Icon name="check" size={18} />
      </button>
      <button className="ctb" data-popup-cancel="" title="Cancel" onClick={() => ((pm.popupClosed = true), (pm.picks = []), (pm.problem = null), touch())}>
        <Icon name="cancel" size={18} />
      </button>
    </div>
  );
}
