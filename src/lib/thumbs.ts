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

// ── 模糊占位图（LQIP）─────────────────────────────
//
// 和缩略图**分开存**：占位图是 32px 的 data URL，只用来在真图就绪前垫一层
// 模糊底。两者生命周期不同（占位图会先到、真图到了要盖在上面淡入），所以不
// 共用一个信号，免得「真图来了」把占位图判定成"已就绪"。

const phAccessors = new Map<string, Accessor0>();
const phSetters = new Map<string, Setter0>();

function ensurePh(path: string): { get: Accessor0; set: Setter0 } {
  let get = phAccessors.get(path);
  let set = phSetters.get(path);
  if (!get || !set) {
    const [g, s] = createSignal<string | undefined>(undefined);
    get = g;
    set = s as Setter0;
    phAccessors.set(path, get);
    phSetters.set(path, set);
  }
  return { get, set };
}

/** 读取某张图已缓存的模糊占位图（undefined = 没有）。 */
export function placeholderUrl(path: string): string | undefined {
  return ensurePh(path).get();
}

/** 写入某张图的模糊占位图。 */
export function setPlaceholderUrl(path: string, url: string | undefined): void {
  if (!url) return;
  ensurePh(path).set(url);
}

/** 切换图库时清空（旧库的卡片会被整体重建，不会读到陈旧值）。 */
export function clearThumbUrls(): void {
  accessors.clear();
  setters.clear();
  phAccessors.clear();
  phSetters.clear();
}

/**
 * 视频文件判断。
 *
 * 视频**不进缩略图队列**（后端没有解码器，硬排只会每轮开库都白试一遍），
 * 所以凡事要「向缩略图队列要结果」的地方都得先把它过滤掉 —— 尤其是整库补扫，
 * 否则视频会永远算「缺缩略图」，每次队列跑空都触发一轮无效重问。
 */
const VIDEO_RE = /\.(mp4|m4v|webm|mov|mkv|avi|wmv|flv|ogv|mpg|mpeg|3gp)$/i;

export function isVideoPath(path: string): boolean {
  return VIDEO_RE.test(path);
}

