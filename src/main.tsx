import { createRoot } from "react-dom/client";
import { Shell } from "./ui/Shell";
import { App } from "./app/app";
import "./ui/styles.css";

const app = new App();
(window as unknown as { opencad: App }).opencad = app;
createRoot(document.getElementById("root")!).render(<Shell app={app} />);
