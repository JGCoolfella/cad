import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Relative base so the same build runs from a web server and from file:// inside Electron.
export default defineConfig({
  base: "./",
  plugins: [react()],
  worker: { format: "es" },
  optimizeDeps: { exclude: ["replicad-opencascadejs"] },
  build: { target: "es2022", chunkSizeWarningLimit: 4000 },
  server: { port: 5173 },
});
