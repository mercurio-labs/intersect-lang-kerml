#!/usr/bin/env python3
"""Qualify the stateful cistern against the native CLI and experimental Run engine."""

import argparse
import json
import subprocess
import tempfile
from pathlib import Path


EXAMPLE = Path(__file__).resolve().parent.parent / "docs/examples/sysml-execution-profile"
EVENTS = ("Reset", "Boost", "Boost", "Reset")


def invoke(*command, status=0):
    result = subprocess.run(command, text=True, capture_output=True, check=False)
    if result.returncode != status:
        raise AssertionError(f"{command}\nExit {result.returncode}\n{result.stdout}{result.stderr}")
    return result.stdout


def one(values):
    (value,) = values
    return value


def name(element):
    return element["values"].get("declaredName")


def project_chart(snapshot):
    """Project just the fixture's flat states and unguarded, item-triggered transitions."""
    elements = {element["id"]: element for element in snapshot["elements"]}

    def relationships(element, kind=None):
        linked = (elements[key] for key in element["values"].get("ownedRelationship", ()))
        return [relation for relation in linked if kind is None or relation["class"] == kind]

    def members(element, kind, membership=None):
        linked = (
            elements[key]
            for relation in relationships(element, membership)
            for key in relation["values"].get("ownedRelatedElement", ())
        )
        return [member for member in linked if member["class"] == kind]

    def typed(element):
        return one(relationships(element, "FeatureTyping"))["values"]["type"]

    def target(succession):
        _, end = members(succession, "ReferenceUsage", "EndFeatureMembership")
        return one(relationships(end, "ReferenceSubsetting"))["values"]["referencedFeature"]

    garden = one(
        element for element in elements.values()
        if element["class"] == "PartUsage" and name(element) == "garden"
    )
    controller = one(members(elements[typed(garden)], "ExhibitStateUsage"))
    chart = elements[typed(controller)]
    states = {state["id"]: name(state) for state in members(chart, "StateUsage")}
    initial = target(one(members(chart, "SuccessionAsUsage")))
    assert initial in states, initial
    transitions = []
    for transition in members(chart, "TransitionUsage"):
        trigger = one(members(transition, "AcceptActionUsage", "TransitionFeatureMembership"))
        assert len(relationships(transition, "TransitionFeatureMembership")) == 1, transition
        payload = one(members(trigger, "ReferenceUsage", "ParameterMembership"))
        signal = elements[typed(payload)]
        assert signal["class"] == "ItemDefinition", signal
        source = one(relationships(transition, "Membership"))["values"]["memberElement"]
        destination = target(one(members(transition, "SuccessionAsUsage")))
        assert source in states and destination in states, transition
        transitions.append((transition["id"], source, signal["id"], destination))
    signals = {
        name(element): element["id"]
        for element in elements.values()
        if element["class"] == "ItemDefinition"
    }
    return garden["id"], name(chart), states, initial, transitions, signals


def chart_source(states, initial, transitions, signals):
    quote = json.dumps
    nodes = ",\n    ".join(f"{{id: {quote(state)}, parent: None, initial: None}}" for state in states)
    edges = ",\n    ".join(
        f"{{id: {quote(edge)}, from: {quote(source)}, event: {quote(signal)}, to: {quote(target)}}}"
        for edge, source, signal, target in transitions
    )
    scenario = ", ".join(
        f"{{at: {at}, event: {quote(signals[event])}}}" for at, event in enumerate(EVENTS, 1)
    )
    return f'''rcore 1;
package experiment.sysml_cistern version "1";
pub fn engine() -> String = "intersect.state-machine@1";
pub fn chart() -> {{
  initial: String,
  states: Sequence<{{id: String, parent: Option<String>, initial: Option<String>}}>,
  transitions: Sequence<{{id: String, from: String, event: String, to: String}}>
}} = {{initial: {quote(initial)}, states: [
    {nodes}
], transitions: [
    {edges}
]}};
pub fn scenario() -> Sequence<{{at: Int, event: String}}> = [{scenario}];
'''


def verify(cli, engine, model, scratch):
    intersect, *pack_options = cli
    export = scratch / "linked"
    invoke(intersect, "export", *pack_options, "--format", "json", "--out", export, model)
    snapshot = json.loads((export / "model.json").read_text(encoding="utf-8"))
    subject, controller, states, initial, transitions, signals = project_chart(snapshot)
    profile = scratch / "chart.rcore"
    profile.write_text(chart_source(states, initial, transitions, signals), encoding="utf-8")
    trace = one(json.loads(invoke(engine, profile)))["trace"]
    assert trace["header"]["profile"]["language_profile"] == "intersect.state-machine@1", trace

    analyze = (
        intersect, "analyze", *pack_options, "--json", "--execution-profile", EXAMPLE / "language.toml"
    )
    unsupported = json.loads(
        invoke(*analyze, "--case", "StatefulCistern::Tank::fillMode", model, status=1)
    )
    assert unsupported["code"] == "unsupported" and unsupported["source"], unsupported

    rows = []
    inputs = scratch / "input.json"
    for entry in trace["entries"]:
        if entry["kind"] not in {"run_start", "step_fired", "input_rejected"}:
            continue
        state_id = one(entry["observation"]["active"])
        state = states[state_id]
        expression = f"StatefulCistern::{controller}::{state}::durationSeconds"
        evaluated = json.loads(
            invoke(intersect, "eval", *pack_options, "--json", "--expr", expression, model)
        )
        duration = one(evaluated["outcome"]["values"])["value"]
        binding = {"name": "fillTime", "quantity": {"value": duration, "unit": "s"}}
        inputs.write_text(json.dumps([binding]), encoding="utf-8")
        report = json.loads(
            invoke(*analyze, "--case", "StatefulCistern::sizeInlet", "--inputs", inputs, model)
        )
        assert report["engine"] == "intersect.scalar-affine@1", report
        solution = report["solution"]
        assert solution["subject"]["target"]["identity"] == subject, solution
        assert solution["inputs"]["fillTime"]["quantity"] == {"value": duration, "unit": "s"}, solution
        flow = solution["result"]["quantity"]
        assert flow["unit"] == "L/s", solution
        edge = entry["observation"]["transition"]
        if edge is not None:
            _, _, event, target = one(record for record in transitions if record[0] == edge)
            assert target == state_id and event == signals[EVENTS[entry["time"] - 1]], entry
        rows.append((entry["time"], entry["kind"], state, duration, flow["value"]))
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("intersect", type=Path)
    parser.add_argument("pack", type=Path)
    parser.add_argument("rcore_engine", type=Path)
    args = parser.parse_args()
    cli = (args.intersect.resolve(), "--pack", args.pack.resolve(), "--read-only")
    model = EXAMPLE / "stateful-cistern.sysml"
    with tempfile.TemporaryDirectory(prefix="sysml-cistern-") as temporary:
        rows = verify(cli, args.rcore_engine.resolve(), model, Path(temporary))
    assert rows == [
        (0, "run_start", "normal", 10, 0.5),
        (1, "input_rejected", "normal", 10, 0.5),
        (2, "step_fired", "boosted", 4, 1.25),
        (3, "input_rejected", "boosted", 4, 1.25),
        (4, "step_fired", "normal", 10, 0.5),
    ], rows
    print("tick event outcome state fill_time_s flow_L_per_s")
    for (at, outcome, state, duration, flow), event in zip(rows, ("start", *EVENTS), strict=True):
        print(at, event, outcome, state, duration, flow)


if __name__ == "__main__":
    main()
