/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import { configDefaults } from "vitest/config";
import react from "@vitejs/plugin-react";

// Relative base so the same build runs from a web server and from file:// inside Electron.
export default defineConfig({
  base: "./",
  plugins: [react()],
  worker: { format: "es" },
  optimizeDeps: { exclude: ["replicad-opencascadejs"] },
  build: { target: "es2022", chunkSizeWarningLimit: 4000 },
  server: { port: 5173 },
  // Agent worktrees under .claude/ hold other branches' copies of the tests.
  test: { exclude: [...configDefaults.exclude, ".claude/**"] },
});
