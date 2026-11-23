import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

// The pure utilities import each other with `~/` (Nuxt's alias for `app/`).
export default defineConfig({
  resolve: { alias: { "~": fileURLToPath(new URL("./app", import.meta.url)) } },
});
