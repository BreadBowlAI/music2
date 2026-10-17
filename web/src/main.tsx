import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import App from "./App";
import { isMac, native } from "./lib/native";

// The Mac keeps its own font; elsewhere the UI is in Inter (index.css).
if (isMac) document.documentElement.dataset.os = "mac";
// The Mac app's window keeps its buttons in the top bar's first 80px: the bar styles itself by
// this (TopBar), so a page in a browser can be laid out as the app is by setting it.
if (native && isMac) document.documentElement.dataset.app = "mac";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
