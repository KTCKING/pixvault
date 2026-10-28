const DEFAULTS = {
  port: 41596,
  token: "",
  folderBySite: true,
  folder: "",
  minSize: 64,
  hoverButton: true,
  includeVideo: true,
};

const $ = (id) => document.getElementById(id);
const testState = $("testState");
const saveState = $("saveState");

async function load() {
  const cfg = await chrome.storage.local.get(DEFAULTS);
  $("port").value = cfg.port;
  $("token").value = cfg.token;
  $("folderBySite").checked = cfg.folderBySite;
  $("folder").value = cfg.folder;
  $("minSize").value = cfg.minSize;
  $("hoverButton").checked = cfg.hoverButton;
  $("includeVideo").checked = cfg.includeVideo;
}

function readForm() {
  return {
    port: Math.min(65535, Math.max(1024, parseInt($("port").value, 10) || 41596)),
    token: $("token").value.trim(),
    folderBySite: $("folderBySite").checked,
    folder: $("folder").value.trim(),
    minSize: Math.max(0, parseInt($("minSize").value, 10) || 0),
    hoverButton: $("hoverButton").checked,
    includeVideo: $("includeVideo").checked,
  };
}

/** 直接问后端，不经过 background 的缓存 —— 测试连接就该反映「现在」的状态。 */
async function probe() {
  const cfg = readForm();
  const headers = {};
  if (cfg.token) headers["x-pixvault-token"] = cfg.token;
  const res = await fetch(`http://127.0.0.1:${cfg.port}/api/ping`, { headers });
  const text = await res.text();
  let json = null;
  try {
    json = JSON.parse(text);
  } catch {
    /* 端口上跑的可能不是 PixVault */
  }
  if (!res.ok) throw new Error((json && json.error) || `HTTP ${res.status}`);
  if (!json || json.app !== "pixvault") throw new Error("这个端口上不是 PixVault");
  return json;
}

$("test").addEventListener("click", async () => {
  testState.className = "state";
  testState.textContent = "测试中…";
  try {
    const r = await probe();
    if (r.library) {
      testState.className = "state ok";
      testState.textContent = `连接正常 · 当前库「${r.library.name}」`;
    } else {
      testState.className = "state bad";
      testState.textContent = "已连上，但 PixVault 里还没打开图库";
    }
  } catch (e) {
    testState.className = "state bad";
    const msg = String((e && e.message) || e);
    testState.textContent = /token|401/i.test(msg) ? "令牌不匹配" : `连接失败：${msg}`;
  }
});

$("save").addEventListener("click", async () => {
  const cfg = readForm();
  await chrome.storage.local.set(cfg);
  $("port").value = cfg.port;
  $("minSize").value = cfg.minSize;
  saveState.className = "state ok";
  saveState.textContent = "已保存";
  setTimeout(() => {
    saveState.textContent = "";
  }, 2200);
});

void load();
