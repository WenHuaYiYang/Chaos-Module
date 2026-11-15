#!/usr/bin/env python3
"""MiBand Studio 的 redmi-watch6 guest 操作台（p65 锚点执行验证用）。

用法（先保证 Studio 已从桌面正常打开、RW6 实例在运行）：
  python work/probe_p65/g.py status
  python work/probe_p65/g.py nsh "ls /data"
  python work/probe_p65/g.py tail 20
  python work/probe_p65/g.py put work/probe_p65/probe_p65.ko probe_e.ko
  python work/probe_p65/g.py insmod /data/probe_e.ko probe_e
  python work/probe_p65/g.py cont            # QMP 恢复（装卸载崩溃后 guest 被应用挂起）

实测口径：
  * 管理器地址每次启动变，从 launcher.json 现读；controlToken/generation 从每实例 config.json 取。
  * 文件接口字段**平铺** `{action, ...}`；写操作要带 X-MiBand-Generation。
  * 上传四步：upload-begin → 分块 POST → start(operation=upload) → 轮询 status；
    目标已存在会停在 conflict，要 `resolve{choice:'replace'}`。
  * NSH 命令回显经常取不到（GET 空），结果以 boot-uart.log 为准 —— `tail` 读日志。
  * guest 崩一次会自复位并被应用挂起（QMP 里状态是 prelaunch），`cont` 就能让它继续跑。
"""
import glob
import json
import os
import socket
import subprocess
import sys
import time
import urllib.request

LAUNCHER = r"C:\Users\YiYang\AppData\Local\MiBandStudio\state\launcher.json"
INST = "i-2cf29b260da6bd98752a28ca9deaf9db"
RUNS = r"C:\Users\YiYang\AppData\Local\MiBandStudio\state\runs\%s" % INST


def base():
    return json.load(open(LAUNCHER, encoding="utf-8"))["url"].rstrip("/")


class Guest:
    def __init__(self):
        c = json.load(urllib.request.urlopen("%s/instances/%s/config.json" % (base(), INST), timeout=15))
        self.tok, self.gen = c["controlToken"], c.get("generation", "")
        self.base = base()

    def h(self, ct="application/json"):
        return {"X-MiBand-Control": self.tok, "X-MiBand-Generation": self.gen,
                "Origin": self.base, "Content-Type": ct}

    def _url(self, ep):
        return "%s/instances/%s%s" % (self.base, INST, ep)

    def get(self, ep):
        return json.load(urllib.request.urlopen(urllib.request.Request(self._url(ep), headers=self.h()), timeout=20))

    def files(self, action, **a):
        body = json.dumps({"action": action, **a}).encode()
        r = urllib.request.Request(self._url("/api/files"), data=body, headers=self.h(), method="POST")
        return json.load(urllib.request.urlopen(r, timeout=30))

    def nsh(self, cmd):
        body = json.dumps({"input": cmd + "\n"}).encode()
        try:
            urllib.request.urlopen(urllib.request.Request(self._url("/api/nuttx"), data=body,
                                                          headers=self.h(), method="POST"), timeout=25).read()
            return True
        except Exception as e:
            print("NSH POST 失败: %s" % e)
            return False

    def clean_jobs(self):
        for j in self.get("/api/files").get("jobs", []):
            try:
                if j.get("state") == "conflict":
                    self.files("resolve", id=j["id"], choice="replace", apply=False)
                elif j.get("state") in ("queued", "running"):
                    self.files("cancel", id=j["id"])
            except Exception:
                pass

    def put(self, local, name):
        self.clean_jobs()
        size = os.path.getsize(local)
        rec = self.files("upload-begin", size=size)["result"]
        tick, chunk = rec["uploadTicket"], rec["chunkBytes"]
        data = open(local, "rb").read()
        for off in range(0, size, chunk):
            urllib.request.urlopen(urllib.request.Request(
                self._url("/api/files/upload?ticket=%s&offset=%d" % (tick, off)),
                data=data[off:off + chunk], headers=self.h("application/octet-stream"),
                method="POST"), timeout=60).read()
        job = self.files("start", request={"operation": "upload", "path": "/data",
                                           "files": [{"ticket": tick, "relativePath": name}],
                                           "directories": []})["result"]
        st = job
        for _ in range(40):
            st = self.files("status", id=job["id"])["result"]
            if st.get("state") == "conflict":
                self.files("resolve", id=job["id"], choice="replace", apply=False)
            elif st.get("state") in ("complete", "failed", "cancelled"):
                break
            time.sleep(0.5)
        return st.get("state"), st.get("completedBytes"), st.get("error")

    def log_path(self):
        d = sorted(glob.glob(os.path.join(RUNS, "*")), key=os.path.getmtime)[-1]
        return os.path.join(d, "boot-uart.log")


def manager(action, body=None):
    """管理器级接口：token 来自管理器自己的 /config.json（不是实例的）。"""
    b = base()
    tok = json.load(urllib.request.urlopen(b + "/config.json", timeout=15)).get("controlToken", "")
    h = {"X-MiBand-Control": tok, "Origin": b, "Content-Type": "application/json"}
    data = None if body is None else json.dumps(body).encode()
    r = urllib.request.Request(b + action, data=data, headers=h, method="POST" if body is not None else "GET")
    try:
        with urllib.request.urlopen(r, timeout=180) as f:
            return f.status
    except Exception as e:
        print("manager %s 失败: %s" % (action, e))
        return None


def restart_guest(wait=25):
    """停实例再启动：guest 被探针弄崩后唯一的自助恢复路径。
    实例级 /api/restart 与 QMP system_reset 都不够（前者 502、后者停在启动早期）。"""
    manager("/api/instances/stop", {"instance": INST})
    time.sleep(2)
    manager("/api/instances/start", {"instance": INST, "skipBinding": True})
    time.sleep(wait)


def qmp(cmd):
    try:
        out = subprocess.run(["powershell", "-NoProfile", "-Command",
                              "(Get-CimInstance Win32_Process -Filter \"Name='qemu-system-arm.exe'\").CommandLine"],
                             capture_output=True, timeout=30).stdout.decode('utf-8', 'replace')
    except Exception:
        out = ""
    port = None
    for line in out.splitlines():
        if "miband-best1502p" in line and "-qmp tcp:" in line:
            port = line.split("-qmp tcp:")[1].split(",")[0]
    if not port:
        print("找不到 RW6 的 QMP 端口")
        return
    host, p = port.rsplit(":", 1)
    s = socket.create_connection((host, int(p)), timeout=8)
    f = s.makefile("rwb")
    f.readline()
    f.write((json.dumps({"execute": "qmp_capabilities"}) + "\n").encode()); f.flush(); f.readline()
    f.write((json.dumps({"execute": cmd}) + "\n").encode()); f.flush()
    print(f.readline().decode("utf-8", "replace").strip())
    f.close(); s.close()


def main():
    argv = sys.argv[1:]
    if not argv:
        sys.exit(__doc__)
    g = Guest()
    op = argv[0]
    if op == "status":
        m = g.get("/api/machine")
        print({k: m.get(k) for k in ("powerState", "bootId", "nativeResourcesReady", "lifecycleError")})
        print("日志 %s" % g.log_path())
    elif op == "nsh":
        print("发送:", g.nsh(" ".join(argv[1:])))
    elif op == "tail":
        n = int(argv[1]) if len(argv) > 1 else 20
        t = open(g.log_path(), encoding="utf-8", errors="replace").read()
        print("\n".join(t.splitlines()[-n:]))
    elif op == "put":
        print(g.put(argv[1], argv[2]))
    elif op == "insmod":
        before = os.path.getsize(g.log_path())
        ok = g.nsh("insmod %s %s" % (argv[1], argv[2]))
        time.sleep(4)
        now = os.path.getsize(g.log_path())
        print("发送=%s 日志 +%d 字节" % (ok, now - before))
        print(open(g.log_path(), encoding="utf-8", errors="replace").read()[before:][-800:])
    elif op == "cont":
        qmp("cont")
    elif op == "restart":
        restart_guest()
        print("已请求 stop+start，等 NSH 就绪后 tail 看")
    elif op == "reset":
        qmp("system_reset"); time.sleep(1); qmp("cont")
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
