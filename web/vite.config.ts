import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// In dev, the desktop app runs its engine on :4100 and loads this server:
//   MUSIC2_DEV_URL=http://localhost:5173 cargo run -p music2-desktop
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    proxy: {
      "/api/ws": { target: "ws://127.0.0.1:4100", ws: true },
      "/api": "http://127.0.0.1:4100",
    },
  },
});
