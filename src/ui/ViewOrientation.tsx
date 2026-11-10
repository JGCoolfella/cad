import { useEffect, useRef, useState } from "react";
import type { App } from "../app/app";
import { CMD, runCommand, type Ctx } from "../commands/commands";

type Face = "front" | "back" | "left" | "right" | "top" | "bottom";

const FACE_POLY: Record<Face, string> = {
  front: "3,8 13,8 13,18 3,18",
  back: "8,3 18,3 18,13 8,13",
  top: "3,8 8,3 18,3 13,8",
  bottom: "3,18 8,13 18,13 13,18",
  left: "3,8 8,3 8,13 3,18",
  right: "13,8 18,3 18,13 13,18",
};

/** Small wireframe cube with one face highlighted (view orientation icons). */
export function CubeIcon({ face, iso, size = 20 }: { face?: Face; iso?: boolean; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 21 21" fill="none" strokeLinejoin="round">
      {iso && (
        <>
          <polygon points={FACE_POLY.top} fill="#9cc3f5" />
          <polygon points={FACE_POLY.front} fill="#4a8fe0" />
          <polygon points={FACE_POLY.right} fill="#2f6dbf" />
        </>
      )}
      {face && <polygon points={FACE_POLY[face]} fill="#3f8ae6" opacity={face === "back" || face === "bottom" || face === "left" ? 0.75 : 0.95} />}
      {/* hidden edges */}
      <path d="M8 13 L3 18 M8 13 L18 13 M8 13 L8 3" stroke="#7a7f88" strokeWidth="0.8" strokeDasharray="1.4 1.2" />
      {/* visible edges */}
      <path d="M3 8 L13 8 L13 18 L3 18 Z M3 8 L8 3 L18 3 L13 8 M18 3 L18 13 L13 18" stroke="#2b2f36" strokeWidth="1" />
    </svg>
  );
}

function NormalToIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 21 21" fill="none">
      <path d="M2 15 L7 11 H19 L14 15 Z" fill="#c9daf3" stroke="#2f6dbf" />
      <path d="M10.5 13 V3 M7.5 6 L10.5 3 L13.5 6" stroke="#2b2f36" strokeWidth="1.3" />
    </svg>
  );
}

function ViewportIcon({ split }: { split: "single" | "h" | "v" | "four" }) {
  return (
    <svg width="20" height="20" viewBox="0 0 21 21" fill="none">
      <rect x="2.5" y="3.5" width="16" height="14" fill="#fff" stroke="#2b2f36" />
      <rect x="2.5" y="3.5" width="16" height="3" fill="#4a8fe0" />
      {(split === "h" || split === "four") && <path d="M2.5 12 H18.5" stroke="#2b2f36" />}
      {(split === "v" || split === "four") && <path d="M10.5 6.5 V17.5" stroke="#2b2f36" />}
    </svg>
  );
}

function SelectorIcon({ on }: { on: boolean }) {
  return (
    <svg width="20" height="20" viewBox="0 0 21 21" fill="none">
      <path d="M4 7 L7 4 H14 L17 7 V14 L14 17 H7 L4 14 Z" fill={on ? "#cfe0fb" : "#eef0f3"} stroke="#2b2f36" />
      <path d="M7 7 H14 V14 H7 Z" fill="#3f8ae6" opacity="0.85" stroke="#2b2f36" strokeWidth="0.8" />
    </svg>
  );
}

function NewViewIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 21 21" fill="none">
      <path d="M3 18 L13 8" stroke="#7a5a2a" strokeWidth="1.8" />
      <path d="M14 3 L15 6 L18 7 L15 8 L14 11 L13 8 L10 7 L13 6 Z" fill="#f4c430" stroke="#a07a00" strokeWidth="0.6" />
    </svg>
  );
}

const STANDARD = [
  ["view.front", "Front"],
  ["view.back", "Back"],
  ["view.left", "Left"],
  ["view.right", "Right"],
  ["view.top", "Top"],
  ["view.bottom", "Bottom"],
  ["view.isometric", "Isometric"],
  ["view.trimetric", "Trimetric"],
  ["view.dimetric", "Dimetric"],
] as const;

/** Heads-up toolbar View Orientation flyout (same layout as SOLIDWORKS). */
export function ViewOrientationFlyout({ app, ctx, onClose }: { app: App; ctx: Ctx; onClose: () => void }) {
  const [more, setMore] = useState(false);
  const [isoMenu, setIsoMenu] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const selectorOn = app.opt<boolean>("view.viewSelector") !== false;
  useEffect(() => {
    ctx.vp?.showSelector(selectorOn);
    if (ctx.vp) ctx.vp.onSelectorPick = onClose;
    return () => {
      ctx.vp?.showSelector(false);
      if (ctx.vp) ctx.vp.onSelectorPick = null;
    };
  }, [selectorOn, ctx.vp, onClose]);
  useEffect(() => {
    const close = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      if (ref.current?.contains(t)) return;
      // Clicks in the graphics area go to the view selector first; it closes us on a pick.
      if (t.classList.contains("overlay") && ctx.vp?.selector.hover) return;
      onClose();
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    const tm = setTimeout(() => window.addEventListener("mousedown", close), 0);
    window.addEventListener("keydown", key);
    return () => {
      clearTimeout(tm);
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", key);
    };
  }, [onClose, ctx.vp]);
  const go = (id: string) => {
    runCommand(id, ctx);
    onClose();
  };
  const B = ({ id, children, title }: { id?: string; children: React.ReactNode; title: string }) => (
    <button className="vo-btn" data-tip={title} data-tip-key={id ? CMD[id]?.shortcut : undefined} onClick={() => id && go(id)} disabled={!id}>
      {children}
    </button>
  );
  return (
    <div className="vo-flyout" ref={ref} onMouseDown={(e) => e.stopPropagation()}>
      <div className="vo-grid">
        <span />
        <span />
        <span />
        <button className="vo-btn" data-tip="New View" data-tip-desc="Saves the current view orientation with a name (coming soon)." disabled>
          <NewViewIcon />
        </button>
        <button className={`vo-btn ${selectorOn ? "on" : ""}`} data-tip="View Selector" data-tip-desc="Shows a cube around the model to select standard and non-standard orientations." data-tip-key="Ctrl+Space" onClick={() => app.setOpt("view.viewSelector", !selectorOn)}>
          <SelectorIcon on={selectorOn} />
        </button>
        <button className={`vo-btn more ${more ? "on" : ""}`} data-tip="More" data-tip-desc="Shows the list of named views." onClick={() => setMore(!more)}>
          »
        </button>

        <span />
        <B id="view.top" title="Top">
          <CubeIcon face="top" />
        </B>
        <span />
        <span className="vo-iso">
          <B id="view.isometric" title="Isometric">
            <CubeIcon iso />
          </B>
          <button className="vo-drop" onClick={() => setIsoMenu(!isoMenu)}>
            ▾
          </button>
          {isoMenu && (
            <div className="flymenu vo-isomenu">
              {(["view.isometric", "view.dimetric", "view.trimetric"] as const).map((id) => (
                <div key={id} className="mi" onClick={() => go(id)}>
                  <span className="mi-icon">
                    <CubeIcon iso size={16} />
                  </span>
                  {CMD[id].label}
                </div>
              ))}
            </div>
          )}
        </span>
        <span />
        <span />

        <B id="view.left" title="Left">
          <CubeIcon face="left" />
        </B>
        <B id="view.front" title="Front">
          <CubeIcon face="front" />
        </B>
        <B id="view.right" title="Right">
          <CubeIcon face="right" />
        </B>
        <B id="view.back" title="Back">
          <CubeIcon face="back" />
        </B>
        <span />
        <span />

        <span />
        <B id="view.bottom" title="Bottom">
          <CubeIcon face="bottom" />
        </B>
        <span />
        <B id="view.normalTo" title="Normal To">
          <NormalToIcon />
        </B>
        <span />
        <span />
      </div>
      <div className="vo-sep" />
      <div className="vo-row">
        <B title="Single View">
          <ViewportIcon split="single" />
        </B>
        <B title="Two View - Horizontal">
          <ViewportIcon split="h" />
        </B>
        <B title="Two View - Vertical">
          <ViewportIcon split="v" />
        </B>
        <B title="Four View">
          <ViewportIcon split="four" />
        </B>
        <B title="Link Views">
          <svg width="20" height="20" viewBox="0 0 21 21" fill="none">
            <rect x="3" y="6" width="7" height="7" stroke="#9aa0a8" />
            <rect x="11" y="8" width="7" height="7" stroke="#9aa0a8" />
          </svg>
        </B>
      </div>
      {more && (
        <div className="vo-more">
          {STANDARD.map(([id, l]) => (
            <div key={id} className="mi" onClick={() => go(id)}>
              *{l}
            </div>
          ))}
          <div className="msep" />
          <div className="mi disabled">Update Standard Views</div>
          <div className="mi disabled">Reset Standard Views</div>
        </div>
      )}
    </div>
  );
}

/** Spacebar Orientation dialog: view list + buttons, with the View Selector shown. */
export function OrientationDialog({ app, ctx, onClose }: { app: App; ctx: Ctx; onClose: () => void }) {
  const [pinned, setPinned] = useState(false);
  const selectorOn = app.opt<boolean>("view.viewSelector") !== false;
  const pinnedRef = useRef(pinned);
  pinnedRef.current = pinned;
  useEffect(() => {
    ctx.vp?.showSelector(selectorOn);
    if (ctx.vp) ctx.vp.onSelectorPick = () => !pinnedRef.current && onClose();
    return () => {
      ctx.vp?.showSelector(false);
      if (ctx.vp) ctx.vp.onSelectorPick = null;
    };
  }, [selectorOn, ctx.vp, onClose]);
  const go = (id: string) => {
    runCommand(id, ctx);
    if (!pinned) onClose();
  };
  return (
    <div className="orient" onMouseDown={(e) => e.stopPropagation()}>
      <div className="orient-head">
        Orientation
        <span className="grow" />
        <button className={pinned ? "on" : ""} data-tip="Pin/Unpin the dialog" onClick={() => setPinned(!pinned)}>
          📌
        </button>
        <button onClick={onClose} data-tip="Close">
          ✕
        </button>
      </div>
      <div className="orient-tools">
        <button data-tip="New View" disabled>
          <NewViewIcon />
        </button>
        <button data-tip="Update Standard Views" disabled>
          <CubeIcon face="front" size={18} />
        </button>
        <button data-tip="Reset Standard Views" onClick={() => go("view.isometric")}>
          <CubeIcon iso size={18} />
        </button>
        <span className="grow" />
        <button className={selectorOn ? "on" : ""} data-tip="View Selector" data-tip-key="Ctrl+Space" onClick={() => app.setOpt("view.viewSelector", !selectorOn)}>
          <SelectorIcon on={selectorOn} />
        </button>
      </div>
      <div className="orient-list">
        {STANDARD.map(([id, l]) => (
          <div key={id} className="mi" onClick={() => go(id)}>
            *{l}
            <span className="sc">{CMD[id].shortcut}</span>
          </div>
        ))}
        <div className="mi" onClick={() => go("view.normalTo")}>
          Normal To <span className="sc">Ctrl+8</span>
        </div>
      </div>
    </div>
  );
}
