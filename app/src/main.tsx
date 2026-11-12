import React from "react";
import ReactDOM from "react-dom/client";
import "./theme.css";

async function boot() {
  const view = window.location.hash.replace("#", "") || "chat";
  if (new URLSearchParams(window.location.search).has("mock")) {
    (await import("./mocks")).installMocks(view);
  }
  const { Droid } = await import("./droid/Droid");
  const { ChatPanel } = await import("./chat/ChatPanel");
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>{view === "droid" ? <Droid /> : <ChatPanel />}</React.StrictMode>,
  );
}

void boot();
