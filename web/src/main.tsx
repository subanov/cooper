import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { makeQueryClient, makeRouter } from "./router";
import "./styles.css";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App router={makeRouter()} queryClient={makeQueryClient()} />
    </StrictMode>,
  );
}
