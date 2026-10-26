import { Show, For, createSignal, createEffect, on, onCleanup } from "solid-js";
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

  // Esc 关闭本层。挂在 window 上而不是弹层 div 上 —— 只有带输入框的弹窗才有
  // 焦点可落，纯确认弹窗打开时焦点还在 body，div 收不到 keydown。
  // App 那一层用捕获阶段快照判断「事件开始时浮层已打开」，所以这里关掉之后
  // 不会连带清空背景的选中项。
  createEffect(on(() => ui.dialog(), (d) => {
    if (!d) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); ui.closeDialog(); }
    };
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));
  }));

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
              class="glass-modal rounded-xl p-5 animate-scale-in"
              style={{ width: "360px" }}
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
