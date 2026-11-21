//! Handwritten (not generated): KerML project interchange archives
//! (`.kpar`, KerML 1.0 model interchange; metadata as OMG's
//! `KerML-Model-Interchange.json` schema defines it).
//!
//! - [`zip`]: the bounded ZIP reader and deterministic writer;
//! - [`project`]: `.project.json` and `.meta.json`;
//! - [`archive`]: reading a checked archive into textual model sources with
//!   archive-qualified resource URIs, and writing one;
//! - [`mod@resolve`]: offline resolution of `usage` dependencies.
//!
//! Only textual notation (`.kerml`, `.sysml`) is loaded. JSON and XMI model
//! content, derived or implied values, nested archives and other
//! metamodels are refused explicitly. The crate performs no I/O and reads
//! no clock (a writer passes `created`), so it builds for
//! `wasm32-unknown-unknown` and every host shares it. See
//! `docs/kpar-plan.md`.
pub mod archive;
pub mod project;
pub mod resolve;
pub mod zip;

pub use archive::{
    ArchiveFile, ArchiveSource, ErrorKind, KparError, Language, ProjectArchive, read, write,
};
pub use project::{Checksum, Meta, Project, Usage};
pub use resolve::{Resolution, StandardLibraryUse, resolve};
pub use zip::ArchiveLimits;
