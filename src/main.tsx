import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/geist/index.css";
import "@fontsource-variable/geist-mono/index.css";
import "./styles.css";
import { Toasts } from "./ui/kit.tsx";
import { MainApp } from "./views/Main.tsx";
import { QuickPaste } from "./views/Quick.tsx";

// One bundle, two windows: the main window and the quick-paste popup (?window=quick).
const quick = new URLSearchParams(location.search).get("window") === "quick";
document.body.classList.toggle("is-quick", quick);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {quick ? <QuickPaste /> : <MainApp />}
    <Toasts />
  </StrictMode>,
);
