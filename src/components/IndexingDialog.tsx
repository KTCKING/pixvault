import { Show } from "solid-js";
import { useLibraryStore, type IndexProgress } from "@/stores/library";

export function IndexingDialog() {
  const library = useLibraryStore();

  const progress = () => library.indexProgress();
  const pct = () => {
    const p = progress();
    if (!p || p.total === 0) return 0;
    return Math.round((p.processed / p.total) * 100);
  };

  return (
    <>
      <Show when={library.isIndexing() && progress()}>
        <div class="fixed inset-0 z-40 flex items-center justify-center" style={{ background: "rgba(0,0,0,0.5)" }}>
          <div class="rounded-xl p-8 w-96 shadow-2xl animate-scale-in" style={{ background: "var(--bg-secondary)", border: "1px solid var(--border-color)" }}>
            <div class="text-center mb-4">
              <div class="text-3xl mb-2">🔍</div>
              <h2 class="text-lg font-semibold" style={{ color: "var(--text-primary)" }}>正在索引文件</h2>
              <p class="text-xs mt-1" style={{ color: "var(--text-muted)" }}>
                {progress()?.processed ?? 0} / {progress()?.total ?? 0} 个文件
              </p>
            </div>

            <div class="w-full h-2 rounded-full overflow-hidden mb-2" style={{ background: "var(--bg-tertiary)" }}>
              <div class="h-full rounded-full transition-all duration-200" style={{ width: `${pct()}%`, background: "var(--accent)" }} />
            </div>

            <div class="text-center">
              <span class="text-xs font-mono" style={{ color: "var(--text-muted)" }}>{pct()}%</span>
            </div>

            <Show when={progress()?.current_file}>
              <p class="text-2xs text-center mt-2 truncate" style={{ color: "var(--text-muted)" }}>
                {progress()?.current_file}
              </p>
            </Show>
          </div>
        </div>
      </Show>

      <Show when={library.indexComplete()}>
        <div class="fixed inset-0 z-40 flex items-center justify-center" style={{ background: "rgba(0,0,0,0.5)" }}>
          <div class="rounded-xl p-8 w-96 shadow-2xl animate-scale-in text-center" style={{ background: "var(--bg-secondary)", border: "1px solid var(--border-color)" }}>
            <div class="text-4xl mb-3">✅</div>
            <h2 class="text-lg font-semibold mb-1" style={{ color: "var(--text-primary)" }}>导入完成</h2>
            <p class="text-sm mb-6" style={{ color: "var(--text-muted)" }}>
              已成功索引 {progress()?.total ?? 0} 个文件
            </p>
            <button class="btn-primary text-base px-8 py-2" onClick={library.dismissComplete}>
              确定
            </button>
          </div>
        </div>
      </Show>
    </>
  );
}