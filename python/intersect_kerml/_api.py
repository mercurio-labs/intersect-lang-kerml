"""Handwritten Python convenience API over the native language SDK."""
from __future__ import annotations

import base64
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import threading
from types import MappingProxyType
from typing import Iterator, Mapping, TypeVar, cast
from urllib.parse import quote
import uuid


class IntersectError(RuntimeError):
    """A native SDK or project operation failed."""


class CompatibilityError(IntersectError):
    pass


class InvalidModelError(IntersectError):
    def __init__(self, diagnostics: tuple[Diagnostic, ...]):
        self.diagnostics = diagnostics
        super().__init__("Analysis has no accepted model: " + "; ".join(d.message for d in diagnostics))


class ConflictError(IntersectError):
    pass


class UnsupportedError(IntersectError):
    pass


class ClosedError(IntersectError):
    pass


@dataclass(frozen=True)
class SourceLocation:
    uri: str | None
    start: int | None
    end: int | None
    # Positions are engine UTF-8 byte offsets, with an exclusive end.


@dataclass(frozen=True)
class Diagnostic:
    severity: str
    message: str
    location: SourceLocation
    code: str | None = None
    phase: str | None = None
    rule: str | None = None
    feature: str | None = None
    element_id: str | None = None


def _freeze(value: object) -> object:
    if isinstance(value, dict):
        return MappingProxyType({k: _freeze(v) for k, v in value.items()})
    if isinstance(value, list):
        return tuple(_freeze(v) for v in value)
    return value


def _digest(data: bytes | None) -> str | None:
    return None if data is None else hashlib.sha256(data).hexdigest()


def _is_link(path: Path) -> bool:
    try:
        info = path.lstat()
    except FileNotFoundError:
        return False
    return stat.S_ISLNK(info.st_mode) or getattr(info, "st_reparse_tag", 0) == getattr(stat, "IO_REPARSE_TAG_MOUNT_POINT", -1)


def _relative(name: str) -> str:
    path = PurePosixPath(name.replace("\\", "/"))
    if (not path.parts or path.is_absolute() or any(p in ("..", ".git") or ":" in p for p in path.parts)
            or path.suffix not in (".sysml", ".kerml")):
        raise ValueError("Source must be a relative .sysml or .kerml path inside the project")
    return path.as_posix()


class Engine:
    """Own a native engine. External packs require explicit trust in their producer."""

    def __init__(self, pack: str | os.PathLike[str] | None = None, *, trusted: bool = False):
        from . import _native, _generated
        resource = Path(__file__).parent
        manifest_path = resource / "bindings.json"
        if not manifest_path.is_file():
            raise CompatibilityError("Generated bindings are missing; build with --python-out")
        self._manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        if self._manifest.get("runtime_api") != 1:
            raise CompatibilityError("Unsupported Python runtime API version")
        if self._manifest["schema_fingerprint"] != _generated.SCHEMA_FINGERPRINT:
            raise CompatibilityError("Generated Python and binding manifest disagree")
        if pack is None:
            pack = resource / "resources" / "sysml.pack"
        elif not trusted:
            raise CompatibilityError("External pack requires trusted=True and a trusted matching producer")
        pack = Path(pack)
        if not pack.is_file():
            raise CompatibilityError("Prepared pack is missing; build a complete wheel with --library")
        if pack.stat().st_size > 512 * 1024 * 1024:
            raise CompatibilityError("Pack exceeds 512 MiB")
        with pack.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        if digest != self._manifest["pack_sha256"]:
            raise CompatibilityError("Pack digest does not match this binding build")
        self._closed = False
        try:
            self._native = _native.Engine(str(pack.resolve()))
            actual = self._call("manifest")
            for key in ("runtime_api", "generator", "schema_fingerprint", "compiler", "pack_sha256", "pack_recipe", "pack_parts"):
                if actual.get(key) != self._manifest.get(key):
                    raise CompatibilityError(f"Native runtime and Python bindings disagree on {key}")
        except Exception:
            if hasattr(self, "_native"):
                self._native.close()
            self._closed = True
            raise

    @property
    def capabilities(self) -> Mapping[str, object]:
        return cast(Mapping[str, object], _freeze(self._manifest["capabilities"]))

    def _call(self, method: str, **params):
        if self._closed:
            raise ClosedError("Engine is closed")
        try:
            return json.loads(self._native.request(method, json.dumps(params, ensure_ascii=False)))
        except RuntimeError as error:
            raise IntersectError(str(error)) from error

    def open(self, folder: str | os.PathLike[str]) -> Project:
        return Project(self, Path(folder).resolve())

    def new_project(self) -> Project:
        return Project(self, None)

    def close(self) -> None:
        if not self._closed:
            self._closed = True
            self._native.close()

    def __enter__(self) -> Engine:
        return self

    def __exit__(self, *_exc) -> None:
        self.close()


_T = TypeVar("_T", bound="Element")


class Element:
    """Read-only element bound to one immutable native snapshot."""
    __slots__ = ("_model", "_id")
    _class_key = ""

    def __init__(self, model: Model | None = None, identity: str | None = None):
        if model is None or identity is None:
            raise UnsupportedError("Elements come from a model; author sources through a transaction")
        # An explicit handle constructor wraps an existing validated element;
        # it never creates a native object or changes its metaclass.
        model._call("inspect", id=identity)
        if self._class_key and not model._call("is_instance", id=identity, **{"class": self._class_key}):
            raise TypeError("Element does not conform to this generated wrapper class")
        self._model, self._id = model, identity

    @property
    def id(self) -> str:
        self._model._check()
        return self._id

    @property
    def metaclass(self) -> str:
        return self._model._call("inspect", id=self._id)["class"]

    @property
    def source_location(self) -> SourceLocation:
        data = self._model._call("inspect", id=self._id)
        return SourceLocation(data["uri"], data["start"], data["end"])

    @property
    def container(self) -> Element | None:
        data = self._model._call("inspect", id=self._id)["container"]
        return None if data is None else self._model.element(data["id"])

    def is_instance(self, metaclass: type[Element] | str) -> bool:
        return self._model._call("is_instance", id=self._id, **{"class": _class_key(metaclass)})

    @property
    def children(self) -> tuple[Element, ...]:
        return tuple(self._model.element(identity) for identity in self._model._call("children", id=self._id))

    def get(self, feature: str, *, raw: bool = False) -> object:
        self._model._check()
        description = self._model._engine._manifest["classes"][self.metaclass]["features"].get(feature)
        if description is None:
            raise AttributeError(feature)
        if description["unsupported"] and (not raw or description["feature"]["derived"] or description["feature"].get("volatile")):
            raise UnsupportedError(description["unsupported"])
        data = self._model._call("get", id=self._id, feature=feature)
        def decode(value):
            if value is None:
                return None
            if data["reference"]:
                return self._model.element(value)
            if raw:
                return _freeze(value)
            enums = self._model._engine._manifest["enums"]
            if data["target"] in enums:
                from . import _generated
                enum = getattr(_generated, enums[data["target"]]["python"])
                for member in enum:
                    if json.loads(member.value) == value:
                        return member
                raise IntersectError("Native enum value has no matching generated literal")
            return _freeze(value)
        return tuple(decode(v) for v in data["value"]) if data["many"] else decode(data["value"])

    def stored_properties(self, *, raw: bool = False) -> Mapping[str, object]:
        names = self._model._call("properties", id=self._id)
        # Unsupported codecs remain explicit when using reflection as well.
        return MappingProxyType({name: self.get(name, raw=raw) for name in names})


def _class_key(value: type[Element] | str | None) -> str | None:
    if value is None or isinstance(value, str):
        return value
    if not isinstance(value, type) or not issubclass(value, Element):
        raise TypeError("Expected a generated Element class or a metaclass name")
    return value._class_key


class Model:
    def __init__(self, engine: Engine, snapshot: int):
        self._engine, self._snapshot = engine, snapshot
        self._closed = False

    def _check(self) -> None:
        if self._closed or self._engine._closed:
            raise ClosedError("Model snapshot or engine is closed")

    def _call(self, method: str, **params):
        self._check()
        return self._engine._call(method, snapshot=self._snapshot, **params)

    def element(self, identity: str) -> Element:
        from ._generated import REGISTRY
        meta = self._call("inspect", id=identity)
        element = object.__new__(REGISTRY[meta["class"]])
        element._model, element._id = self, identity
        return element

    def elements(self, metaclass: type[_T] | str | None = None, *, include_libraries: bool = False) -> Iterator[_T]:
        offset = 0
        while True:
            ids = self._call("elements", **{"class": _class_key(metaclass)}, include_libraries=include_libraries,
                             offset=offset, limit=256)
            for identity in ids:
                yield cast(_T, self.element(identity))
            offset += len(ids)
            if len(ids) < 256:
                return

    def close(self) -> None:
        if not self._closed:
            if not self._engine._closed:
                self._call("release")
            self._closed = True

    def __del__(self):
        try:
            self.close()
        except Exception:
            pass  # Interpreter shutdown must not raise from a finalizer.

    def __enter__(self) -> Model:
        self._check()
        return self

    def __exit__(self, *_exc) -> None:
        self.close()


@dataclass(frozen=True)
class Analysis:
    diagnostics: tuple[Diagnostic, ...]
    model: Model | None
    clean: bool
    notes: tuple[str, ...]

    def require_model(self) -> Model:
        if self.model is None:
            raise InvalidModelError(self.diagnostics)
        self.model._check()
        return self.model


class Project:
    def __init__(self, engine: Engine, folder: Path | None):
        folder = None if folder is None else Path(folder).resolve()
        self._engine, self._folder = engine, folder
        self._sources: dict[str, str] = {}
        self._baseline: dict[str, bytes] = {}
        self._descriptor: str | None = None
        self._archives: list[dict[str, str]] = []
        self._revision = 0
        self._lock = threading.RLock()
        if folder is not None:
            if not folder.is_dir() or folder.is_symlink():
                raise ValueError("Project root must be a folder")
            excluded = {".git", "target", "node_modules", "dist", ".venv", "__pycache__", ".worktrees"}
            count = 0
            source_bytes = archive_bytes = 0
            for directory, dirs, files in os.walk(folder, followlinks=False):
                count += len(dirs) + len(files)
                if count > 4096:
                    raise IntersectError("Project exceeds 4096 folder entries")
                dirs[:] = sorted(d for d in dirs if d not in excluded and not d.startswith(".intersect-save-")
                                 and not _is_link(Path(directory, d))
                                 and Path(directory, d).resolve().is_relative_to(folder))
                for filename in sorted(files):
                    path = Path(directory, filename)
                    if _is_link(path) or not path.resolve().is_relative_to(folder):
                        continue
                    name = path.relative_to(folder).as_posix()
                    if path.suffix not in (".sysml", ".kerml", ".kpar") and name != ".project.json":
                        continue
                    if path.suffix in (".sysml", ".kerml"):
                        if len(self._sources) >= 256:
                            raise IntersectError("Project exceeds 256 sources")
                        remaining = 8 * 1024 * 1024 - source_bytes
                    elif path.suffix == ".kpar":
                        if len(self._archives) >= 64:
                            raise IntersectError("Project exceeds 64 dependency archives")
                        remaining = 8 * 1024 * 1024 - archive_bytes
                    else:
                        remaining = 1024 * 1024
                    if path.stat().st_size > remaining:
                        raise IntersectError(f"Project input budget exceeded: {name}")
                    with path.open("rb") as stream:
                        data = stream.read(remaining + 1)
                    if len(data) > remaining:
                        raise IntersectError(f"Project input grew beyond its budget: {name}")
                    self._baseline[name] = data
                    if path.suffix in (".sysml", ".kerml"):
                        source_bytes += len(data)
                        self._sources[name] = data.decode("utf-8-sig")
                    elif name == ".project.json":
                        self._descriptor = data.decode("utf-8-sig")
                    else:
                        archive_bytes += len(data)
                        self._archives.append({"uri": quote(name, safe="/"), "base64": base64.b64encode(data).decode("ascii")})

    @property
    def sources(self) -> Mapping[str, str]:
        with self._lock:
            return MappingProxyType(dict(self._sources))

    def _analyze(self, sources: dict[str, str]) -> Analysis:
        if len(sources) > 256 or sum(len(text.encode("utf-8")) for text in sources.values()) > 8 * 1024 * 1024:
            raise IntersectError("Project exceeds 256 sources or 8 MiB of text")
        data = self._engine._call("analyze", sources={quote(name, safe="/"): text for name, text in sources.items()},
                                  descriptor=self._descriptor, archives=self._archives)
        diagnostics = tuple(Diagnostic(d["severity"].lower(), d["message"],
            SourceLocation(d.get("resource"), d.get("start"), d.get("end")), d.get("code"), d.get("phase"),
            d.get("rule"), d.get("feature"), d.get("object"))
            for d in data["diagnostics"])
        model = None if data["snapshot"] is None else Model(self._engine, data["snapshot"])
        return Analysis(diagnostics, model, data["clean"], tuple(data["notes"]))

    def analyze(self) -> Analysis:
        with self._lock:
            return self._analyze(dict(self._sources))

    def transaction(self) -> Transaction:
        with self._lock:
            return Transaction(self, self._revision, dict(self._sources))

    def save(self) -> None:
        with self._lock:
            if self._folder is None:
                raise ValueError("Use save_as for an in-memory project")
            folder = self._folder
            desired = dict(self._baseline)
            for name in tuple(desired):
                if Path(name).suffix in (".sysml", ".kerml") and name not in self._sources:
                    del desired[name]
            for name, text in self._sources.items():
                original = self._baseline.get(name)
                desired[name] = original if original is not None and original.decode("utf-8-sig") == text else text.encode("utf-8")
            names = set(self._baseline) | set(desired)
            def checked(name):
                path = folder / name
                if path.resolve().parent != folder and not path.resolve().is_relative_to(folder):
                    raise ConflictError(f"Path leaves project: {name}")
                if _is_link(path) or any(_is_link(parent) for parent in path.parents if parent != folder.parent):
                    raise ConflictError(f"Symbolic link in save path: {name}")
                return path
            def unchanged(path, expected):
                if expected is None:
                    return not path.exists()
                if not path.exists() or path.stat().st_size != len(expected):
                    return False
                with path.open("rb") as stream:
                    return stream.read(len(expected) + 1) == expected
            for name in names:
                path = checked(name)
                if not unchanged(path, self._baseline.get(name)):
                    raise ConflictError(f"External change: {name}")
            changed = {name for name in names if self._baseline.get(name) != desired.get(name)}
            if not changed:
                return
            stage = folder / (".intersect-save-" + uuid.uuid4().hex)
            stage.mkdir()
            for name in changed:
                for section, data in (("old", self._baseline.get(name)), ("new", desired.get(name))):
                    if data is not None:
                        path = stage / section / name
                        path.parent.mkdir(parents=True, exist_ok=True)
                        path.write_bytes(data)
            (stage / "journal.json").write_text(json.dumps({"format": 1, "files": {
                name: {"old": _digest(self._baseline.get(name)), "new": _digest(desired.get(name))}
                for name in sorted(changed)}}, indent=2), encoding="utf-8")
            try:
                for name in sorted(changed):
                    path = checked(name)
                    if not unchanged(path, self._baseline.get(name)):
                        raise ConflictError(f"External change during save: {name}")
                    path.parent.mkdir(parents=True, exist_ok=True)
                    if name in desired:
                        os.replace(stage / "new" / name, path)
                    else:
                        path.unlink()
            except Exception as error:
                raise ConflictError(f"Save interrupted; recovery files and journal remain at {stage}: {error}") from error
            self._baseline = desired
            # The fresh private staging directory is verified before recursive cleanup.
            if stage.resolve().parent == folder and stage.name.startswith(".intersect-save-"):
                shutil.rmtree(stage)

    def save_as(self, folder: str | os.PathLike[str]) -> None:
        with self._lock:
            target = Path(folder).absolute()
            if target.exists():
                raise ConflictError("save_as requires a new destination folder")
            if self._folder is not None and target.resolve().is_relative_to(self._folder):
                raise ConflictError("Destination must be outside the original project")
            target.mkdir(parents=True)
            # Preserve documents and sidecars from a folder project. Skip tooling,
            # links and recovery directories, and never copy the destination into itself.
            if self._folder is not None:
                def ignore(directory, names):
                    return [name for name in names if name in {".git", "target", "node_modules", "dist", ".venv", ".worktrees"}
                            or name.startswith(".intersect-save-") or _is_link(Path(directory, name))]
                shutil.copytree(self._folder, target, dirs_exist_ok=True, ignore=ignore)
            self._folder = target.resolve()
            self.save()


class Transaction:
    def __init__(self, project: Project, revision: int, sources: dict[str, str]):
        self._project, self._revision, self._sources = project, revision, sources
        self._analysis: Analysis | None = None
        self._closed = False

    def _check(self):
        if self._closed:
            raise ClosedError("Transaction is closed")

    def put_source(self, path: str, text: str) -> None:
        self._check()
        if not isinstance(text, str):
            raise TypeError("Source text must be str")
        self._sources[_relative(path)] = text
        self._analysis = None

    def delete_source(self, path: str) -> None:
        self._check()
        del self._sources[_relative(path)]
        self._analysis = None

    def validate(self) -> Analysis:
        self._check()
        self._analysis = self._project._analyze(dict(self._sources))
        return self._analysis

    def commit(self) -> Model:
        self._check()
        with self._project._lock:
            if self._revision != self._project._revision:
                raise ConflictError("Transaction base revision is stale")
            analysis = self._analysis if self._analysis is not None else self.validate()
            model = analysis.require_model()
            self._project._sources = dict(self._sources)
            self._project._revision += 1
            self._closed = True
            return model

    def abort(self) -> None:
        self._closed = True
        self._analysis = None

    def __enter__(self) -> Transaction:
        self._check()
        return self

    def __exit__(self, *_exc) -> None:
        if not self._closed:
            self.abort()
