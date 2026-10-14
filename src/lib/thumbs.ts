/**
 * 缩略图 URL 的存放处：**每张图一个独立信号**。
 *
 * 为什么不用一个大对象信号（`createSignal<Record<string,string>>`）：网格 / 列表
 * 里每个卡片都读同一个信号，任何一张图就绪都会让**全部**卡片重算一遍。列表视图
 * 一次渲染上百行、响应又是成批到达的，这种「一个改动 → 全表重算」会退化成
 * O(n²)，表现为滚动发卡、图片迟迟不出现。
 *
 * 拆成 per-key 信号后，某张图就绪只会让那一张卡片重新求值。
 */
import { createSignal, type Accessor } from "solid-js";
import { convertFileSrc } from "@tauri-apps/api/core";

type Accessor0 = Accessor<string | undefined>;
type Setter0 = (value: string | undefined) => string | undefined;

const accessors = new Map<string, Accessor0>();
const setters = new Map<string, Setter0>();

/**
 * 把后端给的地址转成 `<img src>` 能直接用的地址。
 *
 * 存进来的可能是三种东西：库内缩略图的**本地路径**（走 asset 协议）、原图路径，
 * 或者兜底用的 `data:` URL。只有本地路径才需要 `convertFileSrc` —— 对 data URL
 * 再调一次会把它拼成 `http://asset.localhost/data%3A…`，反而加载不出来。
 */
export function toDisplaySrc(url: string): string {
  return /^(data:|https?:|blob:|asset:)/.test(url) ? url : convertFileSrc(url);
}

function ensure(path: string): { get: Accessor0; set: Setter0 } {
  let get = accessors.get(path);
  let set = setters.get(path);
  if (!get || !set) {
    const [g, s] = createSignal<string | undefined>(undefined);
    get = g;
    set = s as Setter0;
    accessors.set(path, get);
    setters.set(path, set);
  }
  return { get, set };
}

/** 读取某张图当前可用的图片地址（缩略图或原图；undefined = 还没就绪）。 */
export function thumbUrl(path: string): string | undefined {
  return ensure(path).get();
}

/** 写入某张图的图片地址。 */
export function setThumbUrl(path: string, url: string | undefined): void {
  ensure(path).set(url);
}

/** 切换图库时清空（旧库的卡片会被整体重建，不会读到陈旧值）。 */
export function clearThumbUrls(): void {
  accessors.clear();
  setters.clear();
}
