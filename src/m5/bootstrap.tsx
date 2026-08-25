import React from "react";
import ReactDOM from "react-dom/client";

import "./styles.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <main>
      <p className="eyebrow">M5.0</p>
      <h1>Agent-first IDE</h1>
      <p>안전한 Electron 작업대 기준선을 준비하고 있어요.</p>
    </main>
  </React.StrictMode>,
);
