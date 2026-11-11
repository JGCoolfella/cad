/**
 * Equations and global variables (Tools > Equations). Expressions follow SOLIDWORKS:
 * names in double quotes ("Width", "D1@Sketch1"), + - * / ^, parentheses, and functions
 * sin cos tan asin acos atan (degrees), sqrt abs int sgn exp log pi. Lengths are in the
 * document's length unit; angles in degrees.
 */
import type { Dimension, Feature, PartDocument } from "./types";
import { featureParams, setFeatureParam } from "./featureParams";

export interface GlobalVariable {
  name: string;
  expr: string;
}

export interface Equation {
  /** Dimension driven by the equation, e.g. "D1@Sketch1". */
  target: string;
  expr: string;
}

type Tok = { t: "num"; v: number } | { t: "name"; v: string } | { t: "id"; v: string } | { t: "op"; v: string };

function tokenize(s: string): Tok[] {
  const out: Tok[] = [];
  let i = 0;
  while (i < s.length) {
    const c = s[i];
    if (/\s/.test(c)) {
      i++;
    } else if (c === '"') {
      const j = s.indexOf('"', i + 1);
      if (j < 0) throw new Error("Missing closing quote");
      out.push({ t: "name", v: s.slice(i + 1, j) });
      i = j + 1;
    } else if (/[0-9.]/.test(c)) {
      const m = /^[0-9]*\.?[0-9]+(e[+-]?[0-9]+)?|^[0-9]+\.?/i.exec(s.slice(i))!;
      out.push({ t: "num", v: parseFloat(m[0]) });
      i += m[0].length;
    } else if (/[a-z_]/i.test(c)) {
      const m = /^[a-z_][a-z0-9_]*/i.exec(s.slice(i))!;
      out.push({ t: "id", v: m[0].toLowerCase() });
      i += m[0].length;
    } else if ("+-*/^(),".includes(c)) {
      out.push({ t: "op", v: c });
      i++;
    } else throw new Error(`Unexpected "${c}"`);
  }
  return out;
}

const D2R = Math.PI / 180;
const FUNCS: Record<string, (x: number) => number> = {
  sin: (x) => Math.sin(x * D2R),
  cos: (x) => Math.cos(x * D2R),
  tan: (x) => Math.tan(x * D2R),
  asin: (x) => Math.asin(x) / D2R,
  acos: (x) => Math.acos(x) / D2R,
  atan: (x) => Math.atan(x) / D2R,
  sqrt: Math.sqrt,
  abs: Math.abs,
  int: Math.trunc,
  sgn: Math.sign,
  exp: Math.exp,
  log: Math.log,
};

/** Evaluate an expression; `lookup` resolves quoted names. */
export function evaluate(expr: string, lookup: (name: string) => number): number {
  const toks = tokenize(expr.replace(/^\s*=/, ""));
  let i = 0;
  const peek = () => toks[i];
  const take = (v?: string) => {
    const t = toks[i++];
    if (!t || (v && !(t.t === "op" && t.v === v))) throw new Error(v ? `Expected "${v}"` : "Unexpected end");
    return t;
  };
  const primary = (): number => {
    const t = take();
    if (t.t === "num") return t.v;
    if (t.t === "name") return lookup(t.v);
    if (t.t === "op" && t.v === "(") {
      const v = sum();
      take(")");
      return v;
    }
    if (t.t === "op" && t.v === "-") return -power();
    if (t.t === "op" && t.v === "+") return power();
    if (t.t === "id") {
      if (t.v === "pi") return Math.PI;
      const f = FUNCS[t.v];
      if (!f) throw new Error(`Unknown function ${t.v}`);
      take("(");
      const v = sum();
      take(")");
      return f(v);
    }
    throw new Error("Syntax error");
  };
  const power = (): number => {
    const b = primary();
    const t = peek();
    if (t?.t === "op" && t.v === "^") {
      i++;
      return Math.pow(b, power());
    }
    return b;
  };
  const product = (): number => {
    let v = power();
    for (let t = peek(); t?.t === "op" && (t.v === "*" || t.v === "/"); t = peek()) {
      i++;
      v = t.v === "*" ? v * power() : v / power();
    }
    return v;
  };
  const sum = (): number => {
    let v = product();
    for (let t = peek(); t?.t === "op" && (t.v === "+" || t.v === "-"); t = peek()) {
      i++;
      v = t.v === "+" ? v + product() : v - product();
    }
    return v;
  };
  const v = sum();
  if (i < toks.length) throw new Error("Unexpected text after the expression");
  if (!Number.isFinite(v)) throw new Error("The result is not a number");
  return v;
}

interface DimRef {
  get: () => number;
  set: (v: number) => void;
  angle: boolean;
  feature: Feature;
}

/** All named dimensions of a document ("D1@Sketch1", "D1@Boss-Extrude1"). */
export function dimensionIndex(doc: PartDocument): Map<string, DimRef> {
  const m = new Map<string, DimRef>();
  for (const f of doc.features) {
    if (f.type === "sketch") {
      for (const d of Object.values(f.sketch.dimensions) as Dimension[]) {
        if (d.driven) continue;
        m.set(`${d.name}@${f.name}`, { get: () => d.value, set: (v) => (d.value = v), angle: d.kind === "angle", feature: f });
      }
    } else {
      for (const p of featureParams(f)) m.set(`${p.label}@${f.name}`, { get: () => featureParams(f).find((x) => x.key === p.key)!.value, set: (v) => setFeatureParam(f, p.key, v), angle: !!p.angle, feature: f });
    }
  }
  return m;
}

/**
 * Evaluate global variables and equations, writing results into the driven dimensions
 * (lengths converted from document units, angles from degrees). Returns the features
 * whose dimensions changed and any errors by row.
 */
export function applyEquations(doc: PartDocument, perMm: number): { changed: Set<Feature>; values: Record<string, number>; errors: Record<string, string> } {
  const dims = dimensionIndex(doc);
  const values: Record<string, number> = {};
  const errors: Record<string, string> = {};
  const changed = new Set<Feature>();
  const globals = doc.globals ?? [];
  const resolving = new Set<string>();
  const lookup = (name: string): number => {
    if (name in values) return values[name];
    const g = globals.find((x) => x.name.toLowerCase() === name.toLowerCase());
    if (g) {
      if (resolving.has(g.name)) throw new Error(`Circular reference to "${g.name}"`);
      resolving.add(g.name);
      const v = evaluate(g.expr, lookup);
      resolving.delete(g.name);
      values[g.name] = v;
      return v;
    }
    const d = dims.get(name);
    if (d) return d.angle ? d.get() / D2R : d.get() * perMm;
    throw new Error(`Unknown name "${name}"`);
  };
  for (const g of globals) {
    try {
      lookup(g.name);
    } catch (e) {
      errors[g.name] = (e as Error).message;
    }
  }
  for (const eq of doc.equations ?? []) {
    try {
      const d = dims.get(eq.target);
      if (!d) throw new Error(`Unknown dimension "${eq.target}"`);
      const v = evaluate(eq.expr, lookup);
      const internal = d.angle ? v * D2R : v / perMm;
      values[eq.target] = v;
      if (Math.abs(d.get() - internal) > 1e-12) {
        d.set(internal);
        changed.add(d.feature);
      }
    } catch (e) {
      errors[eq.target] = (e as Error).message;
    }
  }
  return { changed, values, errors };
}
