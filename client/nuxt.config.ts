// Nuxt 4 SPA served inside the Tauri webview (https://tauri.app/start/frontend/nuxt/).
// `devServer.port` is fixed so `src-tauri/tauri.conf.json > build.devUrl` can point at it.
export default defineNuxtConfig({
  compatibilityDate: "2025-05-15",
  modules: ["@nuxt/ui", "@nuxt/eslint"],
  css: ["~/assets/css/main.css"],
  devtools: { enabled: false },
  // SPA: `nuxt generate` emits `.output/public/index.html` + client bundles (no SSR).
  ssr: false,
  // No app manifest: the client would fetch it at startup (`/_nuxt/builds/meta/<id>.json`), which
  // the app's CSP (`connect-src` allows only the IPC) blocks, and nothing here uses it.
  experimental: { appManifest: false },
  // The client talks only to the binone server, so nothing is fetched from the internet at
  // runtime: web fonts are off and the icons in use are bundled into the client.
  ui: { fonts: false },
  // Dark, like the prototypes; the colour palette is in app/app.config.ts and assets/css.
  colorMode: { preference: "dark", fallback: "dark" },
  // Off in every real build. NUXT_PUBLIC_SAMPLE_DATA=1 at build time swaps the native side for
  // sample data (app/utils/api/sample.ts) so the screens can be drawn without Tauri.
  runtimeConfig: { public: { sampleData: false } },
  icon: {
    provider: "none",
    clientBundle: { scan: true, icons: ["lucide:loader-circle", "lucide:chevron-down"] },
  },
  devServer: {
    host: "0",
    port: 1420,
  },
  vite: {
    clearScreen: false,
    envPrefix: ["VITE_", "TAURI_"],
    server: { strictPort: true },
  },
  ignore: ["**/src-tauri/**"],
});
