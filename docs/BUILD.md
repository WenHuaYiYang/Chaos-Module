# 构建与发版

## 环境要求

| 依赖 | 用途 | 备注 |
|---|---|---|
| Rust **nightly** + `rust-src` | 编译 `no_std` 模块（`-Z build-std=core,compiler_builtins`） | 版本不钉死，产物因此不可复现 |
| target `thumbv8m.main-none-eabi` | 交叉编译目标 | `rustup target add` |
| Python 3 | `build.py` / `p67_pack.py` / 两个布局脚本 | 纯标准库，图标生成另需 Pillow |
| 系统 C 链接器 `cc` | Rust build script 的宿主链接 | 见[无 cc 的环境](#无-cc-的环境) |
| `arm-none-eabi-*`（可选） | 只用于反汇编核对 | 构建本身用 `rust-lld` |

## 一条命令

首次构建先安装图标依赖（建议使用 Python 3.12 的项目虚拟环境；已有 `.venv` 无需重建）：

```bash
python3.12 -m venv .venv
.venv/bin/python -m pip install -r tools/requirements-build.txt
```

之后直接执行，不需要激活虚拟环境：

```bash
python3 tools/build.py
```

脚本会在编译前检查 Pillow：优先使用启动脚本的 Python，缺依赖时自动尝试项目
`.venv/bin/python`（Windows 为 `.venv/Scripts/python.exe`）。若两者均不可用，
立即报告依赖安装方法，不会先编译模块。依赖只安装在所选环境内，无需修改系统 Python。

输出：

```text
dist/chaos_sup.ko               内核模块
dist/chaos-installer-p67.bin    可直接安装的 p67 容器
dist/assets/                    容器内 ko 副本、图标、字体占位
dist/stage/                     打包临时目录（不入库）
```

常用组合：

```bash
python3 tools/build.py --skip-ko        # 复用 dist/chaos_sup.ko，只重打容器
python3 tools/build.py --no-container   # 只出 ko 和图标
python3 tools/build.py --no-icons       # 复用 dist/assets 的图标（无 Pillow 时用）
```

## 构建流程

```text
cargo +nightly build --release --target thumbv8m.main-none-eabi
  -Z build-std=core,compiler_builtins
  -C target-cpu=cortex-m33 -C target-feature=-fpregs     # 软浮点，加载器才接受
        ↓
rust-lld -r --gc-sections -u module_main -u chaos_ctor -T merge_sections.ld
        ↓  （不 merge 节区时 .text 会是 0，insmod 静默成功但模块没有任何指令）
fix_ko_layout.py        重排各节 sh_addr（链接产物全是 0）
        ↓
ko_slim.js              清空局部符号名 → 文件 251KB→195KB（CHAOS_NO_SLIM=1 可跳过）
        ↓
verify_chaos_ko.py      打印节区/重定位/未定义符号
        ↓
p67_pack.py             ko + 图标 + main.lua → p67 容器
                        （无 python3 时用 tools/p67_pack.js：
                         inspect/build/repack，repack 可直接换容器里的单个资源）
```

`tools/build_ko.sh` 负责前两步，`tools/build.py` 串起全流程。

## 版本号规则

一个版本号出现在三个地方，必须同时改：

| 位置 | 形式 | 说明 |
|---|---|---|
| `tools/p67_pack.py` 的 `VERSION` | `0x000X0Y0Z` | 容器头版本，`0x00010003` = **v1.0.3**（高 16 位主版本、次 8 位次版本、低 8 位修订号） |
| `supervisor/src/page.rs` 关于页 | 字符串 `vX.Y.Z` | 用户能看到的版本 |
| `supervisor/Cargo.toml` 的 `version` | `X.Y.Z` | 与上面保持一致 |

改完补 `docs/CHANGELOG.md`，再跑一次构建。

> 版本号是给人看的标识，不参与兼容性判断：固件地址与协议没有版本协商，装错固件不会因为版本号不同而被拦住。

## 发布检查单

1. `git pull`，确认在最新 `master` / 特性分支上。
2. 改三处版本号 + 写 CHANGELOG。
3. `python3 tools/build.py`（或 `--skip-ko`，如果只改了安装器）。
4. 确认 `dist/chaos_sup.ko` 与 `dist/assets/chaos_sup.bin` 哈希一致、`dist/stage` 已更新。
5. 真机验证：安装 → 重启 → 打开应用 → 看版本号与日志。
6. 提交源码与 `dist/` 产物（同一提交），打 tag `vX.Y.Z`，写 Release 说明（附 CHANGELOG 段落）。

## CI

[`.github/workflows/build.yml`](../.github/workflows/build.yml) 在 push / PR / 手动触发时：

1. 安装 nightly 工具链（`dtolnay/rust-toolchain@nightly`，未钉版本）；
2. 执行 `sh tools/build_ko.sh`（只编译模块，不打包容器）；
3. 上传 `supervisor/chaos_sup.ko` 作为 artifact。

CI 不跑测试、不打容器、不校验 `dist/` 可复现性——发布前请在本地跑一次完整构建。

## 无 cc 的环境

容器/沙盒里常常没有系统 `cc`，Rust 的 build script 会以 `linker 'cc' not found` 失败。
可以用 zig 自带的 C 编译器顶上：

```sh
mkdir -p /tmp/bin
cat > /tmp/bin/cc <<'EOF'
#!/bin/sh
export ZIG_GLOBAL_CACHE_DIR=/tmp/zigcache ZIG_LOCAL_CACHE_DIR=/tmp/zigcache
exec /path/to/zig cc "$@"
EOF
chmod +x /tmp/bin/cc
PATH=/tmp/bin:$PATH python3 tools/build.py --no-icons
```

`--no-icons` 用于没有 Pillow 的环境；图标是仓库里的既有产物，不必每次重生成。

## 排障

| 现象 | 原因 |
|---|---|
| `linker 'cc' not found` | 缺宿主 C 链接器，见上一节 |
| `ModuleNotFoundError: PIL` / 图标 Python 检查失败 | 按上文在项目 `.venv` 中安装 `tools/requirements-build.txt`，然后重跑原命令；仅需复用既有图标时可用 `--no-icons` |
| 生成的 ko 很小 / `insmod` 成功但没反应 | 少了 `-T merge_sections.ld`，`.text` 为 0 |
| 容器头校验失败 | `p67_pack.VERSION` 与解析结果不一致，检查 `inspect()` 输出 |
| 预览图尺寸报错 | `tools/p67-preview.block` 必须是 336×480 |

## 相关文档

- 代码结构：[ARCHITECTURE.md](ARCHITECTURE.md)
- 安装流程：[INSTALL.md](INSTALL.md)
