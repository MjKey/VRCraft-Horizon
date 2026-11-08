#!/usr/bin/env python3
"""Export pinned sources, apply the ViveCraft bundle and build/check a native Horizon."""
import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
COMPAT = json.loads((ROOT / "compatibility.json").read_text(encoding="utf-8"))
PIN = COMPAT["horizon_revision"]
MIXINS = Path(os.environ.get("HORIZON_MIXINS", ROOT / "horizon-mixins")).resolve()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def stage(horizon, stages):
    if not (horizon / ".git").exists():
        raise ValueError("--horizon must be a Git checkout (including a worktree)")
    subprocess.run(
        ["git", "-C", str(horizon), "cat-file", "-e", PIN + "^{commit}"],
        check=True,
        capture_output=True,
    )
    inputs = {"revision": PIN.encode(), "export-schema": b"2"}
    signature = digest(b"".join(k.encode() + b"\0" + value for k, value in sorted(inputs.items())))
    stages.mkdir(parents=True, exist_ok=True)
    out = stages / signature[:20]
    if out.exists():
        marker = json.loads((out / ".horizon-mod-template.json").read_text(encoding="utf-8"))
        if marker["signature"] != signature:
            raise ValueError(f"stage marker mismatch: {out}")
        for path, expected in marker["files"].items():
            if digest((out / path).read_bytes()) != expected:
                raise ValueError(f"staged source was edited: {path}; remove this generated stage to rebuild it")
        return out, False
    temporary = Path(tempfile.mkdtemp(prefix=".mod-template-stage-", dir=stages))
    try:
        process = subprocess.Popen(
            ["git", "-c", "core.autocrlf=false", "-C", str(horizon), "archive", "--format=tar", PIN],
            stdout=subprocess.PIPE,
        )
        try:
            with tarfile.open(fileobj=process.stdout, mode="r|") as archive:
                for member in archive:
                    archive.extract(member, temporary, filter="data")
        finally:
            process.stdout.close()
            code = process.wait()
        if code:
            raise ValueError(f"git archive exited {code}")
        for path, expected in COMPAT["source_sha256"].items():
            if digest((temporary / path).read_bytes()) != expected:
                raise ValueError(f"pinned source hash mismatch: {path}")
        for file in temporary.rglob("*"):
            if file.is_symlink():
                content = file.read_bytes()
                file.unlink()
                file.write_bytes(content)
        (temporary / ".horizon-revision").write_text(PIN + "\n", encoding="utf-8")
        files = {
            p.relative_to(temporary).as_posix(): digest(p.read_bytes())
            for p in sorted(temporary.rglob("*"))
            if p.is_file()
        }
        (temporary / ".horizon-mod-template.json").write_text(
            json.dumps(
                {"signature": signature, "horizon_revision": PIN, "files": files},
                sort_keys=True,
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        os.rename(temporary, out)
    except BaseException:
        shutil.rmtree(temporary, ignore_errors=True)
        raise
    return out, True


def mixin_stage(horizon, stages):
    revision = subprocess.check_output(
        ["git", "-C", str(MIXINS), "rev-parse", "HEAD"], text=True
    ).strip()
    if revision != COMPAT["horizon_mixins_revision"]:
        raise ValueError(
            f"horizon-mixins must be pinned at {COMPAT['horizon_mixins_revision']}, got {revision}"
        )
    subprocess.run(
        ["git", "-C", str(MIXINS), "diff", "--quiet", "HEAD", "--", "Cargo.toml", "Cargo.lock", "crates"],
        check=True,
    )
    original, _ = stage(horizon, stages / "original")
    print("[horizon.py] Building horizon-mixins-cli (--release)...", file=sys.stderr, flush=True)
    build_cmd = [
        "cargo",
        "build",
        "--release",
        "--locked",
        "--target-dir",
        str(MIXINS / "target"),
        "-p",
        "horizon-mixins-cli",
    ]
    offline_res = subprocess.run([*build_cmd, "--offline"], cwd=MIXINS, capture_output=True, text=True)
    if offline_res.returncode != 0:
        subprocess.run(build_cmd, cwd=MIXINS, check=True, stdout=sys.stderr)
    exe_suffix = ".exe" if os.name == "nt" else ""
    cli = MIXINS / f"target/release/horizon-mixins{exe_suffix}"
    output = stages / "modded"
    plan = stages / "plan.json"
    base = ["--workspace", str(original), "--mods", str(ROOT / "mixins")]
    print("[horizon.py] Running horizon-mixins plan...", file=sys.stderr, flush=True)
    subprocess.run([str(cli), "plan", *base, "--out", str(plan)], check=True, capture_output=True, text=True)
    print("[horizon.py] Running horizon-mixins apply...", file=sys.stderr, flush=True)
    applied = subprocess.run(
        [str(cli), "apply", *base, "--out", str(output)], check=True, capture_output=True, text=True
    )
    report = json.loads(applied.stdout)
    return output, report["stage_changed"]


@contextlib.contextmanager
def lock(path):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        if os.name == "nt":
            import msvcrt
            msvcrt.locking(handle.fileno(), msvcrt.LK_LOCK, 1)
            try:
                yield
            finally:
                try:
                    msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
                except OSError:
                    pass
        else:
            import fcntl
            fcntl.flock(handle, fcntl.LOCK_EX)
            yield


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["stage", "check", "build", "test"])
    parser.add_argument("--horizon", type=Path, default=ROOT / "horizon")
    parser.add_argument("--target-dir", type=Path, default=ROOT / "target/horizon-cargo")
    args = parser.parse_args()
    horizon = args.horizon.resolve()
    target = args.target_dir.resolve()
    if target.is_relative_to(horizon):
        parser.error("Cargo target must be outside the original Horizon checkout")
    started = time.monotonic()
    with lock(ROOT / "target/stage.lock"):
        output, changed = mixin_stage(horizon, ROOT / "target/mixins")
        if target.is_relative_to(output) or output.is_relative_to(target):
            parser.error("Cargo target and generated source stage must be separate")
        report = {
            "stage": str(output),
            "stage_changed": changed,
            "horizon_revision": PIN,
            "staging_seconds": time.monotonic() - started,
        }
        if args.action != "stage":
            command = [
                "cargo",
                args.action,
                "--locked",
                "-p",
                "horizon-client",
                "--bin",
                "horizon-client",
                "--no-default-features",
                "--target-dir",
                str(target),
            ]
            if args.action == "build":
                command.extend(["--profile", "fast"])
            with lock(target.parent / (target.name + ".mod-template.lock")):
                run_start = time.monotonic()
                result = subprocess.run([*command, "--offline"], cwd=output, capture_output=True, text=True)
                if result.returncode != 0 and "attempting to make an HTTP request, but --offline was specified" in result.stderr:
                    result = subprocess.run(command, cwd=output, capture_output=True, text=True)
            report.update(
                cargo_seconds=time.monotonic() - run_start,
                cargo_command=command,
                cargo_exit=result.returncode,
            )
            log = ROOT / "target" / f"horizon-{args.action}.log"
            log.write_text(result.stdout + result.stderr, encoding="utf-8")
            report["cargo_log"] = str(log)
            if result.returncode:
                print(result.stderr[-12000:], file=sys.stderr)
                print(json.dumps(report), file=sys.stderr)
                return result.returncode
            if args.action == "build":
                exe_suffix = ".exe" if os.name == "nt" else ""
                report["binary"] = str(target / f"fast/horizon-client{exe_suffix}")
        print(json.dumps(report, indent=2))
        return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            print(error.stderr, file=sys.stderr)
        print(f"Native mod: {error}", file=sys.stderr)
        raise SystemExit(1)
