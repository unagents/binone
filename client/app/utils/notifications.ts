// Which notifications are new, and which of those the band shows. Pure: the screens fetch, these decide.
import type { Notification } from "~/types/binone";

/**
 * The notifications in `next` that were not in `previous`, newest first. `previous: null` means
 * nothing was fetched before (the first load): then nothing is new, so opening the app does not
 * flash a band for everything already on the list.
 */
export function newNotifications(previous: readonly Notification[] | null, next: readonly Notification[]): Notification[] {
  if (previous === null) return [];
  const known = new Set(previous.map((n) => n.id));
  return next.filter((n) => !known.has(n.id)).sort((a, b) => b.at_ms - a.at_ms || b.id - a.id);
}

/** How long a band stays, in seconds. */
export const BAND_SECONDS = 8;

/** The band's text: where it came from and the start of the summary. */
export function bandText(notification: Notification, maxLength = 60): { source: string; summary: string } {
  const chars = [...notification.summary];
  const summary = chars.length > maxLength ? `${chars.slice(0, maxLength).join("")}…` : notification.summary;
  return { source: notification.source_session ?? "binone", summary };
}

/** Bands still to show after `dismissedId` went (or timed out). */
export function withoutBand(bands: readonly Notification[], dismissedId: number): Notification[] {
  return bands.filter((n) => n.id !== dismissedId);
}
