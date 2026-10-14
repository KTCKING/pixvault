import { createSignal, Show, onCleanup } from "solid-js";
import { useFilesStore } from "@/stores/files";
import { api } from "@/lib/tauri-api";
import { activeLibraryId } from "@/stores/library";

export function SearchBar() {
  const files = useFilesStore();
  let inputRef!: HTMLInputElement;
  const [query, setQuery] = createSignal("");
  const [results, setResults] = createSignal<number | null>(null);
  const [searching, setSearching] = createSignal(false);

  const handleSearch = async () => {
    const q = query().trim();
    if (!q) {
      setResults(null);
      // Restore full file list
      files.loadFiles(0);
      return;
    }
    setSearching(true);
    const libId = activeLibraryId();
    if (libId) {
      try {
        const result = await api.searchFiles(libId, { text: q });
        setResults(result.total);
        // Update the grid with search results
        files.setFiles(result.files);
        files.setTotalFiles(result.total);
      } catch {
        setResults(0);
      }
    }
    setSearching(false);
  };

  let debounceTimer: ReturnType<typeof setTimeout>;
  const handleInput = (value: string) => {
    setQuery(value);
    clearTimeout(debounceTimer);
    if (value.trim()) debounceTimer = setTimeout(handleSearch, 300);
    else {
      setResults(null);
      files.loadFiles(0);
    }
  };
  onCleanup(() => clearTimeout(debounceTimer));

  const s = { muted: "var(--text-muted)" };

  return (
    <div class="relative flex items-center">
      <div class="relative">
        <input ref={inputRef} class="input text-xs w-56 py-1 pl-7 pr-8" placeholder="搜索文件... (Ctrl+F)"
          value={query()} onInput={(e) => handleInput(e.currentTarget.value)}
          onKeyDown={(e) => { if (e.key === "Enter") handleSearch(); if (e.key === "Escape") { setQuery(""); setResults(null); files.loadFiles(0); } }} />
        <span class="absolute left-2 top-1/2 -translate-y-1/2 text-xs" style={{ color: s.muted }}>🔍</span>
        <Show when={query()}>
          <button class="absolute right-1 top-1/2 -translate-y-1/2 text-xs" style={{ color: s.muted }}
            onClick={() => { setQuery(""); setResults(null); files.loadFiles(0); inputRef.focus(); }}>✕</button>
        </Show>
      </div>
      <Show when={results() !== null}>
        <span class="ml-2 text-xs" style={{ color: s.muted }}>{searching() ? "搜索中..." : `${results()} 个结果`}</span>
      </Show>
    </div>
  );
}