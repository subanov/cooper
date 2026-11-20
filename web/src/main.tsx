import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { applyPreferences } from "./lib/preferences";
import { makeQueryClient, makeRouter } from "./router";
import "./styles.css";

// Theme and density before the first paint (no inline script: the CSP forbids it).
applyPreferences();

const root = document.getElementById("root");
if (root) {
  const queryClient = makeQueryClient();
  createRoot(root).render(
    <StrictMode>
      <App router={makeRouter(queryClient)} queryClient={queryClient} />
    </StrictMode>,
  );
}
