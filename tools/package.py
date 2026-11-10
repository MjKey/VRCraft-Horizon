#!/usr/bin/env python3
"""Package the ViveCraft VR mod as a source-only `.hnmod`, the file Horizon's registry and launcher take.

The archive holds `horizon-native.json` and `mixins/vivecraft/` (including the `horizon-vr` bundle crate's
`.rs` and `.toml` sources and `LICENSE`), with fixed dates and modes, so the same sources always give the same
bytes and the same SHA-256.
"""
import hashlib
import json
from pathlib import Path
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
GAME = "26.3"


def package():
    compat = json.loads((ROOT / "compatibility.json").read_text(encoding="utf-8"))
    manifests = sorted((ROOT / "mixins").glob("*/horizon-mixin.toml"))
    if len(manifests) != 1:
        raise SystemExit("a .hnmod holds one bundle: keep one mixins/<id>/horizon-mixin.toml")
    bundle = manifests[0].parent
    mixin = tomllib.loads(manifests[0].read_text(encoding="utf-8"))
    mod = mixin["mod"]
    if bundle.name != mod["id"]:
        raise SystemExit(f"rename mixins/{bundle.name}/ to mixins/{mod['id']}/ (the registry finds it by id)")
    if not mod["id"].replace("_", "").isalnum() or not mod["id"].islower():
        raise SystemExit("the registry takes ids of lowercase letters, digits and _ only")
    if mixin.get("compatibility", {}).get("horizon_revision") != compat["horizon_revision"]:
        raise SystemExit("horizon-mixin.toml and compatibility.json pin different Horizon revisions")

    manifest = dict(
        schema=1,
        id=mod["id"],
        version=mod["version"],
        game=compat.get("game", GAME),
        horizon=compat["horizon_revision"],
        mixins=compat["horizon_mixins_revision"],
    )
    if "hooks" in compat:
        manifest["hooks"] = compat["hooks"]

    files = {"horizon-native.json": (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode("utf-8")}
    for path in sorted(bundle.rglob("*")):
        rel_parts = path.relative_to(bundle).parts
        if any(part in ("target", ".git") for part in rel_parts):
            continue
        if path.is_file() and (
            path.suffix in (".rs", ".toml")
            # The bundle's `data/` folder ships as-is (the loader dll and docs):
            # the .hnmod reader allows any file under mixins/<id>/data/.
            or rel_parts[0] == "data"
        ):
            files[path.relative_to(ROOT).as_posix()] = path.read_bytes()

    licence = ROOT / "LICENSE"
    if licence.is_file():
        files.setdefault(f"mixins/{mod['id']}/LICENSE", licence.read_bytes())

    out = ROOT / "target" / f"{mod['id']}-{mod['version']}.hnmod"
    out.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name, content in sorted(files.items()):
            info = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            archive.writestr(info, content)

    print(
        json.dumps(
            dict(
                file=str(out),
                entries=len(files),
                size=out.stat().st_size,
                sha256=hashlib.sha256(out.read_bytes()).hexdigest(),
            ),
            indent=2,
        )
    )


if __name__ == "__main__":
    package()
