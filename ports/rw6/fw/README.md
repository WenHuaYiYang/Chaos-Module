# 固件资产说明

本目录只有两个文件：`fw/p65_100_043/ota.sh` 与 `fw_pkg_rw6/ota.sh`。
它们是**从固件 OTA 包里取出的原始升级脚本**，用于确认分区布局与解包口径，
是取证素材，不是本仓库的安装脚本，也不要在手环上执行。

## 不入库的资产

`fw/` 与 `fw_pkg_rw6/` 下的镜像、签名、表盘容器样本都不在本仓库：

| 路径 | 状态 | 用途 |
|---|---|---|
| `fw/rw6_watchface_mood.bin` | 未入库（`*.bin` 被 `.gitignore` 屏蔽） | RW6 原生表盘容器壳样本，72742 B；`scripts/build_chaos_p65_watchface.py` 以它为前缀逐字节保留 |
| `fw_pkg_rw6/vela_ap.bin` | 未入库 | RW6 完整 AP 镜像，约 11 MB，锚点/名字池取证用 |
| `fw/p65_100_043/vela_ap.bin` 等 5 个镜像 | 未入库 | p65 固件解包镜像，`p65_nameref.py` / `p65_anchor_map.py` 的输入 |
| `fw/p65_100_043/META-INF/CERT.RSA` 等 3 个签名文件 | 未入库 | OTA 签名，属第三方材料，对适配无作用 |

理由有三条：本仓库 `.gitignore` 屏蔽 `*.bin`；镜像体积大且可重新解包取得；
固件与签名的权利归设备厂商，不适合随公开仓库分发。

## 自备方式

从对应版本的官方 OTA 包解出 `vela_ap.bin`、`vela_resource.bin` 等镜像，
按 `ota.sh` 里记录的分区名核对后放回上表路径；
`fw/rw6_watchface_mood.bin` 是 `vela_resource.bin` 偏移 `0x1FF3600` 处的内置「心情」
表盘容器，格式反解记录见 `../docs/Chaos_RW6_1to1复刻计划_20261031.md` §11.13。

补齐镜像后，`ports/rw6/` 内的锚点取证脚本即可按原路径运行；
只做出包与 14 页导航回归则**不需要**这些镜像（只需 `fw/rw6_watchface_mood.bin` 与设备在线）。
