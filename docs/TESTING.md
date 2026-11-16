# 测试指南

以下命令从仓库根目录运行。仅改文档时检查链接与表述即可；修改运行时代码时选择相关回归，交付安装包前再执行完整构建。

## 主机环境

需要 Python 3、Rust、可用的 C 编译器。涉及模糊的测试使用 ASan/UBSan，编译器及对应运行库必须支持。可通过 `CC` 指定编译器。Lua、图像等测试依赖见 [requirements-test.txt](../tools/requirements-test.txt)；构建依赖另见 [构建指南](BUILD.md)。

```sh
python3 -m pip install -r tools/requirements-test.txt
```

Windows 可使用项目虚拟环境的 `.venv/Scripts/python.exe` 代替 `python3`；先将 Rust、Git Bash、clang/LLVM 与 Node 加入 PATH。PowerShell 设置 C 编译器示例：`$env:CC = "clang"`。

## 按改动选择回归

| 修改范围 | 命令 |
|---|---|
| 时间节拍、像素计算、文本缓存 | `python3 tools/test_performance.py` |
| 应用详情目录扫描 | `python3 tools/test_detail_scan.py` |
| 首页 CPU、内存、存储及恢复 | `python3 tools/test_home_stats.py` |
| 桌面/控制中心交互门控 | `python3 tools/test_blur_interaction.py` |
| Blur V2 与 S541 背景采样 | `python3 tools/test_blur_v2.py` |
| 控制中心按钮 | `python3 tools/test_cc_buttons.py` |
| 模糊参数 | `python3 tools/test_blur_params.py` |
| Corona 所有权、恢复与生命周期 | `python3 tools/test_corona_blur.py` |
| 打包与构建缓存 | `python3 tools/test_build.py` |
| 安装器反馈、分批复制、日志与 fetch 开关 | `python3 tools/test_installer.py`（需要 Lupa） |
| 固件目标与符号绑定 | `python3 tools/test_firmware_symbols.py` |
| 蜂窝布局 | `python3 tools/test_honeycomb.py` |
| 应用图标与图标包 | `python3 tools/test_dynamic_icon.py`、`tools/test_gif_icon.py`、`tools/test_static_icon.py`、`tools/test_icon_pack.py`（各自用 Python 执行） |
| 截屏与录屏 | `python3 tools/test_capture.py` |
| ChaosConnect / 本地文件管理 | `python3 tools/test_chaos_connect.py` / `python3 tools/test_local_files.py` |

S541 控制条另有独立 Rust 测试。先创建被忽略的 `target` 目录，再执行（Windows 输出名加 `.exe`）：

```sh
rustc --edition=2021 --test supervisor/tests/s541_harness.rs -o target/s541-tests
./target/s541-tests --test-threads=1
```

测试覆盖移动时不读取 framebuffer、不失效整屏附加层，以及无缓存、晚到缓存和缓冲复用。它不模拟完整固件渲染或证明设备帧率。

## 构建与真机验收

```sh
python3 tools/build.py --firmware 10p-3.101.043
```

后续已有同固件资源时可用 `--no-icons --skip-fetch` 缩短构建；修改主模块后不要使用 `--skip-ko`。核对目标目录的 `build-report.json`、安装包和对应源码提交。

真机检查安装后重启、版本显示、应用管理打开普通/快应用、首页统计恢复、桌面与控制中心单独/同时模糊、S541、反复开关及息屏唤醒。性能验收方法见 [性能说明](PERFORMANCE.md)。

CI 当前执行的测试以 [build.yml](../.github/workflows/build.yml) 为准，上表不表示全部已加入 CI。需要固件原件的 ARM 探针与 ABI 验证另见对应协议文档；主机测试通过不能替代固件或真机验证。
