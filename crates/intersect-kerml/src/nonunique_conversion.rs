// Handwritten Rust source-bound conversion; no Java production dependency.
// Lineage: Pilot KerML.xtext / KerMLValueConverterService.xtend /
// NonuniqueValueConverter.xtend, Copyright2026 Model Driven Solutions, EPL-2.0.
//! Exact parser-admitted Nonunique conversion; no new physical/read authority.
use super::{KERML, KERML_SHA};
use intersect_model::core::query::QueryBudget;
use intersect_model::definitions::bundle::SourceId;
use intersect_model::definitions::bundle::SourceKind;
use intersect_model::definitions::conversion::ProviderId;
use intersect_model::definitions::schema::MemberIdentity;
use intersect_model::storage::DraftClassContract;
use intersect_model::storage::DraftPhysicalDefaults;
use intersect_model::storage::StorageResult;
use intersect_text::draft::DraftCapability;
use intersect_text::grammar::Datatype;
use intersect_text::grammar::Kind;
use intersect_text::preparation::PreparedLanguageDefinition;
use intersect_text::rule_conversion;
use intersect_text::rule_conversion::RuleConversion;
use serde_json::Value;

/// Source-qualified conversion composed with an existing native draft policy.
/// Existing class/member/assignment checks are forwarded without extension.
/// Prepared linking/read traits are intentionally not fabricated by this wrapper.
/// ```compile_fail
/// use intersect_kerml::text_capability::{KerMLNonuniqueCapability, PinnedTextCapability};
/// let forged = serde_json::from_str::<KerMLNonuniqueCapability<PinnedTextCapability>>("{}");
/// ```
pub struct KerMLNonuniqueCapability<C, const N: usize = 1> {
    inner: C,
    bindings: [RuleConversion; N],
}
impl<C, const N: usize> KerMLNonuniqueCapability<C, N> {}

pub fn nonunique_draft_capability<C: DraftCapability>(
    definition: &PreparedLanguageDefinition<'_>,
    inner: C,
    budget: &mut QueryBudget,
) -> Result<KerMLNonuniqueCapability<C>, String> {
    nonunique_in_context(
        definition,
        inner,
        budget,
        "org.omg.kerml.xtext.KerML",
        KERML,
        KERML_SHA,
    )
}

fn nonunique_in_context<C: DraftCapability>(
    definition: &PreparedLanguageDefinition<'_>,
    inner: C,
    budget: &mut QueryBudget,
    context: &str,
    source_uri: &str,
    source_sha: &str,
) -> Result<KerMLNonuniqueCapability<C>, String> {
    budget.charge()?;
    // Do not silently replace an already selected language conversion policy.
    // General multi-policy composition needs its own checked source contract.
    if C::HAS_BOOLEAN_RULE_CONVERSIONS {
        return Err("Nonunique requires an unextended Boolean conversion policy".into());
    }
    let grammar = definition.grammar().ok_or("Missing exact KerML grammar")?;
    budget.charge()?;
    let declaration = grammar
        .declaration_sources
        .get("Nonunique")
        .ok_or("Missing exact Nonunique source declaration")?;
    if grammar.name != context
        || declaration.source.uri != source_uri
        || declaration.source.sha256 != source_sha
    {
        return Err("Unsupported Nonunique source/context identity".into());
    }
    budget.charge()?;
    let rule = grammar
        .rules
        .get("Nonunique")
        .ok_or("Missing exact Nonunique rule")?;
    if rule.datatype != Some(Datatype::EBoolean)
        || rule.nominal_return.is_some()
        || rule.fragment
        || rule.wildcard
        || rule.body.cardinality != '1'
        || !matches!(&rule.body.kind, Kind::Literal(value) if value == "nonunique")
    {
        return Err("Unsupported Nonunique return/source shape".into());
    }
    // Reuse this opaque prepared definition's immutable validated source rows;
    // do not hide a second complete bundle/hash validation in SourceSpan::text.
    let mut original_source = None;
    for source in &definition.original_sources().sources {
        budget.charge()?;
        if source.id == declaration.source {
            if source.kind != SourceKind::Xtext {
                return Err("Nonunique source is not original Xtext".into());
            }
            original_source = Some(source);
            break;
        }
    }
    budget.charge()?;
    let original = original_source
        .ok_or("Missing original Nonunique source")?
        .text
        .get(declaration.start..declaration.end)
        .ok_or("Nonunique declaration range is outside the original source")?;
    if !original
        .trim_start()
        .starts_with("Nonunique returns Ecore::EBoolean")
    {
        return Err("Nonunique declaration range does not identify its original rule".into());
    }
    budget.charge()?;
    let binding = RuleConversion {
        context: grammar.name.clone(),
        rule: "Nonunique".into(),
        declaration: declaration.clone(),
        datatype: "http://www.eclipse.org/emf/2002/Ecore#//EBoolean".into(),
        provider: ProviderId {
            name: "intersect.xtext.keyword-boolean".into(),
            version: "1".into(),
        },
        keywords: [("nonunique".into(), false)].into(),
    };
    rule_conversion::validate(Some(grammar), std::slice::from_ref(&binding))?;
    Ok(KerMLNonuniqueCapability {
        inner,
        bindings: [binding],
    })
}

impl<C: DraftPhysicalDefaults, const N: usize> DraftPhysicalDefaults
    for KerMLNonuniqueCapability<C, N>
{
    fn contracts(&self) -> &[DraftClassContract] {
        self.inner.contracts()
    }
}
impl<C: DraftCapability, const N: usize> DraftCapability for KerMLNonuniqueCapability<C, N> {
    fn permits_scalar_postprocess(
        &self,
        trigger: &str,
        actual: &str,
        member: &intersect_model::definitions::schema::MemberIdentity,
        budget: &mut QueryBudget,
    ) -> Result<bool, String> {
        self.inner
            .permits_scalar_postprocess(trigger, actual, member, budget)
    }

    fn permits_postprocess_reference(&self, actual: &str, member: &MemberIdentity) -> bool {
        self.inner.permits_postprocess_reference(actual, member)
    }
    const HAS_NATIVE_INITIALIZERS: bool = C::HAS_NATIVE_INITIALIZERS;
    fn initializes_computed_created_elements(&self) -> bool {
        self.inner.initializes_computed_created_elements()
    }
    fn native_initializer(
        &self,
        actual: &str,
        budget: &mut QueryBudget,
    ) -> StorageResult<Option<intersect_model::storage::InitializationBinding<'_>>> {
        self.inner.native_initializer(actual, budget)
    }
    const NEEDS_VALUE_ADMISSION: bool = C::NEEDS_VALUE_ADMISSION;
    const RECONCILE_PARSED_CONTAINMENT: bool = C::RECONCILE_PARSED_CONTAINMENT;
    const HAS_BOOLEAN_RULE_CONVERSIONS: bool = true;
    const HAS_STRING_RULE_CONVERSIONS: bool = C::HAS_STRING_RULE_CONVERSIONS;
    fn source_resources(&self) -> Option<&intersect_text::draft::ClosedSourceResources> {
        self.inner.source_resources()
    }
    fn convert_string_rule(
        &self,
        grammar: &intersect_text::grammar::Grammar,
        rule: &str,
        spelling: &str,
        budget: &mut QueryBudget,
    ) -> StorageResult<Option<String>> {
        self.inner
            .convert_string_rule(grammar, rule, spelling, budget)
    }
    fn boolean_rule_conversions(&self) -> &[RuleConversion] {
        &self.bindings
    }
    fn has_delegated_assignments(&self) -> bool {
        self.inner.has_delegated_assignments()
    }
    fn delegated_assignment_target<'a>(
        &'a self,
        actual: &str,
        member: &MemberIdentity,
        append: bool,
        budget: &mut QueryBudget,
    ) -> StorageResult<Option<&'a MemberIdentity>> {
        self.inner
            .delegated_assignment_target(actual, member, append, budget)
    }
    fn original_sources(&self) -> &[SourceId] {
        self.inner.original_sources()
    }
    fn converted_sources(&self) -> &[SourceId] {
        self.inner.converted_sources()
    }
    fn permits_assignment(&self, receiver: &str, member: &MemberIdentity, append: bool) -> bool {
        self.inner.permits_assignment(receiver, member, append)
    }
    fn permits_computed_assignment(
        &self,
        receiver: &str,
        member: &MemberIdentity,
        append: bool,
    ) -> bool {
        self.inner
            .permits_computed_assignment(receiver, member, append)
    }
    fn permits_computed_assignment_observed(
        &self,
        receiver: &str,
        member: &MemberIdentity,
        append: bool,
        budget: &mut QueryBudget,
    ) -> StorageResult<bool> {
        self.inner
            .permits_computed_assignment_observed(receiver, member, append, budget)
    }
    fn validate_assignment_value(
        &self,
        receiver: &str,
        member: &MemberIdentity,
        append: bool,
        value: &Value,
        budget: &mut QueryBudget,
    ) -> StorageResult<()> {
        self.inner
            .validate_assignment_value(receiver, member, append, value, budget)
    }
}

// Only the exact independently checked original SysML profile opts in. Both
// policies reuse the same canonical source/rule validator and forwarding body.
pub(crate) fn sysml_boolean_draft_capability<C: DraftCapability>(
    definition: &PreparedLanguageDefinition<'_>,
    inner: C,
    preparation: &intersect_model::definitions::conversion::InvocationContext<'_>,
    budget: &mut QueryBudget,
) -> Result<KerMLNonuniqueCapability<C, 2>, String> {
    super::sysml_source_check::check(definition, preparation)?;
    // Conservative new-profile retained allowance for both owned bindings;
    // old KerML selection and its fees remain unchanged.
    preparation.retain_storage(4096 + super::structural_literals::BOOLEAN_BINDING_BYTES)?;
    for _ in 0..4096usize.div_ceil(64) + 256 {
        budget.charge()?;
    }
    let old = nonunique_in_context(
        definition,
        inner,
        budget,
        "org.omg.sysml.xtext.SysML",
        "org.omg.sysml.xtext/src/org/omg/sysml/xtext/SysML.xtext",
        "7d9142742c454f5810fe4db61a089abef5e0b8d61ad8bf552631d9136b04f298",
    )?;
    let binding = super::structural_literals::boolean_binding_in_context(
        definition,
        "org.omg.sysml.xtext.SysML",
        budget,
    )?;
    budget.charge()?;
    let [nonunique] = old.bindings;
    Ok(KerMLNonuniqueCapability {
        inner: old.inner,
        bindings: [nonunique, binding],
    })
}
