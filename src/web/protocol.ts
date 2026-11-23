/**
 * Messages between the page, the engine worker and the geometry host workers (OVERHAUL_PLAN
 * W1b). The page sends the webview's commands (`invoke`) to the engine worker; the engine
 * worker sends part jobs to its host workers. Bytes travel as transferred `ArrayBuffer`s.
 */

/** Page → engine worker. */
export type EngineIn =
  | { type: "start"; moduleUrl: string; hosts: number; budgetMb: number; faults?: boolean; hostTimeoutMs?: number; meshes?: MessagePort }
  | { type: "invoke"; id: number; cmd: string; args: unknown }
  /** Test hook: the engine module's file system (`/work`), a file written or read. */
  | { type: "fs"; id: number; op: "write" | "read" | "list" | "mkdir"; path: string; data?: ArrayBuffer };

/** Engine worker → page. */
export type EngineOut =
  | { type: "ready"; identity: Identity; startMs: number }
  | { type: "reply"; id: number; ok: true; json?: ArrayBuffer; bytes?: ArrayBuffer; stats?: RequestStats }
  | { type: "reply"; id: number; ok: false; error: string }
  /** The engine itself trapped or aborted: everything it held is gone (the page starts another). */
  | { type: "fatal"; error: string };

/** What the engine module says it is (`oc_identity`). */
export interface Identity {
  kernel: string;
  occt: string;
  app: string;
}

/** How a request went in the engine worker (W1b measurements). */
export interface RequestStats {
  /** Times the request was deferred for parts regenerated on host workers. */
  deferrals: number;
  /** Parts regenerated on host workers for it. */
  hostParts: number;
  /** Time spent in the engine module (all runs), and waiting for host workers. */
  engineMs: number;
  hostsMs: number;
}

/** Engine worker → the mesh port (the render worker, W1c): a display mesh the engine made,
 * by the handle its reply names (`RenderMesh::to_bytes`). */
export type MeshOut = { type: "mesh"; handle: number; bytes: ArrayBuffer };

/** Engine worker → host worker. */
export type HostIn = { type: "start"; moduleUrl: string; faults?: boolean } | { type: "job"; job: ArrayBuffer };

/** Host worker → engine worker. */
export type HostOut =
  | { type: "ready"; startMs: number }
  | { type: "done"; kind: number; payload: ArrayBuffer }
  /** The host trapped or aborted (it closes itself; its part fails). */
  | { type: "died"; error: string };
