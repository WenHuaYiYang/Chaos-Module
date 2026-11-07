#!/usr/bin/env python3
"""Compile the actual no_std terminal UI/model against host firmware mocks."""
from pathlib import Path
import subprocess
import tempfile
import re

ROOT = Path(__file__).resolve().parents[1]

def check_registration():
    def read(path):
        return (ROOT / "supervisor/src" / path).read_text(encoding="utf-8")
    state, ipc, page, ui, timer = (read(p) for p in
                                 ("state.rs", "ipc.rs", "page.rs", "ui.rs", "watchface.rs"))
    count = int(re.search(r"pub const PAGE_COUNT: usize = (\d+)", state)[1])
    assert count == 36
    assert "MAX_PID: usize = PAGE_COUNT - 1" in state
    assert "MAX_PAGE_ID: usize = crate::state::MAX_PID" in read("fw_api.rs")
    for declaration in ("DESC: [u32; PAGE_COUNT * 29]", "PAGES: [Page; PAGE_COUNT]",
                        "TITLE_TXT: [[u8; 48]; PAGE_COUNT]"):
        assert declaration in state, declaration
    names = ipc.split("let pnames:", 1)[1].split("];", 1)[0]
    assert len(re.findall(r"addr_of!\(PAGE_", names)) == count
    assert "addr_of!(PAGE_NSH)" in names
    table = page.split("pub(crate) static PAGE_TABLE:", 1)[1].split("];", 1)[0]
    assert len(re.findall(r"(?m)^\s*&[^,]+,", table)) == count
    assert "&crate::terminal_ui::PAGE" in table
    assert "&crate::btpan::PAGE_EXP_P" in table
    assert "let mut items = [0u32; PAGE_COUNT];" in ipc
    assert "desc_addr + i as u32 * 116" in ipc
    assert "install(meta, items.as_ptr(), PAGE_COUNT as u32)" in ipc
    assert ui.count("crate::terminal_ui::forget_ui()") >= 2
    resident = timer.split('fn chaos_shake_timer(', 1)[1]
    assert resident.index("crate::terminal_ui::poll_output();") < resident.index("if st_rd!(BOOT_DELAY)")
    print("NSH registration/lifecycle integration: passed", flush=True)

def main():
    check_registration()
    with tempfile.TemporaryDirectory(prefix="chaos-terminal-tests-") as work:
        exe = Path(work) / "terminal_tests"
        subprocess.run(["rustc", "--edition=2021", "--test",
                        str(ROOT / "supervisor/tests/terminal_ui_harness.rs"),
                        "-o", str(exe)], check=True)
        subprocess.run([str(exe)], check=True)

if __name__ == "__main__":
    main()
