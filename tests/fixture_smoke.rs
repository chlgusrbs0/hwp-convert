use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use hwp_convert::bridge;
use hwp_convert::cli::{CliArgs, OutputFormat};
use hwp_convert::exporter;
use hwp_convert::ir::{Block, Document};

const FIXTURE_ROOT: &str = "tests/fixtures";

#[test]
fn official_fixtures_parse_into_non_empty_ir() {
    let inputs = discover_fixture_inputs();
    if inputs.is_empty() {
        eprintln!("no official fixture inputs found under {FIXTURE_ROOT}; smoke test is armed");
        return;
    }

    for input in inputs {
        let document = bridge::rhwp::read_document(&input.path)
            .unwrap_or_else(|error| panic!("failed to parse {}: {error}", input.label));

        assert!(
            document_has_semantic_content(&document),
            "fixture {} parsed successfully but produced no semantic content",
            input.label
        );
    }
}

#[test]
fn official_fixtures_export_all_current_formats() {
    let inputs = discover_fixture_inputs();
    if inputs.is_empty() {
        eprintln!("no official fixture inputs found under {FIXTURE_ROOT}; export smoke test is armed");
        return;
    }

    let output_root = temp_output_dir("fixture-export-smoke");

    for input in inputs {
        for format in current_output_formats() {
            let output_dir = output_root.join(&input.fixture_name).join(format.extension());
            let args = CliArgs {
                input_path: input.path.clone(),
                format,
                recursive: false,
                manifest_path: None,
                resume_manifest_path: None,
                continue_on_error: false,
                output_dir: Some(output_dir),
                skip_existing: false,
            };

            let report = exporter::export(&args).unwrap_or_else(|error| {
                panic!(
                    "failed to export {} as {}: {error}",
                    input.label, format
                )
            });

            assert_eq!(
                report.converted_files().len(),
                1,
                "fixture {} should produce one converted file for {}",
                input.label,
                format
            );
            assert!(
                report.skipped_files().is_empty(),
                "fixture {} unexpectedly skipped during {} export",
                input.label,
                format
            );
            assert!(
                report.failed_files().is_empty(),
                "fixture {} unexpectedly failed during {} export",
                input.label,
                format
            );

            let output_path = &report.converted_files()[0].output_path;
            assert!(
                output_path.is_file(),
                "fixture {} reported output that does not exist for {}: {}",
                input.label,
                format,
                output_path.display()
            );
        }
    }
}

#[derive(Debug, Clone)]
struct FixtureInput {
    fixture_name: String,
    label: String,
    path: PathBuf,
}

fn discover_fixture_inputs() -> Vec<FixtureInput> {
    let root = Path::new(FIXTURE_ROOT);
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };

    let mut fixture_dirs = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    fixture_dirs.sort();

    let mut inputs = Vec::new();
    for fixture_dir in fixture_dirs {
        let Some(fixture_name) = fixture_dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
        else {
            continue;
        };

        for file_name in ["input.hwp", "input.hwpx"] {
            let path = fixture_dir.join(file_name);
            if path.is_file() {
                inputs.push(FixtureInput {
                    label: format!("{fixture_name}/{file_name}"),
                    fixture_name: fixture_name.clone(),
                    path,
                });
            }
        }
    }

    inputs
}

fn document_has_semantic_content(document: &Document) -> bool {
    !document.notes.notes.is_empty()
        || document.sections.iter().any(|section| {
            !section.headers.is_empty()
                || !section.footers.is_empty()
                || section.blocks.iter().any(block_has_semantic_content)
        })
}

fn block_has_semantic_content(block: &Block) -> bool {
    match block {
        Block::Paragraph(paragraph) => !paragraph.inlines.is_empty(),
        Block::Table(table) => !table.rows.is_empty(),
        Block::Image(_) | Block::Equation(_) | Block::Shape(_) | Block::Chart(_) => true,
        Block::Unknown(unknown) => {
            unknown.fallback_text.is_some() || unknown.message.is_some() || unknown.source.is_some()
        }
    }
}

fn current_output_formats() -> [OutputFormat; 5] {
    [
        OutputFormat::Txt,
        OutputFormat::Json,
        OutputFormat::Markdown,
        OutputFormat::Html,
        OutputFormat::Svg,
    ]
}

fn temp_output_dir(test_name: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after UNIX_EPOCH")
        .as_nanos();
    let path = Path::new("target")
        .join("fixture-check")
        .join(format!("{test_name}-{}-{timestamp}", std::process::id()));

    fs::create_dir_all(&path).expect("fixture output directory should be creatable");
    path
}
