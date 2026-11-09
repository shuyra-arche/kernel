#!/usr/bin/env python3
"""Symbolize saved trap PCs and return-address evidence with the matching ELF."""
import argparse
import re
import shutil
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("elf", type=Path)
parser.add_argument("serial", type=Path)
parser.add_argument("--addr2line", default=shutil.which("llvm-addr2line") or "addr2line")
args = parser.parse_args()
for line in args.serial.read_text(errors="replace").splitlines():
    match = re.search(r"\bpc=(0x[0-9a-fA-F]+)", line)
    if match and ("source=Trap" in line or line.startswith("frame source=")):
        print(line, flush=True)
        # Keep the literal PC; subtracting four is incorrect for compressed calls.
        subprocess.run([args.addr2line, "-f", "-C", "-e", str(args.elf), match[1]], check=True)
