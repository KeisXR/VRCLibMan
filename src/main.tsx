import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { BrowserChrome } from "./components/BrowserChrome";
import { api, isTauri } from "./api";
import { startAppearance } from "./appearance";
import "./styles.css";

const browserParams = new URLSearchParams(location.search);
// Both entry points load the saved design before rendering their first controls.
await startAppearance(api.settings, async (changed) => {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>("appearance-changed", (event) => changed(event.payload));
});
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {browserParams.get("browserToolbar") === "1" ? (
      <BrowserChrome hostId={browserParams.get("browserHost") ?? undefined} />
    ) : (
      <App />
    )}
  </React.StrictMode>,
);
