/**
 * The engine worker's geometry host workers (OVERHAUL_PLAN W1b), as `oc_host::Pool` keeps
 * host processes: started when first needed (or when an assembly with parts to regenerate is
 * opened) and kept, one part per worker at a time; a worker that trapped, aborted or did not
 * answer in time is stopped and its part fails (`DIED`, `TIMED_OUT`); the next part on that
 * slot starts a new worker. Workers unused for five minutes are stopped.
 */
import { DIED, TIMED_OUT } from "./module";
import type { HostIn, HostOut } from "./protocol";

/** As the native pool (`crates/oc-host/src/pool.rs`). */
export const JOB_TIMEOUT_MS = 600_000;
export const START_TIMEOUT_MS = 60_000;
export const KEEP_MS = 300_000;

/** A part's result: its kind (`DONE`, `FAILED` from the host; `DIED`, `TIMED_OUT`) and payload. */
export interface PartResult {
  kind: number;
  payload: Uint8Array;
}

interface Slot {
  worker: Worker | null;
  ready: Promise<boolean> | null;
}

export interface HostOptions {
  moduleUrl: string;
  size: number;
  faults?: boolean;
  timeoutMs?: number;
  /** Called when a worker is started (the engine counts them). */
  started?: () => void;
}

export class HostPool {
  private slots: Slot[] = [];
  private lastUsed = 0;
  private idleTimer: ReturnType<typeof setInterval> | null = null;
  /** Workers stopped because they died or hung (all time). */
  died = 0;
  timedOut = 0;
  failedToStart = 0;

  constructor(readonly o: HostOptions) {}

  get size() {
    return this.o.size;
  }

  /** Starts every worker now (an assembly being opened will need them), without waiting. */
  warm() {
    for (let i = 0; i < this.o.size; i++) this.ensure(this.slot(i));
    this.used();
  }

  private slot(i: number): Slot {
    while (this.slots.length <= i) this.slots.push({ worker: null, ready: null });
    return this.slots[i];
  }

  private used() {
    this.lastUsed = Date.now();
    this.idleTimer ??= setInterval(() => {
      if (Date.now() - this.lastUsed >= KEEP_MS) this.stop();
    }, 30_000);
  }

  /** Stops every worker (the next batch starts them again). */
  stop() {
    for (const s of this.slots) this.kill(s);
    if (this.idleTimer) clearInterval(this.idleTimer);
    this.idleTimer = null;
  }

  private kill(s: Slot) {
    s.worker?.terminate();
    s.worker = null;
    s.ready = null;
  }

  /** Starts the slot's worker if it has none; resolves false if it does not start. */
  private ensure(s: Slot): Promise<boolean> {
    if (s.worker && s.ready) return s.ready;
    const w = new Worker(new URL("./host.worker.ts", import.meta.url), { type: "module" });
    s.worker = w;
    this.o.started?.();
    s.ready = new Promise<boolean>((resolve) => {
      const t = setTimeout(() => done(false), START_TIMEOUT_MS);
      const done = (ok: boolean) => {
        clearTimeout(t);
        w.removeEventListener("message", onMsg);
        w.removeEventListener("error", onErr);
        if (!ok) {
          this.failedToStart++;
          if (s.worker === w) this.kill(s);
        }
        resolve(ok);
      };
      const onMsg = (ev: MessageEvent<HostOut>) => done(ev.data.type === "ready");
      const onErr = () => done(false);
      w.addEventListener("message", onMsg);
      w.addEventListener("error", onErr);
    });
    w.postMessage({ type: "start", moduleUrl: this.o.moduleUrl, faults: this.o.faults } satisfies HostIn);
    return s.ready;
  }

  /** One job on the slot's worker. */
  private async call(s: Slot, job: Uint8Array): Promise<PartResult | null> {
    if (!(await this.ensure(s))) return null;
    const w = s.worker!;
    const t0 = performance.now();
    const timeout = this.o.timeoutMs ?? JOB_TIMEOUT_MS;
    return new Promise<PartResult>((resolve) => {
      const t = setTimeout(() => {
        finish();
        this.timedOut++;
        this.kill(s);
        const secs = new Float64Array([(performance.now() - t0) / 1000]);
        resolve({ kind: TIMED_OUT, payload: new Uint8Array(secs.buffer) });
      }, timeout);
      const finish = () => {
        clearTimeout(t);
        w.removeEventListener("message", onMsg);
        w.removeEventListener("error", onErr);
      };
      const died = () => {
        finish();
        this.died++;
        this.kill(s);
        resolve({ kind: DIED, payload: new Uint8Array(0) });
      };
      const onMsg = (ev: MessageEvent<HostOut>) => {
        const m = ev.data;
        if (m.type === "done") {
          finish();
          resolve({ kind: m.kind, payload: new Uint8Array(m.payload) });
        } else if (m.type === "died") died();
      };
      const onErr = () => died();
      w.addEventListener("message", onMsg);
      w.addEventListener("error", onErr);
      const copy = job.slice().buffer;
      w.postMessage({ type: "job", job: copy } satisfies HostIn, [copy]);
    });
  }

  /**
   * Regenerates `jobs` (`oc_host::proto` Job payloads; empty ones are skipped) on at most
   * `size` workers at once; results in job order, `null` where no worker could be started
   * (the engine then regenerates that part itself).
   */
  async run(jobs: Uint8Array[]): Promise<(PartResult | null)[]> {
    this.used();
    const out: (PartResult | null)[] = jobs.map(() => null);
    let next = 0;
    const width = Math.max(1, Math.min(this.o.size, jobs.length));
    await Promise.all(
      Array.from({ length: width }, async (_, i) => {
        const s = this.slot(i);
        for (let j = next++; j < jobs.length; j = next++) {
          if (jobs[j].length) out[j] = await this.call(s, jobs[j]);
        }
      }),
    );
    this.used();
    return out;
  }
}
