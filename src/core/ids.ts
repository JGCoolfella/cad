let counter = 0;
const session = Math.random().toString(36).slice(2, 6);

/** Short unique ids; unique across sessions thanks to the random session prefix. */
export function uid(prefix = "") {
  counter++;
  return `${prefix}${session}${counter.toString(36)}`;
}

export function deepClone<T>(v: T): T {
  return structuredClone(v);
}
