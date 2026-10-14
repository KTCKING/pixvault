import { useLibraryStore } from "@/stores/library";

export function StatusBar() {
  const library = useLibraryStore();

  return (
    <footer class="status-bar">
      {library.nasStatus() && (
        <span>
          {library.nasStatus()?.status === "local" && "💻 本地"}
          {library.nasStatus()?.status === "online" && `🟢 NAS 在线 (${library.nasStatus()?.latency_ms.toFixed(0)}ms)`}
          {library.nasStatus()?.status === "degraded" && `🟡 NAS 延迟 (${library.nasStatus()?.latency_ms.toFixed(0)}ms)`}
          {library.nasStatus()?.status === "offline" && "🔴 NAS 离线"}
        </span>
      )}
      <div class="flex-1" />
    </footer>
  );
}