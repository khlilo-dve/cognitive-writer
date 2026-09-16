// Library crate for cognitive-writer — enables modules to be tested
// and composed independently of the CLI entry-point (main.rs).

pub mod clipboard;
pub mod error;
pub mod eval;
pub mod eval_runs;
pub mod eval_store;
pub mod generate;
pub mod intent;
pub mod io;
pub mod learn;
pub mod llm;
pub mod refine;
pub mod repl;
pub mod styles;
pub mod update;
pub mod utils;
pub mod website;

mod eval_batch;
pub mod eval_calibration;
mod intent_classify;
#[cfg(test)]
mod intent_legacy_extract;
#[cfg(test)]
mod intent_legacy_style;
mod intent_quick;
mod repl_input;
pub mod spec_quality;
mod style_names;
mod style_selection;
mod edit_blocks;
mod local_edit;
