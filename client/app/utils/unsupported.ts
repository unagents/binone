// The headline of a notice for a part of a screen that does not work yet (UnsupportedNotice).

/** Said when nothing else is given: the server has no operation for it. */
export const SERVER_UNSUPPORTED = "サーバーが未対応です";
/** Said when the server has the operation and only this screen lacks it. */
export const SCREEN_UNSUPPORTED = "この画面では未対応です";

/** "<what>: <which side lacks it>". An empty or missing `headline` gives the default (the server). */
export function unsupportedHeadline(what: string, headline?: string | null): string {
  const said = headline?.trim();
  return `${what}: ${said ? said : SERVER_UNSUPPORTED}`;
}
