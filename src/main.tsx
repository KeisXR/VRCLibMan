import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { BrowserChrome } from "./components/BrowserChrome";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {new URLSearchParams(location.search).get("browserToolbar") === "1" ? <BrowserChrome /> : <App />}
  </React.StrictMode>,
);
