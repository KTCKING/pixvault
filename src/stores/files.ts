import { createSignal, createEffect } from "solid-js";
import { api, type FileInfo, type PaginatedFiles, type ViewState, type FolderNode, type ViewCounts, type TrashEntry, type TrashSource } from "@/lib/tauri-api";
import { activeLibraryId, libraries, setLibraries } from "./library";
import { recentWindowDays, purgeDeletesDisk } from "./ui";

const [files, setFiles] = createSignal<FileInfo[]>([]);
const [totalFiles, setTotalFiles] = createSignal(0);
const [selectedFiles, setSelectedFiles] = createSignal<Set<string>>(new Set());
const [currentSort, setCurrentSort] = createSignal({ by: "mtime", order: "DESC" });
const [filterExt, setFilterExt] = createSignal<string | undefined>();
/** 当前浏览的视图（侧边栏点谁就是谁）。默认「所有图片」。 */
const [currentView, setCurrentView] = createSignal<ViewState>({ kind: "all" });
const [isLoading, setIsLoading] = createSignal(false);
/** 回收站里的条目（自带来源库）。 */
const [deletedFiles, setDeletedFiles] = createSignal<TrashEntry[]>([]);
/** 回收站按来源库的汇总，用于顶部筛选条。 */
const [trashSources, setTrashSources] = createSignal<TrashSource[]>([]);
/** 读不出来的库名（NAS 离线等）。 */
const [trashUnavailable, setTrashUnavailable] = createSignal<string[]>([]);
/** 跨库回收站总条目数（侧栏徽标）。 */
const [trashTotal, setTrashTotal] = createSignal(0);
/** 回收站里按来源库筛选（null = 全部）。 */
const [trashFilter, setTrashFilterSignal] = createSignal<string | null>(null);
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
    // 回收站是逻辑库，跨所有库聚合，不依赖当前活动库。
    if (view.kind === "trash") {
      await loadDeletedFiles();
      return;
    }

    const libId = activeLibraryId();
    if (!libId) return;

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

  /**
   * 加载回收站 —— 跨所有库聚合。
   *
   * 回收站是逻辑库，不属于当前活动库。后端遍历 Engine 里已打开的库
   * （启动时已全部打开、连接常驻），每库一句走索引的查询。
   */
  /** 按来源库筛选后重新铺到网格上（回收站视图专用）。 */
  const applyTrashFilter = () => {
    const id = trashFilter();
    const shown = id ? deletedFiles().filter((f) => f.library_id === id) : deletedFiles();
    setFiles(shown);
    setTotalFiles(shown.length);
  };

  /** 切换来源库筛选。`null` = 全部库。 */
  const setTrashFilter = (libraryId: string | null) => {
    setTrashFilterSignal(libraryId);
    if (currentView().kind === "trash") applyTrashFilter();
  };

  /**
   * 某张图属于哪个库。
   *
   * 回收站是跨库的逻辑库，条目来自不同库；凡是「按库定位」的操作
   * （预览、打开、复制、在资源管理器中显示）都必须用条目**自己的**库，
   * 不能一律用当前活动库，否则对其他库的条目会全部失败。
   */
  const libraryIdOfPath = (path: string | null | undefined): string | undefined => {
    if (path && currentView().kind === "trash") {
      const hit = deletedFiles().find((f) => f.path === path);
      if (hit) return hit.library_id;
    }
    return activeLibraryId() ?? undefined;
  };

  const loadDeletedFiles = async () => {
    setIsLoading(true);
    try {
      const payload = await api.getTrash();
      setDeletedFiles(payload.entries);
      setTrashSources(payload.sources);
      setTrashUnavailable(payload.unavailable);
      // 原来选中的来源库如果已经空了，退回「全部」，别让用户停在空视图上。
      if (trashFilter() && !payload.sources.some((s) => s.library_id === trashFilter())) {
        setTrashFilterSignal(null);
      }
      applyTrashFilter();
    } catch {
      setDeletedFiles([]);
      setTrashSources([]);
      setFiles([]);
      setTotalFiles(0);
    } finally {
      setIsLoading(false);
    }
  };

  /** 从回收站视图里摘掉若干条目，并按来源库重算筛选条的计数。 */
  const dropFromTrash = (paths: string[]) => {
    if (paths.length === 0) return;
    const gone = new Set(paths);
    const remaining = deletedFiles().filter((f) => !gone.has(f.path));
    setDeletedFiles(remaining);
    setTrashSources((prev) =>
      prev
        .map((s) => ({
          ...s,
          count: remaining.filter((f) => f.library_id === s.library_id).length,
        }))
        .filter((s) => s.count > 0)
    );
    if (currentView().kind === "trash") applyTrashFilter();
  };

  /**
   * 恢复回收站里的文件回它的**来源库**。
   *
   * `libraryId` 必须由调用方从条目上带下来：回收站跨库，不能想当然用当前活动库。
   */
  const restoreFile = async (filePath: string, libraryId?: string) => {
    const libId = libraryId ?? activeLibraryId();
    if (!libId) return;
    await api.restoreFile(libId, filePath);
    dropFromTrash([filePath]);
    // 恢复到的是**来源库**，不一定是当前活动库，所以按 id 直接改那个库的计数。
    setLibraries((prev) =>
      prev.map((l) => (l.id === libId ? { ...l, file_count: l.file_count + 1 } : l))
    );
    void loadViewCounts();
  };

  /** 彻底删除回收站里的文件（是否连磁盘一起删看设置里的开关）。 */
  const purgeFile = async (filePath: string, libraryId?: string) => {
    const libId = libraryId ?? activeLibraryId();
    if (!libId) return;
    await api.purgeFile(libId, filePath, purgeDeletesDisk());
    dropFromTrash([filePath]);
    void loadViewCounts();
  };

  /** 清空回收站 —— 跨所有库。返回删不掉的文件，交给调用方提示。 */
  const emptyTrash = async () => {
    const result = await api.emptyTrashAll(purgeDeletesDisk());
    setDeletedFiles([]);
    setTrashSources([]);
    if (currentView().kind === "trash") {
      setFiles([]);
      setTotalFiles(0);
    }
    void loadViewCounts();
    return result;
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
  /**
   * 跨库回收站的总条目数（侧栏徽标）。
   *
   * 回收站是逻辑库，不属于任何单一库，所以这个数字不能从 `viewCounts` 里取
   * —— 那个是按当前活动库算的。后端单独给了一个只 COUNT 的命令。
   */
  const loadTrashCount = async () => {
    try {
      setTrashTotal(await api.getTrashCount());
    } catch {
      // ignore — 徽标是锦上添花
    }
  };

  const loadViewCounts = async () => {
    void loadTrashCount();
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
    trashSources,
    trashUnavailable,
    trashTotal,
    trashFilter,
    setTrashFilter,
    libraryIdOfPath,
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
