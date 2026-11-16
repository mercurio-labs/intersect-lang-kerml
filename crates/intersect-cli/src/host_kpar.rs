//! Handwritten host (not generated): project archive (`.kpar`) inputs,
//! dependency archives from the project path, library archives, the
//! `.meta.json` index check and `export --format kpar` of the `intersect`
//! command. Archive reading, checking and writing are `intersect-kpar`'s;
//! this module does the file I/O, the clock and the model queries.
use super::{Origin, UserFile, resource_uri};
use intersect_kerml::language_pack::{KERML_SOURCE, SYSML_SOURCE};
use intersect_kpar::{
    ArchiveFile, ArchiveLimits, Language, Meta, Project, ProjectArchive, Usage,
    archive::entry_uri,
    resolve::{file_name, standard_library_iri},
    zip::check_path,
};
use intersect_model::{core::frozen::Model, definitions::library_part::LibrarySource};
use intersect_text::Source;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
};

/// A problem or note found outside the load (phase `archive`).
pub(super) struct Problem {
    /// `error`, `warning` or `info`.
    pub severity: &'static str,
    pub file: Option<String>,
    pub message: String,
}

impl Problem {
    fn new(severity: &'static str, file: impl Into<String>, message: String) -> Self {
        Self {
            severity,
            file: Some(file.into()),
            message,
        }
    }
    pub fn is_error(&self) -> bool {
        self.severity == "error"
    }
}

/// The archives of a run: every candidate read, the ones loaded (in load
/// order) and the notes found while reading them.
#[derive(Default)]
pub(super) struct Archives {
    pub archives: Vec<ProjectArchive>,
    /// How each archive is named in messages (its path as given or found).
    pub display: Vec<String>,
    pub loaded: Vec<usize>,
    pub roots: Vec<usize>,
    pub problems: Vec<Problem>,
}

pub(super) fn is_archive(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("kpar"))
}

fn read_archive(path: &Path) -> Result<ProjectArchive, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    intersect_kpar::read(&resource_uri(path), &bytes, &ArchiveLimits::default())
        .map_err(|error| error.to_string())
}

/// The `.kpar` files directly inside each project path folder, sorted.
fn project_path_archives(folders: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for folder in folders {
        let entries = std::fs::read_dir(folder)
            .map_err(|error| format!("--project-path {}: {error}", folder.display()))?;
        let mut here = Vec::new();
        for entry in entries {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.is_file() && is_archive(&path) {
                here.push(path);
            }
        }
        here.sort();
        found.extend(here);
    }
    Ok(found)
}

/// Read the archive inputs, resolve their `usage` against the other inputs
/// and the project path, and return the user files of every archive to
/// load, dependencies first.
pub(super) fn read_archive_inputs(
    inputs: &[PathBuf],
    project_path: &[PathBuf],
) -> Result<(Vec<UserFile>, Archives), String> {
    let mut archives = Archives::default();
    for path in inputs {
        archives.roots.push(archives.archives.len());
        archives.archives.push(read_archive(path)?);
        archives.display.push(path.display().to_string());
    }
    if archives.roots.is_empty() {
        return Ok((Vec::new(), archives));
    }
    let given: BTreeSet<PathBuf> = inputs
        .iter()
        .filter_map(|path| path.canonicalize().ok())
        .collect();
    for path in project_path_archives(project_path)? {
        if path.canonicalize().is_ok_and(|real| given.contains(&real)) {
            continue;
        }
        // An unreadable archive on the project path matters only if it is
        // needed: then resolution reports the dependency as unresolved.
        match read_archive(&path) {
            Ok(archive) => {
                archives.archives.push(archive);
                archives.display.push(path.display().to_string());
            }
            Err(error) => archives.problems.push(Problem::new(
                "warning",
                path.display().to_string(),
                format!("skipped on the project path: {error}"),
            )),
        }
    }
    let resolution =
        intersect_kpar::resolve(&archives.archives, &archives.roots).map_err(|errors| {
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        })?;
    let mut provided = BTreeSet::new();
    for used in &resolution.standard_library {
        if provided.insert(used.resource.clone()) {
            archives.problems.push(Problem::new(
                "info",
                used.by.clone(),
                format!(
                    "usage {} is provided by the pack's standard library{}",
                    used.resource,
                    used.version_constraint
                        .as_ref()
                        .map(|c| format!(" (versionConstraint '{c}' not checked)"))
                        .unwrap_or_default()
                ),
            ));
        }
    }
    let mut files = Vec::new();
    for &index in &resolution.order {
        let archive = &archives.archives[index];
        let display = &archives.display[index];
        for warning in &archive.warnings {
            archives
                .problems
                .push(Problem::new("warning", display.clone(), warning.clone()));
        }
        for note in &archive.notes {
            archives
                .problems
                .push(Problem::new("info", display.clone(), note.clone()));
        }
        let origin = if archives.roots.contains(&index) {
            Origin::Root
        } else {
            Origin::Dependency
        };
        for source in &archive.sources {
            files.push(UserFile {
                path: format!("{display}!/{}", source.entry).into(),
                source: Source {
                    uri: source.uri.clone(),
                    text: source.text.clone(),
                },
                source_map: None,
                origin,
                entry: Some(source.path.clone()),
            });
        }
    }
    archives.loaded = resolution.order;
    Ok((files, archives))
}

/// Standard library sources from library archives (the OMG `.kpar` files).
/// Each archive's own `usage` is not resolved: the archives load together.
/// The archives' warnings and notes go to `problems`, as for input
/// archives.
pub(super) fn library_archive_sources(
    paths: &[PathBuf],
    problems: &mut Vec<Problem>,
) -> Result<Vec<LibrarySource>, String> {
    let mut sources = Vec::new();
    for path in paths {
        let archive = read_archive(path)?;
        let display = path.display().to_string();
        for warning in archive.warnings {
            problems.push(Problem::new("warning", display.clone(), warning));
        }
        for note in archive.notes {
            problems.push(Problem::new("info", display.clone(), note));
        }
        for source in archive.sources {
            sources.push(LibrarySource {
                uri: source.uri,
                language: match source.language {
                    Language::KerML => KERML_SOURCE,
                    Language::SysML => SYSML_SOURCE,
                }
                .into(),
                text: source.text,
            });
        }
    }
    if sources.is_empty() {
        return Err("the library archives hold no .kerml or .sysml files".into());
    }
    sources.sort_by(|a, b| a.uri.cmp(&b.uri));
    Ok(sources)
}

/// The top-level member names of each resource in `resources`: the declared
/// names of the root namespace's owned members and the names of its
/// memberships (aliases).
fn top_level_names<'m>(
    model: &'m Model,
    resources: &BTreeSet<&str>,
) -> BTreeMap<&'m str, BTreeSet<&'m str>> {
    let mut names: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for object in model.objects() {
        let resource = object.resource();
        if !resources.contains(resource) {
            continue;
        }
        let Some((parent, _)) = object.container() else {
            continue;
        };
        let name = match parent.container() {
            // `object` is a membership of the root namespace.
            None => object
                .feature("memberName")
                .and_then(|feature| object.str(feature)),
            // `object` is the member a root membership owns.
            Some((grandparent, _)) if grandparent.container().is_none() => object
                .feature("declaredName")
                .and_then(|feature| object.str(feature)),
            Some(_) => None,
        };
        if let Some(name) = name.filter(|name| !name.is_empty()) {
            names.entry(resource).or_default().insert(name);
        }
    }
    names
}

/// Every `.meta.json` index entry of a loaded archive must name a top-level
/// member of the file it points to.
pub(super) fn check_index(model: &Model, archives: &Archives) -> Vec<Problem> {
    let mut wanted = Vec::new();
    for &index in &archives.loaded {
        let archive = &archives.archives[index];
        for (name, path) in &archive.meta.index {
            // A path the archive does not hold is already a read warning.
            if !archive.sources.iter().any(|source| &source.path == path) {
                continue;
            }
            wanted.push((
                index,
                name,
                path,
                entry_uri(&archive.uri, &format!("{}{path}", archive.root)),
            ));
        }
    }
    let resources: BTreeSet<&str> = wanted.iter().map(|(.., uri)| uri.as_str()).collect();
    let names = top_level_names(model, &resources);
    wanted
        .iter()
        .filter(|(_, name, _, uri)| {
            !names
                .get(uri.as_str())
                .is_some_and(|declared| declared.contains(name.as_str()))
        })
        .map(|(index, name, path, _)| {
            Problem::new(
                "error",
                format!(
                    "{}!/{}.meta.json",
                    archives.display[*index], archives.archives[*index].root
                ),
                format!("index entry '{name}': {path} declares no top-level member '{name}'"),
            )
        })
        .collect()
}

/// The OMG standard library archives the `written` resources reference.
fn referenced_standard_libraries(model: &Model, written: &BTreeSet<&str>) -> BTreeSet<String> {
    let inverse = model.inverse();
    let mut by_resource: HashMap<&str, Option<String>> = HashMap::new();
    let mut found = BTreeSet::new();
    for object in model.objects() {
        let resource = object.resource();
        if written.contains(resource) {
            continue;
        }
        let Some(iri) = by_resource
            .entry(resource)
            .or_insert_with(|| standard_library_iri(resource))
            .clone()
        else {
            continue;
        };
        if found.contains(&iri) {
            continue;
        }
        let referenced = inverse.referrers(object.id()).iter().any(|(source, _)| {
            model
                .obj(*source)
                .is_some_and(|source| written.contains(source.resource()))
        });
        if referenced {
            found.insert(iri);
        }
    }
    found
}

/// `seconds` since the Unix epoch as an RFC 3339 UTC date-time.
pub(super) fn rfc3339(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Days to civil date (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// The `created` time of a written archive: `SOURCE_DATE_EPOCH` when set
/// (reproducible builds), else the clock.
fn created() -> Result<String, String> {
    let seconds = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(value) => value
            .trim()
            .parse::<u64>()
            .map_err(|_| format!("SOURCE_DATE_EPOCH is not a number of seconds: {value}"))?,
        Err(_) => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs(),
    };
    Ok(rfc3339(seconds))
}

/// The archive path a loose file is written under: its relative path when
/// that is a safe archive path, else its file name.
fn loose_entry_path(file: &UserFile) -> String {
    let relative = file.source.uri.replace("%20", " ");
    if check_path(&relative).is_ok() {
        relative
    } else {
        file_name(&file.source.uri)
    }
}

/// What `export --format kpar` names the project.
pub(super) struct ProjectName<'a> {
    pub name: &'a str,
    pub version: &'a str,
}

/// Write the input files (loose files and the files of the archives given
/// on the command line, not their dependencies) as one project archive in
/// `out`. Its `usage` keeps the input archives' usages and adds the OMG
/// standard library archives the files reference, the Semantic Library
/// always and the Systems Library for SysML files.
pub(super) fn export_kpar(
    model: &Model,
    user: &[UserFile],
    archives: &Archives,
    project: ProjectName<'_>,
    out: &Path,
) -> Result<PathBuf, String> {
    let written: Vec<(&UserFile, String)> = user
        .iter()
        .filter(|file| file.source_map.is_none())
        .filter_map(|file| match file.origin {
            Origin::Loose => Some((file, loose_entry_path(file))),
            Origin::Root => file.entry.clone().map(|entry| (file, entry)),
            Origin::Dependency => None,
        })
        .collect();
    let resources: BTreeSet<&str> = written
        .iter()
        .map(|(file, _)| file.source.uri.as_str())
        .collect();
    let names = top_level_names(model, &resources);
    let mut index = BTreeMap::new();
    let mut by_path: Vec<&(&UserFile, String)> = written.iter().collect();
    by_path.sort_by(|a, b| a.1.cmp(&b.1));
    for (file, path) in by_path {
        for name in names.get(file.source.uri.as_str()).into_iter().flatten() {
            index
                .entry((*name).to_owned())
                .or_insert_with(|| path.clone());
        }
    }

    let mut usage: Vec<Usage> = Vec::new();
    for &root in &archives.roots {
        for used in &archives.archives[root].project.usage {
            if !usage.contains(used) {
                usage.push(used.clone());
            }
        }
    }
    // Implicit specializations need not be stored references: every file
    // depends on the KerML Semantic Library, and SysML files on the
    // Systems Library.
    let release = intersect_kpar::resolve::STANDARD_LIBRARY_RELEASE;
    let mut implied = BTreeSet::new();
    if !written.is_empty() {
        implied.insert(format!(
            "https://www.omg.org/spec/KerML/{release}/Semantic-Library.kpar"
        ));
    }
    if written
        .iter()
        .any(|(_, path)| Language::of_path(path) == Some(Language::SysML))
    {
        implied.insert(format!(
            "https://www.omg.org/spec/SysML/{release}/Systems-Library.kpar"
        ));
    }
    implied.extend(referenced_standard_libraries(model, &resources));
    for iri in implied {
        if !usage.iter().any(|used| used.resource == iri) {
            usage.push(Usage {
                resource: iri,
                ..Usage::default()
            });
        }
    }
    let project = Project {
        name: project.name.to_owned(),
        version: project.version.to_owned(),
        usage,
        ..Project::default()
    };
    let meta = Meta {
        index,
        created: created()?,
        ..Meta::default()
    };
    let files: Vec<ArchiveFile> = written
        .iter()
        .map(|(file, path)| ArchiveFile {
            path: path.clone(),
            text: file.source.text.clone(),
        })
        .collect();
    let bytes = intersect_kpar::write(&project, &meta, &files).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(out).map_err(|error| format!("{}: {error}", out.display()))?;
    let stem: String = project
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    let path = out.join(format!("{stem}.kpar"));
    super::write_atomic(&path, &bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_formats_utc_dates() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_741_824_000), "2025-03-13T00:00:00Z");
        assert_eq!(rfc3339(1_794_268_799), "2026-11-09T23:59:59Z");
    }

    fn file(uri: &str) -> UserFile {
        UserFile {
            path: uri.into(),
            source: Source {
                uri: uri.into(),
                text: String::new(),
            },
            source_map: None,
            origin: Origin::Loose,
            entry: None,
        }
    }

    #[test]
    fn loose_files_keep_safe_relative_paths() {
        assert_eq!(
            loose_entry_path(&file("models/Simple%20Tests/a.sysml")),
            "models/Simple Tests/a.sysml"
        );
        assert_eq!(loose_entry_path(&file("../shared/b.sysml")), "b.sysml");
        assert_eq!(loose_entry_path(&file("D:/abs/c.kerml")), "c.kerml");
    }

    #[test]
    fn library_archive_warnings_and_notes_are_reported() -> Result<(), String> {
        let root =
            std::env::temp_dir().join(format!("intersect-cli-libkpar-{}", std::process::id()));
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let path = root.join("Lib.kpar");
        let entries = [
            ("Lib/.project.json", r#"{"name":"Lib","version":"1.0.0"}"#),
            (
                "Lib/.meta.json",
                r#"{"index":{"Gone":"Gone.kerml","Base":"Base.kerml"},"created":"2025-03-13T00:00:00Z"}"#,
            ),
            ("Lib/Base.kerml", "package Base;"),
            ("__MACOSX/Lib/._Base.kerml", "junk"),
        ]
        .map(|(path, text)| intersect_kpar::zip::ZipEntry {
            path: path.into(),
            bytes: text.as_bytes().to_vec(),
        });
        let bytes = intersect_kpar::zip::write(&entries).map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        let mut problems = Vec::new();
        let sources = library_archive_sources(std::slice::from_ref(&path), &mut problems);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(sources?.len(), 1);
        let reported: Vec<(&str, &str)> = problems
            .iter()
            .map(|p| (p.severity, p.message.as_str()))
            .collect();
        assert_eq!(reported.len(), 2, "{reported:?}");
        assert!(
            reported[0].0 == "warning" && reported[0].1.contains("index entry 'Gone'"),
            "{reported:?}"
        );
        assert!(
            reported[1].0 == "info" && reported[1].1.contains("__MACOSX"),
            "{reported:?}"
        );
        assert!(
            problems
                .iter()
                .all(|p| p.file.as_deref().is_some_and(|f| f.ends_with("Lib.kpar")))
        );
        Ok(())
    }

    #[test]
    fn archive_inputs_resolve_against_the_project_path() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!("intersect-cli-kpar-{}", std::process::id()));
        let deps = root.join("deps");
        std::fs::create_dir_all(&deps).map_err(|e| e.to_string())?;
        let write = |path: &Path, name: &str, usage: Vec<Usage>, file: &str, text: &str| {
            let project = Project {
                name: name.into(),
                version: "1.0.0".into(),
                usage,
                ..Project::default()
            };
            let meta = Meta {
                created: "2026-01-01T00:00:00Z".into(),
                ..Meta::default()
            };
            let files = [ArchiveFile {
                path: file.into(),
                text: text.into(),
            }];
            let bytes =
                intersect_kpar::write(&project, &meta, &files).map_err(|e| e.to_string())?;
            std::fs::write(path, bytes).map_err(|e| e.to_string())
        };
        let uses = |iri: &str| Usage {
            resource: iri.into(),
            version_constraint: Some("1".into()),
            ..Usage::default()
        };
        let app = root.join("app.kpar");
        write(
            &app,
            "App",
            vec![
                uses("https://example.com/units.kpar"),
                uses("https://www.omg.org/spec/SysML/20250201/Systems-Library.kpar"),
            ],
            "App.sysml",
            "package App;",
        )?;
        write(
            &deps.join("units.kpar"),
            "Units",
            vec![],
            "Units.sysml",
            "package Units;",
        )?;
        std::fs::write(deps.join("broken.kpar"), b"not a zip").map_err(|e| e.to_string())?;

        let result = read_archive_inputs(std::slice::from_ref(&app), std::slice::from_ref(&deps));
        let missing = read_archive_inputs(std::slice::from_ref(&app), &[]);
        let _ = std::fs::remove_dir_all(&root);
        let (files, archives) = result?;
        let order: Vec<(&str, Origin)> = files
            .iter()
            .map(|file| (file.entry.as_deref().unwrap_or(""), file.origin))
            .collect();
        assert_eq!(
            order,
            [
                ("Units.sysml", Origin::Dependency),
                ("App.sysml", Origin::Root)
            ]
        );
        assert!(files[1].source.uri.ends_with("app.kpar!/App.sysml"));
        assert!(
            files[1]
                .path
                .display()
                .to_string()
                .ends_with("app.kpar!/App.sysml")
        );
        let notes: Vec<&str> = archives
            .problems
            .iter()
            .map(|p| p.message.as_str())
            .collect();
        assert!(
            notes
                .iter()
                .any(|n| n.contains("skipped on the project path")),
            "{notes:?}"
        );
        assert!(
            notes
                .iter()
                .any(|n| n.contains("provided by the pack's standard library")),
            "{notes:?}"
        );
        let missing = missing.err().unwrap_or_default();
        assert!(
            missing.contains("no archive named 'units.kpar'"),
            "{missing}"
        );
        Ok(())
    }
}
