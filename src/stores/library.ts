import { createSignal } from "solid-js";
import { api, type LibraryInfo, type NasStatus } from "@/lib/tauri-api";
import { listen } from "@tauri-apps/api/event";

export interface IndexProgress {
  total: number;
  processed: number;
  current_file: string;
  stage: string;
}

const [libraries, setLibraries] = createSignal<LibraryInfo[]>([]);
const [activeLibraryId, setActiveLibraryId] = createSignal<string | null>(null);
const [nasStatus, setNasStatus] = createSignal<NasStatus | null>(null);
const [isIndexing, setIsIndexing] = createSignal(false);
const [indexProgress, setIndexProgress] = createSignal<IndexProgress | null>(null);
const [indexComplete, setIndexComplete] = createSignal(false);
const [isRestoring, setIsRestoring] = createSignal(true);
const [showLibraryPicker, setShowLibraryPicker] = createSignal(false);
const [restoredLibraries, setRestoredLibraries] = createSignal<LibraryInfo[]>([]);

let unlisten: (() => void) | null = null;
async function setupProgressListener() {
  if (unlisten) return;
  unlisten = await listen<IndexProgress>("index-progress", (event) => {
    setIndexProgress(event.payload);
    if (event.payload.stage === "complete") {
      setIsIndexing(false);
      setIndexComplete(true);
    } else {
      setIsIndexing(true);
    }
  });
}
setupProgressListener();

export function useLibraryStore() {
  const activeLibrary = () => {
    const id = activeLibraryId();
    return libraries().find((l) => l.id === id) || null;
  };

  const openLibrary = async (path: string) => {
    // Check if already open
    const existing = libraries().find(l => l.root_path === path);
    if (existing) {
      setActiveLibraryId(existing.id);
      return { library: existing, initial_file_count: existing.file_count };
    }

    setIsIndexing(true);
    setIndexComplete(false);
    setIndexProgress(null);
    const result = await api.openLibrary(path);
    // Check again after async call (backend may have returned existing)
    if (!libraries().find(l => l.id === result.library.id)) {
      setLibraries((prev) => [...prev, result.library]);
    }
    const indexResult = await api.indexLibrary(result.library.id);
    // Update file count after indexing
    setLibraries((prev) => prev.map(l => 
      l.id === result.library.id 
        ? { ...l, file_count: indexResult.processed }
        : l
    ));
    setActiveLibraryId(result.library.id);
    return result;
  };

  const closeLibrary = async (id: string) => {
    await api.closeLibrary(id);
    setLibraries((prev) => prev.filter((l) => l.id !== id));
    if (activeLibraryId() === id) {
      setActiveLibraryId(libraries()[0]?.id || null);
    }
  };

  const dismissComplete = () => {
    setIndexComplete(false);
  };

  const moveLibrary = (from: number, to: number) => {
    if (from === to) return;
    const arr = [...libraries()];
    const [item] = arr.splice(from, 1);
    arr.splice(to, 0, item);
    // Optimistic local reorder for instant feedback…
    setLibraries(arr);
    // …then persist so the order survives a restart.
    api.reorderLibraries(arr.map((l) => l.id)).catch((e) => {
      console.error("保存图库顺序失败:", e);
    });
  };

  const refreshNasStatus = async () => {
    const id = activeLibraryId();
    if (id) {
      const status = await api.getNasStatus(id);
      setNasStatus(status);
    }
  };

  return {
    libraries,
    activeLibraryId,
    activeLibrary,
    nasStatus,
    isIndexing,
    indexProgress,
    indexComplete,
    isRestoring,
    showLibraryPicker,
    restoredLibraries,
    openLibrary,
    closeLibrary,
    setLibraries,
    setActiveLibraryId,
    refreshNasStatus,
    dismissComplete,
    moveLibrary,
    setIsRestoring,
    setShowLibraryPicker,
    setRestoredLibraries,
  };
}

export { libraries, activeLibraryId, setLibraries, setActiveLibraryId, nasStatus, isIndexing, indexProgress, indexComplete, isRestoring, showLibraryPicker, restoredLibraries, setIsRestoring, setShowLibraryPicker, setRestoredLibraries };