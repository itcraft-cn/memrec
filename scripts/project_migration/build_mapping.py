#!/usr/bin/env python3
"""生成一次性项目回迁映射（mapping.json）。

临时脚本（WS2 一次性迁移配套），非常驻 API。功能：
  1. 扫描文件系统中的 `.mr_pid`，得到 目录名 -> 项目 UUID（来源 A：磁盘发现）；
  2. 叠加人工维护的别名表（aliases.json），补足标签与项目名不一致的情形（来源 C：标签即项目名）；
  3. 输出 `mapping.json`，供 `mr-ability/examples/migrate_project_attribution.rs` 使用。

用法：
  python3 build_mapping.py --source <源项目UUID> --out mapping.json

`.mr_pid` 文件内容形如：
  memrec_project_id=<uuid>
  created_at=<rfc3339>
  或为旧格式：仅一行裸 UUID。
"""

import argparse
import json
import subprocess
from pathlib import Path


def scan_mr_pid() -> dict[str, str]:
    """扫描全盘 `.mr_pid`，返回 目录名(小写) -> UUID。优先 plocate，回退 fd。"""
    try:
        out = subprocess.run(
            ["plocate", "-r", r"/\.mr_pid$"],
            capture_output=True,
            text=True,
            check=False,
        ).stdout
    except FileNotFoundError:
        out = ""
    if not out.strip():
        try:
            out = subprocess.run(
                ["fd", "-H", "-g", ".mr_pid", "/", "/home", "/disk2"],
                capture_output=True,
                text=True,
                check=False,
            ).stdout
        except FileNotFoundError:
            out = ""

    result: dict[str, str] = {}
    for line in out.splitlines():
        p = Path(line.strip())
        if not p.is_file():
            continue
        text = p.read_text(errors="ignore")
        uuid = None
        for raw in text.splitlines():
            raw = raw.strip()
            if raw.startswith("memrec_project_id="):
                uuid = raw.split("=", 1)[1].strip()
                break
            # 旧格式：裸 UUID 单行
            if len(raw) == 36 and raw.count("-") == 4:
                uuid = raw
                break
        if uuid:
            result[p.parent.name.lower()] = uuid
    return result


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--source", required=True, help="误归属的源项目 UUID")
    ap.add_argument("--aliases", default="aliases.json")
    ap.add_argument("--out", default="mapping.json")
    args = ap.parse_args()

    aliases: dict[str, list[str]] = {}
    alias_path = Path(args.aliases)
    if alias_path.exists():
        aliases = json.loads(alias_path.read_text())

    discovered = scan_mr_pid()
    projects = []
    for name, uuid in sorted(discovered.items()):
        al = sorted({name, *[a.lower() for a in aliases.get(name, [])]})
        projects.append({"name": name, "uuid": uuid, "aliases": al})

    mapping = {"source_project": args.source, "projects": projects}
    Path(args.out).write_text(json.dumps(mapping, ensure_ascii=False, indent=2))
    print(f"[build_mapping] projects={len(projects)} -> {args.out}")


if __name__ == "__main__":
    main()
