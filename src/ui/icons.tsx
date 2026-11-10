/**
 * Original icon set (simple SVG glyphs). Colour language mirrors common CAD
 * conventions: yellow = added material, blue = sketch/reference, grey = tools.
 */
import type { ReactElement } from "react";

const Y = "#f4c430", YD = "#a07a00", B = "#2f6fd6", BD = "#1d3f7a", G = "#7c8592", GD = "#3e444c", R = "#d23a3a";

type IconFn = (size: number) => ReactElement;

const svg = (size: number, children: ReactElement | ReactElement[], vb = 24) => (
  <svg width={size} height={size} viewBox={`0 0 ${vb} ${vb}`} fill="none" strokeLinejoin="round" strokeLinecap="round">
    {children}
  </svg>
);

export const ICONS: Record<string, IconFn> = {
  extrude: (s) =>
    svg(s, [
      <path key="a" d="M4 17 L12 21 L20 17 L12 13 Z" fill={B} opacity={0.35} stroke={BD} />,
      <path key="b" d="M4 9 L12 13 L20 9 L12 5 Z" fill={Y} stroke={YD} />,
      <path key="c" d="M4 9 V17 L12 21 V13 Z" fill="#e0ad1a" stroke={YD} />,
      <path key="d" d="M20 9 V17 L12 21 V13 Z" fill="#c99512" stroke={YD} />,
    ]),
  revolve: (s) =>
    svg(s, [
      <ellipse key="a" cx="12" cy="7" rx="7" ry="2.6" fill={Y} stroke={YD} />,
      <path key="b" d="M5 7 V17 A7 2.6 0 0 0 19 17 V7" fill="#e0ad1a" stroke={YD} />,
      <path key="c" d="M12 2 V22" stroke={BD} strokeDasharray="3 2" />,
    ]),
  cutExtrude: (s) =>
    svg(s, [
      <path key="a" d="M3 10 L12 14 L21 10 L12 6 Z" fill="#d6dbe2" stroke={GD} />,
      <path key="b" d="M3 10 V17 L12 21 V14 Z" fill="#b9c0ca" stroke={GD} />,
      <path key="c" d="M21 10 V17 L12 21 V14 Z" fill="#a3abb6" stroke={GD} />,
      <path key="d" d="M9 9.5 L12 11 L15 9.5 L12 8 Z" fill={B} stroke={BD} />,
      <path key="e" d="M12 11 V16" stroke={BD} />,
    ]),
  cutRevolve: (s) =>
    svg(s, [
      <ellipse key="a" cx="12" cy="7" rx="7" ry="2.6" fill="#d6dbe2" stroke={GD} />,
      <path key="b" d="M5 7 V17 A7 2.6 0 0 0 19 17 V7" fill="#b9c0ca" stroke={GD} />,
      <ellipse key="c" cx="12" cy="7" rx="3" ry="1.1" fill={B} stroke={BD} />,
    ]),
  fillet: (s) =>
    svg(s, [
      <path key="a" d="M4 20 V8 A4 4 0 0 1 8 4 H20 V20 Z" fill="#d6dbe2" stroke={GD} />,
      <path key="b" d="M4 8 A4 4 0 0 1 8 4" stroke={Y} strokeWidth="3" />,
    ]),
  chamfer: (s) =>
    svg(s, [
      <path key="a" d="M4 20 V9 L9 4 H20 V20 Z" fill="#d6dbe2" stroke={GD} />,
      <path key="b" d="M4 9 L9 4" stroke={Y} strokeWidth="3" />,
    ]),
  plane: (s) => svg(s, [<path key="a" d="M3 16 L9 6 H21 L15 16 Z" fill={B} opacity={0.3} stroke={BD} />]),
  refGeom: (s) =>
    svg(s, [
      <path key="a" d="M3 16 L9 6 H21 L15 16 Z" fill={B} opacity={0.25} stroke={BD} />,
      <path key="b" d="M12 3 V21" stroke={GD} strokeDasharray="2 2" />,
    ]),
  sketch: (s) =>
    svg(s, [
      <rect key="a" x="3" y="6" width="13" height="12" fill="none" stroke={B} strokeWidth="1.6" />,
      <path key="b" d="M14 18 L21 5 L23 6 L16 19 L13.5 20 Z" fill={Y} stroke={YD} />,
    ]),
  exitSketch: (s) =>
    svg(s, [
      <rect key="a" x="2" y="7" width="12" height="11" fill="none" stroke={B} strokeWidth="1.6" />,
      <path key="b" d="M12 12 H22 M18 8 L22 12 L18 16" stroke="#1a8a1a" strokeWidth="2" />,
    ]),
  smartDimension: (s) =>
    svg(s, [
      <path key="a" d="M3 18 H21" stroke={GD} />,
      <path key="b" d="M3 14 V20 M21 14 V20" stroke={GD} />,
      <path key="c" d="M5 11 L3 9 L5 7 M19 7 L21 9 L19 11 M3 9 H21" stroke={B} />,
      <path key="d" d="M10 3 L12 6 L14 3" stroke={R} />,
    ]),
  line: (s) =>
    svg(s, [
      <path key="a" d="M4 19 L20 5" stroke={B} strokeWidth="1.8" />,
      <circle key="b" cx="4" cy="19" r="1.8" fill={BD} />,
      <circle key="c" cx="20" cy="5" r="1.8" fill={BD} />,
    ]),
  centerline: (s) => svg(s, [<path key="a" d="M4 19 L20 5" stroke={B} strokeWidth="1.6" strokeDasharray="5 2 1.5 2" />]),
  rectangle: (s) => svg(s, [<rect key="a" x="4" y="6" width="16" height="12" stroke={B} strokeWidth="1.8" />, <circle key="b" cx="4" cy="18" r="1.6" fill={BD} />, <circle key="c" cx="20" cy="6" r="1.6" fill={BD} />]),
  centerRectangle: (s) =>
    svg(s, [<rect key="a" x="4" y="6" width="16" height="12" stroke={B} strokeWidth="1.8" />, <path key="b" d="M4 6 L20 18 M20 6 L4 18" stroke={B} strokeDasharray="2 2" />, <circle key="c" cx="12" cy="12" r="1.6" fill={BD} />]),
  circle: (s) => svg(s, [<circle key="a" cx="12" cy="12" r="8" stroke={B} strokeWidth="1.8" />, <circle key="b" cx="12" cy="12" r="1.5" fill={BD} />]),
  arc: (s) => svg(s, [<path key="a" d="M4 16 A8 8 0 0 1 20 16" stroke={B} strokeWidth="1.8" />, <circle key="b" cx="12" cy="16" r="1.4" fill={BD} />]),
  threePointArc: (s) =>
    svg(s, [<path key="a" d="M4 17 Q12 2 20 17" stroke={B} strokeWidth="1.8" />, <circle key="b" cx="4" cy="17" r="1.5" fill={BD} />, <circle key="c" cx="20" cy="17" r="1.5" fill={BD} />, <circle key="d" cx="12" cy="9.5" r="1.5" fill={BD} />]),
  tangentArc: (s) => svg(s, [<path key="a" d="M2 18 H10 A6 6 0 0 0 16 12 V6" stroke={B} strokeWidth="1.8" />]),
  point: (s) => svg(s, [<circle key="a" cx="12" cy="12" r="2.6" fill={BD} />]),
  trim: (s) =>
    svg(s, [
      <path key="a" d="M3 12 H21" stroke={B} strokeWidth="1.8" />,
      <path key="b" d="M8 4 V20" stroke={B} strokeWidth="1.8" />,
      <path key="c" d="M14 7 L20 17 M20 7 L14 17" stroke={R} strokeWidth="1.6" />,
    ]),
  relations: (s) =>
    svg(s, [
      <rect key="a" x="3" y="3" width="8" height="8" fill="#c8f0c0" stroke="#3a7a30" />,
      <path key="b" d="M5 7 H9" stroke="#1a3a10" />,
      <rect key="c" x="13" y="13" width="8" height="8" fill="#c8f0c0" stroke="#3a7a30" />,
      <path key="d" d="M17 15 V19" stroke="#1a3a10" />,
    ]),
  zoomFit: (s) => svg(s, [<rect key="a" x="3" y="3" width="18" height="18" stroke={GD} strokeDasharray="3 2" />, <circle key="b" cx="11" cy="11" r="5" stroke={GD} strokeWidth="1.6" />, <path key="c" d="M15 15 L19 19" stroke={GD} strokeWidth="2" />]),
  zoomArea: (s) => svg(s, [<circle key="b" cx="10" cy="10" r="6" stroke={GD} strokeWidth="1.6" />, <path key="c" d="M14.5 14.5 L20 20" stroke={GD} strokeWidth="2.2" />, <rect key="d" x="7" y="7" width="6" height="6" stroke={B} /> ]),
  prevView: (s) => svg(s, [<path key="a" d="M9 7 L4 12 L9 17 M4 12 H15 A5 5 0 0 1 15 22" stroke={GD} strokeWidth="1.6" />]),
  section: (s) => svg(s, [<path key="a" d="M4 8 L12 4 L20 8 V16 L12 20 L4 16 Z" fill="#d6dbe2" stroke={GD} />, <path key="b" d="M4 8 L12 12 L20 8" stroke={GD} />, <path key="c" d="M2 13 L22 11" stroke={R} strokeWidth="1.6" />]),
  viewOrient: (s) =>
    svg(s, [<path key="a" d="M4 8 L12 4 L20 8 V16 L12 20 L4 16 Z" fill="#e7eaee" stroke={GD} />, <path key="b" d="M4 8 L12 12 L20 8 M12 12 V20" stroke={GD} />, <path key="c" d="M12 12 L20 8 V16 L12 20 Z" fill={B} opacity={0.6} />]),
  displayStyle: (s) =>
    svg(s, [<path key="a" d="M4 8 L12 4 L20 8 V16 L12 20 L4 16 Z" fill="#c9ccd1" stroke="#111" />, <path key="b" d="M4 8 L12 12 L20 8 M12 12 V20" stroke="#111" />]),
  hideShow: (s) => svg(s, [<path key="a" d="M2 12 C6 5 18 5 22 12 C18 19 6 19 2 12 Z" stroke={GD} />, <circle key="b" cx="12" cy="12" r="3" fill={GD} />]),
  appearance: (s) => svg(s, [<circle key="a" cx="12" cy="12" r="8" fill="#fff" stroke={GD} />, <circle key="b" cx="9" cy="10" r="2.4" fill={R} />, <circle key="c" cx="15" cy="10" r="2.4" fill={Y} />, <circle key="d" cx="12" cy="15" r="2.4" fill={B} />]),
  scene: (s) => svg(s, [<rect key="a" x="3" y="5" width="18" height="14" rx="2" fill="#dfe8f5" stroke={GD} />, <circle key="b" cx="16" cy="9" r="2" fill={Y} />, <path key="c" d="M3 17 L9 11 L13 15 L16 12 L21 17" stroke={GD} />]),
  viewSettings: (s) => svg(s, [<rect key="a" x="3" y="5" width="18" height="12" rx="1" stroke={GD} />, <path key="b" d="M8 21 H16 M12 17 V21" stroke={GD} />]),
  new: (s) => svg(s, [<path key="a" d="M6 3 H14 L19 8 V21 H6 Z" fill="#fff" stroke={GD} />, <path key="b" d="M14 3 V8 H19" stroke={GD} />]),
  open: (s) => svg(s, [<path key="a" d="M3 7 H9 L11 9 H20 V19 H3 Z" fill="#f6d77a" stroke={YD} />]),
  save: (s) => svg(s, [<path key="a" d="M4 4 H17 L20 7 V20 H4 Z" fill={B} stroke={BD} />, <rect key="b" x="7" y="4" width="9" height="5" fill="#fff" />, <rect key="c" x="7" y="13" width="10" height="7" fill="#e8eef9" /> ]),
  undo: (s) => svg(s, [<path key="a" d="M9 7 L4 12 L9 17 M4 12 H14 A6 6 0 0 1 14 24" stroke={GD} strokeWidth="1.8" />]),
  redo: (s) => svg(s, [<path key="a" d="M15 7 L20 12 L15 17 M20 12 H10 A6 6 0 0 0 10 24" stroke={GD} strokeWidth="1.8" />]),
  rebuild: (s) => svg(s, [<rect key="a" x="8" y="2" width="8" height="20" rx="2" fill="#333" />, <circle key="b" cx="12" cy="6.5" r="2" fill="#e33" />, <circle key="c" cx="12" cy="12" r="2" fill="#fc3" />, <circle key="d" cx="12" cy="17.5" r="2" fill="#3c3" />]),
  options: (s) => svg(s, [<circle key="a" cx="12" cy="12" r="3.5" stroke={GD} strokeWidth="1.8" />, <path key="b" d="M12 2 V5 M12 19 V22 M2 12 H5 M19 12 H22 M4.9 4.9 L7 7 M17 17 L19.1 19.1 M4.9 19.1 L7 17 M17 7 L19.1 4.9" stroke={GD} strokeWidth="1.8" />]),
  select: (s) => svg(s, [<path key="a" d="M5 3 L5 19 L9 15 L12 21 L14 20 L11 14 L17 14 Z" fill="#fff" stroke="#111" />]),
  home: (s) => svg(s, [<path key="a" d="M3 11 L12 3 L21 11 V21 H14 V15 H10 V21 H3 Z" stroke={GD} strokeWidth="1.6" />]),
  print: (s) => svg(s, [<rect key="a" x="5" y="3" width="14" height="6" stroke={GD} />, <rect key="b" x="3" y="9" width="18" height="8" fill="#e7eaee" stroke={GD} />, <rect key="c" x="7" y="14" width="10" height="7" fill="#fff" stroke={GD} />]),
  measure: (s) => svg(s, [<rect key="a" x="2" y="9" width="20" height="7" rx="1" fill={Y} stroke={YD} />, <path key="b" d="M6 9 V12 M10 9 V13 M14 9 V12 M18 9 V13" stroke={YD} />]),
  massProps: (s) => svg(s, [<path key="a" d="M5 20 L8 9 H16 L19 20 Z" fill="#c9ccd1" stroke={GD} />, <circle key="b" cx="12" cy="6" r="3" stroke={GD} strokeWidth="1.6" />]),
  linearPattern: (s) => svg(s, [<rect key="a" x="3" y="13" width="6" height="6" fill={Y} stroke={YD} />, <rect key="b" x="10" y="13" width="6" height="6" fill="#f8e4a0" stroke={YD} />, <rect key="c" x="3" y="5" width="6" height="6" fill="#f8e4a0" stroke={YD} />, <path key="d" d="M18 16 H22 M6 3 V1" stroke={B} />]),
  shell: (s) => svg(s, [<path key="a" d="M4 6 H20 V20 H4 Z" fill="#c9ccd1" stroke={GD} />, <path key="b" d="M7 6 V17 H17 V6" fill="#fff" stroke={GD} />]),
  mirror: (s) => svg(s, [<path key="a" d="M12 2 V22" stroke={B} strokeDasharray="3 2" />, <path key="b" d="M3 8 L9 6 V18 L3 16 Z" fill={Y} stroke={YD} />, <path key="c" d="M21 8 L15 6 V18 L21 16 Z" fill="#f8e4a0" stroke={YD} />]),
  generic: (s) => svg(s, [<rect key="a" x="5" y="5" width="14" height="14" rx="2" stroke={G} />]),
  check: (s) => svg(s, [<path key="a" d="M4 12 L10 18 L20 5" stroke="#1a8a1a" strokeWidth="3" />]),
  cancel: (s) => svg(s, [<path key="a" d="M5 5 L19 19 M19 5 L5 19" stroke={R} strokeWidth="3" />]),
  help: (s) => svg(s, [<circle key="a" cx="12" cy="12" r="9" stroke={B} strokeWidth="1.6" />, <path key="b" d="M9.5 9.5 A2.5 2.5 0 1 1 12 12 V14" stroke={B} strokeWidth="1.8" />, <circle key="c" cx="12" cy="17.5" r="1" fill={B} />]),
  part: (s) => svg(s, [<path key="a" d="M4 8 L12 4 L20 8 V16 L12 20 L4 16 Z" fill={Y} stroke={YD} />, <path key="b" d="M4 8 L12 12 L20 8 M12 12 V20" stroke={YD} />]),
  history: (s) => svg(s, [<rect key="a" x="4" y="4" width="16" height="16" rx="1" fill="#fff" stroke={GD} />, <path key="b" d="M12 8 V12 L15 14" stroke={GD} />]),
  sensor: (s) => svg(s, [<path key="a" d="M6 18 A8 8 0 0 1 18 18" stroke={GD} strokeWidth="1.6" />, <path key="b" d="M12 18 L15 11" stroke={R} strokeWidth="1.6" />]),
  annotations: (s) => svg(s, [<rect key="a" x="3" y="5" width="18" height="14" fill="#fff" stroke={GD} />, <path key="b" d="M7 15 L10 8 L13 15 M8 12.5 H12" stroke={GD} />]),
  material: (s) => svg(s, [<rect key="a" x="3" y="8" width="18" height="8" fill="#c9a37a" stroke="#7a5a3a" />, <path key="b" d="M3 11 H21" stroke="#7a5a3a" />]),
  origin: (s) => svg(s, [<path key="a" d="M5 19 V5 M5 19 H19" stroke={GD} strokeWidth="1.6" />, <path key="b" d="M3 7 L5 4 L7 7 M17 17 L20 19 L17 21" stroke={GD} />]),
  error: (s) => svg(s, [<circle key="a" cx="12" cy="12" r="9" fill={R} />, <path key="b" d="M8 8 L16 16 M16 8 L8 16" stroke="#fff" strokeWidth="2.4" />]),
  pin: (s) => svg(s, [<path key="a" d="M9 3 H15 L14 10 L18 14 H6 L10 10 Z M12 14 V21" stroke={GD} />]),
  search: (s) => svg(s, [<circle key="a" cx="10" cy="10" r="6" stroke={GD} strokeWidth="1.8" />, <path key="b" d="M14.5 14.5 L20 20" stroke={GD} strokeWidth="2" />]),
};

export function Icon({ name, size = 20 }: { name: string; size?: number }) {
  const f = ICONS[name] ?? ICONS.generic;
  return f(size);
}
