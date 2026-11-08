#!/usr/bin/env python3
"""Static Corona UI/license regressions; no firmware or cross compiler needed."""

import hashlib
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
PAGE_FILES = [ROOT / "supervisor/src/page.rs", *sorted((ROOT / "supervisor/src/page").glob("*.rs"))]
PAGES = {path: path.read_text(encoding="utf-8") for path in PAGE_FILES}
PAGE = "\n".join(PAGES.values())
SOURCE_URL = "https://github.com/AstralSightStudios/Canopus-Module-Corona"


def page_impl(name):
    marker = f"impl Page for {name} {{"
    for text in PAGES.values():
        start = text.find(marker)
        if start < 0:
            continue
        end = text.find("\n// =====", start)
        return text[start:] if end < 0 else text[start:end]
    raise AssertionError(f"page implementation not found: {name}")


def live_code(text):
    """去掉整行注释：corona模糊 按钮按要求注释停用，断言只针对仍然生效的代码。"""
    return "\n".join(line for line in text.splitlines() if not line.lstrip().startswith("//"))


class CoronaUiTests(unittest.TestCase):
    def test_registered_pages_are_not_removed_placeholders(self):
        table = PAGE.split("pub(crate) static PAGE_TABLE:", 1)[1]
        entries = re.findall(r"&(?:\w+::)*([A-Z_0-9]+),", table)
        self.assertEqual(entries[7], "PAGE_BLUR_LEGACYP")
        self.assertEqual(entries[20], "PAGE_LICENSESP")
        self.assertEqual(entries[21], "PAGE_LICENSEP")
        ipc = (ROOT / "supervisor/src/ipc.rs").read_text(encoding="utf-8")
        names = ipc.split("let pnames:", 1)[1].split("];", 1)[0]
        self.assertIn("addr_of!(PAGE_BLUR_LEGACY)", names)

    def test_main_page_preserves_entries_and_clarifies_shared_region_scope(self):
        raw = page_impl("BlurPage")
        main = live_code(raw)
        for text in ("系统背景", "模糊v2", "旧版选项", "模糊区域",
                     "适用于所有模糊选项（含通知图标去黑底）"):
            self.assertIn(text, main)
        self.assertNotIn("Corona 固定覆盖四区", main)
        self.assertNotIn("仅对旧版选项生效", main)
        self.assertIn("mode() == 0 && !crate::corona_blur::active()", main)
        # corona模糊 按钮按要求注释停用；保留注释原文便于随时恢复。
        self.assertNotIn('(1, RowSpec::switch("corona模糊', main)
        self.assertIn('// (1, RowSpec::switch("corona模糊', raw)
        self.assertIn('(2, RowSpec::switch("模糊v2', main)
        self.assertIn('4 => nav_goto(BLUR_LEGACY_PID, 0)', main)
        self.assertIn('5 => nav_goto(crate::blur_zone::PAGE_PID, 0)', main)

    def test_selecting_system_background_disables_corona_first(self):
        click = page_impl("BlurPage").split("fn on_click", 1)[1]
        action = click.split("0 => {", 1)[1].split("},", 1)[0]
        self.assertLess(action.index("crate::corona_blur::set(false)"),
                        action.index("crate::background_home::request(0)"))

    def test_main_row_indices_match_build_navigation_and_switch_dispatch(self):
        main = live_code(page_impl("BlurPage"))
        render = main.split("let prev = build_rows", 1)[1].split("let label", 1)[0]
        row_indices = [int(slot) for slot in re.findall(r"\((\d+), RowSpec::", render)]
        # 行1（corona模糊）已停用，槽位留空，其余行号不变。
        self.assertEqual(row_indices, [0, 2, 3, 4, 5, 11])
        expected_rows = (
            '(0, RowSpec::check("系统背景',
            '(2, RowSpec::switch("模糊v2',
            '(3, RowSpec::display(crate::background_home::status()',
            '(4, RowSpec::tap("旧版选项',
            '(5, RowSpec::tap("模糊区域',
            '(11, RowSpec::clickable("返回',
        )
        for row in expected_rows:
            self.assertIn(row, render)
        click = main.split("fn on_click", 1)[1].split("fn is_switch_row", 1)[0]
        self.assertIn("4 => nav_goto(BLUR_LEGACY_PID, 0)", click)
        self.assertIn("5 => nav_goto(crate::blur_zone::PAGE_PID, 0)", click)
        self.assertIn("11 => nav_back()", click)
        self.assertNotRegex(click, r"(?m)^\s*[123]\s*=>")
        self.assertIn("fn is_switch_row(&self, idx: usize) -> bool { idx == 2 }", main)
        switch = main.split("fn on_switch", 1)[1]
        self.assertNotIn("crate::corona_blur::set(!crate::corona_blur::enabled())", switch)
        self.assertIn("crate::corona_blur::set_v2_enabled(!crate::corona_blur::v2_enabled())", switch)

    def test_switch_feedback_uses_each_setting_not_the_shared_engine(self):
        render = live_code(page_impl("BlurPage")).split("fn on_click", 1)[0]
        updates = re.findall(r"fw_api::row_update\((.*?);", render, re.DOTALL)
        self.assertEqual(len(updates), 2)
        expected = {
            0: "mode() == 0 && !crate::corona_blur::active()",
            2: "crate::corona_blur::v2_enabled() as u8",
        }
        for update in updates:
            slot = int(re.search(r"ctx\.rows\.add\((\d+)\)", update).group(1))
            self.assertIn(expected.pop(slot), update)
        self.assertFalse(expected)

    def test_legacy_controls_are_kept_in_their_page(self):
        main = page_impl("BlurPage")
        legacy = page_impl("BlurLegacyPage")
        for text in ("静态背景图(表盘)", "自动截取替换图片", "手动生成或更新表盘背景", "完全透明背景（实验性）"):
            self.assertIn(text, legacy)
            self.assertNotIn(text, main)
        self.assertIn("关闭上面的模糊开关后生效", main)
        self.assertIn("关闭 corona模糊后生效", legacy)

    def test_region_page_explains_scope_includes_corona(self):
        regions = page_impl("BlurZonePage")
        self.assertIn("适用于 Corona 模糊", regions)
        self.assertIn("包括通知图标去黑底", regions)
        self.assertIn("关闭通知区域恢复原图标", regions)
        self.assertIn("同时撤销通知图标去黑底", regions)
        self.assertNotIn("Corona 固定覆盖四区", regions)
        self.assertNotIn("仅对旧版选项生效", regions)

    def test_attribution_is_last_growing_content_label_with_plain_urls(self):
        main = page_impl("BlurPage").split("fn on_click", 1)[0]
        self.assertIn(SOURCE_URL, main)
        self.assertGreater(main.index(SOURCE_URL), main.index("返回"))
        self.assertNotIn('RowSpec::display("说明\\0".as_bytes()', main)
        self.assertNotIn("使用 Corona 模糊实现", main)
        modified_url = "https://github.com/WenHuaYiYang/Chaos-Module/tree/master/supervisor"
        exact_notice = (
            f"说明：“corona模糊”功能源码来自{SOURCE_URL}，"
            f"修改后源码可见：{modified_url}。"
        )
        self.assertIn(exact_notice, main)
        self.assertEqual(main.count(SOURCE_URL), 1)
        self.assertEqual(main.count(modified_url), 1)
        self.assertNotIn("[https://", main)
        self.assertIn("let prev = build_rows(ctx, ctx.title", main)
        self.assertIn("fw_api::label_create(ctx.content)", main)
        self.assertIn("style_apply(label, crate::FW_STYLE_HR_NAME", main)
        self.assertIn("obj_set_width(label, 320)", main)
        self.assertNotIn("obj_set_size_wh(label", main)
        self.assertIn("obj_align_to(label, prev, fw_api::align::OUT_BOTTOM_MID", main)

    def test_about_license_is_reachable_and_uses_upstream_text(self):
        about = page_impl("AboutPage")
        self.assertIn("开放源代码许可", about)
        self.assertIn("idx == 8", about)
        self.assertIn("nav_goto(20, 0)", about)
        self.assertIn("AGPL-3.0-only", page_impl("LicensesPage"))
        # 页面拆到 page/ 子目录后 include_bytes! 的相对路径多了一层 ../，
        # 这里只校验"用的是 vendor 里的原文"，不绑死前缀层数。
        self.assertRegex(PAGE, r'include_bytes!\("[./]*corona_vendor/LICENSE"\)')
        self.assertIn("nav_goto(21, 0)", page_impl("LicensesPage"))

    def test_license_label_has_content_height_not_a_clipping_box(self):
        license_page = page_impl("LicenseTextPage")
        self.assertIn("obj_set_width(label, 320)", license_page)
        self.assertNotIn("obj_set_size_wh(label, 320, 320)", license_page)

    def test_vendored_files_match_recorded_upstream_hashes(self):
        notice = (ROOT / "licenses/CANOPUS-MODULE-CORONA-NOTICE.txt").read_text(encoding="utf-8")
        self.assertIn("AGPL-3.0-only", notice)
        self.assertIn(SOURCE_URL, notice)
        entries = re.findall(r"^([0-9a-f]{64})  (.+)$", notice, re.MULTILINE)
        self.assertEqual(len(entries), 19)
        self.assertNotIn("supervisor/tests/corona/README.md", [path for _, path in entries])
        self.assertNotIn("supervisor/tests/corona/upstream.sha256", [path for _, path in entries])
        for digest, relative in entries:
            with self.subTest(path=relative):
                file = ROOT / relative if relative.startswith("supervisor/") else ROOT / "supervisor/corona_vendor" / relative
                self.assertEqual(hashlib.sha256(file.read_bytes()).hexdigest(), digest)

    def test_license_pages_can_show_all_original_bytes(self):
        lines_per_page = int(re.search(r"const LICENSE_PAGE_LINES: usize = (\d+);", PAGE).group(1))
        view_size = int(re.search(r"static mut LICENSE_VIEW: \[u8; (\d+)\]", PAGE).group(1))
        for filename in ("supervisor/corona_vendor/LICENSE", "licenses/MIBAND10PRO-TUNMODULE-MPL-2.0.txt"):
            original = (ROOT / filename).read_bytes()
            lines = original.splitlines(keepends=True)
            pages = [b"".join(lines[i:i + lines_per_page]) for i in range(0, len(lines), lines_per_page)]
            self.assertEqual(b"".join(pages), original)
            self.assertTrue(all(len(page) < view_size for page in pages))
            self.assertNotIn(b"\0", original)


if __name__ == "__main__":
    unittest.main()
