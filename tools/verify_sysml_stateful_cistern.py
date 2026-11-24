#!/usr/bin/env python3
"""Qualify state-driven sizing through the native SysML CLI."""

import argparse
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path


EXAMPLE = Path(__file__).resolve().parent.parent / "docs/examples/sysml-execution-profile"
EVENTS = EXAMPLE / "cistern-events.json"
MODEL = EXAMPLE / "stateful-cistern.sysml"


def run(intersect, pack, model=MODEL, events=EVENTS, *, status=0):
    command = [
        intersect, "analyze", "--pack", pack, "--read-only", "--json",
        "--execution-profile", EXAMPLE / "language.toml", "--profile", "stateful",
        "--case", "StatefulCistern::sizeInlet",
    ]
    if events is not None:
        command.extend(("--events", events))
    result = subprocess.run([*command, model], text=True, capture_output=True, check=False)
    if result.returncode != status:
        raise AssertionError(f"{command}\nExit {result.returncode}\n{result.stdout}{result.stderr}")
    return json.loads(result.stdout)


def check_trace(report, model):
    assert report["profile"] == "stateful", report
    assert report["engine"] == "intersect.state-affine@1", report
    trace = report["trace"]
    source = model.read_bytes()

    def excerpt(element):
        assert element["identity"], element
        span = element["source"]
        return source[span["start"]:span["end"]]

    assert b"exhibit state fillMode" in excerpt(trace["machine"]), trace
    assert b"state def FillModes" in excerpt(trace["definition"]), trace
    rows = []
    for step in trace["steps"]:
        state = step["state_name"].split("::")[-1]
        outcome = step["outcome"]
        kind = outcome["kind"]
        result = step["solution"]["result"]["quantity"]
        duration = step["state_output"]["quantity"]["value"]
        assert result["unit"] == "L/s", step
        assert step["solution"]["inputs"]["fillTime"]["quantity"] == {"value": duration, "unit": "s"}
        assert f"state {state}".encode() in excerpt(step["state"]), step
        assert excerpt(step["state_output"]), step
        event = step["event"]
        if kind == "transition":
            assert f"transition {event.split('::')[-1].lower()} first".encode() in excerpt(outcome["transition"]), step
        elif kind == "rejected":
            assert outcome["reason"], step
        rows.append((step["at"], event, kind, state, duration, result["value"]))
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("intersect", type=Path)
    parser.add_argument("pack", type=Path)
    args = parser.parse_args()
    intersect, pack = args.intersect.resolve(), args.pack.resolve()
    original = run(intersect, pack)
    rows = check_trace(original, MODEL)
    assert rows == [
        (0, None, "start", "normal", 10, 0.5),
        (1, "StatefulCistern::Reset", "rejected", "normal", 10, 0.5),
        (2, "StatefulCistern::Boost", "transition", "boosted", 4, 1.25),
        (3, "StatefulCistern::Boost", "rejected", "boosted", 4, 1.25),
        (4, "StatefulCistern::Reset", "transition", "normal", 10, 0.5),
    ], rows
    assert original["provenance"]["events_sha256"] == hashlib.sha256(EVENTS.read_bytes()).hexdigest()
    assert run(intersect, pack) == original, "The complete trace must be repeatable"
    assert check_trace(run(intersect, pack, events=None), MODEL) == rows[:1]

    with tempfile.TemporaryDirectory(prefix="sysml-native-cistern-") as scratch:
        source = MODEL.read_text()
        modified = Path(scratch) / "cistern.sysml"
        modified.write_text(source.replace("durationSeconds : Real = 4;", "durationSeconds : Real = 5;"))
        changed = check_trace(run(intersect, pack, modified), modified)
        assert changed[2:4] == [
            (at, event, outcome, state, 5, 1.0) for at, event, outcome, state, _, _ in rows[2:4]
        ], changed

        modified.write_text(source.replace("signal : Boost then boosted", "signal : Boost if true then boosted"))
        unsupported = run(intersect, pack, modified, status=1)
        assert unsupported["phase"] == "analysis" and unsupported["source"], unsupported
        assert "guard" in unsupported["message"].lower(), unsupported

        scenario = json.loads(EVENTS.read_bytes())
        varied_events = Path(scratch) / "events.json"
        varied_events.write_text(json.dumps(list(reversed(scenario))))
        refused = run(intersect, pack, events=varied_events, status=1)
        assert refused["phase"] == "analysis" and "ordered by time" in refused["message"], refused

        varied_events.write_text(json.dumps([{**event, "at": 2} for event in scenario]))
        same_tick = check_trace(run(intersect, pack, events=varied_events), MODEL)
        assert same_tick == [rows[0], *((2, *row[1:]) for row in rows[1:])], same_tick

    print("tick event outcome state fill_time_s flow_L_per_s")
    for at, event, outcome, state, duration, flow in rows:
        print(at, event or "-", outcome, state, duration, flow)


if __name__ == "__main__":
    main()
