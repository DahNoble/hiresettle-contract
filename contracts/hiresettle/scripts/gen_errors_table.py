#!/usr/bin/env python3
"""Regenerate the errors reference table in `src/errors.rs`.

The contract signals errors with `panic!` string messages rather than an error
enum. This script scans every non-test source file for `panic!` calls, resolves
the shared `ERR_*` constants declared in `errors.rs`, and rewrites the
module-level doc table between the BEGIN/END markers in `errors.rs`.

Usage (from `contracts/hiresettle`):

    python3 scripts/gen_errors_table.py          # rewrite src/errors.rs
    python3 scripts/gen_errors_table.py --check  # exit 1 if the table is stale
"""

import re
import sys
from collections import defaultdict
from pathlib import Path

SRC = Path(__file__).resolve().parent.parent / "src"
ERRORS_RS = SRC / "errors.rs"
BEGIN = "//! <!-- BEGIN GENERATED ERRORS TABLE -->"
END = "//! <!-- END GENERATED ERRORS TABLE -->"
# Rows raised from more call sites than this collapse to a per-module summary.
MAX_LISTED_FNS = 4

CONST_RE = re.compile(r'pub\(crate\) const (ERR_\w+): &str = "([^"]*)";')
PANIC_RE = re.compile(r'panic!\(\s*"([^"]*)"\s*(?:,\s*([A-Za-z_]\w*))?')
FN_RE = re.compile(r"\bfn\s+([a-z_]\w*)\s*[<(]")


def load_constants():
    return dict(CONST_RE.findall(ERRORS_RS.read_text()))


def scan(constants):
    """Return {message: {"const": name|None, "sites": [(module, fn)]}}."""
    rows = defaultdict(lambda: {"const": None, "sites": []})
    for path in sorted(SRC.glob("*.rs")):
        if path.name in ("test.rs", "errors.rs"):
            continue
        module = path.stem
        current_fn = "<module>"
        for line in path.read_text().splitlines():
            fn = FN_RE.search(line)
            if fn and not line.lstrip().startswith("//"):
                current_fn = fn.group(1)
            for fmt, arg in PANIC_RE.findall(line):
                const = None
                if fmt == "{}" and arg in constants:
                    const, message = arg, constants[arg]
                else:
                    # Drop the formatted suffix, e.g. "TagEmpty: index {}".
                    message = re.sub(r":?\s*[^:]*\{\}.*$", "", fmt) or fmt
                row = rows[message]
                row["const"] = row["const"] or const
                row["sites"].append((module, current_fn))
    return rows


def render(rows):
    lines = [
        BEGIN,
        "//!",
        "//! | Message | Constant | Raised in | Sites |",
        "//! |---|---|---|---|",
    ]
    for message in sorted(rows, key=str.lower):
        row = rows[message]
        sites = row["sites"]
        fns = sorted({f"`{m}::{f}`" for m, f in sites})
        if len(fns) > MAX_LISTED_FNS:
            modules = sorted({m for m, _ in sites})
            where = f"{len(fns)} functions in " + ", ".join(f"`{m}`" for m in modules)
        else:
            where = ", ".join(fns)
        const = f"`{row['const']}`" if row["const"] else "—"
        lines.append(f"//! | `{message}` | {const} | {where} | {len(sites)} |")
    lines += ["//!", END]
    return "\n".join(lines)


def main():
    text = ERRORS_RS.read_text()
    if BEGIN not in text or END not in text:
        sys.exit(f"markers not found in {ERRORS_RS}")
    head, rest = text.split(BEGIN, 1)
    _, tail = rest.split(END, 1)
    updated = head + render(scan(load_constants())) + tail
    if "--check" in sys.argv:
        if updated != text:
            sys.exit("errors.rs table is stale; run scripts/gen_errors_table.py")
        return
    ERRORS_RS.write_text(updated)


if __name__ == "__main__":
    main()
