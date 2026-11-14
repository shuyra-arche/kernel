#!/usr/bin/env python3
"""Validate an explicitly requested PR comparison's published control revision."""
import argparse
import json
from pathlib import Path
import re


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event", type=Path, required=True)
    args = parser.parse_args()
    body = json.loads(args.event.read_text())["pull_request"]["body"] or ""
    controls = re.findall(r"^Room baseline: ([0-9a-f]{40})$", body, re.MULTILINE)
    if len(controls) != 1:
        parser.error("requested comparison needs exactly one 'Room baseline: <full SHA>' line")
    print("sha=" + controls[0])


if __name__ == "__main__":
    main()
