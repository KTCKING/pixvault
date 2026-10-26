import { For } from "solid-js";
import { useUIStore, type ToastType } from "@/stores/ui";
import { IconCheck, IconClose, IconInfo } from "@/components/icons";

/** Bottom-center transient notifications (copy done, trashed, etc). */
export function ToastHost() {
  const ui = useUIStore();

  const palette = (type: ToastType) => {
    switch (type) {
      // 主色是黑白反转的：深色主题下 --accent 几乎是白的，写死 #fff 会变成白字白底。
      case "success": return { bg: "var(--accent)", color: "var(--accent-contrast)", icon: <IconCheck size={14} /> };
      case "error": return { bg: "var(--danger)", color: "#fff", icon: <IconClose size={14} /> };
      default: return { bg: "var(--bg-tertiary)", color: "var(--text-primary)", icon: <IconInfo size={14} /> };
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
            <span class="flex items-center">{palette(t.type).icon}</span>
            <span>{t.message}</span>
          </div>
        )}
      </For>
    </div>
  );
}
