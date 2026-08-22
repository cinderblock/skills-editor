/**
 * Seed catalog of well-known skill repositories. Listings are fetched live
 * from the GitHub contents API; each directory containing a SKILL.md is an
 * installable skill.
 */
export interface CatalogSource {
  /** Display name of the source. */
  label: string;
  /** owner/repo on GitHub. */
  repo: string;
  /** Clone URL. */
  cloneUrl: string;
  /** Paths inside the repo whose children are skill directories ("" = root). */
  skillRoots: string[];
  note: string;
}

export const CATALOG_SOURCES: CatalogSource[] = [
  {
    label: "Anthropic skills",
    repo: "anthropics/skills",
    cloneUrl: "https://github.com/anthropics/skills.git",
    skillRoots: ["document-skills", "example-skills"],
    note: "Official Anthropic skills (documents, examples)",
  },
  {
    label: "Superpowers",
    repo: "obra/superpowers",
    cloneUrl: "https://github.com/obra/superpowers.git",
    skillRoots: ["skills"],
    note: "Jesse Vincent's community skill collection",
  },
];

export interface CatalogEntry {
  source: CatalogSource;
  /** Path of the skill dir inside the repo. */
  path: string;
  /** Directory name — the default install name. */
  name: string;
}

interface GithubContentItem {
  name: string;
  path: string;
  type: string;
}

/** List installable skills under one root of a source repo. */
export async function listSkills(
  source: CatalogSource,
  root: string,
): Promise<CatalogEntry[]> {
  const url = `https://api.github.com/repos/${source.repo}/contents/${root}`;
  const res = await fetch(url, {
    headers: { Accept: "application/vnd.github+json" },
  });
  if (!res.ok) {
    throw new Error(`GitHub API ${res.status} for ${source.repo}/${root}`);
  }
  const items = (await res.json()) as GithubContentItem[];
  return items
    .filter((i) => i.type === "dir")
    .map((i) => ({ source, path: i.path, name: i.name }));
}

/** Fetch the raw SKILL.md of a catalog entry (for preview). */
export async function fetchSkillMd(entry: CatalogEntry): Promise<string> {
  const url = `https://raw.githubusercontent.com/${entry.source.repo}/HEAD/${entry.path}/SKILL.md`;
  const res = await fetch(url);
  if (!res.ok) throw new Error(`no SKILL.md found (${res.status})`);
  return res.text();
}
