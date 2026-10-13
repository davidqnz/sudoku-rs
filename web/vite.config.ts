import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  // The wasm package is linked from ../crates, outside this directory.
  server: { fs: { allow: [".."] } },
});
