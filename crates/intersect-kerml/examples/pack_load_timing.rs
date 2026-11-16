//! Handwritten host example (language-pack step 9): time loading an existing
//! KerML/SysML language pack, stage by stage, without compiling or parsing
//! anything. Not generated.
//!
//! Usage: `pack_load_timing PACK [RUNS]` (default 7 runs). Each run reads the
//! file, then times `LanguagePack::read` (digests and JSON validation),
//! `NativePilotSession::from_pack(HostCache)`, `load_pack_bindings`, and a
//! breakdown of the bindings load: the section's JSON decode, the bundle
//! rebuild, `load_bindings` (envelope, digest, scope decode and each image's
//! `load_trusted`) and the recipe-result check. It also times a plain JSON
//! decode of every section (to `serde_json::Value` and to `IgnoredAny`) and
//! reports each section's maximum JSON nesting depth, including the nested
//! `save_bindings` text. Medians over the runs are printed last.
//!
//! The bindings breakdown passes the saved text's own compiler identity to
//! `load_bindings`, so it still runs when the pack's identity is stale (after
//! an intersect-model edit); `from_pack` and `load_pack_bindings` are then
//! refused and reported as skipped. All I/O, threads and clocks stay in this
//! example; the library performs none.
use intersect_kerml::{
    kerml_bindings,
    language_pack::{self, RcoreBindingsPart},
    native_pilot::NativePilotSession,
};
use intersect_model::definitions::pack::LanguagePack;
use intersect_model::definitions::pack::PackSectionEncoding;
use intersect_model::definitions::rcore_scope::load_bindings;
use intersect_text::grammar::RuntimeAdmissionLoadMode;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, process::ExitCode, time::Instant};

const WORKER_STACK_BYTES: usize = 64 * 1024 * 1024;

fn ms(clock: Instant) -> f64 {
    clock.elapsed().as_secs_f64() * 1e3
}

/// Maximum nesting of arrays and objects in JSON text, scanning bytes.
fn json_depth(text: &[u8]) -> usize {
    let (mut depth, mut max, mut in_string, mut escaped) = (0usize, 0usize, false, false);
    for &byte in text {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                max = max.max(depth);
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    max
}

fn string_field(value: &serde_json::Value, field: &str) -> Result<String, String> {
    value
        .get(field)
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("no string field {field}"))
}

fn report_depths(pack: &LanguagePack) -> Result<(), String> {
    for section in &pack.manifest().sections {
        let bytes = pack
            .required_section(&section.name)
            .map_err(|e| e.to_string())?;
        println!(
            "section {}: {} bytes, JSON depth {}",
            section.name,
            bytes.len(),
            json_depth(bytes)
        );
    }
    let part: serde_json::Value = pack
        .section_json(language_pack::BINDINGS_SECTION)
        .map_err(|e| e.to_string())?;
    // Both are nested JSON in the section; measure each as its own text.
    let envelope = part.get("saved").ok_or("no saved bindings")?;
    let saved = envelope.to_string();
    let scopes = envelope.get("scopes").ok_or("no scopes")?.to_string();
    println!(
        "save_bindings text {} bytes, depth {}; scopes text {} bytes, depth {}",
        saved.len(),
        json_depth(saved.as_bytes()),
        scopes.len(),
        json_depth(scopes.as_bytes())
    );
    Ok(())
}

fn run_once(path: &str, timings: &mut BTreeMap<&'static str, Vec<f64>>) -> Result<(), String> {
    let mut record = |name: &'static str, value: f64| timings.entry(name).or_default().push(value);
    let clock = Instant::now();
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    record("0 fs read", ms(clock));

    let clock = Instant::now();
    let pack = LanguagePack::read(bytes).map_err(|e| e.to_string())?;
    record("1 LanguagePack::read (total)", ms(clock));

    // The parts of `read`, redone outside it.
    let clock = Instant::now();
    for section in &pack.manifest().sections {
        let bytes = pack
            .required_section(&section.name)
            .map_err(|e| e.to_string())?;
        std::hint::black_box(Sha256::digest(bytes));
    }
    record("1a read: section sha256", ms(clock));
    // Binary sections (the prepared library) are not JSON.
    let json_sections = || {
        pack.manifest()
            .sections
            .iter()
            .filter(|section| section.encoding == PackSectionEncoding::Json)
    };
    let clock = Instant::now();
    for section in json_sections() {
        let bytes = pack
            .required_section(&section.name)
            .map_err(|e| e.to_string())?;
        let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        serde_json::from_str::<serde::de::IgnoredAny>(text).map_err(|e| e.to_string())?;
    }
    record("1b read: utf8 + IgnoredAny parse", ms(clock));

    for section in json_sections() {
        let clock = Instant::now();
        let value: serde_json::Value = pack
            .section_json(&section.name)
            .map_err(|e| e.to_string())?;
        std::hint::black_box(&value);
        let name: &'static str = match section.name.as_str() {
            "program/kerml" => "2a decode program/kerml to Value",
            "program/sysml" => "2b decode program/sysml to Value",
            _ => "2c decode rcore/bindings to Value",
        };
        record(name, ms(clock));
    }

    let clock = Instant::now();
    match NativePilotSession::from_pack(&pack, RuntimeAdmissionLoadMode::HostCache) {
        Ok(session) => {
            record("3 from_pack(HostCache)", ms(clock));
            drop(session);
        }
        Err(error) => println!("from_pack skipped: {error}"),
    }

    let clock = Instant::now();
    match language_pack::load_pack_bindings(&pack) {
        Ok(scopes) => {
            record("4 load_pack_bindings", ms(clock));
            drop(scopes);
        }
        Err(error) => println!("load_pack_bindings skipped: {error}"),
    }

    // Breakdown of the bindings load, by public steps.
    let clock = Instant::now();
    let part: RcoreBindingsPart = pack
        .section_json(language_pack::BINDINGS_SECTION)
        .map_err(|e| e.to_string())?;
    record("5a bindings: section decode", ms(clock));
    let clock = Instant::now();
    let bundle = kerml_bindings::bundle();
    let same = *part.bundle() == bundle;
    record("5b bindings: bundle rebuild + compare", ms(clock));
    if !same {
        return Err("stored bindings bundle differs".into());
    }
    let envelope: serde_json::Value =
        serde_json::from_str(part.saved()).map_err(|e| e.to_string())?;
    let compiler = string_field(&envelope, "compiler")?;
    let scopes_text = envelope.get("scopes").ok_or("no scopes")?.to_string();
    let clock = Instant::now();
    std::hint::black_box(Sha256::digest(scopes_text.as_bytes()));
    serde_json::from_str::<serde::de::IgnoredAny>(&scopes_text).map_err(|e| e.to_string())?;
    record("5c bindings: scopes digest + IgnoredAny parse", ms(clock));
    let clock = Instant::now();
    let scopes = load_bindings(part.saved(), &bundle, &compiler)?;
    record("5d bindings: load_bindings", ms(clock));
    let clock = Instant::now();
    kerml_bindings::check_recipe_results(&scopes)?;
    record("5e bindings: check_recipe_results", ms(clock));
    if scopes.len() != 39 {
        println!("note: {} scopes", scopes.len());
    }
    Ok(())
}

fn run(path: &str, runs: usize) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    println!("pack {path}: {} bytes", bytes.len());
    let pack = LanguagePack::read(bytes).map_err(|e| e.to_string())?;
    report_depths(&pack)?;
    drop(pack);
    let mut timings = BTreeMap::new();
    for run in 0..runs {
        run_once(path, &mut timings)?;
        println!("run {run} done");
    }
    for (name, mut values) in timings {
        values.sort_by(f64::total_cmp);
        let median = values[values.len() / 2];
        let list: Vec<String> = values.iter().map(|v| format!("{v:.1}")).collect();
        println!("{name}: median {median:.1} ms [{}]", list.join(", "));
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first().cloned() else {
        eprintln!("usage: pack_load_timing PACK [RUNS]");
        return ExitCode::from(2);
    };
    let runs = args.get(1).and_then(|n| n.parse().ok()).unwrap_or(7);
    let worker = std::thread::Builder::new()
        .stack_size(WORKER_STACK_BYTES)
        .spawn(move || run(&path, runs));
    match worker.map(|handle| handle.join()) {
        Ok(Ok(Ok(()))) => ExitCode::SUCCESS,
        Ok(Ok(Err(error))) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
        _ => {
            eprintln!("error: worker failed");
            ExitCode::FAILURE
        }
    }
}
