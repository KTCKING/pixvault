import { createSignal } from "solid-js";
import { api, type Tag } from "@/lib/tauri-api";
import { activeLibraryId } from "./library";

const [tags, setTags] = createSignal<Tag[]>([]);
const [selectedTagIds, setSelectedTagIds] = createSignal<Set<number>>(new Set());

export function useTagsStore() {
  const loadTags = async () => {
    const libId = activeLibraryId();
    if (!libId) return;
    const result = await api.getAllTags(libId);
    setTags(result);
  };

  const createTag = async (name: string, parentId?: number, color?: string) => {
    const libId = activeLibraryId();
    if (!libId) return null;
    const tag = await api.createTag(libId, name, parentId, color);
    setTags((prev) => [...prev, tag]);
    return tag;
  };

  const addTagToFile = async (filePath: string, tagId: number) => {
    const libId = activeLibraryId();
    if (!libId) return;
    await api.addTagToFile(libId, filePath, tagId);
  };

  const removeTagFromFile = async (filePath: string, tagId: number) => {
    const libId = activeLibraryId();
    if (!libId) return;
    await api.removeTagFromFile(libId, filePath, tagId);
  };

  const batchAddTags = async (filePaths: string[], tagIds: number[]) => {
    const libId = activeLibraryId();
    if (!libId) return;
    return api.batchAddTags(libId, filePaths, tagIds);
  };

  return {
    tags,
    selectedTagIds,
    loadTags,
    createTag,
    addTagToFile,
    removeTagFromFile,
    batchAddTags,
    setSelectedTagIds,
  };
}