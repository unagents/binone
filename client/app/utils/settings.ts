// Editing the lists of the settings screen (lanes and kinds). Pure; the server checks again.
import { STANDARD_LANES, type LaneDef } from "~/types/binone";

/** The lane that must stay first: the server refuses settings whose first lane is not `draft`. */
export const FIRST_LANE = "draft";

/** A copy of `list` with the item at `index` moved by `delta` places (out of range: unchanged). */
export function moveItem<T>(list: readonly T[], index: number, delta: number): T[] {
  const target = index + delta;
  const copy = [...list];
  if (index < 0 || index >= copy.length || target < 0 || target >= copy.length) return copy;
  const [item] = copy.splice(index, 1);
  copy.splice(target, 0, item as T);
  return copy;
}

/** Whether a lane may move by `delta`: `draft` stays first and nothing goes before it. */
export function canMoveLane(lanes: readonly LaneDef[], index: number, delta: number): boolean {
  const target = index + delta;
  if (index < 0 || index >= lanes.length || target < 0 || target >= lanes.length) return false;
  return lanes[index]?.name !== FIRST_LANE && lanes[target]?.name !== FIRST_LANE;
}

/** Lane names are lower-case letters, digits, `_` and `-`, as the server demands. */
export function validateLaneName(name: string, lanes: readonly LaneDef[]): string | null {
  if (name === "") return "レーンの名前を入れてください";
  if (!/^[a-z0-9_-]+$/.test(name)) return "名前は半角の小文字・数字・_・- だけで入れてください";
  if (lanes.some((lane) => lane.name === name)) return "同じ名前のレーンがあります";
  return null;
}

export function validateKind(kind: string, kinds: readonly string[]): string | null {
  if (kind === "") return "種別の名前を入れてください";
  if (kinds.includes(kind)) return "同じ種別があります";
  return null;
}

/** The lane as the user reads it: its label when it has one, else its name. */
export function laneLabel(lane: LaneDef): string {
  return lane.label && lane.label !== "" ? lane.label : lane.name;
}

export function isStandardLane(name: string): boolean {
  return (STANDARD_LANES as readonly string[]).includes(name);
}

/** The project's name as the user typed it, without the blanks around it. */
export function validateProjectName(name: string, projects: readonly string[]): string | null {
  if (name.trim() === "") return "プロジェクトの名前を入れてください";
  if (projects.includes(name.trim())) return "同じ名前のプロジェクトがあります";
  return null;
}
