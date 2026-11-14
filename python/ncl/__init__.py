"""Python bindings to the Nickel configuration language.

Nickel runs inside the Python process, through nickel-embed
(https://github.com/bahiamovementcenter/rust-nickel). This package only converts
between Nickel and Python; everything about Nickel itself is in nickel-embed.

    >>> import ncl
    >>> ncl.eval('{ port | Number = 80, name = "my" ++ "server" }')
    {'port': 80, 'name': 'myserver'}
"""

from __future__ import annotations

import os
from collections.abc import Iterable, Mapping
from typing import Any, Union

from ._native import NickelError, PackageMap, Source
from ._native import Program as _Program
from ._types import Tag, Variant

__all__ = [
    "NickelError",
    "PackageMap",
    "Program",
    "Source",
    "Tag",
    "Variant",
    "eval",
    "eval_file",
    "export",
]

PathLike = Union[str, "os.PathLike[str]"]
SourceLike = Union[Source, PathLike]

# How Python values given as overrides are written in Nickel.
_PRIORITIES = ("default", "force")


def _source(source: SourceLike) -> Source:
    # A plain string is a path, as on the command line: code goes through Source.code or
    # ncl.eval, so a typo in a path never runs as code.
    return source if isinstance(source, Source) else Source.file(os.fspath(source))


def _priority(priority: str | int | None) -> str | None:
    if priority is None:
        return None
    if isinstance(priority, int) and not isinstance(priority, bool):
        return str(priority)
    if priority in _PRIORITIES:
        return str(priority)
    raise ValueError(f"priority must be 'default', 'force', an int or None, not {priority!r}")


class Program(_Program):
    """A Nickel program: its sources, merged, and how to evaluate them.

    Like the `nickel` command line tool, in process:

    - `sources`: files (paths) or `Source.code(...)`, merged like `nickel export a.ncl b.yaml`.
    - `import_paths`: directories searched by `import "file.ncl"`, like `-I`.
      `NICKEL_IMPORT_PATH` is searched too unless `env_import_path=False`.
    - `contracts`: files whose contracts apply to the result, like `--apply-contract`.
    - `overrides`: `{"server.port": "8080"}`, Nickel code assigned to fields, like
      `--override`. A value may be `(code, priority)`, the priority being `"default"`,
      `"force"` or an int.
    - `field`: evaluate only this field, like `--field`.
    - `packages`: a `PackageMap`, for `import name`.

    Not thread-safe: use a program from the thread that made it.
    """

    def __new__(
        cls,
        *sources: SourceLike,
        import_paths: Iterable[PathLike] = (),
        contracts: Iterable[PathLike] = (),
        overrides: Mapping[str, str | tuple[str, str | int | None]] | None = None,
        field: str | None = None,
        packages: PackageMap | None = None,
        env_import_path: bool = True,
    ) -> Program:
        triples = []
        for path, value in (overrides or {}).items():
            code, priority = value if isinstance(value, tuple) else (value, None)
            triples.append((path, code, _priority(priority)))
        return super().__new__(
            cls,
            [_source(s) for s in sources],
            import_paths=[os.fspath(p) for p in import_paths],
            contracts=[os.fspath(p) for p in contracts],
            overrides=triples,
            field=field,
            packages=packages,
            env_import_path=env_import_path,
        )


def eval(code: str, **options: Any) -> Any:  # noqa: A001 - the natural name, as in `nickel eval`
    """Evaluates Nickel code and returns the result as Python values.

    Options are `Program`'s. Raises `NickelError` if Nickel rejects the code.
    """
    return Program(Source.code(code), **options).eval()


def eval_file(path: PathLike, **options: Any) -> Any:
    """Evaluates a file (Nickel, JSON, YAML or TOML) and returns it as Python values."""
    return Program(path, **options).eval()


def export(*sources: SourceLike, format: str = "json", **options: Any) -> str:  # noqa: A002
    """Like `nickel export`: the merged sources as `json`, `yaml`, `toml` or `text`."""
    return Program(*sources, **options).export(format)
