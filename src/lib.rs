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
pub mod voice_learning;
pub mod voice_revision;
pub mod learning_candidate;
pub mod learning_workspace;
pub mod learning_workspace_cli;
pub mod learning_job;
pub mod learning_sources;
pub mod learning_agent;
mod learning_agent_types;
mod learning_agent_store;
mod learning_agent_scope;
mod learning_agent_tools;
mod learning_agent_model;
mod learning_agent_cli;
mod learning_source_parse;
mod learning_source_fetch;
mod learning_source_cli;
mod learning_transport;
mod research_learning_jobs;
mod research_learning_quality;
mod learning_workspace_store;
mod learning_diff;
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

mod style_learning;
pub mod eval_learning;

mod transfer_quality;
pub mod eval_transfer_audit;
mod transfer_report;

pub mod eval_free_release;
mod free_style_gate;
mod research_budget;
mod research_dataset;
mod research_store;
mod research_receipt;
mod style_compare;
mod research_compare;
mod research_validation;
mod research_route_pilot;
mod research_candidates;
mod style_candidates;
mod research_candidate_matrix;
mod research_candidate_cases;
mod research_atomic_pilot;
mod research_atomic_route;
mod style_profile;
mod research_profile;
mod research_profile_audit;
mod research_pragmatic_profile;
mod research_voice_learning;
mod research_voice_learning_v2;
mod research_voice_learning_v3;
mod research_voice_editor;
mod research_voice_prose;
mod research_voice_pipeline;
mod research_voice_core;
mod research_voice_probe;
mod research_voice_recovery;
mod research_ending_audit;
mod research_convergence;
mod research_generation;
mod research_judgment;
mod research_evaluation;
pub mod research_run;

mod research_learning_agent;

pub mod learning_chat;
mod learning_chat_types;
mod learning_chat_store;
mod learning_chat_parse;
mod learning_chat_reply;

mod research_learning_chat;

pub mod learning_flow;
mod learning_flow_writer;
mod learning_flow_commands;
mod research_learning_flow;

mod learning_document;
mod learning_docx;
mod learning_pdf;
mod learning_source_binary;
mod learning_input_limits;
