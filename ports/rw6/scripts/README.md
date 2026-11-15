# RW6 工具链说明

本目录是从研究工作区 `scripts/` 原样镜像过来的 RW6 专用脚本，共 12 个，**没有改动任何一行代码**。
脚本里的 `ROOT = dirname(dirname(abspath(__file__)))` 会得到 `ports/rw6/`，所以
`work/`、`fw/`、`docs/`、`dist/` 这些字面路径在本仓库内仍然成立。

## 出包与验收（日常只用这三个）

| 脚本 | 作用 | 产物 |
|---|---|---|
| `build_chaos_p65_installer.py` | 把 `work/p65_chaos/` 打成 USTAR + `install.sh` 旧路线 | `dist/chaos-p65-043-v1.tar` |
| `build_chaos_p65_watchface.py` | 以 RW6 原生表盘容器为壳，换 pkgName 与 `_lua/mood/mood.lua`，把 ko/图标追加到文件链尾部并做往返校验 | `dist/chaos-installer-rw6.bin`、`dist/chaos-watchface-rw6.tar` |
| `p65_pages_accept.py` | 通过 `chaos_guest.py` 驱动 14 页导航回归，判据是模块自己的 CUR | 无（连设备跑） |

```bash
cd ports/rw6
python scripts/build_chaos_p65_installer.py
python scripts/build_chaos_p65_watchface.py    # 需要自备 fw/rw6_watchface_mood.bin
python scripts/p65_pages_accept.py             # 需要设备/模拟器在线
```

`dist/` 与 `*.ko` / `*.bin` 被本仓库 `.gitignore` 屏蔽，产物不入库，属正常。

## 锚点与地址取证（一次性研究工具）

`p65_anchor_bind.py`、`p65_anchor_coverage.py`、`p65_anchor_map.py`、
`p65_anchor_resolve.py`、`p65_nameref.py`、`p65_port_coverage.py`、
`p65_prop_scan.py`、`p65_thunk_resolve.py`、`p65_xref_raw.py`

这些脚本读的是**固件镜像**和 10P 侧参考源码，例如
`fw/p65_100_043/vela_ap.bin`、`fw_pkg_043/vela_ap.bin`、`canopus_src/targets/...`、
`Chaos-Module/supervisor/src/`。这些资产不在本仓库（见 `../fw/README.md`），因此
本目录保留它们是为了**可追溯**：结论已经固化在 `docs/*p65*.csv` 与
`docs/fw_address_table_p65.csv` 里，日常出包不需要重跑。若要重跑，请把
`ports/rw6/` 整个目录放回研究工作区根目录（或补齐上述资产后在本目录运行）。

## 命名口径

`p65` 是 RW6 这一档内部代号，`rw6` / `watch6` 指同一台设备（Redmi Watch 6 /
best1502p）。两套前缀都保留原样，避免改动脚本里已成事实的路径与产物名。
