"""监控 pixvault CI 三平台构建，完成后汇报 job 结论。"""
import json
import subprocess
import sys
import time
import urllib.error
import urllib.request

REPO = "KTCKING/pixvault"
RUN_ID = "38520577697"


def get_token() -> str:
    out = subprocess.run(
        ["git", "credential", "fill"],
        input="protocol=https\nhost=github.com\n",
        capture_output=True,
        text=True,
        cwd="C:/Users/pc/WorkBuddy/图片管理/pixvault",
    ).stdout
    return [l.split("=", 1)[1] for l in out.splitlines() if l.startswith("password=")][0].strip()


TOKEN = get_token()
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def api(path: str):
    req = urllib.request.Request(
        f"https://api.github.com/repos/{REPO}{path}",
        headers={"Authorization": f"token {TOKEN}", "User-Agent": "pixvault-ci-watch"},
    )
    with OPENER.open(req, timeout=30) as r:
        return json.load(r)


def main():
    seen = {}
    deadline = time.time() + 75 * 60  # 最长等 75 分钟
    while time.time() < deadline:
        jobs = api(f"/actions/runs/{RUN_ID}/jobs")
        run = api(f"/actions/runs/{RUN_ID}")
        run_status = run["status"]
        all_done = True
        for j in jobs["jobs"]:
            key = (j["name"], j["status"], j["conclusion"])
            if seen.get(j["name"]) != key:
                seen[j["name"]] = key
                print(f"[{time.strftime('%H:%M:%S')}] {j['name']}: {j['status']}/{j['conclusion'] or '-'}", flush=True)
            if j["status"] != "completed":
                all_done = False
        if run_status == "completed":
            print(f"RUN_DONE: {run['conclusion']}", flush=True)
            for name, (_n, _s, concl) in seen.items():
                print(f"  - {name}: {concl}", flush=True)
            return 0
        if all_done and run_status == "completed":
            print(f"RUN_DONE: {run['conclusion']}", flush=True)
            return 0
        time.sleep(45)
    print("TIMEOUT", flush=True)
    return 1


if __name__ == "__main__":
    sys.exit(main())
