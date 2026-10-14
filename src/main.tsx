import { render } from "solid-js/web";
import App from "./App";
import "./styles/globals.css";

console.log("PixVault: main.tsx loaded");

// Solid 的 render() 只追加、不清空挂载容器（要求容器为空）
const rootEl = document.getElementById("root")!;
rootEl.textContent = "";

render(() => <App />, rootEl);

// 移除启动加载屏（位于 #root 之外，移除后应用界面即可见）
document.getElementById("boot-splash")?.remove();
console.log("PixVault: render called");