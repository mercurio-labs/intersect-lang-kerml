//! Diagnostic observations around the unchanged public stored-read adapter.
//! Every operation is forwarded with the original VM's work/storage/control.
//! Instrumentation bookkeeping is outside the VM; this is not acceptance/timing.
use super::*;
use intersect_model::{
    core::query::QueryBudget,
    definitions::{
        bundle::{DefinitionSource, SourceKind},
        conversion::{InvocationContext, InvocationObserver, ProviderId},
        ecore::ParseLimits,
        rcore_behavior::{
            ir_v2::{EntrySelection, Limits},
            modules::ModuleIndexPreparation,
        },
        resources::DeclarationId,
    },
    rcore::ExecutionLimits,
    scope_runtime::{ScopePage, ScopeRead, ScopeValue},
    source_model_functions::SourceModelFunction,
    source_model_reads::SourceModelReads,
};
use std::cell::Cell;

struct Observation<'a> {
    original: InvocationContext<'a>,
    work: Cell<usize>,
    storage: Cell<usize>,
    refused_storage: Cell<usize>,
}
impl InvocationObserver for Observation<'_> {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Err("Diagnostic stored reader unexpectedly invoked a semantic provider".into())
    }
    fn failed_result(&self, error: &str) {
        self.original.failed_result(error);
    }
    fn checkpoint(&self) -> Result<(), String> {
        self.original.checkpoint()
    }
    fn work(&self, units: usize) -> Result<(), String> {
        self.original.work(units)?;
        self.work.set(self.work.get().saturating_add(units));
        Ok(())
    }
    fn retain_storage(&self, bytes: usize) -> Result<(), String> {
        match self.original.retain_storage(bytes) {
            Ok(()) => {
                self.storage.set(self.storage.get().saturating_add(bytes));
                Ok(())
            }
            Err(error) => {
                self.refused_storage.set(bytes);
                Err(error)
            }
        }
    }
}
struct Profile<'a> {
    inner: SourceModelReads<'a>,
    events: RefCell<Vec<Value>>,
}
impl Profile<'_> {
    fn read<T>(
        &self,
        object: &str,
        member: &DeclarationId,
        cursor: Option<usize>,
        original: &InvocationContext<'_>,
        read: impl FnOnce(&InvocationContext<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        if self.events.borrow().len() >= 10_000 {
            return Err("Diagnostic read-event limit exceeded".into());
        }
        let observation = Rc::new(Observation {
            original: original.clone(),
            work: Cell::new(0),
            storage: Cell::new(0),
            refused_storage: Cell::new(0),
        });
        let result = read(&InvocationContext::new(observation.clone()));
        self.events.borrow_mut().push(json!({
            "object":object,"member":member,"cursor":cursor,
            "work":observation.work.get(),"storage":observation.storage.get(),
            "refusedStorage":observation.refused_storage.get(),
            "error":result.as_ref().err(),
        }));
        result
    }
}
impl ScopeRead for Profile<'_> {
    fn check(&self, observation: &InvocationContext<'_>) -> Result<(), String> {
        self.inner.check(observation)
    }
    fn class(&self, object: &str, observation: &InvocationContext<'_>) -> Result<String, String> {
        self.inner.class(object, observation)
    }
    fn singular(
        &self,
        object: &str,
        member: &DeclarationId,
        observation: &InvocationContext<'_>,
    ) -> Result<ScopeValue, String> {
        self.read(object, member, None, observation, |observed| {
            self.inner.singular(object, member, observed)
        })
    }
    fn page(
        &self,
        object: &str,
        member: &DeclarationId,
        cursor: usize,
        observation: &InvocationContext<'_>,
    ) -> Result<ScopePage, String> {
        self.read(object, member, Some(cursor), observation, |observed| {
            self.inner.page(object, member, cursor, observed)
        })
    }
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn m14_profile_actual_stored_reads() -> Result<(), String> {
    let mut host = Host::default();
    let pack = std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?;
    host.load_pack_for_analysis(std::fs::read(pack).map_err(|e| e.to_string())?)?;
    let analysis = host.dispatch(json!({"method":"workspace/analyze","params":{
        "revision":1,"projectNodesOnly":true,"sources":[{
            "uri":"file:///m14/behavior.sysml",
            "text":include_str!("../../tests/fixtures/m14-behavior.sysml")
        }]
    }}))?;
    assert_eq!(analysis["clean"], true, "{analysis}");
    let loaded = host.loaded.as_ref().ok_or("No prepared language")?;
    let prepared = loaded.session.sysml_definition()?;
    let model = host
        .views
        .as_ref()
        .and_then(|v| v.accepted_model())
        .ok_or("No accepted model")?;
    let root = model
        .objects()
        .find(|o| {
            o.resource() == "file:///m14/behavior.sysml" && crate::name(*o) == Some("pipeline")
        })
        .ok_or("No actual pipeline")?;
    let root_identity = root.identity().to_owned();
    let store = intersect_view_host::function_store(model.clone())?;
    let text = std::env::var("M14_NAVIGATION_SOURCE")
        .ok()
        .map(std::fs::read_to_string)
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| include_str!("../../assets/view-navigation.rcore").into());
    let text = format!(
        "{text}\n{}",
        crate::view_rendering::Registry::prepare(loaded)?.control_functions()?
    );
    let mut source = DefinitionSource::new(
        "package:/sysml/views/navigation.rcore",
        SourceKind::RcoreBehavior,
        text,
    );
    source.dependencies.push(prepared.selection().ecore.clone());
    let mut bundle = prepared.sources().clone();
    bundle.sources.push(source.clone());
    let observation = InvocationContext::default();
    let mut indexing = ModuleIndexPreparation::begin(
        &bundle,
        &source.id,
        ParseLimits {
            source_bytes: 1_000_000,
        },
        &observation,
    )
    .map_err(|e| format!("{e:?}"))?;
    while !indexing
        .advance(&observation)
        .map_err(|e| format!("{e:?}"))?
    {}
    let index = indexing.finish().map_err(|e| format!("{e:?}"))?;
    let names = [
        "m14FeatureCandidates",
        "m14ConcreteCandidates",
        "m14RedefinedTargets",
        "features",
    ];
    let entries = names
        .iter()
        .map(|name| {
            let symbol = index
                .module(&source.id)?
                .symbols
                .iter()
                .find(|s| s.name.join(".") == *name)
                .ok_or_else(|| format!("Missing diagnostic query {name}"))?;
            Ok(EntrySelection {
                symbol: symbol.id.clone(),
                types: vec![],
                arguments: vec![],
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let functions = SourceModelFunction::from_prepared_many(
        prepared.model(),
        vec![source.clone()],
        &entries,
        &Limits::default(),
        ExecutionLimits {
            max_model_storage_bytes: 64 * 1024 * 1024,
            ..Default::default()
        },
        ParseLimits {
            source_bytes: 1_000_000,
        },
    )?;
    let mut queries = Vec::new();
    for (name, function) in names.iter().zip(functions) {
        let reads = Profile {
            inner: SourceModelReads::new(&store, prepared.schema()),
            events: RefCell::new(Vec::new()),
        };
        let mut budget = QueryBudget::new(32_000_000);
        let result = function.invoke_with(&reads, &root_identity, vec![], &mut budget);
        // The reusable public adapter remains an uncached control. The ordinary
        // entry is the actual host path with an invocation-local reader.
        let mut model_budget = QueryBudget::new(32_000_000);
        let model_result = function.invoke_model(&store, &root_identity, vec![], &mut model_budget);
        let events = reads.events.into_inner();
        let storage: u64 = events.iter().filter_map(|e| e["storage"].as_u64()).sum();
        eprintln!(
            "M14 stored-read profile {name}: reads={} observed storage={storage}, error={:?}",
            events.len(),
            result.as_ref().err()
        );
        eprintln!(
            "M14 ordinary model entry {name}: error={:?}",
            model_result.as_ref().err()
        );
        queries.push(
            json!({"name":name,"error":result.as_ref().err(),"result":result.as_ref().ok(),
            "remainingWork":budget.remaining(),"observedReadStorage":storage,"events":events,
            "modelError":model_result.as_ref().err(),"modelResult":model_result.as_ref().ok(),
            "modelRemainingWork":model_budget.remaining()}),
        );
    }
    let failures = queries
        .iter()
        .filter(|q| !q["modelError"].is_null())
        .map(|q| format!("{}: {}", q["name"], q["modelError"]))
        .collect::<Vec<_>>();
    let output = std::env::var("M14_READ_PROFILE").map_err(|e| e.to_string())?;
    std::fs::write(output,serde_json::to_vec_pretty(&json!({
        "status":"uncached control diagnostics and corrected ordinary model entries; actual accepted model; not G2 acceptance or timing",
        "source":source.id,"root":root_identity,"analysis":analysis,"queries":queries,
        "scope":"storage counts cover delegated stored reads, excluding VM admission and other operations; original ledgers remain active"
    })).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Corrected ordinary model entries failed: {}; full receipt retained",
            failures.join("; ")
        ))
    }
}
