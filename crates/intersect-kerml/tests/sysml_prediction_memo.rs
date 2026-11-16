//! Handwritten tests (not generated): the runtime prediction memo of a bound
//! SysML language reuses decisions across sources without changing what they
//! parse to. Each source is parsed with the memo off, then twice with it in
//! `Verify` mode (every reused decision is also predicted uncached and must
//! agree) and once on; every parse must give the same model and diagnostics.
//! SysML preparation takes about half a minute, so this is milestone-only.
use intersect_kerml::{frozen_load, native_pilot::NativePilotSession};
use intersect_model::core::frozen::FrozenStore;
use intersect_model::core::storage;
use intersect_model::definitions::conversion::InvocationContext;
use intersect_model::definitions::conversion::InvocationObserver;
use intersect_model::definitions::conversion::ProviderId;
use intersect_text::Source;
use intersect_text::grammar::RuntimePredictionMemoMode as Mode;
use std::rc::Rc;

const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-pilot/v1/sources/"
);
const DEFINITIONS: [&str; 4] = [
    "org.omg.sysml/model/SysML.ecore",
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext",
    "org.omg.kerml.xtext/src/org/omg/kerml/xtext/KerML.xtext",
    "org.omg.sysml.xtext/src/org/omg/sysml/xtext/SysML.xtext",
];
const LIBRARY: [&str; 2] = [
    "sysml.library/Domain Libraries/Quantities and Units/SIPrefixes.sysml",
    "sysml.library/Domain Libraries/Quantities and Units/MeasurementRefCalculations.sysml",
];

struct Observer;
impl InvocationObserver for Observer {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Ok(())
    }
}

fn read(path: &str) -> Result<Source, String> {
    Ok(Source {
        uri: path.replace(' ', "%20"),
        text: std::fs::read_to_string(format!("{CORPUS}{path}"))
            .map_err(|error| format!("{path}: {error}"))?,
    })
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn memoized_sysml_parses_equal_uncached_parses() -> Result<(), String> {
    std::thread::Builder::new()
        .name("sysml-prediction-memo".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(compare)
        .map_err(|error| error.to_string())?
        .join()
        .map_err(|_| "Prediction memo comparison panicked".to_owned())?
}

fn compare() -> Result<(), String> {
    let definitions = DEFINITIONS
        .iter()
        .map(|path| read(path))
        .collect::<Result<Vec<_>, _>>()?;
    let mut sources = LIBRARY
        .iter()
        .map(|path| read(path))
        .collect::<Result<Vec<_>, _>>()?;
    for (uri, text) in [
        (
            "probe/redefinitions.sysml",
            "package P { attribute a : T { :>> x; :>> y = 1 + 2 * 3; } attribute b = a.x; }",
        ),
        (
            "probe/redefinitions-again.sysml",
            "package Q {\n  // hidden tokens move absolute positions\n  attribute a : T { :>> x; :>> y = 4 + 5; }\n}",
        ),
        (
            "probe/error.sysml",
            "package R { attribute a : T { :>> ; } }",
        ),
    ] {
        sources.push(Source {
            uri: uri.into(),
            text: text.into(),
        });
    }
    let preparation = InvocationContext::new(Rc::new(Observer));
    let session = NativePilotSession::prepare(&definitions, &preparation)?;
    session.with_definitions(|_, _, sysml, contracts| {
        let metamodel = frozen_load::frozen_metamodel(sysml)?;
        frozen_load::with_sysml(sysml, contracts, &preparation, |language| {
            let memo = language.language.prediction_memo();
            let parse = |source: &Source| {
                let mut store = FrozenStore::new(metamodel.clone());
                let result = language.parse(source, &mut store);
                let model = storage::export(&store).map_err(|error| error.to_string())?;
                Ok::<_, String>(match result {
                    Ok((evaluation, proxies, consumed)) => (
                        Ok((
                            evaluation
                                .diagnostics
                                .iter()
                                .map(|d| (d.start, d.end, d.message.clone()))
                                .collect::<Vec<_>>(),
                            proxies,
                            consumed,
                        )),
                        model,
                    ),
                    Err(error) => (Err(error), model),
                })
            };
            memo.set_mode(Mode::Off);
            let uncached = sources.iter().map(parse).collect::<Result<Vec<_>, _>>()?;
            assert_eq!(memo.stats().inserted, 0);
            for mode in [Mode::Verify, Mode::Verify, Mode::On] {
                memo.set_mode(mode);
                for (source, expected) in sources.iter().zip(&uncached) {
                    let actual = parse(source)?;
                    assert!(&actual == expected, "{}: {mode:?} differs", source.uri);
                }
            }
            // Every source but the last parses cleanly; the last is rejected.
            for ((parsed, model), source) in uncached.iter().zip(&sources) {
                let clean = parsed
                    .as_ref()
                    .is_ok_and(|(diagnostics, _, _)| diagnostics.is_empty());
                let last = source.uri == "probe/error.sysml";
                assert!(clean != last, "{}: {parsed:?}", source.uri);
                assert!(last || model.elements.len() > 10, "{}", source.uri);
            }
            let stats = memo.stats();
            assert!(stats.verified > 1_000, "{stats:?}");
            assert!(stats.hits > 1_000, "{stats:?}");
            Ok(())
        })
    })
}
