// Draws the app with sample data where there is no Tauri (a headless browser). Off unless the
// build was made with NUXT_PUBLIC_SAMPLE_DATA=1; `?sample-fail=<kind>` makes every call fail.
import { API_ERROR_KINDS, type ApiErrorKind } from "~/utils/api/errors";
import { setNativeCall } from "~/utils/api/transport";

export default defineNuxtPlugin(async () => {
  if (!useRuntimeConfig().public.sampleData) return;
  const { createSampleServer } = await import("~/utils/api/sample");
  const asked = new URLSearchParams(window.location.search).get("sample-fail");
  const failWith = (API_ERROR_KINDS as readonly string[]).includes(asked ?? "") ? (asked as ApiErrorKind) : undefined;
  setNativeCall(createSampleServer({ failWith }));
});
