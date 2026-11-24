#!/usr/bin/env python3
"""Qualify the native thermostat against an analytic and optional Rcore reference."""

import argparse
import json
import math
import subprocess
import tempfile
from pathlib import Path


EXAMPLE = Path(__file__).resolve().parent.parent / "docs/examples/sysml-execution-profile"
MODEL = EXAMPLE / "thermostat.sysml"
MANIFEST = EXAMPLE / "language.toml"


def run(intersect, pack, model=MODEL, manifest=MANIFEST, *, status=0):
    command = [
        intersect, "analyze", "--pack", pack, "--read-only", "--json",
        "--execution-profile", manifest, "--profile", "thermostat",
        "--case", "Thermostat::temperatureRate", model,
    ]
    result = subprocess.run(command, text=True, capture_output=True, check=False)
    if result.returncode != status:
        raise AssertionError(f"{command}\nExit {result.returncode}\n{result.stdout}{result.stderr}")
    return json.loads(result.stdout)


def mode(step):
    return step["state_name"].split("::")[-1].lower()


def qualify(report, model, power, crossing_count, first_tick, reference):
    assert report["engine"] == "intersect.coupled-ode@1", report
    trace = report["trace"]
    source = model.read_bytes()
    assert trace["case"] == "Thermostat::temperatureRate", trace["case"]
    assert trace["initial"]["quantity"]["value"] == 20
    equation = trace["equation_source"]
    assert b"capacityJPerC * rateCPerSecond" in source[equation["start"]:equation["end"]]
    assert [
        (monitor["level"]["quantity"]["value"], monitor["event_name"], monitor["rising"])
        for monitor in trace["crossings"]
    ] == [(40, "Thermostat::TooHot", True), (38, "Thermostat::TooCool", False)]
    assert all(monitor["level"]["identity"] and monitor["event"]["identity"] for monitor in trace["crossings"])

    steps = trace["steps"]
    samples = [step for step in steps if step["outcome"]["kind"] == "sample"]
    crossings = [step for step in steps if step["outcome"]["kind"] == "crossing"]
    assert [step["at"] for step in samples] == list(range(0, 1_800_001, 15_000))
    assert len(crossings) == crossing_count, len(crossings)
    assert crossings[0]["at"] == first_tick, crossings[0]

    anchor_tick, anchor_value, active = 0, 20.0, "heating"
    worst = 0.0
    for step in steps:
        seconds = (step["at"] - anchor_tick) / 1_000
        resting = 20.0 + (power / 5.0 if active == "heating" else 0.0)
        analytic = resting + (anchor_value - resting) * math.exp(-seconds / 240.0)
        assert math.isclose(step["value"], analytic, rel_tol=0, abs_tol=2e-6), (step, analytic)
        if step["outcome"]["kind"] == "sample":
            assert mode(step) == active, step
            assert step["state_output"]["quantity"]["value"] == (power if active == "heating" else 0), step
            continue
        heating = active == "heating"
        level, event = (40.0, "TooHot") if heating else (38.0, "TooCool")
        expected = -240.0 * math.log((level - resting) / (anchor_value - resting))
        worst = max(worst, abs(seconds - expected))
        assert abs(seconds - expected) <= 0.001, (step, seconds, expected)
        assert step["outcome"]["event"] == f"Thermostat::{event}", step
        transition = step["outcome"]["transition"]
        span = transition["source"]
        keyword = b"transition shutoff" if heating else b"transition restart"
        assert keyword in source[span["start"]:span["end"]], transition
        active = "idle" if heating else "heating"
        assert mode(step) == active, step
        anchor_tick, anchor_value = step["at"], step["value"]

    if reference is not None:
        expected_rows = []
        for entry in reference["trace"]["entries"]:
            observation = entry.get("observation") or {}
            if not (observation.get("crossed") or observation.get("sample")):
                continue
            row = (entry["time"], observation["mode"].lower(), observation["values"]["temperatureC"])
            expected_rows.extend(("crossing", *row) for _ in observation["crossed"])
            if observation["sample"]:
                expected_rows.append(("sample", *row))
        actual_rows = [
            (step["outcome"]["kind"], step["at"], mode(step), step["value"]) for step in steps
        ]
        assert len(actual_rows) == len(expected_rows)
        for actual, expected in zip(actual_rows, expected_rows, strict=True):
            assert actual[:3] == expected[:3], (actual, expected)
            assert math.isclose(actual[3], expected[3], rel_tol=0, abs_tol=5e-8), (actual, expected)

    print(f"{power} W: {len(samples)} samples, {crossing_count} crossings, "
          f"first at {first_tick / 1_000:.3f} s, worst interval error {worst * 1_000:.6f} ms")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("intersect", type=Path)
    parser.add_argument("pack", type=Path)
    parser.add_argument("--reference", type=Path, help="Rcore thermostat native trace JSON")
    args = parser.parse_args()
    intersect, pack = args.intersect.resolve(), args.pack.resolve()
    reference = {
        case["case"]: case for case in json.loads(args.reference.read_bytes())
    } if args.reference else None
    source = MODEL.read_text()
    with tempfile.TemporaryDirectory(prefix="sysml-native-thermostat-") as scratch:
        alternate = Path(scratch) / "thermostat.sysml"
        alternate.write_text(source.replace("heaterPowerW : Real = 120.0;", "heaterPowerW : Real = 250.0;"))
        baseline = run(intersect, pack)
        qualify(baseline, MODEL, 120, 23, 430_023, reference["baseline"] if reference is not None else None)
        qualify(
            run(intersect, pack, alternate), alternate, 250, 83, 122_599,
            reference["current"] if reference is not None else None,
        )
        if reference is not None:
            print("All sample and crossing observations match the Rcore reference")
        assert run(intersect, pack) == baseline, "The complete trace must be repeatable"

        alternate.write_text(
            source.replace("shutoffTemperatureC : Real = 40.0;", "shutoffTemperatureC : Real = 35.0;")
            .replace("restartTemperatureC : Real = 38.0;", "restartTemperatureC : Real = 18.0;")
            .replace("lossWPerC : Real = 5.0;", "lossWPerC : Real = 0.0;")
            .replace("capacityJPerC : Real = 1200.0;", "capacityJPerC : Real = 120.0;")
        )
        manifest = Path(scratch) / "language.toml"
        controls = MANIFEST.read_text().replace("end_ms = 1800000", "end_ms = 30000")
        manifest.write_text(controls)
        trace = run(intersect, pack, alternate, manifest)["trace"]["steps"]
        at_crossing = [(step["outcome"]["kind"], mode(step), step["value"]) for step in trace if step["at"] == 15_000]
        assert at_crossing == [("crossing", "idle", 35.0), ("sample", "idle", 35.0)], at_crossing

        manifest.write_text(controls.replace("sample_ms = 15000", "sample_ms = 0"))
        refused = run(intersect, pack, alternate, manifest, status=1)
        assert refused["phase"] == "analysis" and refused["source"], refused
        print("Same-tick transition precedes the regular sample")

        manifest.write_text(controls.replace("event = \"Thermostat::TooHot\"", "event = \"Thermostat::Unbound\""))
        unbound = run(intersect, pack, alternate, manifest, status=1)
        assert unbound["phase"] == "analysis" and unbound["code"] == "unbound" and unbound["source"], unbound


if __name__ == "__main__":
    main()
