import { Show, For, createSignal, createEffect } from "solid-js";
import { useUIStore } from "@/stores/ui";

/**
 * In-app replacement for window.confirm / window.prompt, which are not
 * reliable inside the Tauri webview.
 */
export function DialogHost() {
  const ui = useUIStore();
  const [value, setValue] = createSignal("");

  // Reset the input whenever a new dialog opens.
  createEffect(() => {
    const d = ui.dialog();
    if (d) setValue(d.input?.defaultValue ?? "");
  });

  const run = async (fn: (v: string) => void | Promise<void>) => {
    const v = value();
    ui.closeDialog();
    await fn(v);
  };

  return (
    <Show when={ui.dialog()}>
      {(s) => {
        const submit = () => {
          const acts = s().actions;
          const last = acts[acts.length - 1];
          if (last) void run(last.onClick);
        };
        return (
          <div
            class="fixed inset-0 z-[70] flex items-center justify-center"
            style={{ background: "rgba(0,0,0,0.45)" }}
            onClick={() => ui.closeDialog()}
            onContextMenu={(e) => e.preventDefault()}
          >
            <div
              class="rounded-xl p-5 shadow-2xl animate-scale-in"
              style={{ background: "var(--bg-secondary)", border: "1px solid var(--border-color)", width: "360px" }}
              onClick={(e) => e.stopPropagation()}
            >
              <Show when={s().title}>
                <h3 class="text-sm font-semibold mb-2" style={{ color: "var(--text-primary)" }}>{s().title}</h3>
              </Show>
              <Show when={s().message}>
                <p class="text-xs mb-4 whitespace-pre-line" style={{ color: "var(--text-secondary)" }}>{s().message}</p>
              </Show>
              <Show when={s().input}>
                <input
                  class="input text-sm mb-4"
                  autofocus
                  placeholder={s().input?.placeholder || ""}
                  value={value()}
                  onInput={(e) => setValue(e.currentTarget.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") { e.preventDefault(); submit(); }
                    if (e.key === "Escape") ui.closeDialog();
                  }}
                />
              </Show>
              <div class="flex justify-end gap-2">
                <For each={s().actions}>
                  {(a, i) => {
                    const isPrimary = () => i() === s().actions.length - 1;
                    return (
                      <button
                        class={
                          a.danger
                            ? "btn-danger text-xs px-3 py-1.5"
                            : isPrimary()
                              ? "btn-primary text-xs px-3 py-1.5"
                              : "btn-secondary text-xs px-3 py-1.5"
                        }
                        onClick={() => run(a.onClick)}
                      >
                        {a.label}
                      </button>
                    );
                  }}
                </For>
              </div>
            </div>
          </div>
        );
      }}
    </Show>
  );
}
