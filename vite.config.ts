import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: ["es2021", "chrome100", "safari13"],
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        overlay: resolve(__dirname, "overlay.html"),
      },
      output: {
        // Force React into one shared chunk so both entry points import the
        // same instance. Without this, Vite splits useTauriEvents (shared by
        // main.tsx and overlay.tsx) into its own chunk and bundles a second
        // React copy inside it — causing React error #310 in production.
        manualChunks: {
          "vendor-react": ["react", "react-dom"],
        },
      },
    },
  },
});
