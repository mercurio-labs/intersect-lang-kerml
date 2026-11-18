"""Reproduce the independent small textual KPAR used by the component harness."""
import json
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo


def main():
    destination = Path(__file__).resolve().parents[1] / "crates/intersect-kpar-host/tests/fixtures/motors.kpar"
    entries = {
        ".project.json": json.dumps({"name": "Motors", "version": "1.0.0", "usage": []}, separators=(",", ":")),
        ".meta.json": json.dumps({"created": "2026-11-17T00:00:00Z", "metamodel": "https://www.omg.org/spec/SysML/20250201", "index": {"Motors": "model/Motors.sysml"}}, separators=(",", ":")),
        "model/Motors.sysml": "package Motors { part def Motor; }\n",
    }
    with ZipFile(destination, "w") as archive:
        for name, text in entries.items():
            info = ZipInfo(name, (2026, 11, 17, 0, 0, 0))
            info.compress_type = ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            archive.writestr(info, text.encode())
    print(destination)


if __name__ == "__main__":
    main()
