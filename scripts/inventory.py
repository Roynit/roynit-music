"""Generate a read-only inventory of original Riff route/IPC names for port review."""
from pathlib import Path
import re

RUST = Path(__file__).resolve().parents[1]
REPO = RUST.parent


def main():
    routes = {}
    ipc = {}
    for path in sorted((REPO / "src").rglob("*.js")):
        content = path.read_text(encoding="utf-8")
        relative = path.relative_to(REPO).as_posix()
        for match in re.finditer(r"['\"](/api/[^'\"\s]+)['\"]", content):
            name = match.group(1)
            if "${" not in name:
                routes.setdefault(name, (relative, content.count("\n", 0, match.start()) + 1))
        for match in re.finditer(r"ipcMain\.(?:handle|on|once)\(\s*['\"]([^'\"]+)", content):
            ipc.setdefault(match.group(1), (relative, content.count("\n", 0, match.start()) + 1))

    lines = [
        "# Inventory giao tiếp Riff gốc",
        "",
        "Sinh từ source bởi scripts/inventory.py. Vị trí là lần xuất hiện literal đầu tiên, có thể là allowlist hoặc caller, không nhất thiết là handler.",
        "Đây là danh sách để kiểm tra bỏ sót, không phải xác nhận mọi route/IPC đã được port. Đọc PARITY.md để biết trạng thái thực tế.",
        "",
    ]
    for title, items in [("HTTP routes", routes), ("Electron IPC", ipc)]:
        lines += [f"## {title} ({len(items)})", "", "| Tên | Source gốc |", "|---|---|"]
        for name, (path, line) in sorted(items.items()):
            lines.append(f"| `{name}` | `{path}:{line}` |")
        lines.append("")
    (RUST / "docs" / "LEGACY_INVENTORY.md").write_text("\n".join(lines), encoding="utf-8")
    print(f"Inventoried {len(routes)} routes and {len(ipc)} IPC names.")


if __name__ == "__main__":
    main()
