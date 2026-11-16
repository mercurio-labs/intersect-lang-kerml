#!/usr/bin/env python3
"""Measure prebuilt commands; never build. Times are milliseconds, memory bytes."""
import argparse
import ctypes
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import sys
import tempfile
import time

BSL = ["sysml.library/Kernel Libraries/Kernel Semantic Library/Base.kerml",
       "sysml.library/Kernel Libraries/Kernel Data Type Library/ScalarValues.kerml",
       "sysml.library/Kernel Libraries/Kernel Semantic Library/Links.kerml"]
SOURCES = "fixtures/native-pilot/v1/sources"
PILOT_PROFILES = ("pilot-bsl", "pilot-library")
INTERSECT_LIBRARY_PHASES = {"setup_ms", "parse_kerml_ms", "parse_sysml_ms", "link_ms", "implicit_ms",
                            "defaults_ms", "validate_ms", "total_ms"}


def library_files(root):
    """The 36 KerML and 58 SysML library files, relative to SOURCES, sorted."""
    base = root / SOURCES
    files = sorted(path.relative_to(base).as_posix() for path in (base / "sysml.library").rglob("*")
                   if path.is_file() and path.suffix in (".kerml", ".sysml"))
    if not files:
        raise ValueError(f"no library files under {base / 'sysml.library'}")
    return files


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def stats(values):
    return dict(median=statistics.median(values), min=min(values), max=max(values))


def parse_phases(text, profile):
    if profile != "intersect-bsl":
        # Allow a JSON report with surrounding progress lines.
        decoder = json.JSONDecoder()
        for match in re.finditer(r"\{", text):
            try:
                report, _ = decoder.raw_decode(text[match.start():])
            except ValueError:
                continue
            if isinstance(report, dict) and "timing" in report:
                phases = report["timing"]
                break
        else:
            raise ValueError("no JSON timing object found")
    else:
        patterns = {
            "preparation_ms": r"KerML definition preparation ([\d.]+) ms",
            "redefinitions_ms": r"read \d+ redefinitions ([\d.]+) ms",
            "freeze_ms": r"freeze metamodel ([\d.]+) ms",
            "memory_parse_ms": r"parse into MemoryStore: ([\d.]+) ms",
            "frozen_parse_ms": r"parse into FrozenStore: ([\d.]+) ms",
            "scopes_ms": r"(?:load saved|compile) \d+ Rcore scopes and \d+ defaults ([\d.]+) ms",
            "link_ms": r"^link ([\d.]+) ms",
            "defaults_ms": r"^defaults ([\d.]+) ms",
            "validate_ms": r"^validate ([\d.]+) ms",
        }
        phases = {}
        for name, pattern in patterns.items():
            match = re.search(pattern, text, re.MULTILINE)
            if not match:
                raise ValueError(f"missing phase {name}")
            phases[name] = float(match[1])
    if not isinstance(phases, dict) or not phases:
        raise ValueError("timing must be a nonempty object")
    for key, value in phases.items():
        if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0:
            raise ValueError(f"invalid timing {key}: {value!r}")
    if profile in PILOT_PROFILES:
        required = {"setup_ms", "cold_parse_ms", "cold_resolve_ms", "warm_parse_ms", "warm_resolve_ms"}
        if not required <= phases.keys():
            raise ValueError("missing Pilot cold/warm timings")
    if profile == "intersect-library" and not INTERSECT_LIBRARY_PHASES <= phases.keys():
        raise ValueError("missing Intersect library timings")
    return phases


class MemoryMeter:
    """Direct-child peak only. Windows counters retain the lifetime peak."""
    def __init__(self, process):
        self.handle = None
        try:
            import psutil
            self.process = psutil.Process(process.pid)
            self.backend = "psutil peak_wset" if os.name == "nt" else "psutil sampled rss"
        except ImportError:
            if os.name != "nt":
                raise RuntimeError("install psutil to measure memory on this OS")
            from ctypes import wintypes
            self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
            self.kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
            self.kernel.OpenProcess.restype = wintypes.HANDLE
            self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
            self.handle = self.kernel.OpenProcess(0x0400 | 0x0010, False, process.pid)
            if not self.handle:
                raise ctypes.WinError(ctypes.get_last_error())
            class Counters(ctypes.Structure):
                _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [(name, ctypes.c_size_t) for name in (
                    "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage",
                    "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage")]
            self.counters = Counters()
            self.counters.cb = ctypes.sizeof(Counters)
            self.query = ctypes.WinDLL("psapi", use_last_error=True).GetProcessMemoryInfo
            self.query.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
            self.query.restype = wintypes.BOOL
            self.backend = "GetProcessMemoryInfo PeakWorkingSetSize"

    def read(self):
        if self.handle:
            if not self.query(self.handle, ctypes.byref(self.counters), ctypes.sizeof(self.counters)):
                raise ctypes.WinError(ctypes.get_last_error())
            return self.counters.PeakWorkingSetSize
        info = self.process.memory_info()
        return info.peak_wset if os.name == "nt" else info.rss

    def close(self):
        if self.handle:
            self.kernel.CloseHandle(self.handle)


def run_once(command, cwd, profile, timing_file=None):
    # Files avoid pipe deadlock even when a child prints a large model.
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        start = time.perf_counter()
        child = subprocess.Popen(command, cwd=cwd, stdout=stdout, stderr=stderr)
        meter = None
        peak = 0
        try:
            meter = MemoryMeter(child)
            while True:
                try:
                    peak = max(peak, meter.read())
                except Exception:
                    # A short-lived psutil child can disappear between polls.
                    # Keep measured samples, but never hide a live-process error.
                    if child.poll() is None or not peak:
                        raise
                    break
                try:
                    child.wait(timeout=0.01)
                    break
                except subprocess.TimeoutExpired:
                    pass
            # Windows native handle remains valid after exit; psutil may not.
            if meter.handle:
                peak = max(peak, meter.read())
            elapsed = (time.perf_counter() - start) * 1000
        finally:
            if child.poll() is None:
                child.kill()
            child.wait()
            if meter:
                meter.close()
        stdout.seek(0)
        stderr.seek(0)
        output = stdout.read().decode("utf-8", errors="replace")
        errors = stderr.read().decode("utf-8", errors="replace")
        if child.returncode:
            raise RuntimeError(f"child exited {child.returncode}: {errors}\n{output}")
        text = Path(timing_file).read_text(encoding="utf-8") if timing_file else output
        return dict(wall_ms=elapsed, peak_working_set_bytes=peak,
                    phases=parse_phases(text, profile), memory_backend=meter.backend,
                    stdout=output, stderr=errors)


def summarize(runs):
    keys = runs[0]["phases"].keys()
    if any(run["phases"].keys() != keys for run in runs):
        raise ValueError("phase set changed between runs")
    result = {key: stats([run[key] for run in runs]) for key in ("wall_ms", "peak_working_set_bytes")}
    result.update({key: stats([run["phases"][key] for run in runs]) for key in keys})
    return result


def markdown(report):
    lines = ["| Profile | Metric | Median | Min | Max |", "| --- | --- | ---: | ---: | ---: |"]
    for name, profile in report["profiles"].items():
        for metric, values in profile["summary"].items():
            lines.append(f"| {name} | {metric} | {values['median']:.3f} | {values['min']:.3f} | {values['max']:.3f} |")
    return "\n".join(lines) + "\n"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", action="append", choices=["pilot-bsl", "intersect-bsl", "pilot-library",
                                                                  "intersect-library", "command"])
    parser.add_argument("-n", "--runs", type=int, default=5)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--pilot-jar", type=Path)
    parser.add_argument("--pilot-classes", type=Path)
    parser.add_argument("--intersect-binary", type=Path, help="frozen_kerml_load (intersect-bsl)")
    parser.add_argument("--intersect-library-binary", type=Path, help="frozen_library_load (intersect-library)")
    parser.add_argument("--java", default="java")
    parser.add_argument("--java-option", action="append", default=[], dest="java_options",
                        help="JVM option for the Pilot profiles, as --java-option=-Xmx12g (repeatable)")
    parser.add_argument("--output", type=Path, default=Path("target/perf-vs-pilot"))
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    if args.runs < 1:
        parser.error("runs must be positive")
    root = args.root.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    profiles = args.profile or (["command"] if args.command else ["pilot-bsl", "intersect-bsl"])
    jar = (args.pilot_jar or root / "target/pilot-oracle/kernel/sysml/jupyter-sysml-kernel-0.62.0-all.jar").resolve()
    classes = (args.pilot_classes or root / "target/pilot-oracle/classes").resolve()
    exe = ".exe" if os.name == "nt" else ""
    binary = (args.intersect_binary or root / f"target/release/examples/frozen_kerml_load{exe}").resolve()
    library_binary = (args.intersect_library_binary or root / f"target/release/examples/frozen_library_load{exe}").resolve()
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    report = dict(machine=platform.node(), cpu=platform.processor(), os=platform.platform(),
                  git_revision=revision, cwd=str(root), runs=args.runs,
                  harness_command=[sys.executable, str(Path(__file__).resolve()), *(sys.argv[1:] if argv is None else argv)],
                  pilot_jar_sha256=None, profiles={})
    if any(profile in PILOT_PROFILES for profile in profiles):
        report["pilot_jar_sha256"] = sha256(jar)
        if not (classes / "PilotOracle.class").is_file():
            parser.error("PilotOracle.class must be prebuilt")
    for profile in profiles:
        timing_file = None
        if profile in PILOT_PROFILES:
            timing_file = output / f"{profile}-timing.json"
            files = BSL if profile == "pilot-bsl" else library_files(root)
            command = [args.java, *args.java_options, "-cp", os.pathsep.join([str(jar), str(classes)]),
                       "PilotOracle", str(timing_file), str(root / SOURCES), *files]
        elif profile == "intersect-library":
            if not library_binary.is_file():
                parser.error("frozen_library_load release binary must be prebuilt")
            report["intersect_library_binary_sha256"] = sha256(library_binary)
            command = [str(library_binary), str(root)]
        elif profile == "intersect-bsl":
            if not binary.is_file():
                parser.error("frozen_kerml_load release binary must be prebuilt")
            report["intersect_binary_sha256"] = sha256(binary)
            command = [str(binary), str(root)]
        else:
            command = args.command[1:] if args.command[:1] == ["--"] else args.command
            if not command:
                parser.error("command profile requires a command after --")
        runs = []
        for _ in range(args.runs):
            if timing_file:
                timing_file.unlink(missing_ok=True)
            runs.append(run_once(command, root, profile, timing_file))
        report["profiles"][profile] = dict(command=command, runs=runs, summary=summarize(runs))
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    table = markdown(report)
    (output / "report.md").write_text(table, encoding="utf-8")
    print(table, end="")
    return report


if __name__ == "__main__":
    main()
