import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// Tests unitaires (Vitest) : `src/**/*.test.ts`. Les tests de bout en bout sont dans
// `e2e/` (Playwright) et ne passent pas par ici.
export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
