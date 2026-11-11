/**
 * Hole Wizard data (ISO metric). Clearance holes follow ISO 273 (close / normal /
 * loose); counterbores suit ISO 4762 socket head cap screws; countersinks suit
 * ISO 10642 countersunk head screws (90°); tap drills are for coarse threads.
 */
export type HoleType = "cbore" | "csk" | "hole" | "tap";
export type HoleFit = "close" | "normal" | "loose";

export interface HoleSize {
  size: string;
  nominal: number;
  clearance: Record<HoleFit, number>;
  cboreDia: number;
  cboreDepth: number;
  cskDia: number;
  tapDrill: number;
}

export const ISO_SIZES: HoleSize[] = [
  { size: "M2", nominal: 2, clearance: { close: 2.2, normal: 2.4, loose: 2.6 }, cboreDia: 4.3, cboreDepth: 2.3, cskDia: 4.4, tapDrill: 1.6 },
  { size: "M2.5", nominal: 2.5, clearance: { close: 2.7, normal: 2.9, loose: 3.1 }, cboreDia: 5, cboreDepth: 2.8, cskDia: 5.5, tapDrill: 2.05 },
  { size: "M3", nominal: 3, clearance: { close: 3.2, normal: 3.4, loose: 3.6 }, cboreDia: 6.5, cboreDepth: 3.3, cskDia: 6.72, tapDrill: 2.5 },
  { size: "M4", nominal: 4, clearance: { close: 4.3, normal: 4.5, loose: 4.8 }, cboreDia: 8, cboreDepth: 4.4, cskDia: 8.96, tapDrill: 3.3 },
  { size: "M5", nominal: 5, clearance: { close: 5.3, normal: 5.5, loose: 5.8 }, cboreDia: 10, cboreDepth: 5.4, cskDia: 11.2, tapDrill: 4.2 },
  { size: "M6", nominal: 6, clearance: { close: 6.4, normal: 6.6, loose: 7 }, cboreDia: 11, cboreDepth: 6.5, cskDia: 13.44, tapDrill: 5 },
  { size: "M8", nominal: 8, clearance: { close: 8.4, normal: 9, loose: 10 }, cboreDia: 15, cboreDepth: 8.6, cskDia: 17.92, tapDrill: 6.8 },
  { size: "M10", nominal: 10, clearance: { close: 10.5, normal: 11, loose: 12 }, cboreDia: 18, cboreDepth: 10.8, cskDia: 22.4, tapDrill: 8.5 },
  { size: "M12", nominal: 12, clearance: { close: 13, normal: 13.5, loose: 14.5 }, cboreDia: 20, cboreDepth: 13, cskDia: 26.88, tapDrill: 10.2 },
  { size: "M14", nominal: 14, clearance: { close: 15, normal: 15.5, loose: 16.5 }, cboreDia: 24, cboreDepth: 15, cskDia: 30.8, tapDrill: 12 },
  { size: "M16", nominal: 16, clearance: { close: 17, normal: 17.5, loose: 18.5 }, cboreDia: 26, cboreDepth: 17.5, cskDia: 33.6, tapDrill: 14 },
  { size: "M20", nominal: 20, clearance: { close: 21, normal: 22, loose: 24 }, cboreDia: 33, cboreDepth: 21.5, cskDia: 40.32, tapDrill: 17.5 },
  { size: "M24", nominal: 24, clearance: { close: 25, normal: 26, loose: 28 }, cboreDia: 40, cboreDepth: 25.5, cskDia: 48, tapDrill: 21 },
];

export const HOLE_TYPE_LABEL: Record<HoleType, string> = {
  cbore: "Counterbore",
  csk: "Countersink",
  hole: "Hole",
  tap: "Straight Tap",
};

export const HOLE_FASTENER: Record<HoleType, string> = {
  cbore: "Socket Head Cap Screw - ISO 4762",
  csk: "Countersunk Head Screws - ISO 10642",
  hole: "Drill sizes",
  tap: "Tapped hole",
};

/** Resolved hole geometry (mm) for a size, type and fit. */
export function holeDims(type: HoleType, size: string, fit: HoleFit) {
  const s = ISO_SIZES.find((x) => x.size === size) ?? ISO_SIZES[5];
  const dia = type === "tap" ? s.tapDrill : type === "hole" ? s.nominal : s.clearance[fit];
  return {
    dia,
    cboreDia: type === "cbore" ? s.cboreDia : 0,
    cboreDepth: type === "cbore" ? s.cboreDepth : 0,
    cskDia: type === "csk" ? s.cskDia : 0,
    cskAngle: Math.PI / 2,
    /** Thread (cosmetic) diameter for tapped holes. */
    thread: type === "tap" ? s.nominal : 0,
  };
}
