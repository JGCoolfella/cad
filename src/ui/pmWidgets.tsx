import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import type { App } from "../app/app";
import { formatLength, parseAngle, parseLength, UNIT_INFO } from "../core/units";
import { DISARMED, enterStep } from "./enterKey";

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

/**
 * Keys in a PropertyManager value box: the first Enter applies the value and stays in the
 * box, the next Enter (no typing in between) reaches the PropertyManager as OK
 * (`enterKey.ts`). Other keys stay in the box. Call `reset` when the user types, and spread
 * `events` on the input.
 */
export function useValueBoxKeys(apply: (e: KeyboardEvent<HTMLInputElement>) => void) {
  const state = useRef(DISARMED);
  const reset = () => {
    state.current = DISARMED;
  };
  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    const step = enterStep(state.current, e.key);
    state.current = step.next;
    if (step.apply) apply(e);
    if (step.propagate) return;
    e.stopPropagation();
    if (e.key === "Enter") e.preventDefault();
  };
  return { reset, events: { onKeyDown, onFocus: reset, onBlur: reset } };
}

/** Numeric box: accepts expressions and units; mouse wheel spins the value. */
export function NumField({ label, value, onChange, app, angle }: { label: string; value: number; onChange: (v: number) => void; app: App; angle?: boolean }) {
  const fmt = (v: number) => (angle ? `${+((v * 180) / Math.PI).toFixed(2)}deg` : `${formatLength(v, app.doc.units)}${UNIT_INFO[app.doc.units].length}`);
  const [text, setText] = useState(fmt(value));
  useEffect(() => setText(fmt(value)), [value, app.doc.units]);
  const keys = useValueBoxKeys((e) => commit((e.target as HTMLInputElement).value));
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
        onChange={(e) => {
          keys.reset();
          setText(e.target.value);
        }}
        onFocus={keys.events.onFocus}
        onBlur={(e) => {
          keys.reset();
          commit(e.target.value);
        }}
        onKeyDown={keys.events.onKeyDown}
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
