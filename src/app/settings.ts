/**
 * System Options (Tools > Options > System Options). Defaults follow SOLIDWORKS.
 * Persisted per user in localStorage.
 */
export interface SystemOptions {
  view: {
    reverseWheelZoom: boolean;
    zoomAboutScreenCenter: boolean;
    /** Arrow-key rotation increment in degrees (View > Arrow keys). */
    arrowKeyIncrement: number;
    /** View transition animation length in seconds (0 = off). */
    animationSpeed: number;
    perspective: boolean;
  };
  sketch: {
    enableSnapping: boolean;
    automaticRelations: boolean;
    snapAngle: number; // degrees
    snaps: {
      endpoints: boolean;
      centers: boolean;
      midpoints: boolean;
      quadrants: boolean;
      intersections: boolean;
      nearest: boolean;
      tangent: boolean;
      perpendicular: boolean;
      parallel: boolean;
      hvLines: boolean;
      hvPoints: boolean;
      length: boolean;
      grid: boolean;
      angle: boolean;
    };
    displayGrid: boolean;
    gridSpacing: number;
    /** Prompt for value when a dimension is created ("Input dimension value"). */
    inputDimensionValue: boolean;
    showRelations: boolean;
    /** Rapid Sketch / auto-rotate view normal to sketch plane on creation. */
    autoRotateNormal: boolean;
  };
  colors: {
    background: [string, string];
    model: string;
    edges: string;
    hover: string;
    selected: string;
    sketchUnder: string;
    sketchFull: string;
    sketchOver: string;
    sketchInactive: string;
    inferenceRelation: string;
    inferenceGuide: string;
    dimension: string;
    dimensionDriven: string;
    plane: string;
  };
  performance: {
    /** Render pixel ratio multiplier (lower = faster on huge displays). */
    pixelRatio: number;
    msaa: boolean;
  };
  theme: "light" | "dark";
  /** Every other Tools > Options control, keyed by schema key (see optionsSchema.ts). */
  extra: Record<string, unknown>;
}

export const DEFAULT_OPTIONS: SystemOptions = {
  view: {
    reverseWheelZoom: false,
    zoomAboutScreenCenter: false,
    arrowKeyIncrement: 15,
    animationSpeed: 0.35,
    perspective: false,
  },
  sketch: {
    enableSnapping: true,
    automaticRelations: true,
    snapAngle: 45,
    snaps: {
      endpoints: true,
      centers: true,
      midpoints: true,
      quadrants: true,
      intersections: true,
      nearest: true,
      tangent: true,
      perpendicular: true,
      parallel: true,
      hvLines: true,
      hvPoints: true,
      length: true,
      grid: false,
      angle: true,
    },
    displayGrid: false,
    gridSpacing: 10,
    inputDimensionValue: true,
    showRelations: true,
    autoRotateNormal: true,
  },
  colors: {
    background: ["#ffffff", "#c8cdd4"],
    model: "#c9ccd1",
    edges: "#1a1a1a",
    hover: "#ff9a1f",
    selected: "#2e8bff",
    sketchUnder: "#0000ff",
    sketchFull: "#000000",
    sketchOver: "#ff0000",
    sketchInactive: "#5a5a5a",
    inferenceRelation: "#1f5fff",
    inferenceGuide: "#e8a200",
    dimension: "#000000",
    dimensionDriven: "#7a7a7a",
    plane: "#7aa6d8",
  },
  performance: { pixelRatio: 1, msaa: true },
  theme: "light",
  extra: {},
};

const KEY = "opencad.options.v1";

export function loadOptions(): SystemOptions {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) return deepMerge(structuredClone(DEFAULT_OPTIONS), JSON.parse(raw));
  } catch {
    /* ignore */
  }
  return structuredClone(DEFAULT_OPTIONS);
}

export function saveOptions(o: SystemOptions) {
  try {
    localStorage.setItem(KEY, JSON.stringify(o));
  } catch {
    /* ignore */
  }
}

function deepMerge<T>(base: T, over: unknown): T {
  if (typeof over !== "object" || over === null || Array.isArray(over)) return (over as T) ?? base;
  const out = base as Record<string, unknown>;
  const open = Object.keys(out).length === 0; // free-form maps (e.g. extra) accept any key
  for (const [k, v] of Object.entries(over)) {
    if (!open && !(k in out)) continue;
    out[k] = typeof v === "object" && v !== null && !Array.isArray(v) ? deepMerge(out[k], v) : v;
  }
  return out as T;
}
