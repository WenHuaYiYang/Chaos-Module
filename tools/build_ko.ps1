# 构建 supervisor 内核模块 -> supervisor\chaos_sup.ko
#
# 用法: powershell -ExecutionPolicy Bypass -File tools\build_ko.ps1 [模块目录] [输出 .ko]
# 参数含义与各 flag 的原因见 tools/build_ko.sh(两个脚本干的是同一件事)。
$ErrorActionPreference = "Stop"

$here = $PSScriptRoot
$root = Split-Path -Parent $here
$src = if ($args.Count -ge 1) { $args[0] } else { Join-Path $root "supervisor" }
$out = if ($args.Count -ge 2) { $args[1] } else { Join-Path $src "chaos_sup.ko" }

Push-Location $src
try {
    $env:RUSTFLAGS = "-C target-cpu=cortex-m33 -C target-feature=-fpregs"
    cargo +nightly build --release --target thumbv8m.main-none-eabi -Z build-std=core,compiler_builtins
    if ($LASTEXITCODE -ne 0) { throw "cargo build 失败" }

    $lib = Get-ChildItem "target\thumbv8m.main-none-eabi\release\lib*.a" | Select-Object -First 1
    $hostTriple = ((& rustc +nightly -vV) | Select-String '^host: ').ToString().Split(' ')[1]
    $lldDir = Join-Path (& rustc +nightly --print sysroot) "lib\rustlib\$hostTriple\bin"
    $lld = @("rust-lld.exe", "rust-lld") | ForEach-Object { Join-Path $lldDir $_ } |
        Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $lld) { throw "找不到 rust-lld(缺 nightly 工具链?)" }
    Write-Host "rust-lld: $lld"

    & $lld -flavor gnu -r --gc-sections -u module_main -u chaos_ctor `
        -T (Join-Path $here "merge_sections.ld") -o $out $lib.FullName
    if ($LASTEXITCODE -ne 0) { throw "rust-lld 失败" }
} finally {
    Pop-Location
}

python -X utf8 (Join-Path $here "fix_ko_layout.py") $out
if ($LASTEXITCODE -ne 0) { throw "fix_ko_layout 失败" }
python -X utf8 (Join-Path $here "verify_chaos_ko.py") $out
if ($LASTEXITCODE -ne 0) { throw "verify_chaos_ko 失败" }
Write-Host "OK: $out"
