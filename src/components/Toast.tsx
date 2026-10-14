import { For, Show } from "solid-js";
import { useUIStore, type ToastType } from "@/stores/ui";

/** Bottom-center transient notifications (copy done, trashed, etc.). */
export function ToastHost() {
  const ui = useUIStore();

  const palette = (type: ToastType) => {
    switch (type) {
      case "success": return { bg: "var(--accent)", color: "#fff", icon: "✓" };
      case "error": return { bg: "var(--danger)", color: "#fff", icon: "✕" };
      default: return { bg: "var(--bg-tertiary)", color: "var(--text-primary)", icon: "ℹ" };
    }
  };

  return (
    <div class="fixed left-0 right-0 bottom-12 z-[60] flex flex-col items-center gap-2 pointer-events-none">
      <For each={ui.toasts()}>
        {(t) => (
          <div
            class="toast-item flex items-center gap-2 px-4 py-2 rounded-lg shadow-2xl text-sm pointer-events-auto animate-slide-up"
            style={{
              background: palette(t.type).bg,
              color: palette(t.type).color,
              border: t.type === "info" ? "1px solid var(--border-color)" : "none",
            }}
            onClick={() => ui.dismissToast(t.id)}
          >
            <span class="text-xs">{palette(t.type).icon}</span>
            <span>{t.message}</span>
          </div>
        )}
      </For>
    </div>
  );
}
