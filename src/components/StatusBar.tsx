import { createSignal, onMount, Show } from "solid-js";
import { getVersion } from "@tauri-apps/api/app";
import { useLibraryStore } from "@/stores/library";
import { StatusDot } from "@/components/icons";

export function StatusBar() {
  const library = useLibraryStore();
  /** 版本号：拿不到（比如直接用浏览器打开）就退回构建时注入的值。 */
  const [version, setVersion] = createSignal(__APP_VERSION__);

  onMount(async () => {
    try {
      setVersion(await getVersion());
    } catch {
      /* 保持兜底值 */
    }
  });

  return (
    <footer class="status-bar">
      {library.nasStatus() && (
        <span class="flex items-center gap-1.5">
          <StatusDot tone={library.nasStatus()!.status} />
          {library.nasStatus()?.status === "local" && "本地"}
          {library.nasStatus()?.status === "online" && `NAS 在线 (${library.nasStatus()?.latency_ms.toFixed(0)}ms)`}
          {library.nasStatus()?.status === "degraded" && `NAS 延迟 (${library.nasStatus()?.latency_ms.toFixed(0)}ms)`}
          {library.nasStatus()?.status === "offline" && "NAS 离线"}
        </span>
      )}
      <div class="flex-1" />
      <Show when={version()}>
        <span class="tabular-nums" style={{ color: "var(--text-muted)" }} title="设置 → 关于 里可以看更新内容">
          v{version()}
        </span>
      </Show>
    </footer>
  );
}
