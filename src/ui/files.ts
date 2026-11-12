/**
 * File access that works in the browser (File System Access API with a download
 * fallback) and inside the Electron desktop shell.
 */

type FSWindow = Window & {
  showOpenFilePicker?: (o: object) => Promise<FileSystemFileHandle[]>;
  showSaveFilePicker?: (o: object) => Promise<FileSystemFileHandle>;
};

const PART_TYPES = [{ description: "OpenCAD Part", accept: { "application/json": [".ocpart"] } }];
/** File > Open also reads STEP files (imported as a new part, as SOLIDWORKS does). */
const OPEN_TYPES = [{ description: "OpenCAD Part, STEP", accept: { "application/octet-stream": [".ocpart", ".step", ".stp"] } }];

export async function openTextFile(): Promise<{ text: string; name: string; handle?: FileSystemFileHandle } | null> {
  const w = window as FSWindow;
  if (w.showOpenFilePicker) {
    try {
      const [h] = await w.showOpenFilePicker({ types: OPEN_TYPES });
      const f = await h.getFile();
      return { text: await f.text(), name: f.name, handle: h };
    } catch {
      return null;
    }
  }
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".ocpart,.step,.stp,application/json";
    input.onchange = async () => {
      const f = input.files?.[0];
      resolve(f ? { text: await f.text(), name: f.name } : null);
    };
    input.click();
  });
}

/** Returns the handle written to, null if downloaded, or false if cancelled. */
export async function saveText(text: string, name: string, handle: FileSystemFileHandle | null): Promise<FileSystemFileHandle | null | false> {
  const w = window as FSWindow;
  try {
    let h = handle;
    if (!h && w.showSaveFilePicker) h = await w.showSaveFilePicker({ suggestedName: name, types: PART_TYPES });
    if (h) {
      const ws = await (h as unknown as { createWritable: () => Promise<{ write: (d: string) => Promise<void>; close: () => Promise<void> }> }).createWritable();
      await ws.write(text);
      await ws.close();
      return h;
    }
  } catch (e) {
    if ((e as Error).name === "AbortError") return false;
  }
  await saveBlob(new Blob([text], { type: "application/json" }), name);
  return null;
}

export async function saveBlob(blob: Blob, name: string) {
  const w = window as FSWindow;
  if (w.showSaveFilePicker) {
    try {
      const ext = name.split(".").pop()!;
      const h = await w.showSaveFilePicker({ suggestedName: name, types: [{ description: ext.toUpperCase(), accept: { "application/octet-stream": ["." + ext] } }] });
      const ws = await (h as unknown as { createWritable: () => Promise<{ write: (d: Blob) => Promise<void>; close: () => Promise<void> }> }).createWritable();
      await ws.write(blob);
      await ws.close();
      return;
    } catch (e) {
      if ((e as Error).name === "AbortError") return;
    }
  }
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 5000);
}
