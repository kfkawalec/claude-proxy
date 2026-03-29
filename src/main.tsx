import { render } from "solid-js/web";
import App from "./App";
import "./styles/global.css";
import faviconUrl from "./assets/favicon.svg?url";

const faviconLink = document.createElement("link");
faviconLink.rel = "icon";
faviconLink.type = "image/svg+xml";
faviconLink.href = faviconUrl;
document.head.appendChild(faviconLink);

render(() => <App />, document.getElementById("root")!);
