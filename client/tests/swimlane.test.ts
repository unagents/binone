import { describe, expect, it } from "vitest";
import type { LaneDef, RequestSummary } from "../app/types/binone";
import {
  EMPTY_FILTER,
  applyFilter,
  cardRoute,
  buildBoard,
  filterChoices,
  foldedLane,
  groupItems,
  allProjectsValue,
  isFiltering,
  isLanePinned,
  laneLabelOf,
  normalizeHidden,
  parseView,
  projectFromSelect,
  projectSelect,
  projectToSelect,
  sortItems,
  visibleLanes,
} from "../app/utils/swimlane";

const lanes: LaneDef[] = ["draft", "todo", "prepare", "doing", "done", "archive"].map((name) => ({ name, label: null }));

function card(id: string, over: Partial<RequestSummary> = {}): RequestSummary {
  return { id, version: 1, title: id, project: "binone", kind: "task", tags: [], lane: "draft", currentStep: null, updatedMs: 1000, ...over };
}

const items = [
  card("a", { project: "binone", kind: "feature", tags: ["ui"], lane: "doing", updatedMs: 5 }),
  card("b", { project: "bantou", kind: "task", tags: ["ui", "perf"], lane: "todo", updatedMs: 9 }),
  card("c", { project: "memory", kind: "bug", tags: [], lane: "prepare", updatedMs: 7 }),
  card("d", { project: "binone", kind: "task", tags: ["perf"], lane: "draft", updatedMs: 1 }),
];

describe("applyFilter", () => {
  it("does not filter when nothing is chosen", () => {
    expect(applyFilter(items, EMPTY_FILTER)).toHaveLength(4);
  });
  it("takes one project", () => {
    expect(applyFilter(items, { ...EMPTY_FILTER, project: "binone" }).map((i) => i.id)).toEqual(["a", "d"]);
  });
  it("ORs values inside kinds and inside tags", () => {
    expect(applyFilter(items, { ...EMPTY_FILTER, kinds: ["feature", "bug"] }).map((i) => i.id)).toEqual(["a", "c"]);
    expect(applyFilter(items, { ...EMPTY_FILTER, tags: ["ui", "perf"] }).map((i) => i.id)).toEqual(["a", "b", "d"]);
  });
  it("ANDs the fields with each other", () => {
    const filter = { project: "binone", kinds: ["task", "feature"], tags: ["perf"] };
    expect(applyFilter(items, filter).map((i) => i.id)).toEqual(["d"]);
  });
  it("gives nothing when no card matches", () => {
    expect(applyFilter(items, { ...EMPTY_FILTER, project: "nowhere" })).toEqual([]);
  });
});

describe("sortItems", () => {
  it("sorts by update time both ways, without touching the input", () => {
    const copy = [...items];
    expect(sortItems(items, "UPDATED_DESC").map((i) => i.id)).toEqual(["b", "c", "a", "d"]);
    expect(sortItems(items, "UPDATED_ASC").map((i) => i.id)).toEqual(["d", "a", "c", "b"]);
    expect(items).toEqual(copy);
  });
  it("sorts by project name, newest first inside a project", () => {
    expect(sortItems(items, "PROJECT_ASC").map((i) => i.id)).toEqual(["b", "a", "d", "c"]);
    expect(sortItems(items, "PROJECT_DESC").map((i) => i.id)).toEqual(["c", "a", "d", "b"]);
  });
  it("breaks full ties by id", () => {
    const tie = [card("z", { updatedMs: 1 }), card("y", { updatedMs: 1 })];
    expect(sortItems(tie, "UPDATED_DESC").map((i) => i.id)).toEqual(["y", "z"]);
    expect(sortItems(tie, "UPDATED_ASC").map((i) => i.id)).toEqual(["y", "z"]);
  });
});

describe("hidden lanes", () => {
  it("never hides the first lane", () => {
    expect(visibleLanes(lanes, ["draft", "doing"]).map((l) => l.name)).toEqual(["draft", "todo", "prepare", "done", "archive"]);
    expect(normalizeHidden(lanes, ["draft", "doing", "ghost"])).toEqual(["doing"]);
  });
  it("folds a hidden lane into the visible lane before it", () => {
    expect(foldedLane(lanes, ["prepare", "doing"], "doing")).toBe("todo");
    expect(foldedLane(lanes, ["prepare", "doing"], "prepare")).toBe("todo");
    expect(foldedLane(lanes, ["prepare", "doing"], "done")).toBe("done");
    expect(foldedLane(lanes, ["todo"], "todo")).toBe("draft");
  });
  it("gives null for a lane that is not configured", () => {
    expect(foldedLane(lanes, [], "ghost")).toBeNull();
  });
  it("shows prepare and doing cards in todo when both are hidden", () => {
    const board = buildBoard(items, lanes, { filter: EMPTY_FILTER, hidden: ["prepare", "doing"], order: "UPDATED_DESC", groupBy: "none" });
    expect(board.columns.map((c) => c.lane)).toEqual(["draft", "todo", "done", "archive"]);
    expect(board.columns[1]!.groups[0]!.items.map((i) => i.id)).toEqual(["b", "c", "a"]);
    expect(board.columns[1]!.count).toBe(3);
  });
});

describe("buildBoard", () => {
  it("counts after filtering and keeps empty lanes", () => {
    const board = buildBoard(items, lanes, { filter: { ...EMPTY_FILTER, project: "binone" }, hidden: [], order: "UPDATED_DESC", groupBy: "none" });
    expect(board.columns.map((c) => c.count)).toEqual([1, 0, 0, 1, 0, 0]);
    expect(board.columns[1]!.groups).toEqual([]);
  });
  it("sets aside cards of unknown lanes", () => {
    const board = buildBoard([card("x", { lane: "ghost" })], lanes, { filter: EMPTY_FILTER, hidden: [], order: "UPDATED_DESC", groupBy: "none" });
    expect(board.orphans.map((i) => i.id)).toEqual(["x"]);
    expect(board.columns.every((c) => c.count === 0)).toBe(true);
  });
  it("uses the label when the lane has one", () => {
    const board = buildBoard([], [{ name: "draft", label: "下書き" }, { name: "todo", label: null }], { filter: EMPTY_FILTER, hidden: [], order: "UPDATED_DESC", groupBy: "none" });
    expect(board.columns.map((c) => c.label)).toEqual(["下書き", "todo"]);
  });
});

describe("groupItems", () => {
  it("groups by project, groups by name, cards keep their order", () => {
    const groups = groupItems(sortItems(items, "UPDATED_DESC"), "project");
    expect(groups.map((g) => g.key)).toEqual(["bantou", "binone", "memory"]);
    expect(groups[1]!.items.map((i) => i.id)).toEqual(["a", "d"]);
  });
  it("groups by kind", () => {
    expect(groupItems(items, "kind").map((g) => g.key)).toEqual(["bug", "feature", "task"]);
  });
  it("labels an empty key", () => {
    expect(groupItems([card("n", { kind: "" })], "kind")[0]!.label).toBe("（未設定）");
  });
});

describe("filterChoices and parseView", () => {
  it("lists what the cards have, once, sorted", () => {
    expect(filterChoices(items, "project")).toEqual(["bantou", "binone", "memory"]);
    expect(filterChoices(items, "tags")).toEqual(["perf", "ui"]);
  });
  it("falls back to the defaults on a bad stored view", () => {
    expect(parseView(null).order).toBe("UPDATED_DESC");
    expect(parseView("{oops").groupBy).toBe("none");
    expect(parseView(JSON.stringify({ hidden: ["doing", 3], groupBy: "x", order: "PROJECT_ASC" }))).toEqual({ hidden: ["doing"], groupBy: "none", order: "PROJECT_ASC" });
  });
});

describe("the draft lane is pinned by name too", () => {
  const shifted: LaneDef[] = ["todo", "draft", "doing"].map((name) => ({ name, label: null }));
  it("keeps a lane named draft even when it is not first", () => {
    expect(visibleLanes(shifted, ["draft", "doing"]).map((l) => l.name)).toEqual(["todo", "draft"]);
    expect(normalizeHidden(shifted, ["todo", "draft", "doing"])).toEqual(["doing"]);
    expect(foldedLane(shifted, ["draft"], "draft")).toBe("draft");
  });
});

describe("isLanePinned", () => {
  it("is true for the first lane and for the lane named draft, whatever its place", () => {
    expect(isLanePinned(lanes, "draft")).toBe(true);
    expect(isLanePinned(lanes, "todo")).toBe(false);
    const shifted: LaneDef[] = ["todo", "draft", "doing"].map((name) => ({ name, label: null }));
    expect(isLanePinned(shifted, "draft")).toBe(true);
    expect(isLanePinned(shifted, "todo")).toBe(true);
    expect(isLanePinned(shifted, "doing")).toBe(false);
    expect(isLanePinned(shifted, "ghost")).toBe(false);
  });
});

describe("cardRoute", () => {
  it("sends a draft to the registration screen with its id", () => {
    expect(cardRoute({ id: "req_1", lane: "draft" })).toBe("/requests/new?id=req_1");
    expect(cardRoute({ id: "a b/c", lane: "draft" })).toBe("/requests/new?id=a%20b%2Fc");
  });
  it("sends the others to the request status", () => {
    expect(cardRoute({ id: "req_2", lane: "todo" })).toBe("/requests/req_2");
    expect(cardRoute({ id: "req_3", lane: "doing" })).toBe("/requests/req_3");
  });
});

describe("the project select", () => {
  it("offers all first, under a value that is not empty", () => {
    const select = projectSelect(["bantou", "binone"]);
    expect(select.items.map((item) => item.label)).toEqual(["すべて", "bantou", "binone"]);
    expect(select.items[0]?.value).toBe(select.all);
    expect(select.items.every((item) => item.value !== "")).toBe(true);
  });

  it("never gives an item an empty value, even for a project without a name", () => {
    const select = projectSelect(["", "binone"]);
    expect(select.items.map((item) => item.value)).toEqual([select.all, "binone"]);
  });

  it("keeps the all value apart from the projects' names", () => {
    expect(allProjectsValue(["binone"])).toBe("__all__");
    const taken = ["__all__", "__all___", "binone"];
    const select = projectSelect(taken);
    expect(taken).not.toContain(select.all);
    expect(select.all).not.toBe("");
    expect(new Set(select.items.map((item) => item.value)).size).toBe(select.items.length);
    // The project that has the usual value is still a project: choosing it filters by it.
    expect(projectFromSelect("__all__", select.all)).toBe("__all__");
    expect(projectFromSelect(select.all, select.all)).toBeNull();
  });

  it("turns the all value into no project, and back", () => {
    const { all } = projectSelect(["binone"]);
    expect(projectFromSelect("binone", all)).toBe("binone");
    expect(projectFromSelect(all, all)).toBeNull();
    expect(projectFromSelect("", all)).toBeNull();
    expect(projectFromSelect(undefined, all)).toBeNull();
    expect(projectToSelect(null, all)).toBe(all);
    expect(projectToSelect("", all)).toBe(all);
    expect(projectToSelect("binone", all)).toBe("binone");
  });

  it("goes back to every card when all is chosen again", () => {
    const { all } = projectSelect(filterChoices(items, "project"));
    const chosen = { ...EMPTY_FILTER, project: projectFromSelect("binone", all) };
    expect(applyFilter(items, chosen).map((i) => i.id)).toEqual(["a", "d"]);
    const cleared = { ...EMPTY_FILTER, project: projectFromSelect(all, all) };
    expect(isFiltering(cleared)).toBe(false);
    expect(applyFilter(items, cleared)).toHaveLength(items.length);
  });
});

describe("laneLabelOf", () => {
  const labelled: LaneDef[] = [
    { name: "draft", label: "下書き" },
    { name: "doing", label: "作業中" },
    { name: "done", label: "完了" },
    { name: "archive", label: null },
    { name: "empty", label: "" },
  ];

  it("gives the lane's label, not its internal name", () => {
    expect(laneLabelOf(labelled, "done")).toBe("完了");
  });

  it("falls back to the name without a label, or for a lane that is not configured", () => {
    expect(laneLabelOf(labelled, "archive")).toBe("archive");
    expect(laneLabelOf(labelled, "empty")).toBe("empty");
    expect(laneLabelOf(labelled, "gone")).toBe("gone");
  });

  it("names where a card folded into the lane before came from", () => {
    const board = buildBoard([card("x", { lane: "done" })], labelled, { filter: EMPTY_FILTER, hidden: ["done"], order: "UPDATED_DESC", groupBy: "none" });
    const column = board.columns.find((c) => c.lane === "doing");
    const folded = column?.groups[0]?.items[0];
    expect(folded?.lane).toBe("done");
    expect(laneLabelOf(labelled, folded?.lane ?? "")).toBe("完了");
  });
});
