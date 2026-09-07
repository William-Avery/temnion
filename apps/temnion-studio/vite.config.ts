import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 8080,
    strictPort: true,
  },
  build: {
    target: ["es2021", "chrome100", "safari13"],
    minify: "esbuild",
    sourcemap: true,
  },
});
