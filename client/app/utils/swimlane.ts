// The swimlane's rules, kept apart from the screen: filtering, ordering, folding hidden lanes
// into the lane before them, and grouping. Plain functions on plain data, so they can be tested.
import type { LaneDef, LaneName, RequestSummary } from "~/types/binone";
import { DRAFT_LANE, requestRoute } from "~/utils/routes";

/** Where a press on a card goes. The same rule as `requestRoute`, which the other screens use too. */
export const cardRoute = requestRoute;

export interface SwimlaneFilter {
  /** One project, or none. */
  project: string | null;
  /** OR inside the list; an empty list does not filter. */
  kinds: string[];
  /** OR inside the list; an empty list does not filter. */
  tags: string[];
}

export type SwimlaneOrder = "UPDATED_DESC" | "UPDATED_ASC" | "PROJECT_ASC" | "PROJECT_DESC";
export type SwimlaneGroupBy = "none" | "project" | "kind";

export const EMPTY_FILTER: SwimlaneFilter = { project: null, kinds: [], tags: [] };

/** Project, kinds and tags are ANDed with each other; the values inside kinds and tags are ORed. */
export function applyFilter(items: readonly RequestSummary[], filter: SwimlaneFilter): RequestSummary[] {
  return items.filter(
    (item) =>
      (!filter.project || item.project === filter.project) &&
      (filter.kinds.length === 0 || filter.kinds.includes(item.kind)) &&
      (filter.tags.length === 0 || item.tags.some((tag) => filter.tags.includes(tag))),
  );
}

export function isFiltering(filter: SwimlaneFilter): boolean {
  return filter.project !== null && filter.project !== "" ? true : filter.kinds.length > 0 || filter.tags.length > 0;
}

/**
 * A copy in the wanted order. Ties fall back to the newest update and then the id, so the order
 * never depends on the order the server happened to answer in.
 */
export function sortItems(items: readonly RequestSummary[], order: SwimlaneOrder): RequestSummary[] {
  const byUpdated = (a: RequestSummary, b: RequestSummary) => b.updatedMs - a.updatedMs || a.id.localeCompare(b.id);
  const sign = order === "PROJECT_DESC" ? -1 : 1;
  return [...items].sort((a, b) => {
    switch (order) {
      case "UPDATED_ASC":
        return a.updatedMs - b.updatedMs || a.id.localeCompare(b.id);
      case "UPDATED_DESC":
        return byUpdated(a, b);
      default:
        return sign * a.project.localeCompare(b.project) || byUpdated(a, b);
    }
  });
}


/** The first lane and the lane named `draft` are always shown. */
function isPinned(lanes: readonly LaneDef[], index: number): boolean {
  return index === 0 || lanes[index]?.name === DRAFT_LANE;
}

/** Whether the lane cannot be hidden (the menu shows it checked and disabled). */
export function isLanePinned(lanes: readonly LaneDef[], name: LaneName): boolean {
  return lanes.some((lane, index) => lane.name === name && isPinned(lanes, index));
}

/** The lanes to draw: the configured ones minus the hidden ones. The first lane is always kept. */
export function visibleLanes(lanes: readonly LaneDef[], hidden: readonly LaneName[]): LaneDef[] {
  return lanes.filter((lane, index) => isPinned(lanes, index) || !hidden.includes(lane.name));
}

/** A hidden set that cannot hide the first lane, nor a lane that does not exist. */
export function normalizeHidden(lanes: readonly LaneDef[], hidden: readonly LaneName[]): LaneName[] {
  return lanes.filter((lane, index) => !isPinned(lanes, index) && hidden.includes(lane.name)).map((lane) => lane.name);
}

/**
 * The lane a card is drawn in: its own, or when that one is hidden, the nearest visible lane
 * before it. `null` when the card's lane is not among the configured lanes.
 */
export function foldedLane(lanes: readonly LaneDef[], hidden: readonly LaneName[], lane: LaneName): LaneName | null {
  const index = lanes.findIndex((candidate) => candidate.name === lane);
  if (index < 0) return null;
  for (let i = index; i >= 0; i--) {
    const candidate = lanes[i];
    if (candidate && (isPinned(lanes, i) || !hidden.includes(candidate.name))) return candidate.name;
  }
  return null;
}

export interface SwimlaneGroup {
  key: string;
  label: string;
  items: RequestSummary[];
}

export interface SwimlaneColumn {
  lane: LaneName;
  label: string;
  count: number;
  /** One group with an empty key when not grouping. */
  groups: SwimlaneGroup[];
}

export interface SwimlaneBoard {
  columns: SwimlaneColumn[];
  /** Cards whose lane is not among the configured lanes: they are not drawn. */
  orphans: RequestSummary[];
}

export function swimlaneLabel(lane: LaneDef): string {
  return lane.label && lane.label !== "" ? lane.label : lane.name;
}

/** Groups already in the wanted order; groups are listed by name, the cards keep their order. */
export function groupItems(items: readonly RequestSummary[], by: SwimlaneGroupBy): SwimlaneGroup[] {
  if (by === "none") return items.length === 0 ? [] : [{ key: "", label: "", items: [...items] }];
  const map = new Map<string, RequestSummary[]>();
  for (const item of items) {
    const key = by === "project" ? item.project : item.kind;
    const list = map.get(key);
    if (list) list.push(item);
    else map.set(key, [item]);
  }
  return [...map.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, list]) => ({ key, label: key === "" ? "（未設定）" : key, items: list }));
}

export interface BoardOptions {
  filter: SwimlaneFilter;
  hidden: readonly LaneName[];
  order: SwimlaneOrder;
  groupBy: SwimlaneGroupBy;
}

/** The whole board: filter, then order, then put each card in its (folded) lane, then group. */
export function buildBoard(items: readonly RequestSummary[], lanes: readonly LaneDef[], options: BoardOptions): SwimlaneBoard {
  const ordered = sortItems(applyFilter(items, options.filter), options.order);
  const hidden = normalizeHidden(lanes, options.hidden);
  const shown = visibleLanes(lanes, hidden);
  const byLane = new Map<LaneName, RequestSummary[]>(shown.map((lane) => [lane.name, []]));
  const orphans: RequestSummary[] = [];
  for (const item of ordered) {
    const target = foldedLane(lanes, hidden, item.lane);
    const list = target === null ? undefined : byLane.get(target);
    if (list) list.push(item);
    else orphans.push(item);
  }
  return {
    columns: shown.map((lane) => {
      const list = byLane.get(lane.name) ?? [];
      return { lane: lane.name, label: swimlaneLabel(lane), count: list.length, groups: groupItems(list, options.groupBy) };
    }),
    orphans,
  };
}

/** The choices for the filters, from what the cards actually have (sorted, no duplicates). */
export function filterChoices(items: readonly RequestSummary[], key: "project" | "kind" | "tags"): string[] {
  const set = new Set<string>();
  for (const item of items) {
    if (key === "tags") for (const tag of item.tags) set.add(tag);
    else if (item[key] !== "") set.add(item[key]);
  }
  return [...set].sort((a, b) => a.localeCompare(b));
}

/** What to keep of the user's view between visits; anything unreadable gives the defaults. */
export interface SwimlaneView {
  hidden: LaneName[];
  groupBy: SwimlaneGroupBy;
  order: SwimlaneOrder;
}

export const DEFAULT_VIEW: SwimlaneView = { hidden: [], groupBy: "none", order: "UPDATED_DESC" };

export function parseView(raw: string | null): SwimlaneView {
  if (!raw) return { ...DEFAULT_VIEW, hidden: [] };
  try {
    const value = JSON.parse(raw) as Partial<SwimlaneView> | null;
    const orders: SwimlaneOrder[] = ["UPDATED_DESC", "UPDATED_ASC", "PROJECT_ASC", "PROJECT_DESC"];
    const groups: SwimlaneGroupBy[] = ["none", "project", "kind"];
    return {
      hidden: Array.isArray(value?.hidden) ? value.hidden.filter((name): name is string => typeof name === "string") : [],
      groupBy: groups.includes(value?.groupBy as SwimlaneGroupBy) ? (value?.groupBy as SwimlaneGroupBy) : DEFAULT_VIEW.groupBy,
      order: orders.includes(value?.order as SwimlaneOrder) ? (value?.order as SwimlaneOrder) : DEFAULT_VIEW.order,
    };
  } catch {
    return { ...DEFAULT_VIEW, hidden: [] };
  }
}
