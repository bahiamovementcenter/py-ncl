import os
from fractions import Fraction
from pathlib import Path

import pytest

import ncl

FIXTURES = Path(__file__).parent / "fixtures"


def test_eval_converts_values():
    assert ncl.eval('{ a = 1, b = [true, null, "x"], c = 1.5 }') == {
        "a": 1,
        "b": [True, None, "x"],
        "c": 1.5,
    }


def test_whole_numbers_stay_exact_beyond_64_bits():
    value = ncl.eval("std.number.pow 2 100")
    assert value == 2**100 and isinstance(value, int)


def test_fractions_become_floats():
    assert ncl.eval("1 / 3") == pytest.approx(float(Fraction(1, 3)))


def test_enums():
    tag = ncl.eval("'production")
    assert tag == "production" and isinstance(tag, ncl.Tag)
    assert ncl.eval("'Some 3") == ncl.Variant("Some", 3)


def test_eval_file_with_overrides():
    program = ncl.Program(
        FIXTURES / "server.ncl",
        overrides={"server.port": "8080", "server.host": ('"example.org"', "force")},
    )
    assert program.eval()["server"] == {
        "host": "example.org",
        "port": 8080,
        "environment": "development",
    }


def test_contracts_and_errors():
    with pytest.raises(ncl.NickelError) as raised:
        ncl.eval_file(
            FIXTURES / "server.ncl",
            contracts=[FIXTURES / "schema.ncl"],
            overrides={"server.port": "-1"},
        )
    assert raised.value.kind == "contract"
    assert raised.value.diagnostics[0]["severity"] == "error"
    assert "contract" in str(raised.value)


def test_parse_error_kind():
    with pytest.raises(ncl.NickelError) as raised:
        ncl.eval("{ a = ")
    assert raised.value.kind == "parse"


def test_export_and_field():
    program = ncl.Program(FIXTURES / "server.ncl", field="server.port")
    assert program.export("json").strip() == "80"
    assert "port: 80" in ncl.export(FIXTURES / "server.ncl", format="yaml")


def test_in_memory_sources_merge_with_files():
    data = ncl.Source.code('{ "server": { "port": 9000 } }', format="json", name="extra")
    result = ncl.Program(FIXTURES / "server.ncl", data).eval()
    assert result["server"]["port"] == 9000


def test_import_paths():
    code = '(import "shared.ncl").answer'
    assert ncl.eval(code, import_paths=[FIXTURES / "lib"]) == 42


def test_package_map():
    packages = ncl.PackageMap()
    packages.add("greeting", FIXTURES / "packages" / "greeting")
    assert ncl.eval('(import greeting).hello "xd"', packages=packages) == "hello, xd"
    assert [name for name, _ in packages.packages()] == ["greeting"]


def test_find_manifest_returns_none_without_one(tmp_path):
    assert ncl.PackageMap.find_manifest(tmp_path) is None


def test_typecheck_and_query():
    ncl.Program(FIXTURES / "server.ncl").typecheck()
    with pytest.raises(ncl.NickelError) as raised:
        ncl.Program(ncl.Source.code("(1 + \"a\" : Number)")).typecheck()
    assert raised.value.kind == "typecheck"
    info = ncl.Program(FIXTURES / "server.ncl", field="server").query()
    assert set(info["subfields"]) == {"environment", "host", "port"}


def test_a_plain_string_is_a_path_not_code():
    with pytest.raises(ncl.NickelError) as raised:
        ncl.Program("{ a = 1 }").eval()
    assert raised.value.kind == "io"


# xD's `xd` package, at the commit rust-nickel pins. Fetched from GitHub, so skipped offline.
XD_COMMIT = "2f104cc8c7ea464f30af04d4ab6cbe11de0592d4"


@pytest.mark.skipif(bool(os.environ.get("NCL_TEST_OFFLINE")), reason="fetches xD from GitHub")
def test_xd_package_through_a_manifest(tmp_path):
    (tmp_path / "Nickel-pkg.ncl").write_text(
        f"""{{
  name = "xd-user",
  description = "uses xD",
  version = "0.1.0",
  authors = ["py-ncl"],
  minimal_nickel_version = "1.9.0",
  dependencies = {{
    xd = 'Git {{
      url = "https://github.com/bahiamovementcenter/xD.git",
      ref = 'Commit "{XD_COMMIT}",
      path = "nickel",
    }},
  }},
}} | std.package.Manifest
"""
    )
    manifest = ncl.PackageMap.find_manifest(tmp_path)
    packages = ncl.PackageMap.from_manifest(manifest, tmp_path / "cache")
    assert ncl.eval("(import xd).units.jd.pattern.render [1, 1, 11]", packages=packages) == "11.11"
