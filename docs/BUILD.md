# 构建与发版

## 环境要求

| 依赖 | 用途 | 备注 |
|---|---|---|
| Rust **nightly** + `rust-src` | 编译 `no_std` 模块（`-Z build-std=core,compiler_builtins`） | 版本不钉死，产物因此不可复现 |
| target `thumbv8m.main-none-eabi` | 交叉编译目标 | `rustup target add` |
| Python 3 | `build.py` / `p67_pack.py` / 两个布局脚本 | 图标与 GIF 帧生成使用 Pillow |
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
stage_installer         main.lua 瘦身 (minify_lua, token 序列逐字比对)
                        容器内 ko 副本 strip --strip-unneeded (llvm-objcopy
                        优先, mingw GNU objcopy 不认 ARM ELF; 失败回退未 strip)
        ↓
p67_pack.py             ko + 图标 + main.lua → p67 容器
                        （无 python3 时用 tools/p67_pack.js：
                         inspect/build/repack，repack 可直接换容器里的单个资源）
```

`tools/build_ko.sh` 负责前两步，`tools/build.py` 串起全流程。

> 旧版工作区还有一道 `verify_bt_addresses.py` 蓝牙地址审批门（构建前对照
> `docs/fw_address_table_043.csv` 核每个可调用地址），那份 CSV 不在本仓库，
> 这道门未随迁；地址安全由 `src/fw_api/fwaddr.rs` 的编译期断言承担。

完整构建在编译模块前生成动态 GIF 的帧资源，统一为 **96×96**；安装包的
`chaos_icon.bin` 和 `chaos_icon_settings.bin` 同为 96×96，默认图案来自原版 `tools/chaos.png`。
保留原版与动态，并加入暗色土豆、暗色图标；两张新增静态图按相同流程生成后嵌入模块。暂停和切回原版仍复用已安装文件。
源文件、单独生成/校验方法与播放实现见 [应用图标](DYNAMIC-ICON.md)。

## 版本号规则

源码版本、容器头、锁文件和中英文徽章必须同步：

| 位置 | 形式 | 说明 |
|---|---|---|
| `tools/p67_pack.py` / `tools/p67_pack.js` 的 `VERSION` | `0x000X0Y0Z` | 两个打包入口的容器头版本一致，`0x00010201` = **v1.2.1**（高 16 位主版本、次 8 位次版本、低 8 位修订号） |
| `supervisor/Cargo.toml` 的 `version` | `X.Y.Z` | 关于页通过 `env!("CARGO_PKG_VERSION")` 自动读取，无需手改 |
| `supervisor/Cargo.lock` 的 `chaos_sup` 版本 | `X.Y.Z` | 与模块清单保持一致 |
| `README.md` / `README.en.md` 的版本徽章 | `vX.Y.Z` | 中英文项目入口保持一致 |

改完补 `docs/CHANGELOG.md`，再跑一次构建。

> 版本号是给人看的标识，不参与兼容性判断：固件地址与协议没有版本协商，装错固件不会因为版本号不同而被拦住。

## 发布检查单

1. `git pull`，确认在最新 `master` / 特性分支上。
2. 同步上述版本标识并更新 CHANGELOG。
3. `python3 tools/build.py`（或 `--skip-ko`，如果只改了安装器）。
4. 确认 `dist/chaos_sup.ko` 与 `dist/assets/chaos_sup.bin` 哈希一致、`dist/stage` 已更新。
5. 真机验证：安装 → 重启 → 打开应用 → 看版本号与日志。
6. 提交源码与 `dist/` 产物（同一提交），打 tag `vX.Y.Z`，写 Release 说明（附 CHANGELOG 段落）。

## CI

[`.github/workflows/build.yml`](../.github/workflows/build.yml) 在 push / PR / 手动触发时：

1. 安装 nightly 工具链（`dtolnay/rust-toolchain@nightly`，未钉版本）；
2. 执行 `python3 tools/test_honeycomb.py`，运行蜂窝布局/控制器主机回归；
3. 执行两项图标单选、默认动态/配置升级、193 帧 GIF 及 96×96 原版图标回归；
   图标投递回归执行 `tools/test_icon_pack.py`，需要 `tools/requirements-test.txt` 中的 Lupa，验证真实 Lua 脚本与流式播放器；
4. 执行截图/录屏回归与 `CC=cc python3 tools/test_corona_blur.py` 背景协作回归；
5. 执行 `sh tools/build_ko.sh` 编译模块，并上传 `supervisor/chaos_sup.ko` 作为 artifact；
6. 执行 `python3 tools/build.py --skip-ko`（`apt-get install llvm` 补 objcopy，
   `pip install pillow` 供图标生成）产出完整安装包；
7. 上传 `dist/chaos-installer-p67.bin` 作为 artifact。

CI 运行上述主机测试，不打容器、不校验 `dist/` 可复现性——发布前请在本地跑一次完整构建。固件 ABI 静态核验和机器指令探针需要自备 3.101.043 AP，见 [HONEYCOMB.md](HONEYCOMB.md) 和 [DYNAMIC-ICON.md](DYNAMIC-ICON.md)。

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

## 2026-11-14 完整构建与无损体积优化

- `build.py` 输出完整安装包，`build_ko.sh` 输出主模块，Cargo的`.a`是中间静态库，三者范围不同，不能直接比较体积。
- 旧`build_ko.ps1`仅链接Rust库，缺少现有Corona、network、bridge等C provider及`ko_slim`，已改为调用同一`build_ko.sh`。Windows需要Git Bash的sh和同样的Python3/clang/Node/Rust工具链；本次未在Windows执行。
- 旧shell入口使用`lib*.a | head`，可能误选残留静态库；现固定取本模块`libchaos_sup.a`。
- 完整构建仍包括fetch、capatch、CA证书、原图标、字体槽、网络许可证与安装Lua，不通过删除资源、关闭功能或抽帧缩包。`--skip-ko`会明确提示旧模块不会包含新源码或新内置图片。
- `pack_embedded_assets.py`在统一模块入口执行：三个S541 BGRA素材从331,776字节无损压缩至91,920字节。记录为u16重复像素数+4字节原BGRA，运行时首次使用还原；颜色、alpha、尺寸和预乘格式逐字节不变。原始BGRA仍留在仓库供复验。
- `build.py`新增两张暗色图的同流程生成、容器全部资源逐字节校验，以及`dist/build-report.json`文件大小/哈希/ELF节清单。原`.ko`保留全部重定位和导出符号，不对可加载模块盲目strip。

本机修改前后（同一完整入口，已包含两张新增图）：安装包2,190,661 → 2,025,517字节，减少165,144字节，约7.54%。除主模块外的八项资源与修改前逐字节一致，完整193帧GIF保持原样。主模块约1.62 MB → 1.45 MB。

这是安装文件体积优化，不等于降低运行内存：无损素材在首次使用时解压到常驻BSS，新静态图还需像素缓冲。本次静态加载内存较修改前约增加240 KB（含新功能），不能把压缩节省量算作可用RAM增加。实际GPU、低内存和Windows构建仍需对应环境验证。
