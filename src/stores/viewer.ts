import { createSignal } from "solid-js";
import type { FileInfo } from "@/lib/tauri-api";

const [viewerOpen, setViewerOpen] = createSignal(false);
const [viewerFiles, setViewerFiles] = createSignal<FileInfo[]>([]);
const [viewerIndex, setViewerIndex] = createSignal(0);

export function useViewerStore() {
  const open = (files: FileInfo[], index: number) => {
    setViewerFiles(files);
    setViewerIndex(index);
    setViewerOpen(true);
  };

  const close = () => {
    setViewerOpen(false);
  };

  const next = () => {
    setViewerIndex((i) => Math.min(i + 1, viewerFiles().length - 1));
  };

  const prev = () => {
    setViewerIndex((i) => Math.max(i - 1, 0));
  };

  const currentFile = () => viewerFiles()[viewerIndex()];

  return {
    viewerOpen,
    viewerFiles,
    viewerIndex,
    currentFile,
    open,
    close,
    next,
    prev,
  };
}

export { viewerOpen, viewerFiles, viewerIndex };