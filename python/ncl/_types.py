"""Python types for the Nickel values that have no direct Python equivalent."""

from __future__ import annotations

from typing import Any, NamedTuple


class Tag(str):
    """A Nickel enum tag, such as `'production`. It is a `str`, so `Tag("a") == "a"`."""

    __slots__ = ()

    def __repr__(self) -> str:
        return f"Tag({str.__repr__(self)})"


class Variant(NamedTuple):
    """A Nickel enum variant with an argument, such as `'Some 3`."""

    tag: str
    value: Any
