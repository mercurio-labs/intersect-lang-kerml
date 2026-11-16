//! Handwritten measurement host: the Base/ScalarValues/Links graph through the
//! frozen engine. Inputs are `fixtures/frozen-bsl/` (written by the deleted K01
//! `native_pilot_bench kernel-links`, see its README): `graph.json` (the
//! admitted graph), `schema.json` and `phases.jsonl` (the linker's scope
//! requests). Every requested reference is turned back into
//! an unresolved spelling, the stripped model is imported, linked and
//! validated, and the result must equal the admitted graph.
//!
//! The default resolver is an oracle that answers with the admitted targets;
//! it isolates import, link, opposite completion and validation from scoping.
use intersect_model::core::{
    frozen::{FeatureId, FrozenError, LinkView, Metamodel, ModelBuilder, ObjId},
    model::Model,
};
use intersect_model::definitions::schema::Schema;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};

#[path = "support/kerml_scopes.rs"]
mod kerml_scopes;

struct Request {
    owner: String,
    feature: String,
    spelling: String,
    start: u64,
}

fn ms(clock: Instant) -> f64 {
    clock.elapsed().as_secs_f64() * 1e3
}

fn main() -> Result<(), String> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("Usage: frozen_bsl fixtures/frozen-bsl [--rcore] [--shadow | --vm]")?,
    );
    let read = |name: &str| std::fs::read(dir.join(name)).map_err(|e| format!("{name}: {e}"));
    let schema: Schema =
        serde_json::from_slice(&read("schema.json")?).map_err(|e| e.to_string())?;
    let graph: Model = serde_json::from_slice(&read("graph.json")?).map_err(|e| e.to_string())?;
    let mut requests = Vec::new();
    for line in String::from_utf8_lossy(&read("phases.jsonl")?).lines() {
        let event: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if event["phase"] != "candidate_context" {
            continue;
        }
        let d = &event["data"];
        let text = |k: &str| {
            d[k].as_str()
                .map(str::to_owned)
                .ok_or(format!("request {k}"))
        };
        requests.push(Request {
            owner: text("owner")?,
            feature: text("feature")?,
            spelling: text("spelling")?,
            start: d["start"].as_u64().ok_or("request start")?,
        });
    }
    println!(
        "objects {}, resources {}, scope requests {}",
        graph.elements.len(),
        graph.resources.len(),
        requests.len()
    );

    // Group requests by (owner, feature), in source order, and pair each with
    // the admitted target at the same position.
    let mut groups: BTreeMap<(String, String), Vec<&Request>> = BTreeMap::new();
    for r in &requests {
        groups
            .entry((r.owner.clone(), r.feature.clone()))
            .or_default()
            .push(r);
    }
    let mut stripped = graph.clone();
    let index: HashMap<String, usize> = stripped
        .elements
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id.clone(), i))
        .collect();
    let mut oracle: HashMap<(String, String, String), String> = HashMap::new();
    let mut proxies: Vec<(String, String, String)> = Vec::new();
    let mut skipped = Vec::new();
    for ((owner, feature), mut group) in groups {
        group.sort_by_key(|r| r.start);
        let Some(element) = index
            .get(&owner)
            .and_then(|i| stripped.elements.get_mut(*i))
        else {
            skipped.push(format!("{owner}.{feature}: owner missing"));
            continue;
        };
        let value = element.values.get(&feature).cloned().unwrap_or(Value::Null);
        let (targets, many): (Vec<String>, bool) = match &value {
            Value::String(t) => (vec![t.clone()], false),
            Value::Array(items) => (
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect(),
                true,
            ),
            _ => (vec![], false),
        };
        if targets.len() != group.len() {
            skipped.push(format!(
                "{owner}.{feature}: {} requests, {} stored targets",
                group.len(),
                targets.len()
            ));
            continue;
        }
        let mut consistent = true;
        for (r, target) in group.iter().zip(&targets) {
            let key = (owner.clone(), feature.clone(), r.spelling.clone());
            if oracle.get(&key).is_some_and(|t| t != target) {
                consistent = false;
            }
            oracle.insert(key, target.clone());
        }
        if !consistent {
            skipped.push(format!("{owner}.{feature}: one spelling, several targets"));
            continue;
        }
        element.values.insert(
            feature.clone(),
            if many {
                Value::Array(vec![])
            } else {
                Value::Null
            },
        );
        for r in group {
            proxies.push((owner.clone(), feature.clone(), r.spelling.clone()));
        }
    }
    println!(
        "unresolved references created {}, groups skipped {}",
        proxies.len(),
        skipped.len()
    );
    for s in &skipped {
        println!("  skipped {s}");
    }

    // MOF property redefinitions from the metamodel's own annotations
    // (read from the stored SysML structure; not part of the measured freeze).
    let redefinitions = kerml_scopes::redefinitions(&schema)?;
    let clock = Instant::now();
    let mm = Arc::new(
        Metamodel::freeze(&schema)
            .and_then(|mm| mm.with_redefinitions(&redefinitions))
            .map_err(|e| e.to_string())?,
    );
    let freeze = ms(clock);

    let build = || -> Result<ModelBuilder, String> {
        let mut builder = ModelBuilder::import(mm.clone(), &stripped).map_err(|e| e.to_string())?;
        for (owner, feature, spelling) in &proxies {
            let obj = builder.lookup(owner).ok_or("proxy owner")?;
            let class = mm
                .class(&graph.elements[index[owner]].class)
                .ok_or("owner class")?;
            let f = mm.feature(class, feature).ok_or("proxy feature")?;
            builder
                .add_proxy(obj, f, spelling)
                .map_err(|e| e.to_string())?;
        }
        Ok(builder)
    };
    let clock = Instant::now();
    let builder = build()?;
    let import = ms(clock);
    if std::env::args().any(|a| a == "--rcore") {
        return rcore_agreement(&mm, build()?, &oracle);
    }

    let feature_name = |f: FeatureId| mm.feature_name(f).to_owned();
    let resolver = |view: &LinkView<'_>,
                    owner: ObjId,
                    feature: FeatureId,
                    spelling: &str|
     -> Result<Option<ObjId>, FrozenError> {
        let owner = view.identity(owner).unwrap_or("").to_owned();
        Ok(oracle
            .get(&(owner, feature_name(feature), spelling.to_owned()))
            .and_then(|target| view.lookup(target)))
    };
    let clock = Instant::now();
    let linked = builder.link(&resolver).map_err(|e| e.to_string())?;
    let link = ms(clock);

    // Fields the admitted graph omits are redefined by a subclass feature
    // (MOF redefinition); validation checks them through the redefining one.
    let clock = Instant::now();
    let model = linked.validate().map_err(|e| e.to_string())?;
    let validate = ms(clock);

    let clock = Instant::now();
    let exported = model.to_model().map_err(|e| e.to_string())?;
    let export = ms(clock);

    // Compare with the admitted graph.
    let mut expected: Vec<_> = graph.elements.clone();
    let mut actual: Vec<_> = exported.elements.clone();
    expected.sort_by(|a, b| a.id.cmp(&b.id));
    actual.sort_by(|a, b| a.id.cmp(&b.id));
    let mut differences = 0usize;
    for (e, a) in expected.iter().zip(&actual) {
        if e != a {
            differences += 1;
            if differences <= 5 {
                for (k, v) in &e.values {
                    if a.values.get(k) != Some(v) {
                        println!(
                            "  differs {}.{k}: admitted {v} frozen {:?}",
                            e.id,
                            a.values.get(k)
                        );
                    }
                }
            }
        }
    }
    let same_count = expected.len() == actual.len();
    let same_resources = exported.resources == graph.resources;
    println!(
        "graph equal: {} (elements {} vs {}, differing {differences}, resources equal {same_resources})",
        same_count && differences == 0 && same_resources,
        expected.len(),
        actual.len()
    );
    let clock = Instant::now();
    let oracle_check = exported.validate(&schema);
    println!(
        "existing structural validator, without KerML structural value providers: {} ({:.1} ms)",
        if oracle_check.is_ok() {
            "accepted".to_owned()
        } else {
            format!("{oracle_check:?}")
        },
        ms(clock)
    );
    println!(
        "freeze {freeze:.2} ms, import {import:.2} ms, link {link:.2} ms, validate {validate:.2} ms, export {export:.2} ms"
    );
    if !(same_count && differences == 0 && same_resources) {
        return Err("Frozen graph differs from the admitted graph".into());
    }
    Ok(())
}

/// Link with the source-authored KerML name resolution and compare each
/// request's candidates with the admitted target. Linking itself uses the
/// admitted target, so one disagreement does not change later requests.
fn rcore_agreement(
    mm: &Arc<Metamodel>,
    builder: ModelBuilder,
    oracle: &HashMap<(String, String, String), String>,
) -> Result<(), String> {
    use intersect_model::frozen_scopes::{Execution, RcoreScopes};
    use std::cell::RefCell;
    let clock = Instant::now();
    let (bundle, scopes, cached) = kerml_scopes::compile_cached(mm)?;
    println!(
        "{} {} scopes in {:.1} ms",
        if cached { "loaded saved" } else { "compiled" },
        scopes.len(),
        ms(clock)
    );
    // The direct executor is the authority for scopes (roadmap decision D8);
    // `--shadow` also runs the VM and reports disagreements, `--vm` runs the
    // VM alone. `--fast` is accepted as the default it used to name.
    let execution = if std::env::args().any(|a| a == "--vm") {
        Execution::Vm
    } else if std::env::args().any(|a| a == "--shadow") {
        Execution::Shadow
    } else {
        Execution::Fast
    };
    let resolvers = RcoreScopes::new(mm, &bundle, &scopes)
        .map_err(|e| e.to_string())?
        .with_execution(execution);
    // (owner, feature, spelling, admitted target, Rcore candidates)
    type Outcome = (String, String, String, String, Result<Vec<String>, String>);
    let results: RefCell<Vec<Outcome>> = RefCell::new(Vec::new());
    let record = |view: &LinkView<'_>,
                  owner: ObjId,
                  feature: FeatureId,
                  spelling: &str|
     -> Result<Option<ObjId>, FrozenError> {
        let owner_id = view.identity(owner).unwrap_or("").to_owned();
        let feature_name = mm.feature_name(feature).to_owned();
        let expected = oracle
            .get(&(owner_id.clone(), feature_name.clone(), spelling.to_owned()))
            .cloned()
            .unwrap_or_default();
        let found = resolvers
            .candidates(view, owner, feature, spelling)
            .map(|c| {
                c.iter()
                    .filter_map(|o| view.identity(*o))
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .map_err(|e| e.to_string());
        results.borrow_mut().push((
            owner_id,
            feature_name,
            spelling.to_owned(),
            expected.clone(),
            found,
        ));
        Ok(view.lookup(&expected))
    };
    let clock = Instant::now();
    let linked = builder.link(&record);
    let link = ms(clock);
    let results = results.into_inner();
    let agree = results
        .iter()
        .filter(|(_, _, _, expected, found)| {
            found.as_ref().is_ok_and(|f| f == &vec![expected.clone()])
        })
        .count();
    println!(
        "Rcore name resolution agrees on {agree} of {} requests (link {link:.2} ms, link ok: {})",
        results.len(),
        linked.is_ok()
    );
    let short = |id: &str| id.rsplit('/').next().unwrap_or(id).to_owned();
    for (owner, feature, spelling, expected, found) in &results {
        let ok = found.as_ref().is_ok_and(|f| f == &vec![expected.clone()]);
        if !ok {
            let found = match found {
                Ok(f) => format!("{:?}", f.iter().map(|i| short(i)).collect::<Vec<_>>()),
                Err(e) => format!("error: {}", e.chars().take(300).collect::<String>()),
            };
            println!(
                "  {} .{feature} '{spelling}': expected {} got {found}",
                short(owner),
                short(expected)
            );
        }
    }
    println!("executor stats: {:?}", resolvers.stats());
    for scope in &scopes {
        if let crate_member @ Some(_) = scope_unsupported(&resolvers, mm, scope) {
            println!("  direct executor refuses a scope: {crate_member:?}");
        }
    }
    if agree != results.len() {
        return Err(format!("{} requests disagree", results.len() - agree));
    }
    Ok(())
}

fn scope_unsupported(
    resolvers: &intersect_model::frozen_scopes::RcoreScopes<'_>,
    mm: &Metamodel,
    scope: &intersect_model::definitions::rcore_scope::VerifiedScope,
) -> Option<&'static str> {
    let intersect_model::definitions::schema::MemberIdentity::Source { declaration } =
        scope.member()
    else {
        return None;
    };
    resolvers.unsupported(mm.feature_by_declaration(declaration)?)
}
