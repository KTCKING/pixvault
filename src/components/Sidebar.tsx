import { For, Show, createSignal, onCleanup } from "solid-js";
import type { JSX } from "solid-js";
import { useLibraryStore, warmForLibrary } from "@/stores/library";
import { useFilesStore } from "@/stores/files";
import { useTagsStore } from "@/stores/tags";
import { useUIStore } from "@/stores/ui";
import { COLOR_LABELS, type FolderNode, type Tag } from "@/lib/tauri-api";
import { CircleProgress } from "./CircleProgress";
import { open } from "@tauri-apps/plugin-dialog";
import {
  IconCheck,
  IconChevronRight,
  IconClock,
  IconClose,
  IconFolder,
  IconGlobe,
  IconHeart,
  IconImage,
  IconLibrary,
  IconPlus,
  IconStarFilled,
  IconTrash,
} from "@/components/icons";

/** 侧边栏分区标题。action 的 icon 传 JSX（图标系统统一收口）。 */
function SectionHeader(props: { title: string; action?: { icon: JSX.Element; title: string; onClick: () => void } }) {
  return (
    <div class="sidebar-section flex items-center justify-between">
      <span class="sidebar-title text-2xs font-semibold uppercase tracking-wider" style={{ color: "var(--text-muted)" }}>
        {props.title}
      </span>
      <Show when={props.action}>
        {(a) => (
          <button class="flex p-0.5 rounded hover:bg-[var(--bg-hover)] transition-colors"
            style={{ color: "var(--text-muted)" }} title={a().title} onClick={a().onClick}>
            {a().icon}
          </button>
        )}
      </Show>
    </div>
  );
}

/** 一条侧边栏条目（视图 / 文件夹 / 标签共用）。 */
function NavItem(props: {
  active: boolean;
  icon?: JSX.Element;
  label: string;
  count?: number;
  depth?: number;
  title?: string;
  dropTarget?: boolean;
  /** 资源库条目专用：拖拽排序索引（写入 data-lib-index）。 */
  libIndex?: number;
  onClick: () => void;
  onMouseDown?: (e: MouseEvent) => void;
  onContextMenu?: (e: MouseEvent) => void;
  trailing?: () => void;
}) {
  return (
    <div
      class="sidebar-item"
      classList={{ active: props.active }}
      data-lib-index={props.libIndex}
      style={{
        // 与 .sidebar-item 自带 px-2.5（10px）对齐；树形每层 +14px。
        "padding-left": `${10 + (props.depth ?? 0) * 14}px`,
        ...(props.dropTarget
          ? { background: "var(--accent)", color: "var(--accent-contrast)", outline: "2px dashed var(--accent)", "outline-offset": "2px" }
          : {}),
      }}
      title={props.title ?? props.label}
      data-trash-drop={props.dropTarget ? "" : undefined}
      onClick={props.onClick}
      onMouseDown={props.onMouseDown}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        props.onContextMenu?.(e);
      }}
    >
      <Show when={props.icon}>
        <span class="flex-shrink-0 flex" style={{ color: props.dropTarget ? "var(--accent-contrast)" : "var(--text-muted)" }}>
          {props.icon}
        </span>
      </Show>
      <span class="truncate flex-1">{props.label}</span>
      <Show when={props.count !== undefined && props.count > 0}>
        <span class="text-2xs tabular-nums flex-shrink-0" style={{ color: props.dropTarget ? "var(--accent-contrast)" : "var(--text-muted)" }}>
          {props.count!.toLocaleString()}
        </span>
      </Show>
      <Show when={props.trailing}>
        <button
          class="ml-1 p-0.5 rounded hover:bg-red-500/20 transition-colors flex-shrink-0 flex"
          style={{ color: props.dropTarget ? "var(--accent-contrast)" : "var(--text-muted)" }}
          onClick={(e) => {
            e.stopPropagation();
            props.trailing!();
          }}
        >
          <IconClose size={12} />
        </button>
      </Show>
    </div>
  );
}

/** 文件夹树的一个节点（可递归）。 */
function FolderItem(props: {
  node: FolderNode;
  depth: number;
  activePath: string | null;
  expanded: () => Set<string>;
  toggle: (path: string) => void;
  onSelect: (path: string) => void;
}) {
  const hasChildren = () => props.node.children.length > 0;
  const isOpen = () => props.expanded().has(props.node.path);
  return (
    <>
      <div
        class="sidebar-item"
        classList={{ active: props.activePath === props.node.path }}
        style={{ "padding-left": `${10 + props.depth * 14}px` }}
        title={`${props.node.path} · ${props.node.file_count} 张`}
        onClick={() => props.onSelect(props.node.path)}
      >
        <button
          class="flex-shrink-0 flex transition-transform"
          style={{
            color: "var(--text-muted)",
            transform: isOpen() ? "rotate(90deg)" : "none",
            visibility: hasChildren() ? "visible" : "hidden",
          }}
          onClick={(e) => {
            e.stopPropagation();
            props.toggle(props.node.path);
          }}
        >
          <IconChevronRight size={12} />
        </button>
        <span class="flex-shrink-0 flex" style={{ color: "var(--text-muted)" }}>
          <IconFolder size={14} />
        </span>
        <span class="truncate flex-1">{props.node.name}</span>
        <Show when={props.node.file_count > 0}>
          <span class="text-2xs tabular-nums flex-shrink-0" style={{ color: "var(--text-muted)" }}>
            {props.node.file_count.toLocaleString()}
          </span>
        </Show>
      </div>
      <Show when={isOpen()}>
        <For each={props.node.children}>
          {(child) => (
            <FolderItem node={child} depth={props.depth + 1}
              activePath={props.activePath} expanded={props.expanded}
              toggle={props.toggle} onSelect={props.onSelect} />
          )}
        </For>
      </Show>
    </>
  );
}

/** 标签树的一个节点（按 parent_id 递归）。 */
function TagItem(props: {
  tag: Tag;
  depth: number;
  activeId: number | null;
  childMap: () => Map<number | null, Tag[]>;
  onSelect: (id: number) => void;
}) {
  const children = () => props.childMap().get(props.tag.id) ?? [];
  return (
    <>
      <div
        class="sidebar-item"
        classList={{ active: props.activeId === props.tag.id }}
        style={{ "padding-left": `${10 + props.depth * 14}px` }}
        title={props.tag.name}
        onClick={() => props.onSelect(props.tag.id)}
      >
        <span class="w-2.5 h-2.5 rounded-full flex-shrink-0" style={{ background: props.tag.color || "var(--text-muted)" }} />
        <span class="truncate flex-1">{props.tag.name}</span>
      </div>
      <For each={children()}>
        {(child) => (
          <TagItem tag={child} depth={props.depth + 1}
            activeId={props.activeId} childMap={props.childMap} onSelect={props.onSelect} />
        )}
      </For>
    </>
  );
}

export function Sidebar() {
  const library = useLibraryStore();
  const files = useFilesStore();
  const tags = useTagsStore();
  const ui = useUIStore();

  // Trash drop state is driven by the pointer-based drag in ThumbnailGrid.
  const trashActive = () => ui.trashHover();

  // ── 资源库拖拽排序 ───────────────────────────
  const [dragFrom, setDragFrom] = createSignal<number | null>(null);
  const [dragOver, setDragOver] = createSignal<number | null>(null);
  const [dragY, setDragY] = createSignal(0);
  let dragStartY = 0;
  let isDragging = false;
  let sidebarRef!: HTMLDivElement;

  const handleMouseDown = (e: MouseEvent, idx: number) => {
    if (e.button !== 0) return;
    dragStartY = e.clientY;
    setDragFrom(idx);
    setDragOver(idx);
    setDragY(e.clientY);
    isDragging = false;
    e.preventDefault();
  };

  const handleMouseMove = (e: MouseEvent) => {
    if (dragFrom() === null) return;
    setDragY(e.clientY);
    if (!isDragging && Math.abs(e.clientY - dragStartY) < 5) return;
    isDragging = true;

    const items = sidebarRef.querySelectorAll("[data-lib-index]");
    let overIdx = dragFrom()!;
    items.forEach((item) => {
      const rect = item.getBoundingClientRect();
      const idx = parseInt((item as HTMLElement).dataset.libIndex || "0");
      if (e.clientY >= rect.top && e.clientY <= rect.bottom) {
        overIdx = idx;
      }
    });
    setDragOver(overIdx);
  };

  const handleMouseUp = () => {
    const from = dragFrom();
    const to = dragOver();
    if (from !== null && to !== null && from !== to && isDragging) {
      library.moveLibrary(from, to);
    }
    setDragFrom(null);
    setDragOver(null);
    isDragging = false;
  };

  if (typeof window !== "undefined") {
    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
    onCleanup(() => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    });
  }

  // ── 新建标签 ─────────────────────────────────
  const [newTagName, setNewTagName] = createSignal("");
  const [showNewTag, setShowNewTag] = createSignal(false);

  const handleCreateTag = async () => {
    const name = newTagName().trim();
    if (!name) return;
    await tags.createTag(name);
    setNewTagName("");
    setShowNewTag(false);
  };

  // ── 文件夹树展开状态 ─────────────────────────
  const [expandedFolders, setExpandedFolders] = createSignal<Set<string>>(new Set());
  const toggleFolder = (path: string) => {
    setExpandedFolders((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const view = () => files.currentView();
  const isView = (kind: string) => view().kind === kind;

  const openLibraryDir = async () => {
    const selected = await open({ directory: true, multiple: false, title: "选择文件夹作为图片库" });
    if (selected && typeof selected === "string") {
      await library.openLibrary(selected);
    }
  };

  // 标签父子映射（parent_id 为 null 的是根）。
  const tagChildMap = () => {
    const map = new Map<number | null, Tag[]>();
    for (const t of tags.tags()) {
      const key = t.parent_id ?? null;
      if (!map.has(key)) map.set(key, []);
      map.get(key)!.push(t);
    }
    return map;
  };
  const rootTags = () => tagChildMap().get(null) ?? [];

  return (
    <aside class="app-sidebar w-56 flex-shrink-0 flex flex-col overflow-hidden"
      onContextMenu={(e) => {
        // Right-click on empty sidebar space opens the "new / open library" menu.
        e.preventDefault();
        ui.setContextMenu({
          x: e.clientX,
          y: e.clientY,
          target: { kind: "sidebar" },
        });
      }}
    >
      <div class="flex-1 overflow-y-auto pb-2">
        {/* ── 智能视图 ─────────────────────────────── */}
        <SectionHeader title="智能视图" />
        <div class="px-2 flex flex-col gap-0.5">
          <NavItem icon={<IconImage size={14} />} label="所有图片" count={files.viewCounts()?.all}
            active={isView("all")} onClick={() => files.setView({ kind: "all" })} />
          <NavItem icon={<IconClock size={14} />} label="最近新增"
            active={isView("recent")} onClick={() => files.setView({ kind: "recent" })} />
          <NavItem icon={<IconHeart size={14} />} label="收藏" count={files.viewCounts()?.favorite}
            active={isView("favorite")} onClick={() => files.setView({ kind: "favorite" })} />
          <NavItem icon={<IconStarFilled size={14} />} label="已评分" count={files.viewCounts()?.rated}
            active={isView("rated")} onClick={() => files.setView({ kind: "rated" })} />
          {/* 回收站不在这里 —— 它是跨库的「逻辑库」，入口在下面「资源库」区置底。 */}
        </div>

        {/* ── 颜色标记 ─────────────────────────────── */}
        <SectionHeader title="颜色标记" />
        <div class="px-2.5 pb-1 flex flex-wrap gap-1.5">
          <For each={COLOR_LABELS}>
            {(c) => {
              const n = () => files.viewCounts()?.colors[c.name] ?? 0;
              return (
                <button
                  class="w-5 h-5 rounded-full flex items-center justify-center transition-transform hover:scale-110"
                  classList={{ "ring-2": view().kind === "color" && view().color === c.name }}
                  style={{
                    background: c.hex,
                    "--tw-ring-color": "var(--accent)",
                    opacity: n() > 0 ? 1 : 0.35,
                  }}
                  title={`${c.name}${n() > 0 ? ` · ${n()} 张` : ""}`}
                  onClick={() => files.setView({ kind: "color", color: c.name })}
                />
              );
            }}
          </For>
          <Show when={view().kind === "color"}>
            <button class="text-2xs px-1" style={{ color: "var(--accent)" }}
              onClick={() => files.setView({ kind: "all" })}>清除</button>
          </Show>
        </div>

        {/* ── 资源库 ───────────────────────────────── */}
        <SectionHeader title="资源库" action={{ icon: <IconPlus size={14} />, title: "打开文件夹作为新库", onClick: openLibraryDir }} />
        <div class="px-2 flex flex-col gap-0.5" ref={sidebarRef}>
          <For each={library.libraries()}>
            {(lib, index) => (
              <NavItem
                icon={lib.is_network ? <IconGlobe size={14} /> : <IconLibrary size={14} />}
                label={lib.name}
                count={lib.file_count}
                active={library.activeLibraryId() === lib.id}
                title={`${lib.root_path} · ${lib.file_count} 个文件`}
                libIndex={index()}
                onClick={() => {
                  if (!isDragging) {
                    library.setActiveLibraryId(lib.id);
                  }
                }}
                onMouseDown={(e) => handleMouseDown(e, index())}
                // 右键必须在这里拦下来。不挂的话事件会冒泡到 <aside>，
                // 弹出的是「新建库 / 打开已有库」那个空白区菜单 —— 库行上
                // 想重命名或删除，只能在别的库菜单里找，等于没有。
                onContextMenu={(e) => {
                  ui.setContextMenu({
                    x: e.clientX,
                    y: e.clientY,
                    target: { kind: "library", libraryId: lib.id },
                  });
                }}
                trailing={() => {
                  void library.closeLibrary(lib.id);
                }}
              />
            )}
          </For>
          {/* 缩略图预热进度：整库补完自动收起。 */}
          <For each={library.libraries()}>
            {(lib) => (
              <Show when={warmForLibrary(lib.id)}>
                {(w) => (
                  <div class="sidebar-item" style={{ "padding-left": "10px", cursor: "default" }}
                    title={`${lib.name}：正在生成缩略图，已处理 ${w().done} / ${w().total} 张`}>
                    <CircleProgress value={w().done} max={w().total} />
                    <span class="truncate flex-1 text-2xs" style={{ color: "var(--text-muted)" }}>
                      正在补齐 {lib.name} 的缩略图
                    </span>
                    <span class="text-2xs tabular-nums flex-shrink-0" style={{ color: "var(--accent)" }}>
                      {w().done}/{w().total}
                    </span>
                  </div>
                )}
              </Show>
            )}
          </For>
          {/*
            回收站是「逻辑库」：置底常驻、跨所有库聚合、没有自己的目录。
            它固定排在所有真实库之后，不参与拖拽排序（handleMouseDown 只挂在库行上）。
            拖图片到它上面 = 软删除到该图片所在的库。
          */}
          <NavItem
            icon={<IconTrash size={14} />}
            label="回收站"
            count={files.trashTotal()}
            active={isView("trash")}
            dropTarget={trashActive()}
            title={trashActive()
              ? "松开以移入回收站"
              : "回收站：所有库共享，拖图片到此处删除"}
            onClick={() => files.setView({ kind: "trash" })}
            onContextMenu={() => {
              const n = files.trashTotal();
              if (n === 0) {
                ui.showToast("回收站已经是空的", "info");
                return;
              }
              const alsoDisk = ui.purgeDeletesDisk();
              ui.requestConfirm({
                title: "清空回收站",
                message: alsoDisk
                  ? `将删除所有库回收站里的 ${n} 个文件，并连同磁盘上的原文件一起删除。此操作不可恢复。`
                  : `将把 ${n} 个文件从库中移除，磁盘上的原文件会保留。若希望连原文件一起删，可在设置里打开「彻底删除时一并删除磁盘文件」。`,
                confirmText: alsoDisk ? "删除并清理磁盘" : "从库中移除",
                danger: true,
                onConfirm: async () => {
                  const r = await files.emptyTrash();
                  if (r.failed.length > 0) {
                    ui.showToast(`已清空 ${r.removed} 个，${r.failed.length} 个未能删除`, "error");
                  } else {
                    ui.showToast(`已清空 ${r.removed} 个`);
                  }
                },
              });
            }}
          />
          <Show when={library.libraries().length === 0}>
            <p class="px-2.5 py-1 text-2xs" style={{ color: "var(--text-muted)" }}>还没有打开任何库</p>
          </Show>
        </div>

        {/* ── 文件夹 ───────────────────────────────── */}
        <Show when={library.activeLibrary()}>
          <SectionHeader title="文件夹" />
          <div class="px-2 flex flex-col gap-0.5">
            <Show when={files.folderTree()} fallback={
              <p class="px-2.5 py-1 text-2xs" style={{ color: "var(--text-muted)" }}>加载中…</p>
            }>
              {(tree) => (
                <>
                  <NavItem icon={<IconImage size={14} />} label="整个库" count={tree().file_count}
                    active={view().kind === "all" || view().kind === "recent" || view().kind === "favorite" || view().kind === "rated"}
                    title="显示整个库的图片"
                    onClick={() => files.setView({ kind: "all" })} />
                  <For each={tree().children}>
                    {(node) => (
                      <FolderItem node={node} depth={0}
                        activePath={view().kind === "folder" ? view().folder ?? null : null}
                        expanded={expandedFolders} toggle={toggleFolder}
                        onSelect={(path) => files.setView({ kind: "folder", folder: path })} />
                    )}
                  </For>
                </>
              )}
            </Show>
          </div>
        </Show>

        {/* ── 标签 ─────────────────────────────────── */}
        <Show when={library.activeLibrary()}>
          <SectionHeader title="标签"
            action={{ icon: <IconPlus size={14} />, title: "新建标签", onClick: () => setShowNewTag(!showNewTag()) }} />
          <div class="px-2 flex flex-col gap-0.5">
            <Show when={showNewTag()}>
              <div class="flex gap-1 mb-1 px-2.5">
                <input class="input flex-1 text-xs py-1" placeholder="新建标签..." value={newTagName()}
                  onInput={(e) => setNewTagName(e.currentTarget.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleCreateTag()} />
                <button class="btn-primary text-xs px-2 flex items-center" onClick={handleCreateTag}>
                  <IconCheck size={13} />
                </button>
              </div>
            </Show>
            <Show when={tags.tags().length === 0} fallback={
              <For each={rootTags()}>
                {(tag) => (
                  <TagItem tag={tag} depth={0}
                    activeId={view().kind === "tag" ? view().tagId ?? null : null}
                    childMap={tagChildMap}
                    onSelect={(id) => files.setView({ kind: "tag", tagId: id })} />
                )}
              </For>
            }>
              <p class="px-2.5 py-1 text-2xs" style={{ color: "var(--text-muted)" }}>
                还没有标签。选中图片后在右键菜单里添加。
              </p>
            </Show>
          </div>
        </Show>
      </div>
    </aside>
  );
}
