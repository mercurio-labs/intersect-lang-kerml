#!/usr/bin/env python3
"""Verify the native SysML execution profile with an identity-matched language pack."""

import argparse
import json
import subprocess
import tempfile
from pathlib import Path


FIXTURE = (
    Path(__file__).resolve().parent.parent / "docs/examples/sysml-execution-profile"
)


def run(intersect, pack, case, *, inputs=None, profile=None, manifest=None, model=None, status=0):
    command = [
        intersect,
        "analyze",
        "--pack",
        pack,
        "--read-only",
        "--execution-profile",
        str(manifest or FIXTURE / "language.toml"),
        "--case",
        f"CisternAnalysis::{case}",
        "--json",
    ]
    if inputs is not None:
        command.extend(("--inputs", str(inputs)))
    if profile is not None:
        command.extend(("--profile", profile))
    command.append(str(model or FIXTURE / "cistern.sysml"))
    completed = subprocess.run(command, text=True, capture_output=True, check=False)
    if completed.returncode != status:
        raise AssertionError(
            f"{case}: expected exit {status}, got {completed.returncode}\n"
            f"{completed.stdout}\n{completed.stderr}"
        )
    if status:
        return (completed.stdout + completed.stderr).lower()
    return json.loads(completed.stdout)


def assert_quantity(report, value, model, subject):
    assert report["engine"] == "intersect.scalar-affine@1", report
    assert report["profile"] == "sizing", report
    solution = report["solution"]
    assert solution["result_name"] == "flow", report
    assert solution["result"]["quantity"] == {"value": value, "unit": "L/s"}, report
    assert solution["inputs"]["capacity"]["quantity"] == {"value": 6, "unit": "L"}, report
    target = solution["subject"]["target"]
    assert target["identity"], report
    binding, equation = target["source"], solution["equation_source"]
    content = model.read_bytes()
    assert f"part {subject} : Tank".encode() in content[binding["start"]:binding["end"]]
    assert b"capacity == initial + 'flow' * fillTime" in content[equation["start"]:equation["end"]]
    for field in ("pack_sha256", "manifest_sha256", "library_key"):
        digest = report["provenance"][field]
        assert len(digest) == 64 and int(digest, 16) >= 0, report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("intersect", type=Path)
    parser.add_argument("pack", type=Path)
    args = parser.parse_args()
    intersect, pack = map(lambda path: str(path.resolve()), (args.intersect, args.pack))
    fixture_source = (FIXTURE / "cistern.sysml").read_text(encoding="utf-8")
    first = run(intersect, pack, "sizeInlet", inputs=FIXTURE / "four-seconds.json")
    repeated = run(intersect, pack, "sizeInlet", inputs=FIXTURE / "four-seconds.json")
    assert_quantity(first, 1.25, FIXTURE / "cistern.sysml", "garden")
    assert repeated == first, "Repeated runs must reproduce their provenance and result"

    with tempfile.TemporaryDirectory(prefix="sysml-execution-profile-") as directory:
        model = Path(directory) / "cistern.sysml"
        model.write_text(
            fixture_source.replace(
                "fillTime : DurationValue;", "fillTime : DurationValue = 10 [s];"
            ),
            encoding="utf-8",
        )
        model_bound = run(intersect, pack, "sizeSlow", profile="sizing", model=model)
        assert_quantity(model_bound, 0.5, model, "reserve")
        overwrite = run(
            intersect, pack, "sizeSlow", inputs=FIXTURE / "four-seconds.json", model=model, status=1
        )
        assert "filltime" in overwrite, overwrite

        variant = Path(directory) / "inputs.json"
        variant.write_text(
            json.dumps([{"name": "fillTime", "quantity": {"value": 4000, "unit": "ms"}}]),
            encoding="utf-8",
        )
        converted = run(intersect, pack, "sizeInlet", inputs=variant)
        assert_quantity(converted, 1.25, FIXTURE / "cistern.sysml", "garden")

        variant.write_text(
            json.dumps([{"name": "fillTime", "quantity": {"value": 4, "unit": "L"}}]),
            encoding="utf-8",
        )
        dimension = run(intersect, pack, "sizeInlet", inputs=variant, status=1)
        assert "unit" in dimension or "dimension" in dimension, dimension

        objective_model = Path(directory) / "objective.sysml"
        objective_model.write_text(
            fixture_source.replace(
                "return 'flow' : VolumeFlowRateValue;",
                "return 'flow' : VolumeFlowRateValue;\n"
                "        objective sizing { doc /* Determine the inlet flow. */ }",
            ),
            encoding="utf-8",
        )
        objective = run(
            intersect, pack, "sizeInlet", inputs=FIXTURE / "four-seconds.json",
            model=objective_model, status=1
        )
        assert "explicit sysml objectives" in objective, objective

        for multiplicity in (1, 2):
            result_model = Path(directory) / f"result-{multiplicity}.sysml"
            result_model.write_text(
                fixture_source.replace(
                    "return 'flow' : VolumeFlowRateValue;",
                    f"return 'flow' : VolumeFlowRateValue [{multiplicity}];",
                ),
                encoding="utf-8",
            )
            if multiplicity == 1:
                scalar = run(
                    intersect, pack, "sizeInlet", inputs=FIXTURE / "four-seconds.json",
                    model=result_model,
                )
                assert_quantity(scalar, 1.25, result_model, "garden")
            else:
                non_scalar = run(
                    intersect, pack, "sizeInlet", inputs=FIXTURE / "four-seconds.json",
                    model=result_model, status=1,
                )
                assert "requires multiplicity one" in non_scalar, non_scalar

        for body, diagnostic in (
            ("action extra;", "nested actions and constraints"),
            ('rep language "x" /* foreign */', "foreign analysis bodies"),
        ):
            nested_model = Path(directory) / "nested.sysml"
            nested_model.write_text(
                fixture_source.replace("assert constraint {", f"assert constraint {{ {body}"),
                encoding="utf-8",
            )
            nested = run(
                intersect, pack, "sizeInlet", inputs=FIXTURE / "four-seconds.json",
                model=nested_model, status=1,
            )
            assert diagnostic in nested, nested

        manifest = Path(directory) / "language.toml"
        manifest.write_text(
            (FIXTURE / "language.toml").read_text(encoding="utf-8").replace("@1", "@99"),
            encoding="utf-8",
        )
        unsupported = run(intersect, pack, "sizeInlet", manifest=manifest, status=2)
        assert "scalar-affine@99" in unsupported, unsupported

    missing = run(intersect, pack, "sizeInlet", status=1)
    assert "filltime" in missing, missing
    print("Verified SysML sizing, model bindings, units, provenance, and rejection controls.")


if __name__ == "__main__":
    main()
