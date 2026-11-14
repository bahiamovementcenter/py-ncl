//! The `ncl._native` extension: nickel-embed's `Program` and `PackageMap`, with Nickel values
//! converted to Python ones. Everything about Nickel itself is in nickel-embed; this file only
//! converts. The Python-facing API is in `python/ncl/__init__.py`.

use std::path::PathBuf;

use nickel_embed::{
    Error, ExportFormat, Expr, ExprKind, FieldPath, InputFormat, PackageMap as CorePackageMap,
    Priority, Program as CoreProgram, Source as CoreSource, TypecheckMode,
};
use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};

create_exception!(
    _native,
    NickelError,
    PyException,
    "Nickel rejected a program. `kind` and `diagnostics` say why; `str()` is Nickel's report."
);

/// Turns a nickel-embed error into a `NickelError` with `kind` and `diagnostics` attributes.
fn nickel_error(py: Python<'_>, error: Error) -> PyErr {
    let err = NickelError::new_err(error.to_string_with(nickel_embed::ErrorFormat::Text));
    let value = err.value(py);
    let kind = format!("{:?}", error.kind()).to_lowercase();
    let diagnostics = serde_json::to_string(&error.diagnostics())
        .ok()
        .and_then(|json| {
            py.import("json")
                .and_then(|m| m.call_method1("loads", (json,)))
                .ok()
        });
    let _ = value.setattr("kind", kind);
    let _ = value.setattr(
        "diagnostics",
        diagnostics.unwrap_or_else(|| PyList::empty(py).into_any()),
    );
    err
}

fn input_format(name: Option<&str>) -> PyResult<Option<InputFormat>> {
    name.map(|n| {
        InputFormat::from_extension(n)
            .ok_or_else(|| PyValueError::new_err(format!("unknown input format {n:?}")))
    })
    .transpose()
}

fn export_format(name: &str) -> PyResult<ExportFormat> {
    Ok(match name {
        "json" => ExportFormat::Json,
        "yaml" => ExportFormat::Yaml,
        "yaml-documents" => ExportFormat::YamlDocuments,
        "toml" => ExportFormat::Toml,
        "text" | "raw" => ExportFormat::Text,
        _ => {
            return Err(PyValueError::new_err(format!(
                "unknown export format {name:?}"
            )));
        }
    })
}

fn field_path(path: &str) -> PyResult<FieldPath> {
    path.parse()
        .map_err(|_| PyValueError::new_err(format!("invalid field path {path:?}")))
}

/// A program's input: a file, or code held in memory.
#[pyclass(module = "ncl._native", frozen, from_py_object)]
#[derive(Clone)]
pub struct Source {
    path: Option<PathBuf>,
    code: Option<String>,
    name: Option<String>,
    format: Option<InputFormat>,
}

#[pymethods]
impl Source {
    /// A file, in the format its extension says (`.ncl`, `.json`, `.yaml`, `.toml`, `.txt`)
    /// unless `format` names one.
    #[staticmethod]
    #[pyo3(signature = (path, *, format = None))]
    fn file(path: PathBuf, format: Option<&str>) -> PyResult<Self> {
        Ok(Self {
            path: Some(path),
            code: None,
            name: None,
            format: input_format(format)?,
        })
    }

    /// Code held in memory: Nickel unless `format` says otherwise. `name` is how errors
    /// refer to it.
    #[staticmethod]
    #[pyo3(signature = (text, *, name = None, format = None))]
    fn code(text: String, name: Option<String>, format: Option<&str>) -> PyResult<Self> {
        Ok(Self {
            path: None,
            code: Some(text),
            name,
            format: input_format(format)?,
        })
    }

    fn __repr__(&self) -> String {
        match (&self.path, &self.name) {
            (Some(path), _) => format!("Source.file({:?})", path.display().to_string()),
            (None, Some(name)) => format!("Source.code(..., name={name:?})"),
            (None, None) => "Source.code(...)".to_owned(),
        }
    }
}

impl Source {
    fn to_core(&self) -> CoreSource {
        let mut source = match (&self.path, &self.code) {
            (Some(path), _) => CoreSource::file(path.clone()),
            (None, code) => CoreSource::code(code.clone().unwrap_or_default()),
        };
        if let Some(name) = &self.name {
            source = source.with_name(name.clone());
        }
        if let Some(format) = self.format {
            source = source.with_format(format);
        }
        source
    }
}

/// Where `import name` finds package `name`: for the program, and for each package's own
/// dependencies.
#[pyclass(module = "ncl._native", from_py_object)]
#[derive(Clone, Default)]
pub struct PackageMap {
    inner: CorePackageMap,
}

#[pymethods]
impl PackageMap {
    #[new]
    fn new() -> Self {
        Self::default()
    }

    /// Makes `import name` in the program load the package in `dir`.
    fn add(&mut self, name: &str, dir: PathBuf) {
        self.inner = std::mem::take(&mut self.inner).package(name, dir);
    }

    /// Makes `import name` inside the package added with directory `parent` load the
    /// package in `dir`.
    fn add_dependency(&mut self, parent: PathBuf, name: &str, dir: PathBuf) {
        self.inner = std::mem::take(&mut self.inner).dependency(parent, name, dir);
    }

    /// The packages the program can import, as `{name: dir}`.
    fn packages(&self) -> Vec<(String, PathBuf)> {
        self.inner
            .packages()
            .into_iter()
            .map(|(name, dir)| (name.to_owned(), dir.to_owned()))
            .collect()
    }

    /// The packages of a `Nickel-pkg.ncl` manifest, fetched into `cache_dir` (Nickel's own
    /// cache by default) if needed. Writes `Nickel-pkg.lock` next to the manifest.
    #[staticmethod]
    #[pyo3(signature = (manifest, cache_dir = None))]
    fn from_manifest(
        py: Python<'_>,
        manifest: PathBuf,
        cache_dir: Option<PathBuf>,
    ) -> PyResult<Self> {
        let inner = match cache_dir {
            Some(dir) => CorePackageMap::from_manifest_with_cache(manifest, dir),
            None => CorePackageMap::from_manifest(manifest),
        };
        inner
            .map(|inner| Self { inner })
            .map_err(|e| nickel_error(py, e))
    }

    /// The nearest `Nickel-pkg.ncl` in `dir` or one of its parents.
    #[staticmethod]
    fn find_manifest(dir: PathBuf) -> Option<PathBuf> {
        CorePackageMap::find_manifest(dir)
    }

    fn __repr__(&self) -> String {
        format!("PackageMap({:?})", self.packages())
    }
}

/// A Nickel program: its sources, merged, and how to evaluate them. Like the `nickel`
/// command line tool, but in process.
///
/// Not thread-safe: use a program from the thread that made it.
#[pyclass(module = "ncl._native", unsendable, subclass)]
pub struct Program {
    inner: CoreProgram,
}

#[pymethods]
impl Program {
    #[new]
    #[pyo3(signature = (
        sources,
        *,
        import_paths = Vec::new(),
        contracts = Vec::new(),
        overrides = Vec::new(),
        field = None,
        packages = None,
        env_import_path = true,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        sources: Vec<Source>,
        import_paths: Vec<PathBuf>,
        contracts: Vec<PathBuf>,
        // (field path, Nickel code, priority): see python/ncl/__init__.py.
        overrides: Vec<(String, String, Option<String>)>,
        field: Option<&str>,
        packages: Option<PackageMap>,
        env_import_path: bool,
    ) -> PyResult<Self> {
        let mut builder = CoreProgram::builder().sources(sources.iter().map(Source::to_core));
        builder = builder.import_paths(import_paths);
        if env_import_path {
            builder = builder.import_paths_from_env();
        }
        for contract in contracts {
            builder = builder.apply_contract_file(contract);
        }
        for (path, code, priority) in overrides {
            let priority = match priority.as_deref() {
                None => Priority::Neutral,
                Some("default") => Priority::Default,
                Some("force") => Priority::Force,
                Some(n) => Priority::Numeral(
                    n.parse()
                        .map_err(|_| PyValueError::new_err(format!("invalid priority {n:?}")))?,
                ),
            };
            builder = builder.override_with_priority(field_path(&path)?, code, priority);
        }
        if let Some(field) = field {
            builder = builder.field(field_path(field)?);
        }
        if let Some(map) = packages {
            builder = builder.package_map(map.inner);
        }
        let inner = builder.build().map_err(|e| nickel_error(py, e))?;
        Ok(Self { inner })
    }

    /// Evaluates the program fully and returns it as Python values.
    fn eval(&mut self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let expr = self.inner.eval_deep().map_err(|e| nickel_error(py, e))?;
        to_python(py, &expr)
    }

    /// Like `nickel export`: the program as `json`, `yaml`, `yaml-documents`, `toml` or `text`.
    #[pyo3(signature = (format = "json"))]
    fn export(&mut self, py: Python<'_>, format: &str) -> PyResult<String> {
        let format = export_format(format)?;
        self.inner.export(format).map_err(|e| nickel_error(py, e))
    }

    /// Like `nickel typecheck`. `strict` checks everything as if it were statically typed.
    #[pyo3(signature = (*, strict = false))]
    fn typecheck(&mut self, py: Python<'_>, strict: bool) -> PyResult<()> {
        let mode = if strict {
            TypecheckMode::Enforce
        } else {
            TypecheckMode::Walk
        };
        self.inner.typecheck(mode).map_err(|e| nickel_error(py, e))
    }

    /// Like `nickel query`: the (focused) field's documentation, type, contracts and
    /// sub-fields, as a dict.
    fn query(&mut self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let info = self.inner.query().map_err(|e| nickel_error(py, e))?;
        let meta = info.metadata();
        let dict = PyDict::new(py);
        dict.set_item("doc", meta.doc())?;
        dict.set_item("type", meta.type_annotation())?;
        dict.set_item("contracts", meta.contracts())?;
        dict.set_item("optional", meta.optional())?;
        dict.set_item("not_exported", meta.not_exported())?;
        dict.set_item("subfields", info.subfields())?;
        Ok(dict.into_any().unbind())
    }
}

/// Converts a fully evaluated expression. Whole numbers become exact `int`s, other numbers
/// `float`s; enum tags become `ncl.Tag` (a `str`) and enum variants `ncl.Variant`.
fn to_python(py: Python<'_>, expr: &Expr) -> PyResult<Py<PyAny>> {
    let types = py.import("ncl._types")?;
    convert(py, &types, expr)
}

fn convert(py: Python<'_>, types: &Bound<'_, PyModule>, expr: &Expr) -> PyResult<Py<PyAny>> {
    Ok(match expr.kind() {
        ExprKind::Null => py.None(),
        ExprKind::Bool(b) => b.into_pyobject(py)?.to_owned().into_any().unbind(),
        ExprKind::Number(n) => match n.as_i64() {
            Some(i) => i.into_pyobject(py)?.into_any().unbind(),
            None => {
                let (numerator, denominator) = n.as_rational();
                if denominator == "1" {
                    // Beyond 64 bits: Python's int is exact at any size.
                    py.import("builtins")?
                        .getattr("int")?
                        .call1((numerator,))?
                        .unbind()
                } else {
                    n.as_f64().into_pyobject(py)?.into_any().unbind()
                }
            }
        },
        ExprKind::String(s) => PyString::new(py, s).into_any().unbind(),
        ExprKind::EnumTag(tag) => types.getattr("Tag")?.call1((tag,))?.unbind(),
        ExprKind::EnumVariant(tag, arg) => {
            let arg = convert(py, types, &arg)?;
            types.getattr("Variant")?.call1((tag, arg))?.unbind()
        }
        ExprKind::Record(record) => {
            let dict = PyDict::new(py);
            for (key, value) in record.iter() {
                if let Some(value) = value {
                    dict.set_item(key, convert(py, types, &value)?)?;
                }
            }
            dict.into_any().unbind()
        }
        ExprKind::Array(array) => {
            let items = array
                .iter()
                .map(|item| convert(py, types, &item))
                .collect::<PyResult<Vec<_>>>()?;
            PyList::new(py, items)?.into_any().unbind()
        }
        _ => {
            return Err(PyValueError::new_err(format!(
                "a Nickel value Python can't represent: {expr}"
            )));
        }
    })
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Source>()?;
    m.add_class::<PackageMap>()?;
    m.add_class::<Program>()?;
    m.add("NickelError", m.py().get_type::<NickelError>())?;
    Ok(())
}
