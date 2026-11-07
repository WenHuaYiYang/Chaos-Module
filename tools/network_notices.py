"""Produce the network notices shipped alongside and inside the installer."""
from pathlib import Path
import re

from build_network import SOURCES


def network_notices(repo: Path) -> str:
    network = repo / "supervisor/network"
    parts = ["Chaos network third-party notices (generated; keep with binary distribution)\n"]
    for relative in ("PROVENANCE.md", "third_party/tinypan/SOURCE.md",
                     "third_party/tinypan/LICENSE", "third_party/lwip/SOURCE.md",
                     "third_party/lwip/COPYING"):
        parts.extend([f"\n===== {relative} =====\n", (network / relative).read_text()])
    # COPYING is not the only copyright holder: e.g. dhcp.c names Leon
    # Woestenberg and Axon. Retain every selected file's complete notice.
    sources = {network / item for item in SOURCES if item.startswith("third_party/lwip/")}
    sources.update((network / "third_party/lwip/src/include").rglob("*.h"))
    notices = {}
    for source in sorted(sources):
        for block in re.findall(r"/\*.*?\*/", source.read_text(), re.S):
            if re.search(r"copyright|redistribution and use", block, re.I):
                notices.setdefault(block, []).append(str(source.relative_to(network)))
    if not notices:
        raise ValueError("lwIP copyright notices are missing")
    for block, paths in notices.items():
        parts.extend(["\n===== lwIP source notice =====\n", "\n".join(paths), "\n", block, "\n"])
    return "\n".join(parts)
