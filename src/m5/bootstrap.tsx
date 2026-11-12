import React from "react";
import ReactDOM from "react-dom/client";

import "./styles.css";
import { M5Workbench } from "./workbench.tsx";

const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);
root.render(
  <React.StrictMode>
    <M5Workbench />
  </React.StrictMode>,
);

// Navigation does not invoke React effect cleanups automatically. Release
// editor models, observers and subscriptions before discarding this page.
// A bfcache suspension keeps the mounted tree for a later pageshow instead.
window.addEventListener("pagehide", (event) => {
  if (!event.persisted) root.unmount();
});
