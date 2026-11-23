/**
 * The engine program (`crates/oc-web`, an Emscripten ES module) as the workers use it: its
 * exported entry points and the two buffers data crosses through (`oc_buffer` in, `oc_reply`
 * out). See `crates/oc-web/src/web.rs`.
 */
import type { Identity } from "./protocol";

/** What `oc_request` answered (`web.rs`). */
export const JSON_REPLY = 0;
export const BYTES_REPLY = 1;
export const ERROR_REPLY = 2;
export const DEFERRED = 3;

/** Host replies (`oc_host::proto`) and the worker's own news (`oc_engine::hosts`). */
export const DONE = 3;
export const FAILED = 4;
export const DIED = 0x101;
export const TIMED_OUT = 0x102;

interface Emscripten {
  HEAPU8: Uint8Array;
  FS: {
    writeFile(path: string, data: Uint8Array): void;
    readFile(path: string): Uint8Array;
    readdir(path: string): string[];
    mkdirTree(path: string): void;
  };
  _oc_buffer(len: number): number;
  _oc_reply(): number;
  _oc_reply_len(): number;
  _oc_identity(): void;
  _oc_engine_start(hosts: number, budgetMb: number): void;
  _oc_request(cmdLen: number): number;
  _oc_wanted(): number;
  _oc_prepare(id: number, kind: number): number;
  _oc_hosts_started(): void;
  _oc_set_defer(on: number): void;
  _oc_take_warm(): number;
  _oc_host_start(faults: number): void;
  _oc_host_job(): number;
}

const enc = new TextEncoder();
const dec = new TextDecoder();

/** Loads the program from `url` (`oc-web.js`; its `.wasm` and OCCT's `libocct.so` beside it). */
export async function load(url: string): Promise<OcWeb> {
  const create = (await import(/* @vite-ignore */ url)).default as (o: object) => Promise<Emscripten>;
  return new OcWeb(await create({ print: (t: string) => console.log(t), printErr: (t: string) => console.warn(t) }));
}

export class OcWeb {
  constructor(readonly m: Emscripten) {}

  /** Writes `parts` one after another into the input buffer. */
  private input(...parts: Uint8Array[]) {
    const n = parts.reduce((s, p) => s + p.length, 0);
    const at = this.m._oc_buffer(n);
    // The heap may have grown: take the current view after the call.
    let o = at;
    for (const p of parts) {
      this.m.HEAPU8.set(p, o);
      o += p.length;
    }
  }

  /** A copy of the reply (its own buffer, so it can be transferred). */
  reply(): Uint8Array {
    const at = this.m._oc_reply();
    return this.m.HEAPU8.slice(at, at + this.m._oc_reply_len());
  }

  identity(): Identity {
    this.m._oc_identity();
    return JSON.parse(dec.decode(this.reply()));
  }

  startEngine(hosts: number, budgetMb: number) {
    this.m._oc_engine_start(hosts, budgetMb);
  }

  /** A command with its arguments: what the reply is (`JSON_REPLY`, ...) and its bytes. */
  request(cmd: string, args: unknown): { kind: number; bytes: Uint8Array } {
    const c = enc.encode(cmd);
    this.input(c, args === undefined || args === null ? new Uint8Array(0) : enc.encode(JSON.stringify(args)));
    const kind = this.m._oc_request(c.length);
    return { kind, bytes: this.reply() };
  }

  /** The deferred request's part jobs (`oc_host::proto` Job payloads; empty: none needed). */
  wanted(): Uint8Array[] {
    const n = this.m._oc_wanted();
    const r = this.reply();
    const dv = new DataView(r.buffer, r.byteOffset, r.byteLength);
    const out: Uint8Array[] = [];
    let o = 0;
    for (let i = 0; i < n; i++) {
      const len = dv.getUint32(o, true);
      out.push(r.slice(o + 4, o + 4 + len));
      o += 4 + len;
    }
    return out;
  }

  /** Hands part `id`'s result over; throws the engine's error. */
  prepare(id: number, kind: number, payload: Uint8Array) {
    this.input(payload);
    if (this.m._oc_prepare(id, kind) !== 0) throw new Error(dec.decode(this.reply()));
  }

  hostsStarted() {
    this.m._oc_hosts_started();
  }

  setDefer(on: boolean) {
    this.m._oc_set_defer(on ? 1 : 0);
  }

  takeWarm(): boolean {
    return this.m._oc_take_warm() !== 0;
  }

  startHost(faults: boolean) {
    this.m._oc_host_start(faults ? 1 : 0);
  }

  /** Regenerates a part job: the host reply's kind (`DONE`, `FAILED`) and payload. */
  hostJob(job: Uint8Array): { kind: number; payload: Uint8Array } {
    this.input(job);
    const kind = this.m._oc_host_job();
    return { kind, payload: this.reply() };
  }
}

/** The bytes of a `Uint8Array` as a transferable `ArrayBuffer` of their own. */
export function own(b: Uint8Array): ArrayBuffer {
  return (b.byteOffset === 0 && b.byteLength === b.buffer.byteLength ? b.buffer : b.slice().buffer) as ArrayBuffer;
}
