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
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
