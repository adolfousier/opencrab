//! Document generation tool (#357).
//!
//! `parse_document` reads documents; this is the missing write side. One
//! `generate_document` tool with structured input, dispatching to a native
//! Rust backend per format so document creation works in the distributed
//! binary with zero host dependencies (no Python, no LibreOffice).
//!
//! Backends land format by format; each format module stays small and pure
//! (spec in, file out) so it is testable without the tool plumbing.
//!
//! Layout: [`tool`] (the `Tool` impl and dispatch), [`input`] (the request
//! struct), [`schema`] (the advertised JSON schema) and one backend module
//! per format (`docx`, `pdf`, `pptx`, `xlsx`). This file is declarations
//! only — no function definitions live here (CONTRIBUTING.md).

pub(crate) mod docx;
mod input;
pub(crate) mod pdf;
pub(crate) mod pptx;
mod schema;
mod tool;
pub(crate) mod xlsx;

pub use tool::GenerateDocumentTool;
