// Windows PE helpers for the native app's installer (OVERHAUL_PLAN F3): the DLLs a program
// imports, and the closure of DLLs it needs from a set of directories.
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

/** Names of the DLLs a PE file imports (normal and delay-loaded). */
export function peImports(file) {
  const b = readFileSync(file);
  const pe = b.readUInt32LE(0x3c);
  if (b.toString("latin1", pe, pe + 4) !== "PE\0\0") throw new Error(`${file}: not a PE file`);
  const sections = b.readUInt16LE(pe + 6);
  const optSize = b.readUInt16LE(pe + 20);
  const opt = pe + 24;
  const dd = opt + (b.readUInt16LE(opt) === 0x20b ? 112 : 96); // PE32+ : PE32 data directories
  const dir = (i) => b.readUInt32LE(dd + 8 * i);
  const secs = [];
  for (let i = 0, s = opt + optSize; i < sections; i++, s += 40) {
    secs.push({ va: b.readUInt32LE(s + 12), size: Math.max(b.readUInt32LE(s + 8), b.readUInt32LE(s + 16)), raw: b.readUInt32LE(s + 20) });
  }
  const off = (rva) => {
    const s = secs.find((x) => rva >= x.va && rva < x.va + x.size);
    if (!s) throw new Error(`${file}: RVA ${rva} outside every section`);
    return rva - s.va + s.raw;
  };
  const cstr = (o) => b.toString("latin1", o, b.indexOf(0, o));
  const names = [];
  if (dir(1)) for (let d = off(dir(1)); b.readUInt32LE(d + 12); d += 20) names.push(cstr(off(b.readUInt32LE(d + 12))));
  if (dir(13)) for (let d = off(dir(13)); b.readUInt32LE(d + 4); d += 32) names.push(cstr(off(b.readUInt32LE(d + 4))));
  return names;
}

const systemDir = join(process.env.SystemRoot ?? "C:\\Windows", "System32");
const isSystem = (name) => /^(api|ext)-ms-/i.test(name) || existsSync(join(systemDir, name));

/**
 * The DLLs `exe` needs from `dirs` (searched in order, before the system directory), with
 * their own imports followed. Returns `{ files: Map<lowercase name, path>, system: Set }`;
 * throws on a DLL found nowhere.
 */
export function dllClosure(exe, dirs) {
  const files = new Map();
  const system = new Set();
  const todo = [exe];
  while (todo.length) {
    const f = todo.pop();
    for (const name of peImports(f)) {
      const key = name.toLowerCase();
      if (files.has(key) || system.has(key)) continue;
      const hit = dirs.map((d) => join(d, name)).find((p) => existsSync(p));
      if (hit) {
        files.set(key, hit);
        todo.push(hit);
      } else if (isSystem(name)) {
        system.add(key);
      } else {
        throw new Error(`${f} imports ${name}, found neither in ${dirs.join(", ")} nor in ${systemDir}`);
      }
    }
  }
  return { files, system };
}

/** The PE subsystem: 2 = Windows GUI, 3 = console. */
export function peSubsystem(file) {
  const b = readFileSync(file);
  const pe = b.readUInt32LE(0x3c);
  return b.readUInt16LE(pe + 24 + 68);
}
