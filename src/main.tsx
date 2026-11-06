import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { BrowserChrome } from "./components/BrowserChrome";
import "./styles.css";
import { installIpcRecovery } from "./ipcRecovery";

installIpcRecovery(window);

const browserParams = new URLSearchParams(location.search);
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {browserParams.get("browserToolbar") === "1" ? (
      <BrowserChrome hostId={browserParams.get("browserHost") ?? undefined} />
    ) : (
      <App />
    )}
  </React.StrictMode>,
);
