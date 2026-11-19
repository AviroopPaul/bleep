import React from "react";
import ReactDOM from "react-dom/client";
import "./theme.css";

/** Each window loads only its own view, so small windows (droid, bubble, bar) start fast. */
async function view(name: string): Promise<React.ReactElement> {
  switch (name) {
    case "droid": {
      const { Droid } = await import("./droid/Droid");
      return <Droid />;
    }
    case "bar": {
      const { MeetingBar } = await import("./bar/MeetingBar");
      return <MeetingBar />;
    }
    case "bubble": {
      const { Bubble } = await import("./droid/Bubble");
      return <Bubble />;
    }
    case "settings": {
      const { Settings } = await import("./settings/Settings");
      return <Settings />;
    }
    case "setup": {
      const { Setup } = await import("./setup/Setup");
      return <Setup />;
    }
    default: {
      const { ChatPanel } = await import("./chat/ChatPanel");
      return <ChatPanel />;
    }
  }
}

async function boot() {
  const name = window.location.hash.replace("#", "").split("/")[0] || "chat";
  if (new URLSearchParams(window.location.search).has("mock")) {
    (await import("./mocks")).installMocks(name);
  }
  const el = await view(name);
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<React.StrictMode>{el}</React.StrictMode>);
}

void boot();
