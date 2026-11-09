#!/usr/bin/env python3
"""Audit the real pinned export, deterministic staging and unchanged checkout."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import horizon


def snapshot(root):
    return {
        p.relative_to(root).as_posix(): (horizon.digest(p.read_bytes()), p.stat().st_mtime_ns)
        for p in root.rglob("*")
        if p.is_file()
    }


def audit(original, stage, plan):
    subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "--locked",
            "-p",
            "native-mod-source-audit",
            "--",
            str(original),
            str(stage),
            str(plan),
        ],
        cwd=horizon.ROOT,
        check=True,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--horizon", type=Path, default=horizon.ROOT / "horizon")
    args = parser.parse_args()
    checkout = args.horizon.resolve()
    status = subprocess.check_output(["git", "-C", str(checkout), "status", "--porcelain"])
    (horizon.ROOT / "target").mkdir(exist_ok=True)
    with horizon.lock(horizon.ROOT / "target/stage.lock"):
        with tempfile.TemporaryDirectory(prefix="native-stage-test-", dir=horizon.ROOT / "target") as scratch:
            root = Path(scratch)
            output, changed = horizon.mixin_stage(checkout, root)
            assert changed
            first = snapshot(output)
            second, changed = horizon.mixin_stage(checkout, root)
            assert second == output and not changed and first == snapshot(output)
            plan = json.loads((root / "plan.json").read_text(encoding="utf-8"))
            assert plan["schema"] == horizon.COMPAT["mixin_schema"]
            assert len(plan["injections"]) == 25 and len(plan["modules"]) == 2 and len(plan["crates"]) == 1
            original = next((root / "original").glob("*/.horizon-mod-template.json")).parent
            audit(original, output, root / "plan.json")
    assert subprocess.check_output(["git", "-C", str(checkout), "status", "--porcelain"]) == status
    print(
        json.dumps(
            {
                "status": "ok",
                "original_checkout_unchanged": True,
                "only_planned_asts_changed": True,
                "deterministic_bytes_and_mtimes": True,
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
