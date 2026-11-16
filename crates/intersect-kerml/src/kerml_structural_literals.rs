//! Required-profile LiteralReal admission, using the existing Ecore EDouble codec.
//! No numeric evaluator or alternate parser is installed here.
use intersect_model::core::query::QueryBudget;
use intersect_model::core::query::QueryResult;
use intersect_text::grammar::Datatype;
use intersect_text::grammar::Expr;
use intersect_text::grammar::Grammar;
use intersect_text::grammar::Kind;
use intersect_text::grammar::Rule;
use intersect_text::preparation::PreparedLanguageDefinition;

// Conservative logical allowance for one fixed source/context binding, its two
// owned keyword-map entries, and shared-validator temporary nodes. Required's
// resource holder deducts this from its existing association cap as well.
pub(crate) const BOOLEAN_BINDING_BYTES: usize = 8192;

const URI: &str =
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext";
const HASH: &str = "7bea42879a913f7613ac5e32779174db01e8fd4fe75694f0845c495aa0191aa8";

fn pay(budget: &mut QueryBudget, n: usize) -> QueryResult<()> {
    for _ in 0..n {
        budget.charge()?;
    }
    Ok(())
}
fn equal(a: &str, b: &str, budget: &mut QueryBudget) -> QueryResult<bool> {
    pay(budget, 1 + a.len().max(b.len()).div_ceil(64))?;
    Ok(a == b)
}
fn lookup(count: usize, key: &str, budget: &mut QueryBudget) -> QueryResult<()> {
    let levels = usize::BITS as usize - count.max(1).leading_zeros() as usize;
    let work = levels
        .checked_mul(2 * (1 + key.len().div_ceil(64)))
        .and_then(|n| n.checked_add(1))
        .ok_or("Real literal lookup work overflow")?;
    pay(budget, work)
}
fn rule<'a>(
    grammar: &'a Grammar,
    name: &str,
    start: usize,
    end: usize,
    budget: &mut QueryBudget,
) -> QueryResult<&'a Rule> {
    lookup(grammar.declaration_sources.len(), name, budget)?;
    let span = grammar
        .declaration_sources
        .get(name)
        .ok_or("Missing original real literal declaration")?;
    if !equal(&span.source.uri, URI, budget)? || !equal(&span.source.sha256, HASH, budget)? {
        return Err("Real literal declaration source differs".into());
    }
    pay(budget, 16)?;
    if span.start != start || span.end != end {
        return Err("Real literal declaration range differs".into());
    }
    lookup(grammar.rules.len(), name, budget)?;
    let rule = grammar.rules.get(name).ok_or("Missing real literal rule")?;
    if rule.fragment
        || rule.wildcard
        || rule.override_declared
        || rule.hidden.is_some()
        || !rule.enum_literals.is_empty()
        || rule.nominal_return.is_some()
        || rule.body.cardinality != '1'
        || rule.body.first_set_predicated
    {
        return Err("Real literal rule mode differs".into());
    }
    Ok(rule)
}
fn call(expr: &Expr, name: &str, cardinality: char, budget: &mut QueryBudget) -> QueryResult<()> {
    pay(budget, 3)?;
    let Kind::Call(actual) = &expr.kind else {
        return Err("Real literal canonical call differs".into());
    };
    if expr.cardinality != cardinality || expr.first_set_predicated || !equal(actual, name, budget)?
    {
        return Err("Real literal canonical call boundary differs".into());
    }
    Ok(())
}
fn children<'a>(
    expr: &'a Expr,
    alternative: bool,
    budget: &mut QueryBudget,
) -> QueryResult<&'a [Expr]> {
    pay(budget, 3)?;
    if expr.cardinality != '1' || expr.first_set_predicated {
        return Err("Real literal group boundary differs".into());
    }
    match (&expr.kind, alternative) {
        (Kind::Sequence(rows), false) | (Kind::Alternative(rows), true) => Ok(rows),
        _ => Err("Real literal group differs".into()),
    }
}

pub(crate) fn boolean_binding_in_context(
    definition: &PreparedLanguageDefinition<'_>,
    expected_context: &str,
    budget: &mut QueryBudget,
) -> QueryResult<intersect_model::definitions::rule_conversion::RuleConversion> {
    use intersect_model::definitions::{conversion::ProviderId, rule_conversion::RuleConversion};
    budget.charge()?;
    let grammar = definition
        .grammar()
        .ok_or("Boolean literal requires original grammar")?;
    if !equal(&grammar.name, expected_context, budget)? {
        return Err("Boolean literal grammar context differs".into());
    }
    let literal = rule(grammar, "LiteralBoolean", 13778, 13850, budget)?;
    pay(budget, 5)?;
    if literal.datatype.is_some()
        || literal.inferred_return
        || !equal(&literal.class, "LiteralBoolean", budget)?
    {
        return Err("Boolean literal object return differs".into());
    }
    let Kind::Assign {
        feature,
        many,
        value,
    } = &literal.body.kind
    else {
        return Err("Boolean literal assignment differs".into());
    };
    if *many || !equal(feature, "value", budget)? {
        return Err("Boolean literal member/mode differs".into());
    }
    call(value, "BooleanValue", '1', budget)?;
    let scalar = rule(grammar, "BooleanValue", 13854, 13914, budget)?;
    pay(budget, 4)?;
    if scalar.datatype != Some(Datatype::EBoolean)
        || scalar.inferred_return
        || !scalar.class.is_empty()
    {
        return Err("Boolean literal scalar return differs".into());
    }
    let alternatives = children(&scalar.body, true, budget)?;
    budget.charge()?;
    if alternatives.len() != 2 {
        return Err("Boolean literal keyword count differs".into());
    }
    for (expr, expected) in alternatives.iter().zip(["true", "false"]) {
        pay(budget, 3)?;
        let Kind::Literal(actual) = &expr.kind else {
            return Err("Boolean literal keyword differs".into());
        };
        if expr.cardinality != '1' || expr.first_set_predicated || !equal(actual, expected, budget)?
        {
            return Err("Boolean literal keyword boundary differs".into());
        }
    }
    // All variable strings are exact-bound above before ownership copies. Pay
    // both the fixed retained allowance and actual copy/cleanup/validation work.
    pay(budget, BOOLEAN_BINDING_BYTES.div_ceil(64) + 256)?;
    lookup(grammar.declaration_sources.len(), "BooleanValue", budget)?;
    let declaration = grammar
        .declaration_sources
        .get("BooleanValue")
        .ok_or("Missing Boolean literal source")?;
    let binding = RuleConversion {
        context: grammar.name.clone(),
        rule: "BooleanValue".into(),
        declaration: declaration.clone(),
        datatype: "http://www.eclipse.org/emf/2002/Ecore#//EBoolean".into(),
        provider: ProviderId {
            name: "intersect.xtext.keyword-boolean".into(),
            version: "1".into(),
        },
        keywords: [("true".into(), true), ("false".into(), false)].into(),
    };
    intersect_text::rule_conversion::validate(Some(grammar), std::slice::from_ref(&binding))?;
    Ok(binding)
}
