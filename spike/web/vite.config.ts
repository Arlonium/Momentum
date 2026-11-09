import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  // Les fixtures vivent dans spike/fixtures, hors de la racine Vite.
  server: { fs: { allow: [".."] } },
});
