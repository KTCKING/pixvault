import { createSignal, createEffect } from "solid-js";
import { api, type FileInfo, type PaginatedFiles, type ViewState, type FolderNode, type ViewCounts } from "@/lib/tauri-api";
import { activeLibraryId, libraries, setLibraries } from "./library";
import { recentWindowDays } from "./ui";

const [files, setFiles] = createSignal<FileInfo[]>([]);
const [totalFiles, setTotalFiles] = createSignal(0);
const [selectedFiles, setSelectedFiles] = createSignal<Set<string>>(new Set());
const [currentSort, setCurrentSort] = createSignal({ by: "mtime", order: "DESC" });
const [filterExt, setFilterExt] = createSignal<string | undefined>();
/** 当前浏览的视图（侧边栏点谁就是谁）。默认「所有图片」。 */
const [currentView, setCurrentView] = createSignal<ViewState>({ kind: "all" });
const [isLoading, setIsLoading] = createSignal(false);
const [deletedFiles, setDeletedFiles] = createSignal<FileInfo[]>([]);
/** 侧边栏「文件夹」树。 */
const [folderTree, setFolderTree] = createSignal<FolderNode | null>(null);
/** 侧边栏智能视图 / 颜色标记的计数徽标。 */
const [viewCounts, setViewCounts] = createSignal<ViewCounts | null>(null);

const PAGE_SIZE = 100;

export function useFilesStore() {
  /** 把当前视图翻译成 get_files 的查询参数。 */
  const queryForView = (view: ViewState, sort: { by: string; order: string }) => {
    switch (view.kind) {
      case "recent":
        // 「最近新增」= 最近 N 天导入的（N 在设置里选 7 / 30），按导入时间倒序。
        return { sortBy: "created_at", sortOrder: "DESC", filterFolder: undefined, filterFavorite: undefined, filterColor: undefined, filterMinRating: undefined, filterTag: undefined, filterRecentDays: recentWindowDays() };
      case "favorite":
        return { sortBy: "created_at", sortOrder: "DESC", filterFolder: undefined, filterFavorite: true, filterColor: undefined, filterMinRating: undefined, filterTag: undefined, filterRecentDays: undefined };
      case "rated":
        return { sortBy: "rating", sortOrder: "DESC", filterFolder: undefined, filterFavorite: undefined, filterColor: undefined, filterMinRating: 1, filterTag: undefined, filterRecentDays: undefined };
      case "color":
        return { sortBy: "mtime", sortOrder: "DESC", filterFolder: undefined, filterFavorite: undefined, filterColor: view.color, filterMinRating: undefined, filterTag: undefined, filterRecentDays: undefined };
      case "folder":
        return { sortBy: "mtime", sortOrder: "DESC", filterFolder: view.folder, filterFavorite: undefined, filterColor: undefined, filterMinRating: undefined, filterTag: undefined, filterRecentDays: undefined };
      case "tag":
        return { sortBy: "mtime", sortOrder: "DESC", filterFolder: undefined, filterFavorite: undefined, filterColor: undefined, filterMinRating: undefined, filterTag: view.tagId, filterRecentDays: undefined };
      default:
        return { sortBy: sort.by, sortOrder: sort.order, filterFolder: undefined, filterFavorite: undefined, filterColor: undefined, filterMinRating: undefined, filterTag: undefined, filterRecentDays: undefined };
    }
  };

  /**
   * 加载当前视图的文件。
   *
   * `view` 允许显式传入，避免在 `setView` 里又回头读 `currentView()` signal：
   * 一旦有 effect 通过 setView 间接触发 loadFiles，读 signal 会让该 effect
   * 意外订阅「当前视图」，进而在切换视图时自我重置。
   */
  const loadFiles = async (offset = 0, view: ViewState = currentView()) => {
    const libId = activeLibraryId();
    if (!libId) return;

    // 回收站是独立的数据通道。
    if (view.kind === "trash") {
      await loadDeletedFiles();
      return;
    }

    if (offset === 0) {
      setFiles([]);
      setTotalFiles(0);
    }
    setIsLoading(true);
    try {
      const q = queryForView(view, currentSort());
      const result = await api.getFiles(
        libId,
        offset,
        PAGE_SIZE,
        q.sortBy,
        q.sortOrder,
        filterExt(),
        q.filterTag,
        q.filterFolder,
        q.filterFavorite,
        q.filterColor,
        q.filterMinRating,
        q.filterRecentDays
      );
      if (offset === 0) {
        setFiles(result.files);
      } else {
        setFiles((prev) => [...prev, ...result.files]);
      }
      setTotalFiles(result.total);
    } finally {
      setIsLoading(false);
    }
  };

  /**
   * 切换侧边栏视图。每种视图带一个合理的默认排序（最近新增按导入时间、
   * 已评分按评分），用户随后仍可在工具栏改。
   */
  const setView = async (view: ViewState) => {
    setCurrentView(view);
    setSelectedFiles(new Set<string>());
    const defaultSort: Record<string, { by: string; order: string }> = {
      recent: { by: "created_at", order: "DESC" },
      favorite: { by: "created_at", order: "DESC" },
      rated: { by: "rating", order: "DESC" },
    };
    if (defaultSort[view.kind]) setCurrentSort(defaultSort[view.kind]);
    await loadFiles(0, view);
  };

  const toggleSelect = (path: string, multi = false) => {
    setSelectedFiles((prev) => {
      const next = multi ? new Set(prev) : new Set<string>();
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  };

  /**
   * Batch-set the selection (used by marquee / drag-select).
   * @param paths   file paths to mark as selected
   * @param additive  when true, union with `base` (Ctrl/Cmd-drag) instead of replacing
   * @param base    selection snapshot taken when the drag started
   */
  const setSelection = (paths: Iterable<string>, additive = false, base?: Set<string>) => {
    const next = additive ? new Set<string>(base ?? []) : new Set<string>();
    for (const p of paths) next.add(p);
    setSelectedFiles(next);
  };

  const selectAll = () => {
    setSelectedFiles(new Set<string>(files().map((f) => f.path)));
  };

  const clearSelection = () => {
    setSelectedFiles(new Set<string>());
  };

  const setFilesDirect = (newFiles: FileInfo[]) => {
    setFiles(newFiles);
  };

  const setTotalDirect = (total: number) => {
    setTotalFiles(total);
  };

  const updateRating = async (filePath: string, rating: number) => {
    const libId = activeLibraryId();
    if (!libId) return;
    await api.updateFileRating(libId, filePath, rating);
    setFiles((prev) =>
      prev.map((f) => (f.path === filePath ? { ...f, rating } : f))
    );
    // 「已评分」视图的计数可能变化。
    void loadViewCounts();
  };

  /** 收藏 / 取消收藏（即时 UI + 后台刷新计数）。 */
  const setFavorite = async (filePath: string, favorite: boolean) => {
    const libId = activeLibraryId();
    if (!libId) return;
    await api.setFileFavorite(libId, filePath, favorite);
    setFiles((prev) =>
      prev.map((f) => (f.path === filePath ? { ...f, favorite } : f))
    );
    void loadViewCounts();
  };

  /** 设置 / 清除颜色标记（color 传 null 清除）。 */
  const setColor = async (filePath: string, color: string | null) => {
    const libId = activeLibraryId();
    if (!libId) return;
    await api.setFileColor(libId, filePath, color);
    setFiles((prev) =>
      prev.map((f) => (f.path === filePath ? { ...f, color_label: color } : f))
    );
    void loadViewCounts();
  };

  /** Reflect the active library's file count in the sidebar (instant UI). */
  const syncLibraryCount = (count: number) => {
    const id = activeLibraryId();
    if (!id) return;
    setLibraries((prev) => prev.map((l) => (l.id === id ? { ...l, file_count: count } : l)));
  };

  const deleteFile = async (filePath: string) => {
    const libId = activeLibraryId();
    if (!libId) return;
    await api.deleteFile(libId, filePath);
    setFiles((prev) => prev.filter((f) => f.path !== filePath));
    setTotalFiles((p) => p - 1);
    syncLibraryCount(totalFiles() - 1);
  };

  /** Move files to trash (soft-delete). Accepts explicit paths or the current selection. */
  const trashFiles = async (paths?: string[]) => {
    const libId = activeLibraryId();
    if (!libId) return;
    const targets = paths && paths.length > 0 ? paths : [...selectedFiles()];
    if (targets.length === 0) return;
    await api.trashFiles(libId, targets);
    const targetSet = new Set(targets);
    setFiles((prev) => prev.filter((f) => !targetSet.has(f.path)));
    const newTotal = totalFiles() - targets.length;
    setTotalFiles(newTotal);
    setSelectedFiles((prev) => {
      const next = new Set(prev);
      for (const p of targets) next.delete(p);
      return next;
    });
    syncLibraryCount(newTotal);
    void loadViewCounts();
  };

  /** Load the trash contents of the active library. */
  const loadDeletedFiles = async () => {
    const libId = activeLibraryId();
    if (!libId) return;
    const result = await api.getDeletedFiles(libId);
    setDeletedFiles(result);
    setFiles(result);
    setTotalFiles(result.length);
  };

  /** Restore a file from trash back into the library. */
  const restoreFile = async (filePath: string) => {
    const libId = activeLibraryId();
    if (!libId) return;
    await api.restoreFile(libId, filePath);
    setDeletedFiles((prev) => prev.filter((f) => f.path !== filePath));
    if (currentView().kind === "trash") {
      setFiles((prev) => prev.filter((f) => f.path !== filePath));
      setTotalFiles((p) => p - 1);
    }
    syncLibraryCount(totalFiles() + 1);
    void loadViewCounts();
  };

  /** Permanently delete a file from trash. */
  const purgeFile = async (filePath: string) => {
    const libId = activeLibraryId();
    if (!libId) return;
    await api.purgeFile(libId, filePath);
    setDeletedFiles((prev) => prev.filter((f) => f.path !== filePath));
    if (currentView().kind === "trash") {
      setFiles((prev) => prev.filter((f) => f.path !== filePath));
      setTotalFiles((p) => p - 1);
    }
    void loadViewCounts();
  };

  /** Permanently delete everything in the trash. */
  const emptyTrash = async () => {
    const libId = activeLibraryId();
    if (!libId) return;
    await api.emptyTrash(libId);
    setDeletedFiles([]);
    if (currentView().kind === "trash") {
      setFiles([]);
      setTotalFiles(0);
    }
    // Emptying the trash doesn't change the active file count, but re-sync
    // to be safe against any drift.
    syncLibraryCount(totalFiles());
    void loadViewCounts();
  };

  /** Update a single file's metadata in the local list (for instant UI feedback). */
  const updateFileTags = async (filePath: string) => {
    const libId = activeLibraryId();
    if (!libId) return;
    try {
      const info = await api.getFileInfo(libId, filePath);
      setFiles((prev) => prev.map((f) => (f.path === filePath ? { ...f, tags: info.tags } : f)));
    } catch {
      // ignore
    }
  };

  /** 侧边栏「文件夹」树。 */
  const loadFolderTree = async () => {
    const libId = activeLibraryId();
    if (!libId) return;
    try {
      const tree = await api.getFolderTree(libId);
      setFolderTree(tree);
    } catch (e) {
      console.error("加载文件夹树失败:", e);
    }
  };

  /** 侧边栏计数徽标。 */
  const loadViewCounts = async () => {
    const libId = activeLibraryId();
    if (!libId) return;
    try {
      const counts = await api.getViewCounts(libId);
      setViewCounts(counts);
    } catch {
      // ignore — 徽标是锦上添花
    }
  };

  return {
    files,
    totalFiles,
    selectedFiles,
    currentSort,
    currentView,
    filterExt,
    isLoading,
    deletedFiles,
    folderTree,
    viewCounts,
    loadFiles,
    setView,
    toggleSelect,
    setSelection,
    selectAll,
    clearSelection,
    updateRating,
    setFavorite,
    setColor,
    deleteFile,
    trashFiles,
    loadDeletedFiles,
    restoreFile,
    purgeFile,
    emptyTrash,
    updateFileTags,
    loadFolderTree,
    loadViewCounts,
    setCurrentSort,
    setFilterExt,
    setFiles: setFilesDirect,
    setTotalFiles: setTotalDirect,
  };
}

export { files, totalFiles, selectedFiles, isLoading, deletedFiles, currentView, folderTree, viewCounts };
