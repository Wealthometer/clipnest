import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Tauri serves the built files; in development it loads them from this dev server.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5174, strictPort: true },
  build: { target: ["es2022", "chrome120", "safari16"], outDir: "dist", emptyOutDir: true },
});
