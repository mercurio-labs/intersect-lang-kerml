"""Collect milestone-only Pilot reports in independent JVMs, never production.

Every group receives a fresh parser/resource set/library/validator/evaluator.
Workers are bounded and all children are reaped before this program returns.
Only complete successful reports enter the manifest. Existing outputs are
never overwritten; failed logs and return codes remain available for diagnosis.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time


def collect(args, group):
    group_id = group["id"]
    if not re.fullmatch(r"[a-z0-9-]+", group_id):
        raise ValueError("Invalid group id")
    model = args.out / f"{group_id}-model.json"
    report = args.out / f"{group_id}-eval.json"
    log = args.out / f"{group_id}.log"
    if any(p.exists() for p in (model, report, log)):
        raise ValueError(f"Refusing to overwrite {group_id}")
    command = [str(args.java), "-Xmx6g", "-cp", args.classpath,
               "PilotEvalOracle", "--model-out", str(model), str(report),
               str(args.library), str(args.root), *group["files"]]
    start = time.monotonic()
    with log.open("w", encoding="utf-8") as stream:
        process = subprocess.Popen(command, stdout=stream, stderr=subprocess.STDOUT)
        print(f"Started {group_id}: PID {process.pid}", flush=True)
        try:
            code = process.wait()
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait()
    result = {"group": group_id, "files": group["files"], "command": command,
              "exit": code, "seconds": time.monotonic() - start, "log": str(log)}
    if code == 0:
        data = json.loads(report.read_text(encoding="utf-8"))
        if "fresh_eval" in group:
            previous = json.loads(Path(group["fresh_eval"]).read_text(encoding="utf-8"))
            for key in ("resources", "diagnostics", "sites", "observation_isolation"):
                if data[key] != previous[key]:
                    raise ValueError(f"Fresh-load comparison changed {group_id}: {key}")
            result["fresh_load_equivalence"] = True
        result.update(sites=len(data["sites"]), diagnostics=len(data["diagnostics"]),
                      eval_sha256=hashlib.sha256(report.read_bytes()).hexdigest(),
                      model_sha256=hashlib.sha256(model.read_bytes()).hexdigest())
    print(f"Finished {group_id}: exit={code}, seconds={result['seconds']:.1f}", flush=True)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java", type=Path, required=True)
    parser.add_argument("--classpath", required=True)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--jobs", type=int, choices=range(1, 4), default=1)
    args = parser.parse_args()
    args.out = args.out.resolve()
    args.library = args.library.resolve()
    args.root = args.root.resolve()
    args.out.mkdir(parents=True, exist_ok=True)
    groups = json.loads(args.manifest.read_text(encoding="utf-8"))["groups"]
    if len({g["id"] for g in groups}) != len(groups):
        raise ValueError("Duplicate group id")
    records = []
    failures = []
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        futures = {pool.submit(collect, args, group): group["id"] for group in groups}
        for future in as_completed(futures):
            try:
                records.append(future.result())
            except Exception as error:
                failures.append({"group": futures[future], "error": str(error)})
            (args.out / "manifest.json").write_text(json.dumps({
                "strategy": "independent fresh JVM per group; isolated observations",
                "groups": records, "collection_failures": failures}, indent=2) + "\n", encoding="utf-8")
    print("All owned verification processes have completed", flush=True)
    return int(bool(failures) or any(row["exit"] != 0 for row in records))


if __name__ == "__main__":
    raise SystemExit(main())
