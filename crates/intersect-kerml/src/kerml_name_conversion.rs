//! Handwritten KerML Name/qualified-name conversion from pinned lexical grammar
//! and ElementUtil.unescapeString/parseQualifiedName. No Java in production.
use intersect_model::core::query::QueryBudget;
use intersect_model::core::query::QueryResult;
use intersect_text::grammar::Datatype;
use intersect_text::grammar::Grammar;
use intersect_text::grammar::Kind;
use intersect_text::preparation::PreparedLanguageDefinition;

const URI: &str =
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext";
const HASH: &str = "7bea42879a913f7613ac5e32779174db01e8fd4fe75694f0845c495aa0191aa8";

/// Borrowed exact immutable grammar, never a serialized authority marker.
pub(crate) struct KerMLNameConversion<'g> {
    grammar: &'g Grammar,
}
impl<'g> KerMLNameConversion<'g> {
    pub(crate) fn bind(
        definition: &'g PreparedLanguageDefinition<'_>,
        budget: &mut QueryBudget,
    ) -> QueryResult<Self> {
        budget.charge()?;
        let grammar = definition
            .grammar()
            .ok_or("KerML Name requires its grammar")?;
        for (name, start, end) in [
            ("Name", 14342, 14375),
            ("ID", 14691, 14774),
            ("UNRESTRICTED_NAME", 14778, 14915),
        ] {
            budget.charge()?;
            lookup_work(grammar.declaration_sources.len(), name.len(), budget)?;
            let span = grammar
                .declaration_sources
                .get(name)
                .ok_or("Missing original KerML Name declaration")?;
            for _ in URI.bytes().chain(HASH.bytes()) {
                budget.charge()?;
            }
            if span.source.uri != URI
                || span.source.sha256 != HASH
                || span.start != start
                || span.end != end
            {
                return Err("KerML Name declaration/source differs".into());
            }
        }
        budget.charge()?;
        lookup_work(grammar.rules.len(), "Name".len(), budget)?;
        let rule = grammar.rules.get("Name").ok_or("Missing KerML Name rule")?;
        if rule.datatype != Some(Datatype::EString)
            || !rule.class.is_empty()
            || !rule.inferred_return
            || rule.fragment
            || rule.wildcard
            || rule.override_declared
            || !rule.enum_literals.is_empty()
            || rule.nominal_return.is_some()
            || rule.hidden.is_some()
            || rule.body.cardinality != '1'
            || rule.body.first_set_predicated
        {
            return Err("KerML Name return/shape differs".into());
        }
        let Kind::Alternative(alternatives) = &rule.body.kind else {
            return Err("KerML Name alternatives differ".into());
        };
        if alternatives.len() != 2 {
            return Err("KerML Name alternative count differs".into());
        }
        for (body, name) in alternatives.iter().zip(["ID", "UNRESTRICTED_NAME"]) {
            budget.charge()?;
            if body.cardinality != '1'
                || body.first_set_predicated
                || !matches!(&body.kind,Kind::Call(callee) if callee==name)
            {
                return Err("KerML Name actual operand differs".into());
            }
            lookup_work(grammar.terminals.len(), name.len(), budget)?;
            let terminal = grammar
                .terminals
                .get(name)
                .ok_or("Missing KerML Name terminal")?;
            if terminal.datatype != Datatype::EString {
                return Err("KerML Name terminal return differs".into());
            }
        }
        Ok(Self { grammar })
    }
    pub(crate) fn convert(
        &self,
        grammar: &Grammar,
        rule: &str,
        raw: &str,
        budget: &mut QueryBudget,
    ) -> QueryResult<Option<String>> {
        budget.charge()?;
        if !std::ptr::eq(grammar, self.grammar) {
            return Err("KerML Name conversion grammar differs".into());
        }
        budget.charge()?;
        if rule != "Name" {
            return Ok(None);
        }
        decode(raw, budget).map(Some)
    }
}

fn decode(raw: &str, budget: &mut QueryBudget) -> QueryResult<String> {
    budget.charge()?;
    for _ in raw.bytes() {
        budget.charge()?;
    } // reserve/output byte bound
    let mut value = String::new();
    value
        .try_reserve(raw.len())
        .map_err(|_| "KerML Name allocation failed")?;
    if !raw.starts_with('\'') {
        let mut chars = raw.chars();
        let first = chars.next().ok_or("Empty unquoted KerML Name")?;
        budget.charge()?;
        if !(first.is_ascii_alphabetic() || first == '_') {
            return Err("Invalid unquoted KerML Name".into());
        }
        value.push(first);
        for c in chars {
            budget.charge()?;
            if !(c.is_ascii_alphanumeric() || c == '_') {
                return Err("Invalid unquoted KerML Name".into());
            }
            value.push(c);
        }
        return Ok(value);
    }
    let inner = raw
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .ok_or("Unterminated KerML Name")?;
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        budget.charge()?;
        let c = if c == '\\' {
            budget.charge()?;
            match chars.next().ok_or("Incomplete KerML Name escape")? {
                'b' => '\u{0008}',
                't' => '\t',
                'n' => '\n',
                'f' => '\u{000c}',
                'r' => '\r',
                '"' => '"',
                '\'' => '\'',
                '\\' => '\\',
                _ => return Err("Invalid KerML Name escape".into()),
            }
        } else if c == '\'' {
            return Err("Unescaped quote inside KerML Name".into());
        } else {
            c
        };
        value.push(c);
    }
    Ok(value)
}

fn lookup_work(count: usize, key_bytes: usize, budget: &mut QueryBudget) -> QueryResult<()> {
    let levels = usize::BITS as usize - count.max(1).leading_zeros() as usize;
    let work = levels
        .checked_mul(2)
        .and_then(|n| n.checked_mul(1 + key_bytes.div_ceil(64)))
        .and_then(|n| n.checked_add(1))
        .ok_or("KerML Name lookup work overflow")?;
    for _ in 0..work {
        budget.charge()?;
    }
    Ok(())
}
