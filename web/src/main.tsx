import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import init from "sudoku-wasm";
import App from "./App";
import "./styles.css";

init().then(() => {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
});
