# Chaos BTPAN / Web 来源与许可

## 目标与移植边界

目标为 Xiaomi Band 10 Pro（p67），Vela **3.101.043**。固件地址、
IPC 与 Classic L2CAP 记录由该版本 AP 验证，不沿用 Band 11 / q66 的地址。
核验脚本为 `tools/probe_btpan_abi.py`、`tools/probe_network_abi.py`。

网络层和网页控制台移植参考用户提供的本地工程
`~/Other2/Vela_NativeApp-syspatch-band-11`，主要对应其中的
`ELF/src/{bt_probe,pan_radio,pan_service,network_service,network_sdp,nap_sdp,pan_ethernet,pan_ip,pan_libc,pan_tinypan_tap,syspatch_httpd}.c`、
相关头文件、`ELF/web/index.html` 与终端通信流程。
Chaos 端添加了 Rust UI/NSH 桥接、独立任务、认证与边界校验、固件适配和回归测试；
参考工程的原始说明见其根目录 `THIRD_PARTY.md`。
该本地快照不是 Git 仓库，未发现覆盖其自有代码的总许可证；
**不能把这些参考工程的自有文件重新标注为 MIT/BSD**。
用户提供的源码用于此次移植；对外再分发还应由代码权利人确认这些文件的授权。
以下第三方许可仅覆盖列明的第三方组件，不是对整个参考工程的重新授权。

## TinyPAN（MIT）

- 上游：https://github.com/Akhil-Chaturvedi/TinyPAN
- 提交：`cfd845fd740d028ca1b6193196325bb672c26915`
- 本地：`third_party/tinypan/`，仅 BNEP C core 和声明头文件。
- 来源和 MIT 全文分别保存在 `SOURCE.md`、`LICENSE`，沿用参考工程的归属声明。
- 本地修改：去掉未使用的 stdio 引用，端口头关闭日志与 TinyPAN 自带 lwIP 集成。
  不包含 TinyPAN 的 supervisor、SLIP 或固件地址。

## lwIP 2.2.1（BSD-3-Clause）

- 上游：https://github.com/lwip-tcpip/lwip
- 标签：`STABLE-2_2_1_RELEASE`
- 提交：`77dcd25a72509eb83f72b033d219b1d40cd8eb95`
- 本地：`third_party/lwip/src/`，上游源码/头文件保留，端口与配置独立放在 `ports/lwip/`。
- 许可全文保存在 `third_party/lwip/COPYING`，每个文件的原始版权声明同样保留。
- `tools/build_network.py` 使用显式文件列表，只链接所需 IPv4、DHCP、DNS、TCP、UDP、ARP 等核心。

## 分发

`tools/build.py` 自动产生 `dist/THIRD-PARTY-NETWORK.txt` 并放入安装器容器。
其中包含本说明、来源、MIT/BSD 全文，以及实际选用 lwIP C 文件和头文件中的版权/许可块。
设备端“关于 → 开放源代码许可”保留 Corona / MPL，并增加 TinyPAN / lwIP 全文入口。
这些声明不替代 Chaos 本身已有的 AGPL 及对应源代码分发义务。

本模块的 PAN/lwIP 是模块私有网络栈，不创建系统全局网络设备，
不声称为其它系统应用提供联网；网页服务是受密码保护的明文 HTTP/WebSocket，
只应在可信、可达的蓝牙共享网络使用，不应映射到公网。
