/// <reference types="vitest/config" />
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The SPA is served by cooper-server from the same origin (ADR-0010). In development,
// Vite proxies the API to a locally running cooper-server.
const server = process.env.COOPER_DEV_SERVER ?? "http://localhost:8080";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  server: {
    port: 5173,
    proxy: {
      "/api": { target: server, changeOrigin: false },
      "/health": { target: server, changeOrigin: false },
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
    assetsDir: "assets",
    // Pages are lazy route components (src/router.tsx). Libraries go into their own chunks:
    // they change less often than app code, so browsers keep them cached across releases.
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            { name: "react", test: /node_modules[\\/](react|react-dom|scheduler)[\\/]/ },
            { name: "tanstack", test: /node_modules[\\/]@tanstack[\\/]/ },
            { name: "intl", test: /node_modules[\\/](react-intl|@formatjs|intl-messageformat)/ },
            { name: "vendor", test: /node_modules[\\/]/ },
          ],
        },
      },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
    // Lazy pages are imported (and transformed) inside the test that first opens them, so
    // flows across several screens take longer than the 5 s default under a loaded runner.
    testTimeout: 10_000,
  },
});
