import React from "react";
import ReactDOM from "react-dom/client";

import "./styles.css";
import { M5Workbench } from "./workbench.tsx";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <M5Workbench />
  </React.StrictMode>,
);
