import ReactDOM from "react-dom/client";
import App from "./App";
import { bootTheme } from "./store/theme";
import "material-symbols/outlined.css";
import "./styles/globals.css";

// The window runs opaque on WebKitGTK 2.54+ (see webkit.rs); flag it so the
// CSS squares the corners the transparent window rounded.
if (new URLSearchParams(window.location.search).get("opaque") === "1") {
  document.documentElement.dataset.opaque = "1";
}

// Apply the saved theme before first paint to avoid a flash of the default.
bootTheme();

const root = document.getElementById("root");
if (root) {
  ReactDOM.createRoot(root).render(<App />);
}
