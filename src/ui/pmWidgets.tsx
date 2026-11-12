import { useEffect, useState, type ReactNode } from "react";
import type { App } from "../app/app";
import { formatLength, parseAngle, parseLength, UNIT_INFO } from "../core/units";

/** PropertyManager group box (collapsible, optionally with a checkbox in the title). */
export function Group({ title, children, checkable, checked, onCheck, collapsed, disabled }: { title: string; children?: ReactNode; checkable?: boolean; checked?: boolean; onCheck?: (on: boolean) => void; collapsed?: boolean; disabled?: boolean }) {
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
  // System Options > Spin Box Increments.
  const step = angle
    ? (Number(app.opt("spin.angle") ?? 1) * Math.PI) / 180
    : app.doc.units === "IPS"
      ? Number(app.opt("spin.english") ?? 0.1) * 25.4
      : Number(app.opt("spin.metric") ?? 10);
  return (
    <div className="numfield" title={label}>
      <span className="nf-label">{label}</span>
      <input
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={(e) => commit(e.target.value)}
        onKeyDown={(e) => {
          // Enter commits the value and then bubbles to the PropertyManager (Enter = OK).
          if (e.key === "Enter") commit((e.target as HTMLInputElement).value);
          else e.stopPropagation();
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
