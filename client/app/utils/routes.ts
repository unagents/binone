// Where a request opens. One rule for every way in (a swimlane card by press or Enter, a row of
// the list, the links on the status and registration screens): a draft opens the registration
// screen, where it is edited; any other request opens its status. The lane is told by its name.
export const DRAFT_LANE = "draft";

export function requestRoute(request: { id: string; lane: string }): string {
  const id = encodeURIComponent(request.id);
  return request.lane === DRAFT_LANE ? `/requests/new?id=${id}` : `/requests/${id}`;
}
