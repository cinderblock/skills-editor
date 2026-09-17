/**
 * Path helpers for the UI. Paths come from the backend in the platform's own
 * form (`C:\…` on Windows, `/…` elsewhere), so never hard-code a separator.
 */

/** The separator a path already uses, defaulting to the POSIX one. */
export function sepOf(path: string): string {
  return path.includes("\\") && !path.includes("/") ? "\\" : path.includes("\\") ? "\\" : "/";
}

/** Join a directory to a forward-slash relative path, in the dir's own style. */
export function joinPath(dir: string, rel: string): string {
  const sep = sepOf(dir);
  const trimmed = dir.endsWith(sep) ? dir.slice(0, -sep.length) : dir;
  return `${trimmed}${sep}${sep === "\\" ? rel.replace(/\//g, "\\") : rel}`;
}

/** True when `path` sits inside `dir` (not `dir` itself). */
export function isUnder(path: string | null, dir: string): boolean {
  if (!path) return false;
  const sep = sepOf(dir);
  const base = dir.endsWith(sep) ? dir : dir + sep;
  return path.startsWith(base);
}

/** The last segment of a path, whichever separator it uses. */
export function baseName(path: string): string {
  const cut = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return cut < 0 ? path : path.slice(cut + 1);
}

/** Everything before the last segment. */
export function dirName(path: string): string {
  const cut = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return cut < 0 ? "" : path.slice(0, cut);
}

/** A path relative to `dir`, with forward slashes; falls back to the input. */
export function relativeTo(path: string, dir: string): string {
  return isUnder(path, dir) ? path.slice(dir.length + 1).replace(/\\/g, "/") : path;
}
