import { readFileSync } from "node:fs";
import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig(({ command }) => ({
  plugins: [react()],
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "chrome120",
  },
  server: {
    port: 5280,
    strictPort: true,
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  // The mock preview (preview.html) is for local design work only.
  define: {
    __PREVIEW__: JSON.stringify(command === "serve"),
    __APP_VERSION__: JSON.stringify(
      JSON.parse(readFileSync("package.json", "utf8")).version,
    ),
  },
  clearScreen: false,
}));
