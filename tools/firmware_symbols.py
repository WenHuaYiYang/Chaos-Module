#!/usr/bin/env python3
"""Extract exact Vela AP identities and generate reviewed firmware bindings.

Only reviewed records belonging to the exact AP hash become compiler inputs.
Byte matches are search results, never an ABI approval or a callable export.
The normal extraction path uses only the Python standard library.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import sys
import time
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_TARGET = "10p-3.101.043"
SCHEMA = 1
MAX_AP = 64 * 1024 * 1024
GENERATED = ("symbols.json", "firmware.rs", "firmware.h", "firmware.ld")
ID_PATTERN = re.compile(r"[a-z0-9][a-z0-9-]*-\d+\.\d+\.\d+\Z")


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def read_json(path: Path) -> dict:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"JSON root must be an object: {path}")
    return value


def write_json(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def target_id(value: str) -> str:
    if not ID_PATTERN.fullmatch(value):
        raise ValueError(f"目标必须为机型-系统版本，例如 {DEFAULT_TARGET}，收到 {value!r}")
    return value


def identifier(value: str) -> str:
    value = re.sub(r"[^A-Za-z0-9_]", "_", value).upper()
    if not value or value[0].isdigit():
        value = "S_" + value
    return value


def address(value: str | int) -> int:
    result = int(value, 0) if isinstance(value, str) else value
    if not isinstance(result, int) or isinstance(result, bool) or not 0 < result <= 0xFFFFFFFF:
        raise ValueError(f"invalid ARM32 address: {value!r}")
    return result


def read_ap(path: Path) -> tuple[bytes, dict]:
    """Read only the AP member; never execute ota.sh or extract archive paths."""
    info = {"input_name": path.name, "input_sha256": digest(path)}
    if zipfile.is_zipfile(path):
        with zipfile.ZipFile(path) as archive:
            entries = [e for e in archive.infolist() if e.filename == "vela_ap.bin"]
            if len(entries) != 1:
                raise ValueError("固件 ZIP 必须恰好包含一个 vela_ap.bin 根条目")
            entry = entries[0]
            if entry.flag_bits & 1 or not 16 <= entry.file_size <= MAX_AP:
                raise ValueError("AP 条目加密或长度异常")
            ap = archive.read(entry)  # zipfile also verifies its CRC.
            info.update(format="vela-ota-zip", member="vela_ap.bin", zip_crc32=f"{entry.CRC:08x}")
    else:
        if not 16 <= path.stat().st_size <= MAX_AP:
            raise ValueError("原始 AP 长度异常；完整 OTA 应使用 ZIP 格式")
        ap = path.read_bytes()
        info.update(format="raw-ap")
    if ap[:4] != b"\xff" * 4 or ap[16:20] != bytes.fromhex("14480047"):
        raise ValueError("未识别的 AP 封装/启动头，不能沿用 p67 的地址映射")
    info.update(ap_sha256=sha(ap), ap_size=len(ap))
    return ap, info


def validate_records(seed: dict, ap_hash: str) -> list[dict]:
    if seed.get("schema") != SCHEMA or seed.get("ap_sha256") != ap_hash:
        raise ValueError("核验资料不属于当前精确 AP；禁止沿用其他固件 symbol")
    records = seed.get("symbols")
    if not isinstance(records, list):
        raise ValueError("reviewed.json 缺少 symbols 数组")
    names = set()
    result = []
    for original in records:
        record = dict(original)
        name = identifier(record["name"])
        if name in names:
            raise ValueError(f"symbol 名称冲突：{name}")
        names.add(name)
        if record.get("review") not in ("exact-target-reviewed", "existing-chaos-baseline"):
            raise ValueError(f"未核验记录不能导出：{name}")
        if not record.get("evidence") or not record.get("prototype"):
            raise ValueError(f"symbol 缺少 ABI/证据资料：{name}")
        if record['review'] == 'exact-target-reviewed':
            proof = record['evidence']
            if (proof.get('approval_state') != 'APPROVED'
                    or proof.get('policy') in ('forbidden', 'withdrawn')
                    or record.get('verification_level') not in
                       ('STATIC_RECOVERED', 'STATIC_CONFIRMED', 'DEVICE_PROBED', 'DEVICE_PROVEN')):
                raise ValueError(f"记录的核验标记与来源审批/证据等级冲突：{name}")
        elif record.get('verification_level') != 'EXISTING_PRODUCTION_BASELINE':
            raise ValueError(f"缺少现有 Chaos 基线身份：{name}")
        value = address(record["address"])
        if record["kind"] == "function":
            if record.get("instruction_set") != "thumb" or not value & 1:
                raise ValueError(f"函数 callable 地址必须携带 Thumb bit：{name}")
        elif record["kind"] not in ("global", "data", "string"):
            raise ValueError(f"不支持的导出类型：{record['kind']}")
        record.update(identifier=name, address=f"0x{value:08X}")
        result.append(record)
    return sorted(result, key=lambda r: r["identifier"])


def generated_files(target: dict, records: list[dict]) -> dict[str, bytes]:
    ident = target_id(target["id"])
    if f"{target.get('model')}-{target.get('version')}" != ident:
        raise ValueError("目标机型/版本与目录标识不一致")
    if not re.fullmatch(r"[0-9a-f]{64}", target.get('ap_sha256', '')):
        raise ValueError("缺少精确固件 SHA-256")
    values = {r["name"]: address(r["address"]) for r in records}
    aliases = target.get("bindings", {})
    for name, symbol in aliases.items():
        if not re.fullmatch(r"FW_[A-Z0-9_]+", name) or symbol not in values:
            raise ValueError(f"编译绑定没有对应的核验 symbol：{name} -> {symbol}")
    # Both languages receive the same address values, not independent tables.
    rust = ["// Generated by tools/firmware_symbols.py; do not edit.",
            f'pub const TARGET_ID: &str = "{ident}";',
            f'pub const FIRMWARE_VERSION: &str = "{target["version"]}";',
            f'pub const AP_SHA256: &str = "{target["ap_sha256"]}";',
            "#[no_mangle]", f'pub static chaos_firmware_target: [u8; {len(ident) + 1}] = *b"{ident}\\0";']
    header = ["/* Generated by tools/firmware_symbols.py; do not edit. */",
              "#ifndef CHAOS_FIRMWARE_SYMBOLS_H", "#define CHAOS_FIRMWARE_SYMBOLS_H", "#include <stdint.h>",
              f'#define CHAOS_FIRMWARE_ID "{ident}"',
              f'#define CHAOS_FIRMWARE_VERSION "{target["version"]}"',
              f'#define CHAOS_FIRMWARE_AP_SHA256 "{target["ap_sha256"]}"']
    linker = ["/* Generated reviewed absolute symbols; choose ONE target per link. */"]
    for r in records:
        key, value = r["identifier"], address(r["address"])
        rust.append(f"pub const SYM_{key}: u32 = 0x{value:08X};")
        header.append(f"#define CHAOS_SYM_{key} UINT32_C(0x{value:08X})")
        linker.append(f"chaos_fw_{key.lower()} = 0x{value:08X};")
    for name, symbol in sorted(aliases.items()):
        rust.append(f"pub const {name}: u32 = SYM_{identifier(symbol)};")
        header.append(f"#define CHAOS_{name} CHAOS_SYM_{identifier(symbol)}")
    header.append("#endif")
    return {"symbols.json": (json.dumps({"schema": SCHEMA, "target": ident,
            "ap_sha256": target["ap_sha256"], "symbols": records}, ensure_ascii=False,
            sort_keys=True, indent=2) + "\n").encode(),
            "firmware.rs": ("\n".join(rust) + "\n").encode(),
            "firmware.h": ("\n".join(header) + "\n").encode(),
            "firmware.ld": ("\n".join(linker) + "\n").encode()}


def load_target(repo: Path, ident: str, *, require_build: bool = True) -> dict:
    ident = target_id(ident)
    folder = repo / "symbol" / ident
    if not (folder / 'target.json').is_file() or not (folder / 'manifest.json').is_file():
        raise ValueError(f'缺少精确固件资料：{folder}；请先运行 firmware_symbols.py extract')
    target = read_json(folder / "target.json")
    manifest = read_json(folder / "manifest.json")
    if target.get("id") != ident or target.get("schema") != SCHEMA:
        raise ValueError("symbol 目录和目标标识不一致")
    if manifest.get("ap_sha256") != target.get("ap_sha256"):
        raise ValueError("固件摘要与已提取 manifest 不一致，请重新提取")
    for name in ("target.json", "reviewed.json", *GENERATED):
        if digest(folder / name) != manifest.get("files", {}).get(name):
            raise ValueError(f"symbol 资料已改变或损坏，请重新提取：{ident}/{name}")
    seed = read_json(folder / "reviewed.json")
    records = validate_records(seed, target["ap_sha256"])
    for name, data in generated_files(target, records).items():
        if (folder / name).read_bytes() != data:
            raise ValueError(f"生成绑定与核验资料不一致：{name}")
    if require_build:
        # A hash gate is deliberate: a bag of function matches is not a full
        # replacement for p67's private object/layout/BT/font ABI.
        if (ident != DEFAULT_TARGET or target.get("chaos_abi") != "p67-043"
                or target["ap_sha256"] != "519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec"):
            reason = target.get("build_blockers", ["完整 Chaos ABI 尚未核验"])
            raise ValueError(f"{ident} 不能构建完整 Chaos：" + "；".join(reason))
        if set(target.get("bindings", {})) != {"FW_REGISTER_DRIVER", "FW_UNREGISTER_DRIVER",
                "FW_APP_LOOKUP", "FW_APP_INSTALL", "FW_INIT_BUFFER", "FW_NOTIFY"}:
            raise ValueError("模块启动链 symbol 绑定不完整")
    return {**target, "symbol_dir": folder, "manifest_sha256": digest(folder / "manifest.json")}


def import_reviewed(folder: Path, ap_hash: str) -> dict:
    """Import exact-hash APPROVED address records, preserving their proof level."""
    records, excluded = [], {}
    for path in sorted(folder.glob('*.json')):
        item = read_json(path)
        if (item.get('approval_state') != 'APPROVED'
                or item.get('policy') in ('forbidden', 'withdrawn')
                or item.get('status') in ('FORBIDDEN', 'WITHDRAWN')
                or item.get('kind') not in ('function', 'global', 'data', 'string')):
            key = item.get('approval_state') or 'UNREVIEWED'
            excluded[key] = excluded.get(key, 0) + 1
            continue
        if item.get('provenance', {}).get('firmware_sha256') != ap_hash:
            raise ValueError(f'核验记录属于另一固件：{path.name}')
        kind = item['kind']
        value = item.get('callable_address') if kind == 'function' else item.get('entry_address')
        if not value or not item.get('prototype'):
            raise ValueError(f'已批准记录缺少 callable/ABI 资料：{path.name}')
        records.append({'name': item['name'], 'kind': kind, 'address': value,
                        'instruction_set': item.get('instruction_set', 'n/a'),
                        'prototype': item['prototype'], 'review': 'exact-target-reviewed',
                        'verification_level': item['status'],
                        'evidence': {'source_symbol_id': item['symbol_id'],
                                     'evidence_ids': item.get('proof', {}).get('evidence_ids', []),
                                     'approval_state': item['approval_state'], 'policy': item['policy'],
                                     'promotion': item.get('promotion', {})}})
    if not records:
        raise ValueError('导入目录没有属于当前固件的已批准地址记录')
    seed = {'schema': SCHEMA, 'ap_sha256': ap_hash, 'symbols': records,
            'excluded_source_records': excluded}
    validate_records(seed, ap_hash)
    return seed


def write_extraction(repo: Path, path: Path, ident: str, unpack: Path | None = None,
                     reference: tuple[Path, str] | None = None,
                     reviewed_dir: Path | None = None) -> dict:
    start = time.monotonic()
    ident = target_id(ident)
    ap, image = read_ap(path)
    folder = repo / "symbol" / ident
    target_path, seed_path = folder / "target.json", folder / "reviewed.json"
    if target_path.exists():
        target = read_json(target_path)
        if target.get("ap_sha256") != image["ap_sha256"] or target.get("id") != ident:
            raise ValueError("输入镜像与该目录核验资料不一致，未写入任何 symbol")
    else:
        model, version = ident.rsplit("-", 1)
        target = {"schema": SCHEMA, "id": ident, "model": model, "version": version,
                  "ap_sha256": image["ap_sha256"], "ap_size": len(ap),
                  "chaos_abi": None, "bindings": {},
                  "build_blockers": ["新固件尚缺完整 ABI 与私有 hook 核验"]}
    if target.get("ap_size") != len(ap):
        raise ValueError("AP 长度与目标资料不一致")
    seed = read_json(seed_path) if seed_path.exists() else {
        "schema": SCHEMA, "ap_sha256": image["ap_sha256"], "symbols": []}
    if reviewed_dir is not None:
        imported = import_reviewed(reviewed_dir, image['ap_sha256'])
        combined = {r['name']: r for r in seed['symbols']}
        for record in imported['symbols']:
            old = combined.get(record['name'])
            if old is not None:
                if any(old.get(k) != record.get(k) for k in ('kind', 'prototype')) or address(old['address']) != address(record['address']):
                    raise ValueError(f"导入核验记录与现有记录冲突：{record['name']}")
            else:
                combined[record['name']] = record
        seed = {**seed, 'symbols': sorted(combined.values(), key=lambda r: r['name']),
                'excluded_source_records': imported['excluded_source_records']}
    records = validate_records(seed, image["ap_sha256"])
    generated = generated_files(target, records)
    candidates = match_candidates(ap, reference[0], repo, reference[1]) if reference else []
    if unpack is not None and (unpack / 'vela_ap.bin').resolve() == path.resolve():
        raise ValueError("解包输出不能覆盖输入固件")
    # All potentially failing input validation precedes writes to the profile.
    folder.mkdir(parents=True, exist_ok=True)
    if not target_path.exists():
        write_json(target_path, target)
    write_json(seed_path, seed)
    for name, data in generated.items():
        (folder / name).write_bytes(data)
    candidate_report = {"schema": SCHEMA, "target": ident, "ap_sha256": image["ap_sha256"],
                        "reference": reference[1] if reference else None,
                        "callable_exports": False, "matches": candidates}
    write_json(folder / "candidates.json", candidate_report)
    manifest = {"schema": SCHEMA, "target": ident, **image, "reviewed_exports": len(records),
                "search_results": len(candidates), "files": {
                    name: digest(folder / name) for name in ("target.json", "reviewed.json", *GENERATED)}}
    write_json(folder / "manifest.json", manifest)
    try:
        load_target(repo, ident)
        ready = True
    except ValueError:
        ready = False
    if unpack is not None:
        unpack.mkdir(parents=True, exist_ok=True)
        dest = unpack / "vela_ap.bin"
        dest.write_bytes(ap)
    report = [f"# {ident} 符号提取报告", "", f"AP SHA-256：`{image['ap_sha256']}`",
              f"AP 长度：{len(ap)} 字节；输入：`{path.name}`。", "",
              f"已核验导出：{len(records)}；匹配搜索结果：{len(candidates)}。",
              "", "firmware.rs / firmware.h / firmware.ld 只包含 reviewed.json 的精确固件记录。",
              "candidates.json 不参与编译，匹配分数和字节相似不代表 ABI 一致或设备验证。", "",
              "完整 Chaos 构建：" + ("支持当前 .043 基线。" if ready else
                  "尚未支持；需要完成 target.json 中列出的 ABI 阻塞项。"), ""]
    (folder / "REPORT.md").write_text("\n".join(report), encoding="utf-8")
    return {"target": ident, "ap_sha256": image["ap_sha256"], "exports": len(records),
            "candidates": len(candidates), "full_chaos_build_ready": ready,
            "seconds": round(time.monotonic() - start, 3), "folder": str(folder)}


def match_candidates(target_ap: bytes, reference_path: Path, repo: Path, ident: str) -> list[dict]:
    """Unique anchor search; explicitly never promotes a result to callable.

    Only XIP entry addresses with a known file mapping can seed this pass.
    RAM/PSRAM addresses need the corresponding relocation map, not subtraction.
    A 64-byte match can include literals or a neighbouring function, so its
    result is deliberately kept outside the reviewed export files.
    """
    ref_ap, meta = read_ap(reference_path)
    target = load_target(repo, ident, require_build=False)
    if meta["ap_sha256"] != target["ap_sha256"]:
        raise ValueError("参考镜像与参考 symbol 摘要不一致")
    records = read_json(target["symbol_dir"] / "symbols.json")["symbols"]
    results = []
    for record in records:
        if record["kind"] != "function":
            continue
        old = address(record["address"]) & ~1
        off = old - 0x0C0C0000
        if not 16 <= off <= len(ref_ap) - 64:
            continue
        pattern = ref_ap[off:off + 64]
        hit = target_ap.find(pattern)
        if hit < 16 or hit & 1 or target_ap.find(pattern, hit + 1) != -1:
            continue
        results.append({"name": record["name"], "reference_address": record["address"],
                        "target_file_offset": f"0x{hit:X}", "match_bytes": 64,
                        "pattern_sha256": sha(pattern), "status": "REVIEW_REQUIRED"})
    return results


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=ROOT)
    sub = parser.add_subparsers(dest="command", required=True)
    extract = sub.add_parser("extract", help="快速读取 OTA/AP 并生成核验 symbol 绑定")
    extract.add_argument("image", type=Path)
    extract.add_argument("--target", required=True)
    extract.add_argument("--unpack", type=Path, help="可选：另外保存原始 AP；不提交镜像")
    extract.add_argument("--reference-image", type=Path)
    extract.add_argument("--reference-target")
    extract.add_argument("--reviewed-dir", type=Path, help="可选：导入同一 AP 的已批准 symbol JSON 目录")
    extract.add_argument("--exports-only", action="store_true", help="明确允许只导出局部核验 symbol，尚不能构建完整 Chaos")
    check = sub.add_parser("check", help="核验生成物摘要及完整 Chaos 兼容性")
    check.add_argument("--target", default=DEFAULT_TARGET)
    check.add_argument("--exports-only", action="store_true")
    prepare = sub.add_parser("prepare", help="给 Rust/C 构建准备同一套绑定")
    prepare.add_argument("--target", default=DEFAULT_TARGET)
    prepare.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    repo = args.repo.expanduser().resolve()
    if args.command == "extract":
        if bool(args.reference_image) != bool(args.reference_target):
            raise ValueError("参考匹配必须同时提供 --reference-image 和 --reference-target")
        ref = (args.reference_image.expanduser(), args.reference_target) if args.reference_image else None
        result = write_extraction(repo, args.image.expanduser(), args.target, args.unpack, ref, args.reviewed_dir)
        print(json.dumps(result, ensure_ascii=False, indent=2))
        if not args.exports_only:
            load_target(repo, args.target)
        return 0
    target = load_target(repo, args.target, require_build=not getattr(args, "exports_only", False))
    if args.command == "prepare":
        args.out.mkdir(parents=True, exist_ok=True)
        for name in GENERATED:
            shutil.copyfile(target["symbol_dir"] / name, args.out / name)
        write_json(args.out / "profile.json", {k: v for k, v in target.items() if k != "symbol_dir"})
    print(f"OK {target['id']} AP={target['ap_sha256']} symbols={target['symbol_dir']}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, zipfile.BadZipFile) as error:
        print(f"固件 symbol 处理失败：{error}", file=sys.stderr)
        raise SystemExit(2)
