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
  const { MeetingBar } = await import("./bar/MeetingBar");
  const { Bubble } = await import("./droid/Bubble");
  const { Settings } = await import("./settings/Settings");
  const pick = () => {
    switch (view) {
      case "droid": return <Droid />;
      case "bar": return <MeetingBar />;
      case "bubble": return <Bubble />;
      case "settings": return <Settings />;
      default: return <ChatPanel />;
    }
  };
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<React.StrictMode>{pick()}</React.StrictMode>);
}

void boot();
