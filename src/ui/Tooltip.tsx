import { useEffect, useState } from "react";

interface Tip {
  x: number;
  y: number;
  title: string;
  desc?: string;
  key?: string;
  below: boolean;
}

/**
 * One tooltip layer for the whole app. Any element with `data-tip` (title),
 * optional `data-tip-desc` and `data-tip-key` gets a SOLIDWORKS-style tooltip:
 * bold name, shortcut, then a one-line description, after a short hover delay.
 */
export function TooltipLayer() {
  const [tip, setTip] = useState<Tip | null>(null);
  useEffect(() => {
    let timer = 0;
    let current: HTMLElement | null = null;
    const show = (el: HTMLElement) => {
      const r = el.getBoundingClientRect();
      const below = r.bottom + 90 < window.innerHeight;
      setTip({
        x: Math.min(r.left, window.innerWidth - 320),
        y: below ? r.bottom + 6 : r.top - 6,
        title: el.dataset.tip!,
        desc: el.dataset.tipDesc,
        key: el.dataset.tipKey,
        below,
      });
    };
    const over = (e: MouseEvent) => {
      const el = (e.target as HTMLElement).closest?.("[data-tip]") as HTMLElement | null;
      if (el === current) return;
      current = el;
      clearTimeout(timer);
      setTip(null);
      if (el) timer = window.setTimeout(() => show(el), 550);
    };
    const hide = () => {
      clearTimeout(timer);
      current = null;
      setTip(null);
    };
    window.addEventListener("mouseover", over);
    window.addEventListener("mousedown", hide, true);
    window.addEventListener("wheel", hide, true);
    window.addEventListener("keydown", hide, true);
    return () => {
      window.removeEventListener("mouseover", over);
      window.removeEventListener("mousedown", hide, true);
      window.removeEventListener("wheel", hide, true);
      window.removeEventListener("keydown", hide, true);
    };
  }, []);
  if (!tip) return null;
  return (
    <div className={`tip ${tip.below ? "" : "above"}`} style={{ left: tip.x, top: tip.y }}>
      <div className="tip-title">
        {tip.title}
        {tip.key && <span className="tip-key">({tip.key})</span>}
      </div>
      {tip.desc && <div className="tip-desc">{tip.desc}</div>}
    </div>
  );
}
