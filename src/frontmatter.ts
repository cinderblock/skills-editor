import YAML from "yaml";

export interface ParsedSkillFile {
  /** Raw YAML text between the fences, or null when there is no frontmatter. */
  frontmatter: string | null;
  body: string;
}

/**
 * Split a SKILL.md into frontmatter YAML and markdown body.
 *
 * Must be a byte-exact inverse of joinFrontmatter — any lossiness here makes
 * the editor mutate untouched parts of the file on every keystroke.
 */
export function splitFrontmatter(text: string): ParsedSkillFile {
  const m = text.match(/^﻿?---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/);
  if (!m) return { frontmatter: null, body: text };
  return { frontmatter: m[1], body: text.slice(m[0].length) };
}

/** Reassemble a SKILL.md, preserving the file's newline style verbatim. */
export function joinFrontmatter(frontmatter: string | null, body: string): string {
  if (frontmatter === null) return body;
  const eol = frontmatter.includes("\r\n") || body.includes("\r\n") ? "\r\n" : "\n";
  const fm = frontmatter
    .replace(/\r?\n$/, "")
    .split(/\r\n|\n/)
    .join(eol);
  return `---${eol}${fm}${eol}---${eol}${body}`;
}

export interface FrontmatterFields {
  name: string;
  description: string;
  /** Rendered YAML of every other key, for display. */
  restSummary: string;
}

export function readFields(
  frontmatter: string | null,
  /** Extra keys the caller edits itself, so they stay out of restSummary. */
  handled: string[] = [],
): FrontmatterFields {
  if (frontmatter === null) return { name: "", description: "", restSummary: "" };
  try {
    const doc = YAML.parse(frontmatter) ?? {};
    const { name, description, ...rest } = doc;
    for (const key of handled) delete rest[key];
    return {
      name: typeof name === "string" ? name : "",
      description: typeof description === "string" ? description : "",
      restSummary: Object.keys(rest).length ? YAML.stringify(rest).trim() : "",
    };
  } catch {
    return { name: "", description: "", restSummary: "" };
  }
}

/**
 * A memory note's type. Claude Code has written it both at the top level and
 * nested under `metadata:`; read either.
 */
export function readMemoryType(frontmatter: string | null): string {
  if (frontmatter === null) return "";
  try {
    const doc = YAML.parse(frontmatter) ?? {};
    const nested = doc?.metadata?.type;
    return typeof doc.type === "string" ? doc.type : typeof nested === "string" ? nested : "";
  } catch {
    return "";
  }
}

/** True when the type lives under `metadata:` rather than at the top level. */
export function memoryTypeIsNested(frontmatter: string | null): boolean {
  if (frontmatter === null) return false;
  try {
    const doc = YAML.parse(frontmatter) ?? {};
    return typeof doc.type !== "string" && typeof doc?.metadata?.type === "string";
  } catch {
    return false;
  }
}

/** Set the type where it already lives, so Claude's own files keep their shape. */
export function updateMemoryType(frontmatter: string | null, value: string): string {
  const doc = YAML.parseDocument(frontmatter ?? "");
  if (doc.contents === null) return YAML.stringify({ type: value }).replace(/\n$/, "");
  if (memoryTypeIsNested(frontmatter)) doc.setIn(["metadata", "type"], value);
  else doc.set("type", value);
  return doc.toString({ lineWidth: 0 }).replace(/\n$/, "");
}

/**
 * Write name/description back into the YAML while preserving all other keys,
 * comments, and ordering (yaml Document round-trip).
 */
export function updateFields(
  frontmatter: string | null,
  name: string,
  description: string,
): string {
  const doc = YAML.parseDocument(frontmatter ?? "");
  if (doc.contents === null) {
    return YAML.stringify({ name, description }).replace(/\n$/, "");
  }
  doc.set("name", name);
  doc.set("description", description);
  return doc.toString({ lineWidth: 0 }).replace(/\n$/, "");
}
