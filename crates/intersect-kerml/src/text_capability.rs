//! Handwritten capability pieces of the frozen-engine text load (not
//! generated). They do not depend on the native draft (K01) pipeline.
//!
//! The frozen load parses with `intersect_text::draft::OpenDraftCapability`,
//! which admits every stored feature the grammar assigns and constructs every
//! concrete class at its schema defaults. Of its inner capability it uses only
//! the source identities, the Boolean rule conversions (`Nonunique` and, for
//! SysML, the structural Boolean literal) and delegated assignments (none
//! here). The inner capabilities below are therefore source gates: each checks
//! that the prepared definition is the exact pinned KerML or SysML owner, and
//! carries no constructor table or assignment table of its own.
//!
//! The KerML `Name` value conversion (`kerml_string_rules`) and the computed
//! setting providers (`crate::operand_settings`, `sysml_setting_providers`)
//! complete the binding.
use intersect_model::core::query::QueryBudget;
use intersect_model::core::query::QueryResult;
use intersect_model::definitions::bundle::SourceId;
use intersect_model::definitions::conversion::InvocationContext;
use intersect_model::definitions::schema::MemberIdentity;
use intersect_model::definitions::setting::ComputedSettingContract;
use intersect_model::definitions::setting::SettingOperation;
use intersect_model::storage::DraftClassContract;
use intersect_model::storage::DraftPhysicalDefaults;
use intersect_text::draft::DraftCapability;
use intersect_text::grammar::Profile;
use intersect_text::preparation::PreparedLanguageDefinition;
use std::collections::BTreeMap;

#[path = "kerml_name_conversion.rs"]
pub(crate) mod name_conversion;
#[path = "nonunique_conversion.rs"]
pub(crate) mod nonunique_conversion;
#[path = "kerml_structural_literals.rs"]
pub(crate) mod structural_literals;
#[path = "sysml_source_check.rs"]
pub(crate) mod sysml_source_check;

pub(crate) use nonunique_conversion::sysml_boolean_draft_capability;
pub use nonunique_conversion::{KerMLNonuniqueCapability, nonunique_draft_capability};

pub(crate) const EXPRESSIONS: &str =
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext";
pub(crate) const KERML: &str = "org.omg.kerml.xtext/src/org/omg/kerml/xtext/KerML.xtext";
pub(crate) const SCHEMA: &str = "org.omg.sysml/model/SysML.ecore";
pub(crate) const EXPRESSION_SHA: &str =
    "7bea42879a913f7613ac5e32779174db01e8fd4fe75694f0845c495aa0191aa8";
pub(crate) const KERML_SHA: &str =
    "cc02be703955603d2daa91f878f299f950195fcd2e3c59563ae17643ba91dfaa";
pub(crate) const EMPTY_BEHAVIOR_SHA: &str =
    "5eaff959a081947c82b71b13afd07e0547e2acb92cbc3bf98efba24511760c8c";
pub(crate) const REVISION: &str =
    "14328861d658861a80924bfdae4ac95f62bea53b3d64e0ccc886a9fdda72c0b1";

/// The source identities of one prepared definition: (original, converted).
pub(crate) fn source_ids(
    definition: &PreparedLanguageDefinition<'_>,
) -> (Vec<SourceId>, Vec<SourceId>) {
    let ids = |bundle: &intersect_model::definitions::bundle::DefinitionBundle| {
        bundle
            .sources
            .iter()
            .map(|source| source.id.clone())
            .collect()
    };
    (
        ids(definition.original_sources()),
        ids(definition.sources()),
    )
}

/// The exact pinned KerML definition: the four original and four converted
/// sources, the schema revision and the complete KerML grammar of `profile`.
pub(crate) fn check_kerml_definition(
    definition: &PreparedLanguageDefinition<'_>,
    profile: Profile,
) -> Result<(Vec<SourceId>, Vec<SourceId>), String> {
    definition.original_sources().validate()?;
    definition.sources().validate()?;
    let (original, converted) = source_ids(definition);
    let wanted: BTreeMap<_, _> = [
        (SCHEMA, crate::PILOT_SCHEMA_SHA256),
        (EXPRESSIONS, EXPRESSION_SHA),
        (KERML, KERML_SHA),
        ("probe-behavior.rcore", EMPTY_BEHAVIOR_SHA),
    ]
    .into();
    if original.len() != wanted.len()
        || original
            .iter()
            .any(|source| wanted.get(source.uri.as_str()).copied() != Some(source.sha256.as_str()))
    {
        return Err("Unsupported minimal KerML draft original source closure".into());
    }
    let wanted: BTreeMap<_, _> = [
        (
            "org.omg.sysml/model/SysML.ecore.rcore",
            crate::PILOT_RCORE_SCHEMA_SHA256,
        ),
        (EXPRESSIONS, EXPRESSION_SHA),
        (KERML, KERML_SHA),
        ("probe-behavior.rcore", EMPTY_BEHAVIOR_SHA),
    ]
    .into();
    if converted.len() != wanted.len()
        || converted
            .iter()
            .any(|source| wanted.get(source.uri.as_str()).copied() != Some(source.sha256.as_str()))
    {
        return Err("Unsupported minimal KerML draft converted source closure".into());
    }
    let schema = definition.schema();
    let grammar = definition
        .grammar()
        .ok_or("Missing complete KerML grammar")?;
    if schema.revision != REVISION
        || schema.classes.len() != 175
        || grammar.entry != "RootNamespace"
        || grammar.rules.len() != 273
        || grammar.terminals.len() != 9
        || grammar.profile != profile
    {
        return Err("Minimal draft requires the complete exact prepared KerML definition".into());
    }
    Ok((original, converted))
}

/// The agreement on the seven pinned `operand` computed-setting contracts
/// (one per `InvocationExpression` class): each contract's class and member.
pub(crate) fn operand_agreement(
    definition: &PreparedLanguageDefinition<'_>,
    converted: &[SourceId],
    contracts: &[ComputedSettingContract],
) -> QueryResult<BTreeMap<String, MemberIdentity>> {
    let schema = definition.schema();
    let expected = schema
        .classes
        .keys()
        .filter(|class| schema.conforms(class, "InvocationExpression"))
        .collect::<Vec<_>>();
    if contracts.len() != 7 || expected.len() != 7 {
        return Err("K01 computed admission requires the original seven operand contracts".into());
    }
    let mut computed = BTreeMap::new();
    let mut validated = None;
    for contract in contracts {
        // Validated at the first contract, as before, and then once only.
        let validated = match validated {
            Some(validated) => validated,
            None => *validated.insert(definition.sources().validated()?),
        };
        // The handwritten provider validates every selected contract; no
        // provider is installed by this metadata agreement.
        crate::OperandSettings::new_in(schema, &validated, contract.clone())?;
        if contract.sources != converted
            || contract.operations != [SettingOperation::Append]
            || !expected.contains(&&contract.class)
            || computed
                .insert(contract.class.clone(), contract.member.clone())
                .is_some()
        {
            return Err(
                "K01 operand contracts have foreign sources, modes or duplicate classes".into(),
            );
        }
    }
    if expected
        .iter()
        .any(|class| !computed.contains_key(class.as_str()))
    {
        return Err("K01 operand contract class coverage is incomplete".into());
    }
    Ok(computed)
}

/// The inner capability of one exact pinned language owner for
/// `OpenDraftCapability`: its source identities and, for KerML, the agreed
/// `operand` computed assignments. It admits no stored assignment and holds no
/// constructor contracts; the open capability supplies both from the grammar
/// and the metamodel.
/// ```compile_fail
/// use intersect_kerml::text_capability::PinnedTextCapability;
/// let forged = serde_json::from_str::<PinnedTextCapability>("{}");
/// ```
pub struct PinnedTextCapability {
    original: Vec<SourceId>,
    converted: Vec<SourceId>,
    computed: BTreeMap<String, MemberIdentity>,
}

impl DraftPhysicalDefaults for PinnedTextCapability {
    fn contracts(&self) -> &[DraftClassContract] {
        &[]
    }
}

impl DraftCapability for PinnedTextCapability {
    const NEEDS_VALUE_ADMISSION: bool = true;
    const RECONCILE_PARSED_CONTAINMENT: bool = true;
    fn original_sources(&self) -> &[SourceId] {
        &self.original
    }
    fn converted_sources(&self) -> &[SourceId] {
        &self.converted
    }
    /// The open capability admits every stored feature the grammar assigns;
    /// it asks the inner capability only about derived features, and no
    /// derived feature is assignable here.
    fn permits_assignment(&self, _: &str, _: &MemberIdentity, _: bool) -> bool {
        false
    }
    fn permits_computed_assignment(
        &self,
        actual: &str,
        member: &MemberIdentity,
        append: bool,
    ) -> bool {
        append
            && self
                .computed
                .get(actual)
                .is_some_and(|expected| expected == member)
    }
}

/// The KerML capability of the frozen load: the exact pinned NativeV80 KerML
/// definition with its seven `operand` contracts. Wrap it with
/// `nonunique_draft_capability` and `OpenDraftCapability`.
pub fn kerml_capability(
    definition: &PreparedLanguageDefinition<'_>,
    contracts: &[ComputedSettingContract],
) -> QueryResult<PinnedTextCapability> {
    let (original, converted) = check_kerml_definition(definition, Profile::NativeV80)?;
    let computed = operand_agreement(definition, &converted, contracts)?;
    Ok(PinnedTextCapability {
        original,
        converted,
        computed,
    })
}

/// The inner capability for SysML text on the frozen path: the exact pinned
/// NativeV80 SysML definition with SysML's two Boolean rule conversions
/// (`Nonunique` and the structural Boolean literal), for `OpenDraftCapability`.
pub fn sysml_capability(
    definition: &PreparedLanguageDefinition<'_>,
    preparation: &InvocationContext<'_>,
    budget: &mut QueryBudget,
) -> Result<KerMLNonuniqueCapability<PinnedTextCapability, 2>, String> {
    let (original, converted) = sysml_source_check::source_closure(definition)?;
    let pinned = PinnedTextCapability {
        original,
        converted,
        computed: BTreeMap::new(),
    };
    sysml_boolean_draft_capability(definition, pinned, preparation, budget)
}

/// One provider per computed-setting contract of the SysML grammar: the
/// Pilot `operand` extension and the SysML `isReference` setting.
pub fn sysml_setting_providers<'a>(
    definition: &'a PreparedLanguageDefinition<'_>,
    contracts: &[ComputedSettingContract],
) -> Result<Vec<Box<dyn intersect_model::setting::SourceSettingValues + 'a>>, String> {
    if contracts.is_empty() {
        return Ok(Vec::new());
    }
    // One bundle validation for all the contracts, not one each.
    let bundle = definition.sources().validated()?;
    contracts
        .iter()
        .map(
            |contract| -> Result<Box<dyn intersect_model::setting::SourceSettingValues + 'a>, String> {
                if contract.provider.name == crate::OPERAND_PROVIDER {
                    Ok(Box::new(crate::OperandSettings::new_in(
                        definition.schema(),
                        &bundle,
                        contract.clone(),
                    )?))
                } else if contract.provider.name == crate::sysml_usage::USAGE_REFERENCE_PROVIDER {
                    Ok(Box::new(crate::sysml_usage::UsageReferenceSettings::new_in(
                        definition.schema(),
                        &bundle,
                        contract.clone(),
                    )?))
                } else {
                    Err(format!(
                        "Unknown SysML setting provider {}",
                        contract.provider.name
                    ))
                }
            },
        )
        .collect()
}

/// KerML's `Name` value conversion (unrestricted names lose their quotes and
/// escapes, as in the Pilot's value converter) and its `REGULAR_COMMENT`
/// terminal conversion, for `OpenDraftCapability::with_string_rules`.
pub struct KerMLStringRules<'g>(name_conversion::KerMLNameConversion<'g>);

/// The KerML string rule conversions bound to `definition`'s grammar.
pub fn kerml_string_rules<'g>(
    definition: &'g PreparedLanguageDefinition<'_>,
    budget: &mut QueryBudget,
) -> QueryResult<KerMLStringRules<'g>> {
    name_conversion::KerMLNameConversion::bind(definition, budget).map(KerMLStringRules)
}

impl intersect_text::draft::StringRuleConversion for KerMLStringRules<'_> {
    fn convert(
        &self,
        grammar: &intersect_text::grammar::Grammar,
        rule: &str,
        spelling: &str,
        budget: &mut QueryBudget,
    ) -> intersect_model::storage::StorageResult<Option<String>> {
        self.0.convert(grammar, rule, spelling, budget)
    }
    /// A `REGULAR_COMMENT` (`/* ... */`) becomes its body as the Pilot stores
    /// it: without the delimiters, with LF line ends, without a leading blank
    /// line, without the first line's leading whitespace, and without each
    /// later line's indentation and leading `*`.
    ///
    /// A `STRING_VALUE` (`"..."`: a string literal's value, a comment's
    /// `locale`, a textual representation's `language`) loses its quotes and
    /// escapes, as the Pilot's parser post-processors store it
    /// (`ElementUtil.unescapeString`).
    fn convert_terminal(
        &self,
        terminal: &str,
        text: &str,
    ) -> intersect_model::storage::StorageResult<Option<String>> {
        if terminal == "REGULAR_COMMENT" || terminal.ends_with("::REGULAR_COMMENT") {
            return Ok(Some(comment_body(text)));
        }
        if terminal == "STRING_VALUE" || terminal.ends_with("::STRING_VALUE") {
            return Ok(Some(unescape_string(text)));
        }
        Ok(None)
    }
}

/// The Pilot's `ElementUtil.unescapeString`: text that starts with a quote
/// loses its first and last characters, and each backslash escape becomes its
/// character (`\b \t \n \f \r \" \' \\`; any other escaped character becomes
/// a space). Other text is returned unchanged.
fn unescape_string(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if !matches!(chars.first(), Some('"' | '\'')) {
        return text.to_owned();
    }
    let len = chars.len();
    let mut value = String::with_capacity(text.len());
    let mut start = 1;
    while let Some(offset) = chars
        .get(start..)
        .and_then(|rest| rest.iter().position(|c| *c == '\\'))
    {
        let escape = start + offset;
        value.extend(chars.get(start..escape).unwrap_or_default());
        value.push(match chars.get(escape + 1) {
            Some('b') => '\u{0008}',
            Some('t') => '\t',
            Some('n') => '\n',
            Some('f') => '\u{000c}',
            Some('r') => '\r',
            Some('"') => '"',
            Some('\'') => '\'',
            Some('\\') => '\\',
            _ => ' ',
        });
        start = escape + 2;
    }
    if len > 1 && start < len - 1 {
        value.extend(chars.get(start..len - 1).unwrap_or_default());
    }
    value
}

fn comment_body(text: &str) -> String {
    let inner = text.strip_prefix("/*").unwrap_or(text);
    let inner = inner.strip_suffix("*/").unwrap_or(inner);
    let normalized = inner.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = Vec::new();
    for (index, line) in normalized.split('\n').enumerate() {
        if index == 0 {
            lines.push(line.trim_start().to_owned());
            continue;
        }
        let line = line.trim_start();
        let line = match line.strip_prefix('*') {
            Some(rest) => rest.strip_prefix(' ').unwrap_or(rest),
            None => line,
        };
        lines.push(line.to_owned());
    }
    if lines.first().is_some_and(|first| first.trim().is_empty()) && lines.len() > 1 {
        lines.remove(0);
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::unescape_string;

    #[test]
    fn string_values_lose_quotes_and_escapes_as_in_the_pilot() {
        assert_eq!(unescape_string("\"kelvin\""), "kelvin");
        assert_eq!(unescape_string("\"\""), "");
        assert_eq!(unescape_string("\"a\\\"b\\\\c\\n\""), "a\"b\\c\n");
        assert_eq!(unescape_string("\"x\\qy\""), "x y");
        assert_eq!(unescape_string("'single'"), "single");
        assert_eq!(unescape_string("plain"), "plain");
    }
}
