/**
 * Material library (common engineering materials with typical properties). Values are
 * nominal handbook figures in SI units: density kg/m³, elastic modulus and yield
 * strength Pa.
 */
export interface Material {
  category: string;
  name: string;
  density: number;
  modulus: number;
  poisson: number;
  yield: number;
}

const GPa = 1e9, MPa = 1e6;

export const MATERIALS: Material[] = [
  { category: "Steel", name: "1023 Carbon Steel Sheet (SS)", density: 7858, modulus: 205 * GPa, poisson: 0.29, yield: 282.69 * MPa },
  { category: "Steel", name: "AISI 1020", density: 7900, modulus: 200 * GPa, poisson: 0.29, yield: 351.57 * MPa },
  { category: "Steel", name: "AISI 1045 Steel, cold drawn", density: 7850, modulus: 205 * GPa, poisson: 0.29, yield: 530 * MPa },
  { category: "Steel", name: "AISI 304", density: 8000, modulus: 190 * GPa, poisson: 0.29, yield: 206.81 * MPa },
  { category: "Steel", name: "AISI 316 Stainless Steel Sheet (SS)", density: 8000, modulus: 193 * GPa, poisson: 0.27, yield: 137.9 * MPa },
  { category: "Steel", name: "AISI 4340 Steel, normalized", density: 7850, modulus: 205 * GPa, poisson: 0.32, yield: 710 * MPa },
  { category: "Steel", name: "Plain Carbon Steel", density: 7800, modulus: 210 * GPa, poisson: 0.28, yield: 220.59 * MPa },
  { category: "Steel", name: "Alloy Steel", density: 7700, modulus: 210 * GPa, poisson: 0.28, yield: 620.42 * MPa },
  { category: "Iron", name: "Gray Cast Iron", density: 7200, modulus: 66.17 * GPa, poisson: 0.27, yield: 0 },
  { category: "Iron", name: "Ductile Iron", density: 7100, modulus: 120 * GPa, poisson: 0.31, yield: 551.49 * MPa },
  { category: "Aluminium Alloys", name: "1060 Alloy", density: 2700, modulus: 69 * GPa, poisson: 0.33, yield: 27.57 * MPa },
  { category: "Aluminium Alloys", name: "5052-H32", density: 2680, modulus: 70.3 * GPa, poisson: 0.33, yield: 193 * MPa },
  { category: "Aluminium Alloys", name: "6061 Alloy", density: 2700, modulus: 69 * GPa, poisson: 0.33, yield: 55.15 * MPa },
  { category: "Aluminium Alloys", name: "6061-T6 (SS)", density: 2700, modulus: 69 * GPa, poisson: 0.33, yield: 275 * MPa },
  { category: "Aluminium Alloys", name: "6063-T5", density: 2700, modulus: 69 * GPa, poisson: 0.33, yield: 145 * MPa },
  { category: "Aluminium Alloys", name: "7075-T6 (SN)", density: 2810, modulus: 72 * GPa, poisson: 0.33, yield: 505 * MPa },
  { category: "Copper Alloys", name: "Brass", density: 8500, modulus: 100 * GPa, poisson: 0.33, yield: 239.4 * MPa },
  { category: "Copper Alloys", name: "Copper", density: 8900, modulus: 110 * GPa, poisson: 0.37, yield: 258.6 * MPa },
  { category: "Copper Alloys", name: "Bronze", density: 8800, modulus: 110 * GPa, poisson: 0.34, yield: 125 * MPa },
  { category: "Titanium Alloys", name: "Ti-6Al-4V Solution treated and aged (SS)", density: 4430, modulus: 104.8 * GPa, poisson: 0.31, yield: 827 * MPa },
  { category: "Plastics", name: "ABS", density: 1020, modulus: 2 * GPa, poisson: 0.394, yield: 0 },
  { category: "Plastics", name: "ABS PC", density: 1070, modulus: 2.41 * GPa, poisson: 0.3897, yield: 0 },
  { category: "Plastics", name: "Acrylic (Medium-high impact)", density: 1200, modulus: 3 * GPa, poisson: 0.35, yield: 45 * MPa },
  { category: "Plastics", name: "Delrin 2700 NC010, Low Viscosity Acetal Copolymer", density: 1420, modulus: 2.6 * GPa, poisson: 0.35, yield: 63 * MPa },
  { category: "Plastics", name: "Nylon 6/10", density: 1400, modulus: 8.3 * GPa, poisson: 0.28, yield: 139 * MPa },
  { category: "Plastics", name: "PA Type 6", density: 1120, modulus: 2.62 * GPa, poisson: 0.3897, yield: 0 },
  { category: "Plastics", name: "PC High Viscosity", density: 1190, modulus: 2.32 * GPa, poisson: 0.3912, yield: 62 * MPa },
  { category: "Plastics", name: "PE High Density", density: 952, modulus: 1.07 * GPa, poisson: 0.41, yield: 22 * MPa },
  { category: "Plastics", name: "PP Copolymer", density: 890, modulus: 0.896 * GPa, poisson: 0.4103, yield: 27.6 * MPa },
  { category: "Plastics", name: "PVC Rigid", density: 1300, modulus: 2.41 * GPa, poisson: 0.3825, yield: 40.7 * MPa },
  { category: "Plastics", name: "PLA", density: 1240, modulus: 3.5 * GPa, poisson: 0.36, yield: 60 * MPa },
  { category: "Plastics", name: "PETG", density: 1270, modulus: 2.1 * GPa, poisson: 0.38, yield: 50 * MPa },
  { category: "Other Non-metals", name: "Glass", density: 2457.6, modulus: 68.9 * GPa, poisson: 0.23, yield: 0 },
  { category: "Other Non-metals", name: "Ceramic Porcelain", density: 2300, modulus: 22 * GPa, poisson: 0.22, yield: 0 },
  { category: "Woods", name: "Oak", density: 700, modulus: 11 * GPa, poisson: 0.3, yield: 0 },
  { category: "Woods", name: "Pine", density: 530, modulus: 9 * GPa, poisson: 0.3, yield: 0 },
  { category: "Woods", name: "Balsa", density: 160, modulus: 3 * GPa, poisson: 0.3, yield: 0 },
];

export function materialByName(name: string | undefined | null): Material | null {
  return MATERIALS.find((m) => m.name === name) ?? null;
}
