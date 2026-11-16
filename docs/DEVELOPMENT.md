# 开发工作流

本 fork 的主分支为 `master`，远端 `origin` 应指向 `https://github.com/qxdho/Chaos-Module.git`。原项目 `WenHuaYiYang/Chaos-Module` 可作为 `upstream` 跟踪。

## 本地目录与远端

当前工作区统一在 `Chaos-Module-master` 修改、测试和构建。`fork-publish` 是历史副本，不再用于开发或复制覆盖。工具与交付备份放在仓库外，构建输出留在被忽略的 `dist/`、`target/` 等目录。

在该仓库目录检查：

```sh
git status --short --branch
git remote -v
git branch --show-current
```

GitHub Desktop 通过 **File → Add local repository** 选择 `Chaos-Module-master`。通过 **Repository → Repository settings → Remote** 核对主远端地址属于 `qxdho`；仓库名称相同不代表远端相同。仅当 `origin` 配置错误时修改：

```sh
git remote set-url origin https://github.com/qxdho/Chaos-Module.git
```

## 修改、验证与提交

1. 先查看工作区差异，保留已有未提交改动。更新远端前先提交或暂存自己的工作，遇到冲突逐项处理。
2. 修改代码时同步相关文档，按 [测试指南](TESTING.md) 选择回归；需要交付二进制时按 [构建指南](BUILD.md) 构建。
3. 在 GitHub Desktop 的 Changes 中检查差异并提交。文档修改无需编译；修改模块源码后不要使用 `--skip-ko` 交付旧模块。
4. 用户自行点击 **Push origin**。推送前确认当前仓库和分支；不需要建立另一份发布副本，也不需要强制推送来同步普通修改。

提交源码、构建脚本、必要资源和文档。不要提交工具链、缓存、固件原件、密钥或生成的安装包；二进制放到 Release 或 CI artifact，并提供对应提交的完整源码。

## 跟进上游

查看上游和合并上游是两步操作。先 fetch 并比较差异，再决定合并范围；不直接用上游目录覆盖本 fork 的修复。涉及固件地址或协议的变更需要重新验证，版本号相同不表示二进制相同。
