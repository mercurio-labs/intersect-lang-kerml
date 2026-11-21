# Resolved references and contextual completion

The shared `intersect-kerml-ide::Host` dispatches `textDocument/definition` and
`textDocument/completion`. Native hosts and Rust/WASM consumers use the same
implementation; applications supply transport and presentation only.

## Navigation

Text's construction parser retains exact active reference occurrences. The
frozen loader joins them with actual, uniquely resolved linker targets in
`FrozenLoad::navigation`. This is a per-load side table, not a spelling search
or an addition to the frozen snapshot encoding.

Definition requests require the current project revision and matching open
buffers. Responses are LSP location links with UTF-16 origin ranges and target
resource/ranges. Cross-file, dependency and prepared-library destinations use
their original source URIs. Unresolved, ambiguous, stale or source-less targets
do not produce invented locations. Navigation within prepared library source
references is not indexed by the project load; references from project source
to library declarations are supported. Target selection uses the declaration's
semantic range because a separate declaration-name span is not retained.

## Completion

Full-document synchronization remains independent of accepted analysis.
Completion reparses the current source buffers into an isolated store over the
prepared library. A unique cursor marker and at most four parser-verified probe
variants identify the actual reference owner, feature and qualified spelling.
Repairs cover missing statement terminators and balanced closing delimiters;
they are not general error recovery. Full-buffer probes preserve declarations
after the cursor. Prefix probes support unfinished tails, but can omit later
declarations when the full buffer cannot parse.

Declared names, short names and membership aliases seed bounded queries. Each
suggestion must resolve uniquely through the exact source-authored scope bound
to the parser-identified reference feature. Scope rules decide visibility,
imports, shadowing, inheritance, qualified traversal and target conformance;
there is no global-symbol fallback. Existing scope limitations, including import
filter conditions and some visibility/escaped-name cases, remain unchanged.
Names requiring quotes are inserted quoted; names with unsupported quote,
backslash or namespace-separator escapes are omitted.

Limits are 256 KiB for the edited buffer, 2 MiB/256 sources for the probe batch,
20 million shared parser work units across attempts, and 128 deterministic
spelling queries with 100,000 scope reads per query and the existing bounded
scope executor. Truncated spelling inventories set `isIncomplete`. Larger or
unsupported contexts retain lexical keywords/snippets; comments, strings and
unfinished quoted names offer no suggestions. Parsing and scope queries cannot
publish a model or change accepted navigation/revisions. Hosts discard replies
after cancellation, buffer/workspace changes or worker restart.

## Development dependency pairing

The editor API uses additions since the published `0.1.0-alpha.1` crates.
The [workspace manifest](../Cargo.toml) records the exact public Git revisions
for Text, Model and build-support. The standalone consumer manifests and locks
use the same source identities. When the editor APIs are available in a new
registry release, update the matching adapters and all maintained locks together.

## Verification

The IDE quick tier covers lexical behavior, versions, Unicode positions and
probe repairs. Pack-dependent milestone cases cover cross-file/library
navigation, stale rejection, contextual names, aliases, short/quoted names,
ambiguity, private imports, qualified names, inheritance, declarations after
the cursor and incomplete edits. Existing evaluation and source-command tests
remain independent regression checks. Run through `tools/test_report.py` with
`INTERSECT_IDE_TEST_PACK` naming a matching trusted pack. Browser and real-WASM
acceptance live in Mercurio.
