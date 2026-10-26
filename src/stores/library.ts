import { createSignal } from "solid-js";
import { api, type LibraryInfo, type NasStatus, type WarmProgress } from "@/lib/tauri-api";
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

/**
 * 缩略图预热进度 —— 按图库 id 索引。
 *
 * 打开 / 导入 / 恢复图库时后端会自动把缺缩略图的文件排进后台队列（默认启用），
 * 处理过程中推 `thumb-warm` 事件；侧边栏据此在图库名旁边画圆形进度环。
 */
const [warmProgress, setWarmProgressSignal] = createSignal<Record<string, WarmProgress>>({});

function setWarmProgress(list: WarmProgress[]) {
  const map: Record<string, WarmProgress> = {};
  for (const w of list) map[w.library_id] = w;
  setWarmProgressSignal(map);
}

let unlistenWarm: (() => void) | null = null;
async function setupWarmListener() {
  if (unlistenWarm) return;
  unlistenWarm = await listen<WarmProgress[]>("thumb-warm", (event) => {
    setWarmProgress(event.payload ?? []);
  });
  // 事件只在"有变化"时推送，首次进来先拉一次当前快照，
  // 免得"界面刚打开时后端已经在跑"这种情况看不到进度环。
  try {
    setWarmProgress(await api.getWarmProgress());
  } catch {
    /* 忽略：拿不到就不显示进度环 */
  }
}
setupWarmListener();

/**
 * 取某个库"值得显示进度环"的预热状态。
 *
 * 返回 undefined 的两种情况：没有预热记录，或本轮已经处理完 —— 圆环只在真的
 * 还在补的时候出现，跑完就收起，不占地方。
 */
export function warmForLibrary(id: string): WarmProgress | undefined {
  const w = warmProgress()[id];
  return w && w.total > 0 && w.done < w.total ? w : undefined;
}

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
    warmProgress,
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
    setWarmProgress,
  };
}

export { libraries, activeLibraryId, setLibraries, setActiveLibraryId, nasStatus, isIndexing, indexProgress, indexComplete, isRestoring, showLibraryPicker, restoredLibraries, setIsRestoring, setShowLibraryPicker, setRestoredLibraries, warmProgress, setWarmProgress };