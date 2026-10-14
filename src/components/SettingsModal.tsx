import { createSignal, Show } from "solid-js";
import { api, type ImportSummary } from "@/lib/tauri-api";
import { useLibraryStore } from "@/stores/library";
import { open } from "@tauri-apps/plugin-dialog";
import { useTheme } from "@/stores/theme";

interface Props {
  onClose: () => void;
  onImported: () => void;
}

export function SettingsModal(props: Props) {
  const theme = useTheme();
  const library = useLibraryStore();
  const [busy, setBusy] = createSignal(false);
  const [result, setResult] = createSignal<ImportSummary | null>(null);
  const [error, setError] = createSignal("");

  const s = {
    primary: "var(--text-primary)",
    secondary: "var(--text-secondary)",
    muted: "var(--text-muted)",
    bg: "var(--bg-secondary)",
    tertiary: "var(--bg-tertiary)",
    border: "var(--border-color)",
    accent: "var(--accent)",
  };

  const pickAndImport = async (kind: "eagle" | "pixcall") => {
    setBusy(true);
    setError("");
    setResult(null);
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: kind === "eagle" ? "选择 Eagle 资源库 (.library 文件夹)" : "选择 Pixcall 资源库文件夹",
      });
      if (!selected) {
        setBusy(false);
        return;
      }

      const summary =
        kind === "eagle"
          ? await api.importEagleLibrary(selected as string)
          : await api.importPixcallLibrary(selected as string);

      // Import created a dedicated library — add it to the sidebar list.
      library.setLibraries((prev) => {
        if (prev.find((l) => l.id === summary.library.id)) return prev;
        return [...prev, summary.library];
      });
      library.setActiveLibraryId(summary.library.id);

      setResult(summary);
      props.onImported();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center" style={{ background: "rgba(0,0,0,0.5)" }}>
      <div
        class="rounded-xl p-6 w-[520px] max-w-[90vw] shadow-2xl animate-scale-in"
        style={{ background: s.bg, border: `1px solid ${s.border}`, color: s.primary }}
      >
        <div class="flex items-center justify-between mb-5">
          <h2 class="text-lg font-medium" style={{ color: s.primary }}>设置</h2>
          <button class="btn-ghost text-sm px-2" onClick={props.onClose} title="关闭">✕</button>
        </div>

        <div class="text-sm mb-4" style={{ color: s.secondary }}>从其它素材管理软件导入资源库</div>

        <div class="flex flex-col gap-3">
          <ImportRow
            icon="🦅"
            title="导入 Eagle 资源库"
            desc="读取 .library 内的 metadata.json，保留文件夹层级（映射为标签）、标签、备注、评分与来源链接"
            busy={busy()}
            onPick={() => pickAndImport("eagle")}
          />
          <ImportRow
            icon="📦"
            title="导入 Pixcall 资源库"
            desc="读取 .pixcall/database/main.db，保留文件夹层级、标签、描述、评分与来源网址"
            busy={busy()}
            onPick={() => pickAndImport("pixcall")}
          />
        </div>

        <Show when={busy()}>
          <div class="text-sm mt-4" style={{ color: s.muted }}>正在导入，请稍候…</div>
        </Show>

        <Show when={error()}>
          <div class="text-sm mt-4 px-3 py-2 rounded-lg" style={{ background: "var(--danger)", color: "#fff" }}>
            {error()}
          </div>
        </Show>

        <Show when={result()}>
          <div class="text-sm mt-4 px-3 py-3 rounded-lg" style={{ background: s.tertiary, border: `1px solid ${s.border}` }}>
            <div style={{ color: s.primary }}>导入完成 · 已创建新库「{result()!.library.name}」</div>
            <div class="mt-1 flex flex-wrap gap-x-4 gap-y-1" style={{ color: s.secondary }}>
              <span>文件 {result()!.imported_files} / {result()!.total_items}</span>
              <span>新建标签 {result()!.created_tags}</span>
            </div>
            <Show when={result()!.errors.length > 0}>
              <div class="mt-1" style={{ color: "var(--danger)" }}>{result()!.errors.length} 个文件导入失败</div>
            </Show>
          </div>
        </Show>

        <div class="text-xs mt-5" style={{ color: s.muted }}>
          导入会新建一个独立的图片库（以源库命名），不会与当前库的图片合并，也不会复制或移动原始文件。
        </div>
      </div>
    </div>
  );
}

interface RowProps {
  icon: string;
  title: string;
  desc: string;
  busy: boolean;
  onPick: () => void;
}

function ImportRow(props: RowProps) {
  return (
    <div
      class="flex items-start gap-3 px-4 py-3 rounded-lg transition-colors cursor-pointer"
      style={{
        background: "var(--bg-tertiary)",
        border: "1px solid var(--border-color)",
      }}
      onClick={() => !props.busy && props.onPick()}
    >
      <span class="text-xl leading-none mt-0.5">{props.icon}</span>
      <div class="flex-1 min-w-0">
        <div class="text-sm font-medium" style={{ color: "var(--text-primary)" }}>{props.title}</div>
        <div class="text-xs mt-0.5 leading-relaxed" style={{ color: "var(--text-muted)" }}>{props.desc}</div>
      </div>
      <span class="text-sm" style={{ color: "var(--accent)" }}>→</span>
    </div>
  );
}
