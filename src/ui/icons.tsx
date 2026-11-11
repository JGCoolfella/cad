/**
 * Original icon set. Visual language (common to CAD tools, not copied artwork):
 * yellow = added material, blue = sketch geometry / references, grey = existing
 * body, red = removal / cancel. 24×24 viewBox, 1px dark outlines, soft gradients.
 */
import { Fragment, type ReactElement, type ReactNode } from "react";

const O = "#3a3f47"; // outline
const SK = "#1f6fd6"; // sketch blue
const SKD = "#123f86";
const RED = "#d23a3a";
const GRN = "#1a8a1a";

type IconFn = (size: number) => ReactElement;

const grads = (
  <defs>
    <linearGradient id="oc-y" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stopColor="#ffe17a" />
      <stop offset="1" stopColor="#e3a613" />
    </linearGradient>
    <linearGradient id="oc-yd" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stopColor="#e9b52a" />
      <stop offset="1" stopColor="#b98208" />
    </linearGradient>
    <linearGradient id="oc-g" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stopColor="#eef1f5" />
      <stop offset="1" stopColor="#b9c1cc" />
    </linearGradient>
    <linearGradient id="oc-gd" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stopColor="#b3bcc8" />
      <stop offset="1" stopColor="#8a95a3" />
    </linearGradient>
    <linearGradient id="oc-b" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stopColor="#8fc0ff" />
      <stop offset="1" stopColor="#2f78e0" />
    </linearGradient>
  </defs>
);

const svg = (size: number, ...children: ReactNode[]) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" strokeLinejoin="round" strokeLinecap="round">
    {grads}
    {children.map((c, i) => (
      <Fragment key={i}>{c}</Fragment>
    ))}
  </svg>
);

/** Isometric block with top/left/right faces. */
const block = (top: string, left: string, right: string, x = 0, y = 0, h = 8) => (
  <g transform={`translate(${x} ${y})`}>
    <path d={`M3 8 L12 4 L21 8 L12 12 Z`} fill={top} stroke={O} strokeWidth="0.9" />
    <path d={`M3 8 V${8 + h} L12 ${12 + h} V12 Z`} fill={left} stroke={O} strokeWidth="0.9" />
    <path d={`M21 8 V${8 + h} L12 ${12 + h} V12 Z`} fill={right} stroke={O} strokeWidth="0.9" />
  </g>
);
const yellowBlock = (x = 0, y = 0, h = 8) => block("url(#oc-y)", "#f0bc2c", "url(#oc-yd)", x, y, h);
const greyBlock = (x = 0, y = 0, h = 8) => block("url(#oc-g)", "#c3cad4", "url(#oc-gd)", x, y, h);
const sketchRect = (d: string) => <path d={d} stroke={SK} strokeWidth="1.5" fill="none" />;
const pt = (x: number, y: number, c = SKD) => <circle cx={x} cy={y} r="1.6" fill={c} />;
const sparkle = (x: number, y: number) => <path d={`M${x} ${y - 3} L${x + 0.8} ${y - 0.8} L${x + 3} ${y} L${x + 0.8} ${y + 0.8} L${x} ${y + 3} L${x - 0.8} ${y + 0.8} L${x - 3} ${y} L${x - 0.8} ${y - 0.8} Z`} fill="#ffd23f" stroke="#a07a00" strokeWidth="0.5" />;

/** Wireframe cube used by display-style icons. */
const dsCube = (size: number, fill: boolean, edges: "solid" | "hlv" | "none" | "wire") =>
  svg(
    size,
    fill && <path key="t" d="M4 8 L12 4 L20 8 L12 12 Z" fill="url(#oc-g)" />,
    fill && <path key="l" d="M4 8 V16 L12 20 V12 Z" fill="#c3cad4" />,
    fill && <path key="r" d="M20 8 V16 L12 20 V12 Z" fill="url(#oc-gd)" />,
    edges !== "none" && <path key="v" d="M4 8 L12 4 L20 8 L12 12 Z M4 8 V16 L12 20 L20 16 V8 M12 12 V20" stroke="#111" strokeWidth="1" />,
    edges === "hlv" && <path key="h" d="M4 16 L12 12 M20 16 L12 12" stroke="#777" strokeWidth="0.8" strokeDasharray="1.5 1.2" />,
    edges === "wire" && <path key="w" d="M4 16 L12 12 L20 16 M12 4 V12" stroke="#111" strokeWidth="0.9" />,
  );

export const ICONS: Record<string, IconFn> = {
  // ---------------------------------------------------------------- features
  extrude: (s) => svg(s, sketchRect("M3 20 L12 16 L21 20"), yellowBlock(0, 0, 8), <path key="a" d="M12 2 V0" stroke={O} />),
  revolve: (s) =>
    svg(
      s,
      <ellipse key="a" cx="13" cy="6.5" rx="7" ry="2.6" fill="url(#oc-y)" stroke={O} strokeWidth="0.9" />,
      <path key="b" d="M6 6.5 V17 A7 2.6 0 0 0 20 17 V6.5 A7 2.6 0 0 1 6 6.5 Z" fill="url(#oc-yd)" stroke={O} strokeWidth="0.9" />,
      <path key="c" d="M3 1.5 V22.5" stroke={SK} strokeWidth="1.2" strokeDasharray="4 1.5 1 1.5" />,
      <path key="d" d="M1 10 A3 1.6 0 0 0 5 10" stroke={RED} strokeWidth="1" />,
    ),
  swept: (s) => svg(s, <path key="a" d="M3 20 C 8 20, 8 6, 18 5" stroke={SK} strokeWidth="1.3" strokeDasharray="3 1.5" />, <path key="b" d="M2 17 C 7 17, 7 3, 17 2 L21 6 C 11 7, 11 21, 6 21 Z" fill="url(#oc-y)" stroke={O} strokeWidth="0.9" />),
  lofted: (s) => svg(s, <path key="a" d="M4 18 L9 21 L14 18 L9 15 Z" fill="none" stroke={SK} strokeWidth="1.2" />, <path key="b" d="M8 5 L15 3 L20 6 L13 8 Z" fill="url(#oc-y)" stroke={O} strokeWidth="0.9" />, <path key="c" d="M4 18 L8 5 L13 8 L9 21 Z" fill="#f0bc2c" stroke={O} strokeWidth="0.9" />, <path key="d" d="M9 21 L13 8 L20 6 L14 18 Z" fill="url(#oc-yd)" stroke={O} strokeWidth="0.9" />),
  boundary: (s) => svg(s, <path key="a" d="M3 18 C 7 10, 15 22, 21 12 L18 5 C 13 13, 7 2, 4 9 Z" fill="url(#oc-y)" stroke={O} strokeWidth="0.9" />, <path key="b" d="M4 9 L3 18 M18 5 L21 12" stroke={SK} strokeWidth="1.4" />),
  cutExtrude: (s) => svg(s, greyBlock(0, 2, 8), <path key="a" d="M8.5 8.8 L12 7.2 L15.5 8.8 L12 10.4 Z" fill="#5e6772" stroke={O} strokeWidth="0.7" />, <path key="b" d="M12 5.5 V1.5 M10.5 3 L12 1.5 L13.5 3" stroke={RED} strokeWidth="1.1" />),
  hole: (s) => svg(s, greyBlock(0, 2, 8), <ellipse key="a" cx="12" cy="10" rx="3.2" ry="1.6" fill="#4a535e" stroke={O} strokeWidth="0.7" />, <path key="b" d="M8.8 10 V14 A3.2 1.6 0 0 0 15.2 14 V10" stroke={O} strokeWidth="0.6" strokeDasharray="1.2 1" />),
  cutRevolve: (s) =>
    svg(
      s,
      <ellipse key="a" cx="13" cy="6.5" rx="7" ry="2.6" fill="url(#oc-g)" stroke={O} strokeWidth="0.9" />,
      <path key="b" d="M6 6.5 V17 A7 2.6 0 0 0 20 17 V6.5 A7 2.6 0 0 1 6 6.5 Z" fill="url(#oc-gd)" stroke={O} strokeWidth="0.9" />,
      <ellipse key="c" cx="13" cy="6.5" rx="3" ry="1.1" fill="#4a535e" />,
      <path key="d" d="M3 1.5 V22.5" stroke={SK} strokeWidth="1.2" strokeDasharray="4 1.5 1 1.5" />,
    ),
  sweptCut: (s) => svg(s, greyBlock(0, 2, 8), <path key="a" d="M6 10 C 9 6, 14 14, 18 9" stroke="#4a535e" strokeWidth="2.6" />),
  loftedCut: (s) => svg(s, greyBlock(0, 2, 8), <path key="a" d="M8 9 L12 7 L16 9 L12 11 Z" fill="#4a535e" />, <path key="b" d="M10 15 L12 14 L14 15" stroke="#4a535e" strokeWidth="1.2" />),
  boundaryCut: (s) => svg(s, greyBlock(0, 2, 8), <path key="a" d="M6 9 C 9 6, 13 13, 18 8" stroke="#4a535e" strokeWidth="2.2" />),
  fillet: (s) => svg(s, <path key="a" d="M3 21 V10 A6 6 0 0 1 9 4 H21 V21 Z" fill="url(#oc-g)" stroke={O} strokeWidth="0.9" />, <path key="b" d="M3 10 A6 6 0 0 1 9 4" stroke="#e3a613" strokeWidth="3" />, <path key="c" d="M3 10 A6 6 0 0 1 9 4" stroke={O} strokeWidth="0.7" />),
  chamfer: (s) => svg(s, <path key="a" d="M3 21 V10 L9 4 H21 V21 Z" fill="url(#oc-g)" stroke={O} strokeWidth="0.9" />, <path key="b" d="M3 10 L9 4" stroke="#e3a613" strokeWidth="3" />),
  linearPattern: (s) => svg(s, yellowBlock(-4, 6, 4), block("#fff2c4", "#f6dc8a", "#e9c766", 4, 2, 4), block("#fff2c4", "#f6dc8a", "#e9c766", -4, -2, 4), <path key="a" d="M18 20 H23 M21 18 L23 20 L21 22" stroke={SK} strokeWidth="1" />),
  rib: (s) => svg(s, greyBlock(0, 6, 4), <path key="a" d="M10 4 L14 2 L14 12 L10 14 Z" fill="url(#oc-y)" stroke={O} strokeWidth="0.8" />),
  draft: (s) => svg(s, <path key="a" d="M5 20 L8 5 H16 L19 20 Z" fill="url(#oc-g)" stroke={O} strokeWidth="0.9" />, <path key="b" d="M8 5 L5 20" stroke="#e3a613" strokeWidth="2.4" />, <path key="c" d="M8 20 V5" stroke={SK} strokeWidth="0.8" strokeDasharray="2 1.5" />),
  shell: (s) => svg(s, <path key="a" d="M3 8 L12 4 L21 8 V16 L12 20 L3 16 Z" fill="url(#oc-g)" stroke={O} strokeWidth="0.9" />, <path key="b" d="M6 8.3 L12 5.7 L18 8.3 L12 10.9 Z" fill="#5e6772" stroke={O} strokeWidth="0.7" />),
  wrap: (s) => svg(s, <ellipse key="a" cx="12" cy="6" rx="7" ry="2.5" fill="url(#oc-g)" stroke={O} strokeWidth="0.9" />, <path key="b" d="M5 6 V18 A7 2.5 0 0 0 19 18 V6" fill="url(#oc-gd)" stroke={O} strokeWidth="0.9" />, <path key="c" d="M8 11 L11 13 L14 11 L16 13" stroke={SK} strokeWidth="1.5" />),
  intersect: (s) => svg(s, <circle key="a" cx="9" cy="12" r="6" fill="url(#oc-g)" stroke={O} strokeWidth="0.9" />, <circle key="b" cx="15" cy="12" r="6" fill="url(#oc-y)" opacity="0.85" stroke={O} strokeWidth="0.9" />),
  mirror: (s) => svg(s, <path key="a" d="M12 1 V23" stroke={SK} strokeWidth="1.2" strokeDasharray="4 1.5 1 1.5" />, <path key="b" d="M2 8 L9 5 V19 L2 16 Z" fill="url(#oc-y)" stroke={O} strokeWidth="0.9" />, <path key="c" d="M22 8 L15 5 V19 L22 16 Z" fill="#fff2c4" stroke={O} strokeWidth="0.9" />),
  plane: (s) => svg(s, <path key="a" d="M2 17 L8 6 H22 L16 17 Z" fill="#e7eef9" stroke="#5c8fd8" strokeWidth="1.1" />),
  planeShown: (s) => svg(s, <path key="a" d="M2 17 L8 6 H22 L16 17 Z" fill="#9ec3f5" stroke="#2f6dbf" strokeWidth="1.1" />),
  refGeom: (s) => svg(s, <path key="a" d="M2 17 L8 6 H22 L16 17 Z" fill="#e7eef9" stroke="#5c8fd8" strokeWidth="1.1" />, <path key="b" d="M12 1 V23" stroke={O} strokeWidth="1" strokeDasharray="3 1.5" />),
  curves: (s) => svg(s, <path key="a" d="M3 20 C 3 8, 12 20, 12 10 S 21 4, 21 4" stroke={SK} strokeWidth="1.6" />, <path key="b" d="M3 17 C 6 12, 9 16, 12 7" stroke="#e3a613" strokeWidth="1.2" />),
  instant3d: (s) => svg(s, yellowBlock(0, 3, 6), <path key="a" d="M12 7 V1 M10 3 L12 1 L14 3" stroke={SK} strokeWidth="1.3" />, <circle key="b" cx="12" cy="7" r="1.4" fill={SK} />),
  // ---------------------------------------------------------------- sketch
  sketch: (s) => svg(s, sketchRect("M3 7 H15 V19 H3 Z"), pt(3, 19), pt(15, 7), <path key="a" d="M13 20 L21 5 L23 6.2 L15 21 L12.5 22 Z" fill="url(#oc-y)" stroke={O} strokeWidth="0.8" />, <path key="b" d="M12.5 22 L13 20 L15 21" fill={O} />),
  exitSketch: (s) => svg(s, sketchRect("M2 6 H13 V18 H2 Z"), <path key="a" d="M9 12 H22 M18 8 L22 12 L18 16" stroke={GRN} strokeWidth="2" />),
  smartDimension: (s) =>
    svg(
      s,
      <path key="a" d="M3 13 V21 M21 13 V21" stroke={O} strokeWidth="0.9" />,
      <path key="b" d="M3 17 H21" stroke={SK} strokeWidth="1.2" />,
      <path key="c" d="M3 17 L6 15.5 V18.5 Z M21 17 L18 15.5 V18.5 Z" fill={SK} />,
      <path key="d" d="M6 3 L12 9 L18 3" stroke={RED} strokeWidth="1.4" />,
      <path key="e" d="M10 10.5 L12 12.5 L14 10.5" fill="none" stroke={O} strokeWidth="0.9" />,
    ),
  line: (s) => svg(s, <path key="a" d="M4 20 L20 4" stroke={SK} strokeWidth="1.8" />, pt(4, 20), pt(20, 4)),
  centerline: (s) => svg(s, <path key="a" d="M3 21 L21 3" stroke={SK} strokeWidth="1.5" strokeDasharray="5 2 1.2 2" />),
  rectangle: (s) => svg(s, sketchRect("M4 6 H20 V18 H4 Z"), pt(4, 18), pt(20, 6)),
  centerRectangle: (s) => svg(s, sketchRect("M3 6 H21 V18 H3 Z"), <path key="a" d="M3 6 L21 18 M21 6 L3 18" stroke={SK} strokeWidth="0.8" strokeDasharray="2 1.5" />, pt(12, 12)),
  slot: (s) => svg(s, <path key="a" d="M8 7 H16 A5 5 0 0 1 16 17 H8 A5 5 0 0 1 8 7 Z" stroke={SK} strokeWidth="1.6" />, <path key="b" d="M8 12 H16" stroke={SK} strokeWidth="0.8" strokeDasharray="2 1.5" />, pt(8, 12), pt(16, 12)),
  circle: (s) => svg(s, <circle key="a" cx="12" cy="12" r="8" stroke={SK} strokeWidth="1.7" />, pt(12, 12), <path key="b" d="M12 12 L17.6 6.4" stroke={SK} strokeWidth="0.8" strokeDasharray="1.5 1.2" />),
  arc: (s) => svg(s, <path key="a" d="M4 17 A8 8 0 0 1 20 17" stroke={SK} strokeWidth="1.8" />, pt(12, 17), pt(4, 17), pt(20, 17)),
  threePointArc: (s) => svg(s, <path key="a" d="M3 18 Q12 1 21 18" stroke={SK} strokeWidth="1.8" />, pt(3, 18), pt(21, 18), pt(12, 9.5)),
  tangentArc: (s) => svg(s, <path key="a" d="M2 19 H10" stroke={SK} strokeWidth="1.8" />, <path key="b" d="M10 19 A7 7 0 0 0 17 12 V5" stroke={SK} strokeWidth="1.8" />, pt(10, 19)),
  polygon: (s) => svg(s, <path key="a" d="M12 3 L20 8 V16 L12 21 L4 16 V8 Z" stroke={SK} strokeWidth="1.6" />, <circle key="b" cx="12" cy="12" r="9" stroke={SK} strokeWidth="0.7" strokeDasharray="2 1.5" />),
  spline: (s) => svg(s, <path key="a" d="M3 18 C 7 2, 12 22, 21 6" stroke={SK} strokeWidth="1.7" />, pt(3, 18), pt(12, 12), pt(21, 6)),
  ellipse: (s) => svg(s, <ellipse key="a" cx="12" cy="12" rx="9" ry="5.5" stroke={SK} strokeWidth="1.7" />, pt(12, 12)),
  sketchFillet: (s) => svg(s, <path key="a" d="M4 21 V11 A7 7 0 0 1 11 4 H21" stroke={SK} strokeWidth="1.7" />, <path key="b" d="M4 4 L8 8" stroke={O} strokeWidth="0.8" strokeDasharray="1.5 1.2" />),
  text: (s) => svg(s, <path key="a" d="M5 20 L11 4 H13 L19 20 M8 14 H16" stroke={SK} strokeWidth="1.8" />),
  point: (s) => svg(s, <circle key="a" cx="12" cy="12" r="3" fill={SKD} />, <path key="b" d="M12 4 V7 M12 17 V20 M4 12 H7 M17 12 H20" stroke={SK} strokeWidth="1" />),
  trim: (s) => svg(s, <path key="a" d="M2 12 H22" stroke={SK} strokeWidth="1.7" />, <path key="b" d="M8 3 V21" stroke={SK} strokeWidth="1.7" />, <path key="c" d="M13 7 L21 17 M21 7 L13 17" stroke={RED} strokeWidth="1.6" />),
  convert: (s) => svg(s, greyBlock(0, 2, 8), <path key="a" d="M3 10 L12 6 L21 10" stroke={SK} strokeWidth="1.8" />),
  offset: (s) => svg(s, sketchRect("M3 9 H13 V19 H3 Z"), <path key="a" d="M7 5 H17 V15" stroke={SK} strokeWidth="1.5" strokeDasharray="3 1.5" />, <path key="b" d="M14 8 L18 4" stroke={O} strokeWidth="0.8" />),
  sketchMirror: (s) => svg(s, <path key="a" d="M12 1 V23" stroke={SK} strokeWidth="1.2" strokeDasharray="4 1.5 1 1.5" />, <path key="b" d="M3 6 L9 9 L5 17" stroke={SK} strokeWidth="1.6" />, <path key="c" d="M21 6 L15 9 L19 17" stroke={SK} strokeWidth="1.6" strokeDasharray="2.5 1.5" />),
  sketchPattern: (s) => svg(s, <circle key="a" cx="5" cy="17" r="2.5" stroke={SK} strokeWidth="1.5" />, <circle key="b" cx="12" cy="17" r="2.5" stroke={SK} strokeWidth="1" strokeDasharray="1.5 1" />, <circle key="c" cx="19" cy="17" r="2.5" stroke={SK} strokeWidth="1" strokeDasharray="1.5 1" />, <path key="d" d="M5 10 H20 M18 8 L20 10 L18 12" stroke={O} strokeWidth="0.9" />),
  move: (s) => svg(s, <path key="a" d="M12 2 V22 M2 12 H22 M12 2 L9.5 4.5 M12 2 L14.5 4.5 M12 22 L9.5 19.5 M12 22 L14.5 19.5 M2 12 L4.5 9.5 M2 12 L4.5 14.5 M22 12 L19.5 9.5 M22 12 L19.5 14.5" stroke={SK} strokeWidth="1.4" />),
  repair: (s) => svg(s, sketchRect("M3 9 L10 4 L17 9"), <path key="a" d="M13 21 L21 13 L19 11 L11 19 Z" fill="#c3cad4" stroke={O} strokeWidth="0.8" />),
  quickSnaps: (s) => svg(s, <path key="a" d="M4 20 L20 4" stroke={SK} strokeWidth="1.4" />, <circle key="b" cx="12" cy="12" r="3.2" stroke="#e8a200" strokeWidth="1.5" />),
  rapidSketch: (s) => svg(s, sketchRect("M3 8 H14 V19 H3 Z"), <path key="a" d="M15 3 L11 12 H16 L12 21" stroke="#e8a200" strokeWidth="1.6" />),
  instant2d: (s) => svg(s, sketchRect("M3 14 H21"), <path key="a" d="M3 9 V19 M21 9 V19" stroke={O} strokeWidth="0.8" />, <path key="b" d="M8 6 H16 M14 4 L16 6 L14 8" stroke={SK} strokeWidth="1.3" />),
  shadedContours: (s) => svg(s, <path key="a" d="M4 6 H20 V18 H4 Z" fill="#b4c8ff" stroke={SK} strokeWidth="1.5" />),
  construction: (s) => svg(s, <path key="a" d="M3 21 L21 3" stroke={SK} strokeWidth="1.5" strokeDasharray="4 2" />, <path key="b" d="M3 10 H11" stroke={SK} strokeWidth="1.5" />),
  autoRelations: (s) => svg(s, <rect key="a" x="3" y="3" width="10" height="10" rx="1" fill="#ffe46b" stroke="#a08000" />, <path key="b" d="M5.5 8 H10.5" stroke="#3a2e00" strokeWidth="1.4" />, <path key="c" d="M13 20 L21 12" stroke={SK} strokeWidth="1.5" />),
  grid: (s) => svg(s, <path key="a" d="M3 3 H21 V21 H3 Z M9 3 V21 M15 3 V21 M3 9 H21 M3 15 H21" stroke="#8a95a3" strokeWidth="0.9" />, <circle key="b" cx="15" cy="9" r="1.8" fill={SK} />),
  relations: (s) =>
    svg(
      s,
      <rect key="a" x="2" y="2" width="10" height="10" rx="1" fill="#d4f2cc" stroke="#3a7a30" />,
      <path key="b" d="M4.5 7 H9.5" stroke="#1a3a10" strokeWidth="1.4" />,
      <rect key="c" x="12" y="12" width="10" height="10" rx="1" fill="#d4f2cc" stroke="#3a7a30" />,
      <path key="d" d="M17 14.5 V19.5" stroke="#1a3a10" strokeWidth="1.4" />,
      <path key="e" d="M12 7 H17 V12" stroke={RED} strokeWidth="1" />,
    ),
  // ---------------------------------------------------------------- view
  zoomFit: (s) => svg(s, <rect key="a" x="2.5" y="2.5" width="19" height="19" stroke="#8a95a3" strokeDasharray="3 2" />, <circle key="b" cx="11" cy="11" r="5" fill="#eef5ff" stroke={O} strokeWidth="1.5" />, <path key="c" d="M14.5 14.5 L19 19" stroke={O} strokeWidth="2.2" />),
  zoomArea: (s) => svg(s, <circle key="b" cx="10" cy="10" r="6.5" fill="#eef5ff" stroke={O} strokeWidth="1.5" />, <path key="c" d="M15 15 L21 21" stroke={O} strokeWidth="2.4" />, <rect key="d" x="7" y="7" width="6" height="6" stroke={SK} strokeWidth="1.2" />),
  prevView: (s) => svg(s, <path key="a" d="M9 6 L4 11 L9 16" stroke={O} strokeWidth="1.6" />, <path key="b" d="M4 11 H14 A6 6 0 0 1 14 23" stroke={O} strokeWidth="1.6" />),
  section: (s) => svg(s, greyBlock(0, 2, 8), <path key="a" d="M1 13 L23 9" stroke={RED} strokeWidth="1.6" />, <path key="b" d="M3 10 L12 6 L21 10 L12 14 Z" fill="#f6b4b4" opacity="0.6" />),
  viewOrient: (s) => svg(s, <path key="a" d="M4 8 L12 4 L20 8 V16 L12 20 L4 16 Z" fill="#eef1f5" stroke={O} strokeWidth="1" />, <path key="b" d="M4 8 L12 12 L20 8 M12 12 V20" stroke={O} strokeWidth="1" />, <path key="c" d="M12 12 L20 8 V16 L12 20 Z" fill="url(#oc-b)" />),
  displayStyle: (s) => svg(s, <path key="a" d="M4 8 L12 4 L20 8 V16 L12 20 L4 16 Z" fill="url(#oc-g)" stroke="#111" />, <path key="b" d="M4 8 L12 12 L20 8 M12 12 V20" stroke="#111" />),
  dsShadedEdges: (s) => dsCube(s, true, "solid"),
  dsShaded: (s) => dsCube(s, true, "none"),
  dsHlr: (s) => dsCube(s, false, "solid"),
  dsHlv: (s) => dsCube(s, false, "hlv"),
  dsWireframe: (s) => dsCube(s, false, "wire"),
  hideShow: (s) => svg(s, <path key="a" d="M2 12 C6 5 18 5 22 12 C18 19 6 19 2 12 Z" fill="#fff" stroke={O} strokeWidth="1.2" />, <circle key="b" cx="12" cy="12" r="3.4" fill={SK} />, <circle key="c" cx="13" cy="11" r="1" fill="#fff" />),
  appearance: (s) => svg(s, <circle key="a" cx="12" cy="12" r="9" fill="#fff" stroke={O} strokeWidth="0.9" />, <circle key="b" cx="9" cy="9.5" r="3" fill={RED} />, <circle key="c" cx="15" cy="9.5" r="3" fill="#f4c430" />, <circle key="d" cx="12" cy="15" r="3" fill={SK} />),
  scene: (s) => svg(s, <rect key="a" x="2" y="4" width="20" height="16" rx="2" fill="#dfe8f5" stroke={O} strokeWidth="0.9" />, <circle key="b" cx="17" cy="8.5" r="2.2" fill="#f4c430" />, <path key="c" d="M2 18 L9 11 L13 15 L16 12 L22 18" fill="#9ab07a" stroke={O} strokeWidth="0.8" />),
  viewSettings: (s) => svg(s, <rect key="a" x="2.5" y="4" width="19" height="13" rx="1.5" fill="#eef5ff" stroke={O} strokeWidth="1.1" />, <path key="b" d="M8 21 H16 M12 17 V21" stroke={O} strokeWidth="1.3" />),
  // ---------------------------------------------------------------- file / general
  new: (s) => svg(s, <path key="a" d="M5 2.5 H14 L19.5 8 V21.5 H5 Z" fill="#fff" stroke={O} />, <path key="b" d="M14 2.5 V8 H19.5" fill="#e6e9ee" stroke={O} />, sparkle(17.5, 17.5)),
  open: (s) => svg(s, <path key="a" d="M2.5 6 H9 L11 8 H20.5 V19.5 H2.5 Z" fill="#f6d77a" stroke="#a07a00" />, <path key="b" d="M2.5 19.5 L5.5 11 H23 L20.5 19.5 Z" fill="#ffe8a3" stroke="#a07a00" />),
  save: (s) => svg(s, <path key="a" d="M3.5 3.5 H17 L20.5 7 V20.5 H3.5 Z" fill="url(#oc-b)" stroke={SKD} />, <rect key="b" x="7" y="3.5" width="9" height="5.5" fill="#fff" stroke={SKD} strokeWidth="0.6" />, <rect key="c" x="6.5" y="13" width="11" height="7.5" fill="#eef4fd" stroke={SKD} strokeWidth="0.6" />),
  undo: (s) => svg(s, <path key="a" d="M8.5 6 L3.5 11 L8.5 16" stroke={O} strokeWidth="1.8" />, <path key="b" d="M3.5 11 H14 A6 6 0 0 1 14 23" stroke={O} strokeWidth="1.8" />),
  redo: (s) => svg(s, <path key="a" d="M15.5 6 L20.5 11 L15.5 16" stroke={O} strokeWidth="1.8" />, <path key="b" d="M20.5 11 H10 A6 6 0 0 0 10 23" stroke={O} strokeWidth="1.8" />),
  rebuild: (s) => svg(s, <rect key="a" x="8" y="1.5" width="8" height="21" rx="2.5" fill="#2e3238" />, <circle key="b" cx="12" cy="6.5" r="2.2" fill="#ff4a3d" />, <circle key="c" cx="12" cy="12" r="2.2" fill="#ffcf33" />, <circle key="d" cx="12" cy="17.5" r="2.2" fill="#3fd04a" />),
  options: (s) => svg(s, <path key="a" d="M12 2.5 L13.6 5 L16.5 4.2 L17 7.2 L19.8 8.3 L18.6 11 L20.5 13.4 L18 15 L18.2 18 L15.2 18.2 L13.6 20.8 L12 19 L10.4 20.8 L8.8 18.2 L5.8 18 L6 15 L3.5 13.4 L5.4 11 L4.2 8.3 L7 7.2 L7.5 4.2 L10.4 5 Z" fill="url(#oc-g)" stroke={O} strokeWidth="0.9" />, <circle key="b" cx="12" cy="12" r="3.2" fill="#fff" stroke={O} />),
  select: (s) => svg(s, <path key="a" d="M5 2.5 V19 L9.2 15 L12.2 21.5 L14.6 20.4 L11.6 14 H17.5 Z" fill="#fff" stroke="#111" strokeWidth="1.1" />),
  home: (s) => svg(s, <path key="a" d="M3 11.5 L12 3.5 L21 11.5" stroke={O} strokeWidth="1.6" />, <path key="b" d="M5.5 10 V20.5 H10 V14.5 H14 V20.5 H18.5 V10" fill="#fff" stroke={O} strokeWidth="1.4" />),
  print: (s) => svg(s, <rect key="a" x="6" y="2.5" width="12" height="6" fill="#fff" stroke={O} />, <rect key="b" x="2.5" y="8.5" width="19" height="9" rx="1.5" fill="url(#oc-g)" stroke={O} />, <rect key="c" x="6.5" y="14" width="11" height="7.5" fill="#fff" stroke={O} />),
  measure: (s) => svg(s, <rect key="a" x="1.5" y="8.5" width="21" height="7" rx="1" fill="url(#oc-y)" stroke="#a07a00" />, <path key="b" d="M5 8.5 V11.5 M9 8.5 V12.5 M13 8.5 V11.5 M17 8.5 V12.5" stroke="#7a5a00" />),
  massProps: (s) => svg(s, <path key="a" d="M4.5 21 L7.5 9 H16.5 L19.5 21 Z" fill="url(#oc-gd)" stroke={O} />, <circle key="b" cx="12" cy="6" r="3" fill="#fff" stroke={O} strokeWidth="1.4" />, <text key="c" x="12" y="18.5" fontSize="7" fontWeight="bold" textAnchor="middle" fill="#fff">kg</text>),
  generic: (s) => svg(s, <rect key="a" x="5" y="5" width="14" height="14" rx="2" stroke="#9aa3ae" />),
  check: (s) => svg(s, <path key="a" d="M4 12.5 L10 18.5 L20.5 5.5" stroke={GRN} strokeWidth="3.2" />),
  cancel: (s) => svg(s, <path key="a" d="M5 5 L19 19 M19 5 L5 19" stroke={RED} strokeWidth="3.2" />),
  help: (s) => svg(s, <circle key="a" cx="12" cy="12" r="9.5" fill="#fff" stroke={SK} strokeWidth="1.5" />, <path key="b" d="M9.3 9.3 A2.7 2.7 0 1 1 12 12 V14.3" stroke={SK} strokeWidth="2" />, <circle key="c" cx="12" cy="17.8" r="1.2" fill={SK} />),
  part: (s) => svg(s, yellowBlock(0, 1, 8)),
  history: (s) => svg(s, <rect key="a" x="3.5" y="3.5" width="17" height="17" rx="1.5" fill="#fff" stroke={O} />, <circle key="b" cx="12" cy="12" r="5.5" stroke={SK} strokeWidth="1.2" />, <path key="c" d="M12 8.5 V12 L14.5 13.5" stroke={O} strokeWidth="1.2" />),
  sensor: (s) => svg(s, <path key="a" d="M4 18 A8 8 0 0 1 20 18" stroke={O} strokeWidth="1.6" />, <path key="b" d="M12 18 L15.5 10.5" stroke={RED} strokeWidth="1.8" />, <circle key="c" cx="12" cy="18" r="1.6" fill={O} />),
  annotations: (s) => svg(s, <rect key="a" x="2.5" y="4.5" width="19" height="15" rx="1" fill="#fff" stroke={O} />, <path key="b" d="M6.5 16 L10 8 L13.5 16 M7.8 13 H12.2" stroke={SK} strokeWidth="1.4" />),
  material: (s) => svg(s, <path key="a" d="M2.5 9 L8 5 H21.5 L16 9 Z" fill="#e2c49a" stroke="#7a5a3a" strokeWidth="0.8" />, <rect key="b" x="2.5" y="9" width="13.5" height="9" fill="#c9a37a" stroke="#7a5a3a" strokeWidth="0.8" />, <path key="c" d="M16 9 L21.5 5 V14 L16 18 Z" fill="#a9825a" stroke="#7a5a3a" strokeWidth="0.8" />),
  origin: (s) => svg(s, <path key="a" d="M5 19 V4 M5 19 H20" stroke={O} strokeWidth="1.5" />, <path key="b" d="M2.8 6.5 L5 3.5 L7.2 6.5 M17.5 16.8 L20.5 19 L17.5 21.2" stroke={O} strokeWidth="1.3" />),
  error: (s) => svg(s, <circle key="a" cx="12" cy="12" r="10" fill={RED} />, <path key="b" d="M8 8 L16 16 M16 8 L8 16" stroke="#fff" strokeWidth="2.6" />),
  pin: (s) => svg(s, <path key="a" d="M9 3 H15 L14 10 L18 14 H6 L10 10 Z" fill="#fff" stroke={O} />, <path key="b" d="M12 14 V21" stroke={O} strokeWidth="1.4" />),
  search: (s) => svg(s, <circle key="a" cx="10" cy="10" r="6" stroke={O} strokeWidth="1.8" />, <path key="b" d="M14.5 14.5 L20.5 20.5" stroke={O} strokeWidth="2.2" />),
  // ---------------------------------------------------------------- sketch tool families
  midpointLine: (s) => svg(s, <path key="a" d="M4 20 L20 4" stroke={SK} strokeWidth="1.8" />, pt(4, 20), pt(20, 4), <circle key="m" cx="12" cy="12" r="2.2" fill="#fff" stroke={RED} strokeWidth="1.3" />),
  rect3: (s) => svg(s, sketchRect("M3 14 L12 4 L21 11 L12 21 Z"), pt(3, 14), pt(12, 4), pt(21, 11)),
  rectCenter3: (s) => svg(s, sketchRect("M3 14 L12 4 L21 11 L12 21 Z"), <path key="a" d="M3 14 L21 11 M12 4 L12 21" stroke={SK} strokeWidth="0.8" strokeDasharray="2 1.5" />, pt(12, 12.5)),
  parallelogram: (s) => svg(s, sketchRect("M8 6 H21 L16 18 H3 Z"), pt(3, 18), pt(16, 18), pt(21, 6)),
  perimeterCircle: (s) => svg(s, <circle key="a" cx="12" cy="12" r="8" stroke={SK} strokeWidth="1.7" />, pt(12, 4), pt(5.1, 16), pt(18.9, 16)),
  slotCenter: (s) => svg(s, <path key="a" d="M8 7 H16 A5 5 0 0 1 16 17 H8 A5 5 0 0 1 8 7 Z" stroke={SK} strokeWidth="1.6" />, <path key="b" d="M8 12 H16" stroke={SK} strokeWidth="0.8" strokeDasharray="2 1.5" />, pt(12, 12, RED), pt(16, 12)),
  slotArc3: (s) => svg(s, <path key="a" d="M3 17 A10 10 0 0 1 21 17 L17.6 18.6 A6.2 6.2 0 0 0 6.4 18.6 Z" stroke={SK} strokeWidth="1.5" />, pt(4.7, 17.8), pt(19.3, 17.8), pt(12, 9.8)),
  slotArcCenter: (s) => svg(s, <path key="a" d="M3 15 A10 10 0 0 1 21 15 L17.6 16.6 A6.2 6.2 0 0 0 6.4 16.6 Z" stroke={SK} strokeWidth="1.5" />, pt(12, 20, RED), pt(4.7, 15.8), pt(19.3, 15.8)),
  styleSpline: (s) => svg(s, <path key="a" d="M3 18 C 7 2, 12 22, 21 6" stroke={SK} strokeWidth="1.7" />, <path key="b" d="M3 18 L8 5 L14 20 L21 6" stroke={O} strokeWidth="0.7" strokeDasharray="2 1.5" />, pt(8, 5, O), pt(14, 20, O)),
  splineSurface: (s) => svg(s, <path key="a" d="M2 17 L8 7 H22 L16 17 Z" fill="#e7eef9" stroke="#5c8fd8" strokeWidth="0.9" />, <path key="b" d="M6 15 C 9 8, 13 16, 18 9" stroke={SK} strokeWidth="1.7" />),
  equationCurve: (s) => svg(s, <path key="a" d="M3 19 C 9 19, 9 5, 21 5" stroke={SK} strokeWidth="1.6" />, <text key="b" x="13" y="20" fontSize="9" fontStyle="italic" fill={O}>fx</text>),
  partialEllipse: (s) => svg(s, <path key="a" d="M3 12 A9 5.5 0 0 1 21 12" stroke={SK} strokeWidth="1.7" />, <path key="b" d="M3 12 A9 5.5 0 0 0 21 12" stroke={SK} strokeWidth="0.7" strokeDasharray="2 1.5" />, pt(3, 12), pt(21, 12), pt(12, 12)),
  parabola: (s) => svg(s, <path key="a" d="M4 4 Q12 34 20 4" stroke={SK} strokeWidth="1.7" />, pt(12, 13, O)),
  conic: (s) => svg(s, <path key="a" d="M4 20 Q10 2 21 12" stroke={SK} strokeWidth="1.7" />, <path key="b" d="M4 20 L10 2 L21 12" stroke={O} strokeWidth="0.7" strokeDasharray="2 1.5" />, pt(4, 20), pt(21, 12)),
  sketchChamfer: (s) => svg(s, <path key="a" d="M4 21 V11 L11 4 H21" stroke={SK} strokeWidth="1.7" />, <path key="b" d="M4 4 L7.5 7.5" stroke={O} strokeWidth="0.8" strokeDasharray="1.5 1.2" />),
  sketchPlane: (s) => svg(s, <path key="a" d="M2 17 L8 6 H22 L16 17 Z" fill="#e7eef9" stroke="#5c8fd8" strokeWidth="1.1" />, <path key="b" d="M9 14 L15 9" stroke={SK} strokeWidth="1.4" />),
  extend: (s) => svg(s, <path key="a" d="M3 19 L12 10" stroke={SK} strokeWidth="1.7" />, <path key="b" d="M12 10 L18 4" stroke={SK} strokeWidth="1.4" strokeDasharray="2 1.5" />, <path key="c" d="M16 2 L22 8" stroke={O} strokeWidth="1.3" />),
  silhouette: (s) => svg(s, <circle key="a" cx="12" cy="12" r="7" fill="url(#oc-g)" stroke={O} strokeWidth="0.7" />, <circle key="b" cx="12" cy="12" r="8.6" stroke={SK} strokeWidth="1.5" />),
  intersectionCurve: (s) => svg(s, <path key="a" d="M3 8 L12 4 L21 8 L12 12 Z" fill="url(#oc-g)" stroke={O} strokeWidth="0.7" />, <path key="b" d="M8 20 L8 6 L18 3 L18 17 Z" fill="#e7eef9" opacity="0.8" stroke="#5c8fd8" strokeWidth="0.8" />, <path key="c" d="M8 10.2 L18 6.6" stroke={SK} strokeWidth="1.8" />),
  circularSketchPattern: (s) => svg(s, <circle key="a" cx="12" cy="5" r="2.5" stroke={SK} strokeWidth="1.5" />, <circle key="b" cx="5" cy="16" r="2.5" stroke={SK} strokeWidth="1" strokeDasharray="1.5 1" />, <circle key="c" cx="19" cy="16" r="2.5" stroke={SK} strokeWidth="1" strokeDasharray="1.5 1" />, <path key="d" d="M16 8 A6.5 6.5 0 0 1 17 12" stroke={O} strokeWidth="0.9" />),
  copyEntities: (s) => svg(s, sketchRect("M3 9 H13 V19 H3 Z"), <path key="a" d="M9 5 H21 V15 H15" stroke={SK} strokeWidth="1.2" strokeDasharray="2.5 1.5" />),
  rotateEntities: (s) => svg(s, sketchRect("M5 12 L12 5 L19 12 L12 19 Z"), <path key="a" d="M3 5 A10 10 0 0 1 12 1.5 M12 1.5 L9.5 0.5 M12 1.5 L10.5 3.8" stroke={O} strokeWidth="1" />),
  scaleEntities: (s) => svg(s, sketchRect("M3 13 H11 V21 H3 Z"), <path key="a" d="M3 3 H21 V21" stroke={SK} strokeWidth="1.2" strokeDasharray="2.5 1.5" />, <path key="b" d="M12 12 L19 5 M19 5 H15.5 M19 5 V8.5" stroke={O} strokeWidth="1" />),
  stretchEntities: (s) => svg(s, sketchRect("M3 8 H14 V16 H3 Z"), <path key="a" d="M14 8 H21 V16 H14" stroke={SK} strokeWidth="1.2" strokeDasharray="2.5 1.5" />, <path key="b" d="M15 12 H23 M21 10 L23 12 L21 14" stroke={O} strokeWidth="1" />),
  addRelation: (s) => svg(s, <rect key="a" x="2" y="6" width="12" height="12" rx="1" fill="#d4f2cc" stroke="#3a7a30" />, <path key="b" d="M5 12 H11" stroke="#1a3a10" strokeWidth="1.5" />, <path key="c" d="M19 3 V11 M15 7 H23" stroke={GRN} strokeWidth="2" />),
  fullyDefine: (s) => svg(s, sketchRect("M4 7 H16 V19 H4 Z"), <path key="a" d="M4 4 H16 M4 3 V5 M16 3 V5 M19 7 V19 M18 7 H20 M18 19 H20" stroke={O} strokeWidth="0.9" />),
  sketch3d: (s) => svg(s, <path key="a" d="M4 17 L12 21 L20 17 M12 21 V11 M4 17 V8 L12 4 L20 8 V17" stroke={SK} strokeWidth="1.2" />, <text key="b" x="12" y="13" fontSize="7" fontWeight="bold" textAnchor="middle" fill={O}>3D</text>),
  dimH: (s) => svg(s, <path key="a" d="M4 6 V20 M20 12 V20" stroke={O} strokeWidth="0.9" />, <path key="b" d="M4 16 H20" stroke={SK} strokeWidth="1.3" />, <path key="c" d="M4 16 L7 14.5 V17.5 Z M20 16 L17 14.5 V17.5 Z" fill={SK} />, <path key="d" d="M4 6 L20 12" stroke={O} strokeWidth="1.2" />),
  dimV: (s) => svg(s, <path key="a" d="M6 4 H20 M12 20 H20" stroke={O} strokeWidth="0.9" />, <path key="b" d="M16 4 V20" stroke={SK} strokeWidth="1.3" />, <path key="c" d="M16 4 L14.5 7 H17.5 Z M16 20 L14.5 17 H17.5 Z" fill={SK} />, <path key="d" d="M6 4 L12 20" stroke={O} strokeWidth="1.2" />),
  dimBaseline: (s) => svg(s, <path key="a" d="M4 4 V20 M12 12 V20 M20 6 V20" stroke={O} strokeWidth="0.9" />, <path key="b" d="M4 15 H12 M4 9 H20" stroke={SK} strokeWidth="1.3" />),
  dimChain: (s) => svg(s, <path key="a" d="M4 8 V20 M12 8 V20 M20 8 V20" stroke={O} strokeWidth="0.9" />, <path key="b" d="M4 12 H20" stroke={SK} strokeWidth="1.3" />, <path key="c" d="M12 12 L10 10.8 V13.2 Z M12 12 L14 10.8 V13.2 Z" fill={SK} />),
  dimOrdinate: (s) => svg(s, <path key="a" d="M4 20 V6 M10 20 V10 M17 20 V10" stroke={SK} strokeWidth="1.2" />, <text key="b" x="2" y="5.5" fontSize="5.5" fill={O}>0</text>, <text key="c" x="8" y="9" fontSize="5.5" fill={O}>10</text>, <text key="d" x="15" y="9" fontSize="5.5" fill={O}>25</text>),
  dimPath: (s) => svg(s, <path key="a" d="M3 18 C 7 6, 14 22, 21 8" stroke={SK} strokeWidth="1.6" />, <path key="b" d="M3 12 C 7 0, 14 16, 21 2" stroke={O} strokeWidth="0.9" strokeDasharray="2 1.5" />),
  dimPreview: (s) => svg(s, <path key="a" d="M3 16 H21 M3 12 V20 M21 12 V20" stroke={SK} strokeWidth="1.2" />, <path key="b" d="M2 8 C6 3 18 3 22 8 C18 11 6 11 2 8 Z" fill="#fff" stroke={O} strokeWidth="0.9" />, <circle key="c" cx="12" cy="7.2" r="1.8" fill={O} />),
  // ---------------------------------------------------------------- context toolbars
  selectOther: (s) => svg(s, greyBlock(0, 2, 8), <path key="a" d="M14 10 V21 L17 18.3 L19 22.5 L20.8 21.6 L18.8 17.5 H22.5 Z" fill="#fff" stroke="#111" strokeWidth="0.9" />),
  block: (s) => svg(s, <rect key="a" x="3" y="3" width="18" height="18" rx="1" stroke={O} strokeDasharray="3 2" />, <path key="b" d="M7 16 V9 H13 A3.5 3.5 0 0 1 13 16 Z" stroke={SK} strokeWidth="1.5" />),
  replace: (s) => svg(s, <path key="a" d="M4 8 H15 M12 5 L15 8 L12 11" stroke={SK} strokeWidth="1.5" />, <path key="b" d="M20 16 H9 M12 13 L9 16 L12 19" stroke={O} strokeWidth="1.5" />),
  numeric: (s) => svg(s, <rect key="a" x="2.5" y="7" width="19" height="10" rx="1" fill="#fff" stroke={O} />, <text key="b" x="12" y="15" fontSize="7.5" textAnchor="middle" fill={SK} fontWeight="bold">12.5</text>),
  zoomSelection: (s) => svg(s, <circle key="b" cx="10" cy="10" r="6.5" fill="#eef5ff" stroke={O} strokeWidth="1.5" />, <path key="c" d="M15 15 L21 21" stroke={O} strokeWidth="2.4" />, <path key="d" d="M7 12 L13 8" stroke={SK} strokeWidth="1.8" />),
  zoomInOut: (s) => svg(s, <circle key="b" cx="10" cy="10" r="6.5" fill="#eef5ff" stroke={O} strokeWidth="1.5" />, <path key="c" d="M15 15 L21 21" stroke={O} strokeWidth="2.4" />, <path key="d" d="M10 6.5 V13.5 M8 8.5 L10 6.5 L12 8.5 M8 11.5 L10 13.5 L12 11.5" stroke={SK} strokeWidth="1.2" />),
  rotateView: (s) => svg(s, <path key="a" d="M19.5 12 A7.5 7.5 0 1 1 12 4.5" stroke={SK} strokeWidth="2" />, <path key="b" d="M12 1.5 L16 4.5 L12 7.5 Z" fill={SK} />),
  pan: (s) => svg(s, <path key="a" d="M12 2 V22 M2 12 H22" stroke={SK} strokeWidth="1.6" />, <path key="b" d="M12 2 L9.5 5 H14.5 Z M12 22 L9.5 19 H14.5 Z M2 12 L5 9.5 V14.5 Z M22 12 L19 9.5 V14.5 Z" fill={SK} />),
  rollView: (s) => svg(s, <path key="a" d="M4.5 12 A7.5 7.5 0 1 0 12 4.5" stroke={SK} strokeWidth="2" />, <path key="b" d="M12 1.5 L8 4.5 L12 7.5 Z" fill={SK} />, <circle key="c" cx="12" cy="12" r="1.6" fill={O} />),
  drawing: (s) => svg(s, <rect key="a" x="3" y="4" width="18" height="16" fill="#fff" stroke={O} />, <rect key="b" x="6" y="7" width="6" height="5" stroke={SK} />, <circle key="c" cx="16" cy="9.5" r="2.5" stroke={SK} />, <path key="d" d="M14 17 H19 M14 15 H19" stroke={O} strokeWidth="0.8" />),
  lasso: (s) => svg(s, <path key="a" d="M12 4 C 4 4, 2 9, 5 12 C 8 15, 20 14, 20 9 C 20 6, 16 4, 12 4 Z" stroke={O} strokeWidth="1.3" strokeDasharray="3 1.8" />, <path key="b" d="M6 13 C 5 16, 8 17, 7 21" stroke={O} strokeWidth="1.3" />),
  invert: (s) => svg(s, <rect key="a" x="3" y="3" width="11" height="11" fill={SK} opacity="0.8" />, <rect key="b" x="10" y="10" width="11" height="11" fill="#fff" stroke={SK} strokeWidth="1.2" />),
  filter: (s) => svg(s, <path key="a" d="M3 4 H21 L14 12.5 V20 L10 17.5 V12.5 Z" fill="#ffe17a" stroke="#a07a00" />),
  boxSelect: (s) => svg(s, <rect key="a" x="3.5" y="4.5" width="17" height="15" fill="#e3ecfb" stroke={O} strokeWidth="1.1" />),
  normalTo: (s) => svg(s, <path key="a" d="M2 17 L7 12 H22 L17 17 Z" fill="#c9daf3" stroke="#2f6dbf" />, <path key="b" d="M12 14.5 V3 M8.8 6.4 L12 3 L15.2 6.4" stroke={O} strokeWidth="1.5" />),
  editFeature: (s) => svg(s, yellowBlock(0, 3, 6), <path key="a" d="M14 21 L22 10 L23.5 11.2 L15.6 22 L13.6 22.6 Z" fill="#8fc0ff" stroke={SKD} strokeWidth="0.7" />),
  editSketch: (s) => svg(s, sketchRect("M3 6 H14 V17 H3 Z"), <path key="a" d="M12 21 L20 9 L22 10.3 L14 22 L11.8 22.8 Z" fill="url(#oc-y)" stroke={O} strokeWidth="0.7" />),
  suppress: (s) => svg(s, greyBlock(0, 1, 6), <path key="a" d="M12 14 V23 M8.5 19.5 L12 23 L15.5 19.5" stroke={SK} strokeWidth="1.8" />),
  rollback: (s) => svg(s, <path key="a" d="M9 6 L4 11 L9 16" stroke={O} strokeWidth="1.8" />, <path key="b" d="M4 11 H20" stroke={O} strokeWidth="1.8" />, <path key="c" d="M3 20 H21" stroke={SK} strokeWidth="2.4" />),
  delete: (s) => svg(s, <path key="a" d="M5 5 L19 19 M19 5 L5 19" stroke={RED} strokeWidth="3" />),
  hide: (s) => svg(s, <path key="a" d="M2 12 C6 5 18 5 22 12 C18 19 6 19 2 12 Z" fill="#fff" stroke={O} strokeWidth="1.2" />, <circle key="b" cx="12" cy="12" r="3.2" fill="#9aa3ae" />, <path key="c" d="M4 20 L20 4" stroke={RED} strokeWidth="1.8" />),
  comment: (s) => svg(s, <path key="a" d="M3 4 H21 V16 H11 L6 21 V16 H3 Z" fill="#fff6c4" stroke="#a07a00" />),
  parentChild: (s) => svg(s, <rect key="a" x="8" y="2.5" width="8" height="6" fill="#fff" stroke={O} />, <rect key="b" x="2.5" y="15.5" width="8" height="6" fill="#fff" stroke={O} />, <rect key="c" x="13.5" y="15.5" width="8" height="6" fill="#fff" stroke={O} />, <path key="d" d="M12 8.5 V12 M6.5 15.5 V12 H17.5 V15.5" stroke={O} />),
  configure: (s) => svg(s, yellowBlock(-3, 4, 5), <path key="a" d="M13 21 L21 9 L23 10.3 L15 22 L12.8 22.8 Z" fill="url(#oc-g)" stroke={O} strokeWidth="0.7" />),
  favorites: (s) => svg(s, <path key="a" d="M12 2.5 L14.8 9 L21.7 9.6 L16.4 14.1 L18.1 21 L12 17.2 L5.9 21 L7.6 14.1 L2.3 9.6 L9.2 9 Z" fill="url(#oc-y)" stroke="#a07a00" />),
  folder: (s) => svg(s, <path key="a" d="M2.5 6 H9 L11 8 H21.5 V19.5 H2.5 Z" fill="#8fc0ff" stroke={SKD} />),
  properties: (s) => svg(s, <rect key="a" x="4" y="3" width="16" height="18" fill="#fff" stroke={O} />, <path key="b" d="M7 8 H17 M7 12 H17 M7 16 H13" stroke={SK} strokeWidth="1.3" />),
  transparency: (s) => svg(s, <circle key="a" cx="9" cy="12" r="6.5" fill="url(#oc-y)" stroke="#a07a00" />, <circle key="b" cx="15.5" cy="12" r="6" fill="#fff" opacity="0.6" stroke={O} strokeDasharray="2 1.5" />),
};

const REL_GLYPH: Record<string, ReactNode> = {
  horizontal: <path d="M6 12 H18" stroke="#12300c" strokeWidth="2" />,
  horizontalPoints: <path d="M6 12 H18" stroke="#12300c" strokeWidth="2" />,
  vertical: <path d="M12 6 V18" stroke="#12300c" strokeWidth="2" />,
  verticalPoints: <path d="M12 6 V18" stroke="#12300c" strokeWidth="2" />,
  collinear: <path d="M5 16 L11 11 M13 9.5 L19 4.5" stroke="#12300c" strokeWidth="2" />,
  perpendicular: <path d="M6 18 H18 M12 18 V6" stroke="#12300c" strokeWidth="2" />,
  parallel: <path d="M8 18 L13 6 M12 18 L17 6" stroke="#12300c" strokeWidth="2" />,
  equal: <path d="M7 9.5 H17 M7 14.5 H17" stroke="#12300c" strokeWidth="2" />,
  tangent: (
    <>
      <circle cx="12" cy="14" r="4.5" stroke="#12300c" strokeWidth="1.6" />
      <path d="M5 9.5 H19" stroke="#12300c" strokeWidth="1.8" />
    </>
  ),
  concentric: (
    <>
      <circle cx="12" cy="12" r="6" stroke="#12300c" strokeWidth="1.5" />
      <circle cx="12" cy="12" r="2.5" stroke="#12300c" strokeWidth="1.5" />
    </>
  ),
  coradial: (
    <>
      <circle cx="12" cy="12" r="5.5" stroke="#12300c" strokeWidth="2.2" />
      <circle cx="12" cy="12" r="1.2" fill="#12300c" />
    </>
  ),
  midpoint: (
    <>
      <path d="M5 17 L19 7" stroke="#12300c" strokeWidth="1.6" />
      <circle cx="12" cy="12" r="2.4" fill="#12300c" />
    </>
  ),
  coincident: (
    <>
      <path d="M5 17 L19 7" stroke="#12300c" strokeWidth="1.6" />
      <circle cx="15.5" cy="9.5" r="2.4" fill="#12300c" />
    </>
  ),
  symmetric: <path d="M12 5 V19 M5 9 L9 12 L5 15 M19 9 L15 12 L19 15" stroke="#12300c" strokeWidth="1.6" fill="none" />,
  intersection: (
    <>
      <path d="M6 6 L18 18 M18 6 L6 18" stroke="#12300c" strokeWidth="1.6" />
      <circle cx="12" cy="12" r="2.2" fill="#12300c" />
    </>
  ),
  fix: <path d="M12 5 V17 M7 12 A5 5 0 0 0 17 12 M9.5 8 H14.5" stroke="#12300c" strokeWidth="1.7" fill="none" />,
  equidistant: <path d="M12 12 L6 7 M12 12 L18 7 M12 12 L12 19" stroke="#12300c" strokeWidth="1.6" />,
  offset: <path d="M5 16 L12 9 L19 16 M5 20 L12 13 L19 20" fill="none" stroke="#12300c" strokeWidth="1.5" />,
};

/** Relation icon ("rel:<type>"): green tile with the relation's symbol. */
function relIcon(type: string, size: number) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" strokeLinecap="round">
      <rect x="2.5" y="2.5" width="19" height="19" rx="2" fill="#d4f2cc" stroke="#3a7a30" />
      {REL_GLYPH[type] ?? null}
    </svg>
  );
}

export function Icon({ name, size = 20 }: { name: string; size?: number }) {
  if (name.startsWith("rel:")) return relIcon(name.slice(4), size);
  const f = ICONS[name] ?? ICONS.generic;
  return f(size);
}
