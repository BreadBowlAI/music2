import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// In dev, the desktop app runs its engine on :4100 and loads this server:
//   MUSIC2_DEV_URL=http://localhost:5173 cargo run -p music2-desktop
export default defineConfig({
  plugins: [react(), tailwindcss()],
  // The code editor (Monaco, and its TypeScript worker) is several MB. It is its own chunk,
  // loaded the first time the code view opens; its workers come in with `?worker` imports.
  build: { chunkSizeWarningLimit: 8000 },
  server: {
    port: 5173,
    proxy: {
      "/api/ws": { target: "ws://127.0.0.1:4100", ws: true },
      "/api": "http://127.0.0.1:4100",
    },
  },
});
