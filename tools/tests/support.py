"""Shared paths for the tools checks.

Importing this puts tools/ on the import path, so checks can import the
generators and helpers they verify (`dialog_kit`, `xrc_sizer_checks`, ...).
"""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
TOOLS = ROOT / 'tools'
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))


def rust_source(path):
    """A Rust source file with the unit tests it includes through
    `#[cfg(test)] #[path = "..."] mod ...;` or `include!` (they live under
    tests/unit/)."""
    path = Path(path)
    text = path.read_text(encoding='utf-8')
    included = re.findall(r'#\[cfg\(test\)\][^\n]*?(?:#\[path\s*=\s*|include!\()"([^"]+)"', text)
    # Paths inside inline modules are resolved by Rust from a deeper directory;
    # the file is found by name below tests/unit either way.
    for rel in included:
        target = (path.parent / rel).resolve()
        if not target.exists():
            crate = next(p for p in path.parents if (p / 'Cargo.toml').exists())
            matches = list((crate / 'tests' / 'unit').rglob(Path(rel).name))
            target = next((m for m in matches if str(m).endswith(rel.split('tests/unit/')[-1])), None)
        if target and target.exists():
            text += '\n' + target.read_text(encoding='utf-8')
    return text
