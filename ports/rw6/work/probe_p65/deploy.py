#!/usr/bin/env python3
"""把一个模块文件传到 redmi-watch6 guest 的 /data，再用 NSH 装进去跑。

用法: python work/probe_p65/deploy.py work/probe_p65/probe_p65.ko [模块名]

为什么要单独一份：Studio 的文件接口是"开票 + 按块 POST + 提交清单 + 轮询任务"四步，
GUI 里看不出协议，这里按 web-ui/files.js 的调用序列复刻。
"""
import json
import os
import sys
import time
import urllib.request

BASE = "http://127.0.0.1:51215"
INST = "i-2cf29b260da6bd98752a28ca9deaf9db"


def cfg():
    url = "%s/instances/%s/config.json" % (BASE, INST)
    return json.load(urllib.request.urlopen(url, timeout=15))


GEN = ""


def hdr(token, ctype="application/json"):
    # 写操作要带 X-MiBand-Generation：缺了会被判成字段无效（409 fs-invalid-request）。
    return {"X-MiBand-Control": token, "X-MiBand-Generation": GEN,
            "Origin": BASE, "Content-Type": ctype}


def files(token, action, **args):
    # 字段是**平铺**的 {action, ...args}；start 那份才把任务塞进 request 里。
    body = json.dumps({"action": action, **args}).encode()
    req = urllib.request.Request("%s/instances/%s/api/files" % (BASE, INST),
                                 data=body, headers=hdr(token), method="POST")
    with urllib.request.urlopen(req, timeout=30) as f:
        return json.load(f)


def nsh(token, cmd, wait=1.2):
    body = json.dumps({"input": cmd + "\n"}).encode()
    req = urllib.request.Request("%s/instances/%s/api/nuttx" % (BASE, INST),
                                 data=body, headers=hdr(token), method="POST")
    urllib.request.urlopen(req, timeout=20).read()
    time.sleep(wait)
    got = urllib.request.urlopen(urllib.request.Request(
        "%s/instances/%s/api/nuttx" % (BASE, INST), headers=hdr(token)), timeout=20)
    return json.loads(got.read().decode("utf-8", "replace")).get("nuttx", {}).get("output", "")


def upload(token, local, name, dest="/data"):
    size = os.path.getsize(local)
    rec = files(token, "upload-begin", size=size)
    rec = rec.get("request") or rec.get("result") or rec
    ticket, chunk = rec["uploadTicket"], rec["chunkBytes"]
    data = open(local, "rb").read()
    for off in range(0, size, chunk):
        req = urllib.request.Request(
            "%s/instances/%s/api/files/upload?ticket=%s&offset=%d" % (BASE, INST, ticket, off),
            data=data[off:off + chunk], headers=hdr(token, "application/octet-stream"), method="POST")
        with urllib.request.urlopen(req, timeout=60) as f:
            assert json.load(f).get("ok", True), "分块上传被拒"
    job = files(token, "start", request={"operation": "upload", "path": dest,
                                         "files": [{"ticket": ticket, "relativePath": name}],
                                         "directories": []})
    job = job.get("result") or job
    jid = job.get("id") or job.get("jobId")
    for _ in range(40):
        st = files(token, "status", id=jid)
        st = st.get("result") or st
        if st.get("state") in ("complete", "failed", "error"):
            return st
        time.sleep(0.5)
    return {"state": "timeout"}


def main():
    local = sys.argv[1]
    name = os.path.basename(local)
    mod = sys.argv[2] if len(sys.argv) > 2 else os.path.splitext(name)[0]
    c = cfg()
    token = c["controlToken"]
    global GEN
    GEN = c.get("generation", "")
    print("upload ->", upload(token, local, name))
    print(nsh(token, "ls -l /data/%s" % name))
    print(nsh(token, "insmod /data/%s %s" % (name, mod), wait=2.0))
    print(nsh(token, "lsmod"))
    print(nsh(token, "xd 0x3C4965B8 32"))
    print(nsh(token, "dmesg", wait=1.5))


if __name__ == "__main__":
    main()
