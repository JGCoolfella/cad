import type { UnitSystem } from "./types";

/** Length units per unit system (SOLIDWORKS Document Properties > Units). */
export const UNIT_INFO: Record<UnitSystem, { length: string; perMm: number; decimals: number; label: string }> = {
  MMGS: { length: "mm", perMm: 1, decimals: 2, label: "MMGS (millimeter, gram, second)" },
  CGS: { length: "cm", perMm: 0.1, decimals: 2, label: "CGS (centimeter, gram, second)" },
  MKS: { length: "m", perMm: 0.001, decimals: 4, label: "MKS (meter, kilogram, second)" },
  IPS: { length: "in", perMm: 1 / 25.4, decimals: 3, label: "IPS (inch, pound, second)" },
};

const UNIT_TO_MM: Record<string, number> = {
  mm: 1,
  cm: 10,
  m: 1000,
  in: 25.4,
  '"': 25.4,
  ft: 304.8,
  "'": 304.8,
  um: 0.001,
};

export function formatLength(mm: number, units: UnitSystem, withUnit = false, decimals?: number, keepZeros = false) {
  const u = UNIT_INFO[units];
  const v = mm * u.perMm;
  const fixed = v.toFixed(decimals ?? u.decimals);
  const s = keepZeros ? fixed : trimZeros(fixed);
  return withUnit ? `${s}${u.length}` : s;
}

export function formatAngle(rad: number) {
  return `${trimZeros(((rad * 180) / Math.PI).toFixed(2))}°`;
}

function trimZeros(s: string) {
  if (!s.includes(".")) return s;
  return s.replace(/\.?0+$/, "");
}

/**
 * Parse a value typed in a numeric box. Like SOLIDWORKS, accepts arithmetic
 * ("10+5/2") and explicit units ("1in", "25.4mm") which override the document unit.
 * Returns millimetres (or radians when `angle` is set), or null if invalid.
 */
export function parseLength(text: string, units: UnitSystem): number | null {
  const t = text.trim().replace(/^=/, "");
  if (!t) return null;
  const m = /^(.*?)(mm|cm|m|in|ft|um|"|')?\s*$/i.exec(t);
  if (!m) return null;
  const unit = m[2]?.toLowerCase();
  const v = evalArithmetic(m[1]);
  if (v === null) return null;
  if (unit) return v * UNIT_TO_MM[unit];
  return v / UNIT_INFO[units].perMm;
}

export function parseAngle(text: string): number | null {
  const t = text.trim().replace(/^=/, "");
  const m = /^(.*?)(deg|°|rad)?\s*$/i.exec(t);
  if (!m) return null;
  const v = evalArithmetic(m[1]);
  if (v === null) return null;
  return m[2]?.toLowerCase() === "rad" ? v : (v * Math.PI) / 180;
}

/** Tiny safe arithmetic evaluator: + - * / ^ parentheses, pi, sqrt, sin/cos/tan (degrees). */
export function evalArithmetic(src: string): number | null {
  const s = src.replace(/\s+/g, "");
  let i = 0;
  const peek = () => s[i];
  function num(): number {
    if (peek() === "(") {
      i++;
      const v = expr();
      if (s[i] !== ")") throw new Error("paren");
      i++;
      return v;
    }
    if (peek() === "-") {
      i++;
      return -factor();
    }
    if (peek() === "+") {
      i++;
      return factor();
    }
    const fm = /^(sqrt|sin|cos|tan|abs)\(/.exec(s.slice(i));
    if (fm) {
      i += fm[1].length;
      const a = num();
      const d = (x: number) => (x * Math.PI) / 180;
      switch (fm[1]) {
        case "sqrt": return Math.sqrt(a);
        case "sin": return Math.sin(d(a));
        case "cos": return Math.cos(d(a));
        case "tan": return Math.tan(d(a));
        default: return Math.abs(a);
      }
    }
    if (s.slice(i, i + 2).toLowerCase() === "pi") {
      i += 2;
      return Math.PI;
    }
    const m = /^(\d+\.?\d*|\.\d+)(e[+-]?\d+)?/i.exec(s.slice(i));
    if (!m) throw new Error("num");
    i += m[0].length;
    return parseFloat(m[0]);
  }
  function factor(): number {
    let v = num();
    while (peek() === "^") {
      i++;
      v = Math.pow(v, num());
    }
    return v;
  }
  function term(): number {
    let v = factor();
    while (peek() === "*" || peek() === "/") {
      const op = s[i++];
      const r = factor();
      v = op === "*" ? v * r : v / r;
    }
    return v;
  }
  function expr(): number {
    let v = term();
    while (peek() === "+" || peek() === "-") {
      const op = s[i++];
      const r = term();
      v = op === "+" ? v + r : v - r;
    }
    return v;
  }
  try {
    const v = expr();
    if (i !== s.length || !isFinite(v)) return null;
    return v;
  } catch {
    return null;
  }
}
