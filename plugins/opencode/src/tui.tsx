// @ts-nocheck — TUI JSX is resolved at runtime by opencode (same as quota plugin)
/** @jsxImportSource @opentui/solid */
import type { TuiPlugin, TuiPluginApi, TuiPluginModule } from "@opencode-ai/plugin/tui";
import { createEffect, createSignal, onCleanup } from "solid-js";
import { readFileSync, unlinkSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

const id = "@mingxy/cerebro";
const SIDEBAR_ORDER = 160;

function readAutoStoreFromFile(sessionId: string | undefined): boolean {
  if (!sessionId) return true;
  try {
    const filePath = join(tmpdir(), `cerebro_autostore_${sessionId}.json`);
    const data = JSON.parse(readFileSync(filePath, "utf-8"));
    return data.enabled ?? true;
  } catch {
    return true;
  }
}

function SidebarContentView(props: {
  api: TuiPluginApi;
  sessionID: string;
}) {
  const [autoStore, setAutoStore] = createSignal(true);
  const theme = () => props.api.theme.current;

  const readAutoStore = () => readAutoStoreFromFile(props.sessionID);

  const unsubscribers = [
    props.api.event.on("session.updated", () => {
      setAutoStore(readAutoStore());
    }),
    props.api.event.on("tui.session.select", (event) => {
      if (event.properties?.sessionID === props.sessionID) {
        setAutoStore(readAutoStore());
      }
    }),
  ];

  createEffect(() => {
    props.sessionID;
    setAutoStore(readAutoStore());
  });

  const interval = setInterval(() => {
    setAutoStore(readAutoStore());
  }, 2000);

  onCleanup(() => {
    clearInterval(interval);
    for (const unsubscribe of unsubscribers) unsubscribe();
  });

  return (
    <box gap={0}>
      <text fg={theme()?.text} wrapMode="none">
        Cerebro
      </text>
      <box flexDirection="row" gap={1}>
        <text
          flexShrink={0}
          style={{ fg: autoStore() ? theme()?.success : theme()?.textMuted }}
        >
          •
        </text>
        <text fg={theme()?.textMuted} wrapMode="none">
          {"Auto-store: " + (autoStore() ? "ON" : "OFF")}
        </text>
      </box>
    </box>
  );
}

const tui: TuiPlugin = async (api) => {
  try {
    const raw = readFileSync(join(tmpdir(), "cerebro_startup_toast.json"), "utf-8");
    const toast = JSON.parse(raw);
    // Retry: opencode 1.17.14+ delays TUI init; fixed timeout may fire before endpoint is ready
    let attempts = 0;
    const maxAttempts = 16;
    const tryToast = () => {
      attempts++;
      try {
        api.ui.toast(toast);
        try { unlinkSync(join(tmpdir(), "cerebro_startup_toast.json")); } catch {}
      } catch {
        if (attempts < maxAttempts) setTimeout(tryToast, 500);
      }
    };
    setTimeout(tryToast, 1000);
  } catch {}
};

const pluginModule: TuiPluginModule & { id: string } = {
  id,
  tui,
};

export default pluginModule;
