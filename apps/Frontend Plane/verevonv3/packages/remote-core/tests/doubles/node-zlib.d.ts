/**
 * Minimal declaration so the test tree can zstd-COMPRESS fixtures with Node's
 * built-in zlib (Node ≥ 22.15) without pulling @types/node into a package
 * whose `src/` must stay browser-only. remote-core itself only decompresses
 * (fzstd); compression exists solely so the fake host can produce real
 * CursorData payloads.
 */
declare module 'node:zlib' {
  export function zstdCompressSync(buffer: Uint8Array): Uint8Array;
}
