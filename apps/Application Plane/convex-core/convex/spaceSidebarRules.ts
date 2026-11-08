/**
 * The personal Spaces sidebar document (WS16-S7): its limits, its validator and
 * its compare-and-set rule, as pure functions.
 *
 * # Why this is pure and has no imports
 *
 * Every Space-side decision in this package is tested directly
 * (`test/space-sidebar.test.cjs`); the handlers in `spaceSidebar.ts` are thin
 * shells around these functions plus the auth checks and indexed reads a unit
 * test cannot exercise without a deployment. Keep this file free of Convex
 * imports so a test can `require` it.
 *
 * # What the document is
 *
 * A layout, not content: starred room refs, hidden room refs, custom sections
 * (a label the person typed plus room refs), per-built-in-group collapse/sort
 * and four computed-section toggles. No room names, no thread ids, no message
 * text. Nothing in authorization reads it: hide, star and sort change what is
 * drawn, never what is allowed.
 *
 * # Two copies of the limits
 *
 * `SIDEBAR_LIMITS` is mirrored in the SPA (`src/shared/actions/space-sidebar
 * -limits.json`). The numbers are pinned by a hash-checked golden
 * (`test/fixtures/space-sidebar-limits.json` + `.sha256`) so the two planes
 * cannot drift silently.
 */

export const SIDEBAR_LIMITS = {
  /** Serialised document size, UTF-8 bytes. */
  maxBytes: 32768,
  starred: 100,
  hidden: 500,
  sections: 30,
  refsPerSection: 200,
  /** Section label length, in Unicode code points after NFC. */
  name: 40,
} as const;

/** Closed set of built-in group ids; owned by the SPA group registry. */
export const SIDEBAR_GROUP_IDS = ["channels", "dms", "personal", "archived"] as const;
export const SIDEBAR_SECTION_SORTS = ["manual", "name", "activity"] as const;
export const SIDEBAR_GROUP_SORTS = ["name", "activity"] as const;

const REF_PATTERN = /^[A-Za-z0-9_.:-]{1,128}$/;
const SECTION_ID_PATTERN = /^[A-Za-z0-9_-]{1,64}$/;

export type SidebarSectionSort = (typeof SIDEBAR_SECTION_SORTS)[number];
export type SidebarGroupSort = (typeof SIDEBAR_GROUP_SORTS)[number];

export interface SidebarSection {
  id: string;
  name: string;
  collapsed: boolean;
  sort: SidebarSectionSort;
  refs: string[];
}

export interface SidebarGroupState {
  id: string;
  collapsed?: boolean;
  sort?: SidebarGroupSort;
}

export interface SidebarDoc {
  v: 1;
  starred: string[];
  hidden: string[];
  sections: SidebarSection[];
  groups: SidebarGroupState[];
  computed: { needsYou: boolean; unread: boolean; working: boolean; muted: boolean };
}

function fail(reason: string): never {
  throw new Error(`invalid_sidebar_doc:${reason}`);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function onlyKeys(value: Record<string, unknown>, allowed: readonly string[], where: string) {
  for (const key of Object.keys(value)) {
    if (!allowed.includes(key)) fail(`${where}_unknown_key`);
  }
}

function refList(value: unknown, max: number, where: string): string[] {
  if (!Array.isArray(value)) fail(`${where}_not_array`);
  if (value.length > max) fail(`${where}_too_many`);
  const seen = new Set<string>();
  for (const ref of value) {
    if (typeof ref !== "string" || !REF_PATTERN.test(ref)) fail(`${where}_bad_ref`);
    if (seen.has(ref)) fail(`${where}_duplicate_ref`);
    seen.add(ref);
  }
  return value as string[];
}

function utf8Length(text: string): number {
  return new TextEncoder().encode(text).length;
}

/**
 * Check a sidebar document against every cap and rule. Returns the same
 * document typed as `SidebarDoc`; throws `invalid_sidebar_doc:<reason>`.
 *
 * - `v` is 1; no keys outside the schema at any level.
 * - `starred` <= 100, `hidden` <= 500, `sections` <= 30 with <= 200 refs each.
 * - Refs match `^[A-Za-z0-9_.:-]{1,128}$` and appear once per list.
 * - A ref is in at most one section.
 * - Section ids are unique and match `^[A-Za-z0-9_-]{1,64}$`; names are
 *   already NFC and trimmed, 1..40 code points, unique ignoring case.
 * - Group ids are from the closed set and appear once.
 * - The serialised document is at most 32 KiB.
 */
export function validateSidebarDoc(input: unknown): SidebarDoc {
  if (!isRecord(input)) fail("not_object");
  onlyKeys(input, ["v", "starred", "hidden", "sections", "groups", "computed"], "doc");
  if (input.v !== 1) fail("bad_version");

  refList(input.starred, SIDEBAR_LIMITS.starred, "starred");
  refList(input.hidden, SIDEBAR_LIMITS.hidden, "hidden");

  if (!Array.isArray(input.sections)) fail("sections_not_array");
  if (input.sections.length > SIDEBAR_LIMITS.sections) fail("sections_too_many");
  const sectionIds = new Set<string>();
  const sectionNames = new Set<string>();
  const sectionRefs = new Set<string>();
  for (const raw of input.sections) {
    if (!isRecord(raw)) fail("section_not_object");
    onlyKeys(raw, ["id", "name", "collapsed", "sort", "refs"], "section");
    if (typeof raw.id !== "string" || !SECTION_ID_PATTERN.test(raw.id)) fail("section_bad_id");
    if (sectionIds.has(raw.id)) fail("section_duplicate_id");
    sectionIds.add(raw.id);
    if (typeof raw.name !== "string") fail("section_bad_name");
    if (raw.name !== raw.name.normalize("NFC") || raw.name !== raw.name.trim()) {
      fail("section_name_not_normalized");
    }
    const nameLength = Array.from(raw.name).length;
    if (nameLength < 1 || nameLength > SIDEBAR_LIMITS.name) fail("section_name_length");
    const nameKey = raw.name.toLowerCase();
    if (sectionNames.has(nameKey)) fail("section_duplicate_name");
    sectionNames.add(nameKey);
    if (typeof raw.collapsed !== "boolean") fail("section_bad_collapsed");
    if (!(SIDEBAR_SECTION_SORTS as readonly unknown[]).includes(raw.sort)) fail("section_bad_sort");
    for (const ref of refList(raw.refs, SIDEBAR_LIMITS.refsPerSection, "section_refs")) {
      if (sectionRefs.has(ref)) fail("section_ref_in_two_sections");
      sectionRefs.add(ref);
    }
  }

  if (!Array.isArray(input.groups)) fail("groups_not_array");
  if (input.groups.length > SIDEBAR_GROUP_IDS.length) fail("groups_too_many");
  const groupIds = new Set<string>();
  for (const raw of input.groups) {
    if (!isRecord(raw)) fail("group_not_object");
    onlyKeys(raw, ["id", "collapsed", "sort"], "group");
    if (!(SIDEBAR_GROUP_IDS as readonly unknown[]).includes(raw.id)) fail("group_unknown_id");
    if (groupIds.has(raw.id as string)) fail("group_duplicate_id");
    groupIds.add(raw.id as string);
    if (raw.collapsed !== undefined && typeof raw.collapsed !== "boolean") fail("group_bad_collapsed");
    if (raw.sort !== undefined && !(SIDEBAR_GROUP_SORTS as readonly unknown[]).includes(raw.sort)) {
      fail("group_bad_sort");
    }
  }

  if (!isRecord(input.computed)) fail("computed_not_object");
  onlyKeys(input.computed, ["needsYou", "unread", "working", "muted"], "computed");
  for (const key of ["needsYou", "unread", "working", "muted"]) {
    if (typeof input.computed[key] !== "boolean") fail("computed_bad_flag");
  }

  if (utf8Length(JSON.stringify(input)) > SIDEBAR_LIMITS.maxBytes) fail("too_large");
  return input as unknown as SidebarDoc;
}

/** JSON with object keys sorted and `undefined` dropped: order-independent equality. */
function canonical(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (isRecord(value)) {
    const keys = Object.keys(value)
      .filter((key) => value[key] !== undefined)
      .sort();
    return `{${keys.map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value) ?? "null";
}

export function sidebarDocsEqual(a: unknown, b: unknown): boolean {
  return canonical(a) === canonical(b);
}

export type SidebarPutDecision =
  | { action: "create"; revision: 1 }
  | { action: "update"; revision: number }
  | { action: "noop"; revision: number }
  | { action: "conflict"; revision: number };

/**
 * The compare-and-set rule for `putForGateway`.
 *
 * `baseRevision` is the revision the caller last saw (0 when it has never seen
 * a row). `stored` is the current row, or `null`.
 *
 * - Base equals the stored revision: write (create at revision 1 when there is
 *   no row, else revision + 1). An unchanged document is a no-op success and
 *   does not bump the revision.
 * - Base differs: a conflict, unless the stored document already equals the
 *   proposed one, which is a replayed PUT and counts as success at the stored
 *   revision.
 */
export function decideSidebarPut(
  stored: { revision: number; doc: unknown } | null,
  baseRevision: number,
  proposed: unknown,
): SidebarPutDecision {
  const storedRevision = stored ? stored.revision : 0;
  if (stored && sidebarDocsEqual(stored.doc, proposed)) {
    return { action: "noop", revision: storedRevision };
  }
  if (baseRevision !== storedRevision) return { action: "conflict", revision: storedRevision };
  if (!stored) return { action: "create", revision: 1 };
  return { action: "update", revision: storedRevision + 1 };
}
