# 构建 supervisor 内核模块 -> supervisor\chaos_sup.ko
#
# 用法: powershell -ExecutionPolicy Bypass -File tools\build_ko.ps1 [模块目录] [输出 .ko]
# 参数含义与各 flag 的原因见 tools/build_ko.sh(两个脚本干的是同一件事)。
$ErrorActionPreference = "Stop"

$here = $PSScriptRoot
$root = Split-Path -Parent $here
$src = if ($args.Count -ge 1) { $args[0] } else { Join-Path $root "supervisor" }
$out = if ($args.Count -ge 2) { $args[1] } else { Join-Path $src "chaos_sup.ko" }

# 与build.py共用完整入口，旧脚本遗漏C模块且未做ko_slim，不能比较体积。
# Windows需Git Bash的sh，以及PATH中的Python3、clang、Node和Rust。
$sh = Get-Command sh -ErrorAction SilentlyContinue
if (-not $sh) { throw "需要Git Bash的sh；完整构建不能省略C模块，请安装并加入PATH。" }
$script = (Join-Path $here "build_ko.sh").Replace('\', '/')
$src = [IO.Path]::GetFullPath($src).Replace('\', '/')
$out = [IO.Path]::GetFullPath($out).Replace('\', '/')
& $sh.Source $script $src $out
if ($LASTEXITCODE -ne 0) { throw "完整模块构建失败，退出码 $LASTEXITCODE" }
