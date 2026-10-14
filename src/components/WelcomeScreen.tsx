import { createSignal } from "solid-js";
import { open } from "@tauri-apps/plugin-dialog";

interface Props {
  onOpen: (path: string) => Promise<unknown>;
  isIndexing?: boolean;
}

export function WelcomeScreen(props: Props) {
  const [loading, setLoading] = createSignal(false);

  const handleOpenFolder = async () => {
    setLoading(true);
    try {
      const selected = await open({ directory: true, multiple: false, title: "选择文件夹作为图片库" });
      if (selected && typeof selected === "string") {
        await props.onOpen(selected);
      }
    } catch (e) {
      console.error("打开文件夹失败:", e);
    } finally {
      setLoading(false);
    }
  };

  const isLoading = () => loading() || (props.isIndexing || false);

  return (
    <div class="flex-1 flex items-center justify-center" style={{ background: "var(--bg-primary)" }}>
      <div class="text-center max-w-md animate-fade-in">
        <div class="mb-6">
          <svg class="w-16 h-16 mx-auto" style={{ color: "var(--accent)" }} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
            <path d="M12 2L2 7l10 5 10-5-10-5z" /><path d="M2 17l10 5 10-5" /><path d="M2 12l10 5 10-5" />
          </svg>
        </div>
        <h1 class="text-2xl font-bold mb-2" style={{ color: "var(--text-primary)" }}>欢迎使用 PixVault</h1>
        <p class="text-sm mb-8" style={{ color: "var(--text-muted)" }}>
          高性能图片管理工具，为创意工作流而生。
          打开文件夹即可开始 — 本地或 NAS 均支持。
        </p>
        <button class="btn-primary text-base px-6 py-2.5 mb-4" onClick={handleOpenFolder} disabled={isLoading()}>
          {isLoading() ? "正在索引..." : "📂 打开文件夹"}
        </button>
        <p class="text-xs mb-6" style={{ color: "var(--text-muted)" }}>或拖放文件夹到窗口任意位置</p>

        <div class="grid grid-cols-3 gap-4 text-left">
          {[
            ["⚡", "极速浏览", "10 万张图片秒开"],
            ["🔒", "开放格式", "标准 SQLite，无锁定"],
            ["🌐", "NAS 就绪", "SMB 支持离线降级"],
          ].map(([icon, title, desc]) => (
            <div class="p-3 rounded-lg" style={{ background: "var(--bg-secondary)" }}>
              <div class="text-lg mb-1">{icon}</div>
              <div class="text-xs font-medium" style={{ color: "var(--text-primary)" }}>{title}</div>
              <div class="text-2xs" style={{ color: "var(--text-muted)" }}>{desc}</div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}