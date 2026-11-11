import { useEffect, useRef, useState } from "react";
import type { App } from "../app/app";
import { CMD, runCommand, type Ctx } from "../commands/commands";
import { formatLength, parseAngle, parseLength, UNIT_INFO } from "../core/units";
import { Icon } from "./icons";

/** Confirmation corner (top-right of the graphics area). */
export function ConfirmationCorner({ app, ctx }: { app: App; ctx: Ctx }) {
  void ctx;
  if (app.pm && "feature" in app.pm) {
    return (
      <div className="confirm-corner">
        <button title="OK" onClick={() => app.acceptPM()}>
          <Icon name="check" size={22} />
        </button>
        <button title="Cancel" onClick={() => app.cancelPM()}>
          <Icon name="cancel" size={22} />
        </button>
      </div>
    );
  }
  if (app.sketch) {
    return (
      <div className="confirm-corner">
        <button title="Exit Sketch" onClick={() => app.exitSketch(true)}>
          <Icon name="exitSketch" size={24} />
        </button>
        <button title="Discard Changes and Exit" onClick={() => app.exitSketch(false)}>
          <Icon name="cancel" size={22} />
        </button>
      </div>
    );
  }
  return null;
}

/** "Modify" box for editing a dimension value. */
export function ModifyBox({ app }: { app: App }) {
  const m = app.modify!;
  const d = app.sketch?.data.dimensions[m.dimId];
  const isAngle = d?.kind === "angle";
  const fmt = (v: number) => (isAngle ? `${+((v * 180) / Math.PI).toFixed(4)}` : formatLength(v, app.doc.units));
  const [text, setText] = useState(d ? fmt(d.value) : "");
  const ref = useRef<HTMLInputElement>(null);
  const orig = useRef(d?.value ?? 0);
  // Type straight away: focus the value (after the click that opened the box has finished,
  // so the graphics area cannot take focus back).
  useEffect(() => {
    const go = () => {
      ref.current?.focus({ preventScroll: true });
      ref.current?.select();
    };
    go();
    const t = window.setTimeout(go, 0);
    const r = requestAnimationFrame(go);
    return () => {
      clearTimeout(t);
      cancelAnimationFrame(r);
    };
  }, [m.dimId]);
  // Changing an existing dimension is one sketch undo step (a new one is undone with its creation).
  const recorded = useRef(m.isNew);
  if (!d) return null;
  const parse = (t: string) => (isAngle ? parseAngle(t) : parseLength(t, app.doc.units));
  const apply = (t: string) => {
    const v = parse(t);
    if (v === null || (v <= 0 && !isAngle)) {
      app.flash("Please enter a valid positive value.", "error");
      return false;
    }
    if (Math.abs(v - d.value) < 1e-12) return true;
    if (!recorded.current) {
      app.sketchCheckpoint();
      recorded.current = true;
    }
    return app.editor!.setDimensionValue(m.dimId, v);
  };
  const close = () => {
    app.modify = null;
    app.changed();
  };
  const ok = () => {
    if (apply(text)) close();
  };
  const cancel = () => {
    if (Math.abs(d.value - orig.current) > 1e-12) app.editor!.setDimensionValue(m.dimId, orig.current);
    close();
  };
  const step = isAngle ? Math.PI / 180 : 1 / UNIT_INFO[app.doc.units].perMm;
  const spin = (dir: number) => {
    const v = (parse(text) ?? d.value) + dir * step;
    setText(fmt(v));
    if (!recorded.current) {
      app.sketchCheckpoint();
      recorded.current = true;
    }
    app.editor!.setDimensionValue(m.dimId, v);
  };
  return (
    <div className="modify" style={{ left: m.x, top: m.y }} onMouseDown={(e) => e.stopPropagation()}>
      <div className="modify-head">
        Modify <span className="muted">{d.name}</span>
      </div>
      <div className="modify-tools">
        <button title="Save the current value and exit the dialog" onClick={ok}>
          <Icon name="check" size={14} />
        </button>
        <button title="Restore the original value and exit the dialog" onClick={cancel}>
          <Icon name="cancel" size={14} />
        </button>
        <button title="Regenerate the model with the current value" onClick={() => apply(text)}>
          <Icon name="rebuild" size={14} />
        </button>
        <button title="Reverse the sense of the dimension" disabled>
          ±
        </button>
      </div>
      <div className="modify-input">
        <input
          ref={ref}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") ok();
            if (e.key === "Escape") cancel();
            if (e.key === "ArrowUp") spin(1);
            if (e.key === "ArrowDown") spin(-1);
          }}
          onWheel={(e) => spin(e.deltaY < 0 ? 1 : -1)}
        />
        <span className="unit">{isAngle ? "deg" : UNIT_INFO[app.doc.units].length}</span>
      </div>
    </div>
  );
}

/** Spacebar Orientation dialog with standard views. */
export function OrientationMenu({ ctx, onClose }: { ctx: Ctx; onClose: () => void }) {
  const views: [string, string][] = [
    ["view.front", "Front"],
    ["view.back", "Back"],
    ["view.left", "Left"],
    ["view.right", "Right"],
    ["view.top", "Top"],
    ["view.bottom", "Bottom"],
    ["view.isometric", "Isometric"],
    ["view.trimetric", "Trimetric"],
    ["view.dimetric", "Dimetric"],
    ["view.normalTo", "Normal To"],
  ];
  return (
    <div className="orient" onMouseDown={(e) => e.stopPropagation()}>
      <div className="orient-head">
        Orientation
        <button onClick={onClose}>✕</button>
      </div>
      <ViewCube ctx={ctx} onPick={onClose} />
      <div className="orient-grid">
        {views.map(([id, label]) => (
          <button
            key={id}
            onClick={() => {
              runCommand(id, ctx);
              onClose();
            }}
            title={CMD[id].shortcut}
          >
            {label}
          </button>
        ))}
      </div>
    </div>
  );
}

/** Simple view selector cube: click a face to orient. */
function ViewCube({ ctx, onPick }: { ctx: Ctx; onPick: () => void }) {
  const faces: [string, string, number, number][] = [
    ["view.top", "Top", 40, 4],
    ["view.left", "Left", 4, 40],
    ["view.front", "Front", 40, 40],
    ["view.right", "Right", 76, 40],
    ["view.back", "Back", 112, 40],
    ["view.bottom", "Bottom", 40, 76],
  ];
  return (
    <div className="cube">
      {faces.map(([id, l, x, y]) => (
        <button key={id} style={{ left: x, top: y }} onClick={() => (runCommand(id, ctx), onPick())}>
          {l}
        </button>
      ))}
    </div>
  );
}

/** Mouse gesture guide (right-drag). Directions: 0 right, 1 up, 2 left, 3 down. */
export function GestureWheel({ x, y, dir, labels }: { x: number; y: number; dir: number | null; labels: string[] }) {
  const pos = [
    [60, 0],
    [0, -50],
    [-60, 0],
    [0, 50],
  ];
  const host = document.querySelector(".gl-host")?.getBoundingClientRect();
  const ox = (host?.left ?? 0) + x, oy = (host?.top ?? 0) + y;
  return (
    <div className="gesture" style={{ left: ox, top: oy }}>
      <div className="g-center" />
      {labels.map((l, i) => (
        <div key={i} className={`g-item ${dir === i ? "on" : ""}`} style={{ transform: `translate(calc(-50% + ${pos[i][0]}px), calc(-50% + ${pos[i][1]}px))` }}>
          {l}
        </div>
      ))}
    </div>
  );
}

/** Shortcut bar ("S" key): context-sensitive commands at the cursor. */
export function ShortcutBar({ x, y, ctx, onClose }: { x: number; y: number; ctx: Ctx; onClose: () => void }) {
  const ids = ctx.app.sketch
    ? ["sketch.line", "sketch.rectangle", "sketch.circle", "sketch.arc", "sketch.centerline", "sketch.point", "sketch.smartDimension", "sketch.trim", "sketch.relations", "sketch.exit"]
    : ["sketch.sketch", "feat.extrude", "feat.revolve", "feat.cutExtrude", "feat.cutRevolve", "feat.fillet", "feat.chamfer", "feat.plane", "view.zoomFit", "view.normalTo"];
  useDismiss(onClose);
  return (
    <div className="shortcutbar" style={{ left: x - 20, top: y - 20 }} onMouseDown={(e) => e.stopPropagation()}>
      {ids.map((id) => (
        <button key={id} title={CMD[id].label} onClick={() => (onClose(), runCommand(id, ctx))}>
          <Icon name={CMD[id].icon ?? "generic"} size={20} />
        </button>
      ))}
    </div>
  );
}

/** Close on outside mousedown or Escape. Uses a ref so re-renders never drop the listener. */
function useDismiss(onClose: () => void) {
  const ref = useRef(onClose);
  ref.current = onClose;
  useEffect(() => {
    const close = () => ref.current();
    const key = (e: KeyboardEvent) => e.key === "Escape" && ref.current();
    const t = setTimeout(() => window.addEventListener("mousedown", close), 0);
    window.addEventListener("keydown", key, true);
    return () => {
      clearTimeout(t);
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", key, true);
    };
  }, []);
}
