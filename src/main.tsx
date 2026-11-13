import React from "react";
import ReactDOM from "react-dom/client";
import "./styles.css";
import { installIpcRecovery } from "./ipcRecovery";

installIpcRecovery(window);

const browserParams = new URLSearchParams(location.search);
const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);

// A separate browser window opens with every page moved out of the panel. Its toolbar loads only
// the browser controls, not every screen of the app.
if (browserParams.get("browserToolbar") === "1") {
  void import("./components/BrowserChrome").then(({ BrowserChrome }) => {
    root.render(
      <React.StrictMode>
        <BrowserChrome hostId={browserParams.get("browserHost") ?? undefined} />
      </React.StrictMode>,
    );
  });
} else {
  void import("./App").then(({ default: App }) => {
    root.render(
      <React.StrictMode>
        <App />
      </React.StrictMode>,
    );
  });
}
