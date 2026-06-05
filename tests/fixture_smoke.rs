use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use hwp_convert::bridge;
use hwp_convert::cli::{CliArgs, OutputFormat};
use hwp_convert::exporter;
use hwp_convert::ir::{Block, Document, Inline, Paragraph};

const FIXTURE_ROOT: &str = "tests/fixtures";
const BASIC_TEXT_KOREAN_PARAGRAPH: &str = "기본 한글 문단";
const BASIC_TEXT_MIXED_PARAGRAPH: &str = "English 123 mixed text";
const BASIC_TEXT_LINE_BREAK_BEFORE: &str = "줄바꿈 앞";
const BASIC_TEXT_LINE_BREAK_AFTER: &str = "줄바꿈 뒤";
const BASIC_TEXT_TAB_BEFORE: &str = "탭 앞";
const BASIC_TEXT_TAB_AFTER: &str = "탭 뒤";

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

#[test]
fn official_fixtures_match_feature_expectations() {
    let inputs = discover_fixture_inputs();
    if inputs.is_empty() {
        eprintln!("no official fixture inputs found under {FIXTURE_ROOT}; feature test is armed");
        return;
    }

    for input in inputs {
        let document = bridge::rhwp::read_document(&input.path)
            .unwrap_or_else(|error| panic!("failed to parse {}: {error}", input.label));

        match input.fixture_name.as_str() {
            "basic_text" => assert_basic_text_fixture(&input, &document),
            _ => {}
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

fn assert_basic_text_fixture(input: &FixtureInput, document: &Document) {
    let paragraphs = collect_paragraphs(document);
    assert_eq!(
        paragraphs.len(),
        4,
        "fixture {} should preserve exactly four non-empty paragraphs and drop the empty paragraph",
        input.label
    );

    let paragraph_texts = paragraphs
        .iter()
        .map(|paragraph| paragraph_plain_text(paragraph))
        .collect::<Vec<_>>();

    assert!(
        paragraph_texts
            .iter()
            .any(|text| text == BASIC_TEXT_KOREAN_PARAGRAPH),
        "fixture {} should preserve the Korean paragraph text",
        input.label
    );
    assert!(
        paragraph_texts
            .iter()
            .any(|text| text == BASIC_TEXT_MIXED_PARAGRAPH),
        "fixture {} should preserve the mixed English/number paragraph text",
        input.label
    );
    assert!(
        paragraphs
            .iter()
            .any(|paragraph| paragraph_has_line_break_case(paragraph)),
        "fixture {} should preserve an inline line break inside a paragraph",
        input.label
    );
    assert!(
        paragraphs
            .iter()
            .any(|paragraph| paragraph_has_tab_case(paragraph)),
        "fixture {} should preserve an inline tab inside a paragraph",
        input.label
    );
}

fn collect_paragraphs(document: &Document) -> Vec<&Paragraph> {
    document
        .sections
        .iter()
        .flat_map(|section| &section.blocks)
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .collect()
}

fn paragraph_has_line_break_case(paragraph: &Paragraph) -> bool {
    let text = paragraph_plain_text(paragraph);

    text == format!("{BASIC_TEXT_LINE_BREAK_BEFORE}\n{BASIC_TEXT_LINE_BREAK_AFTER}")
        && paragraph
            .inlines
            .iter()
            .any(|inline| matches!(inline, Inline::LineBreak))
}

fn paragraph_has_tab_case(paragraph: &Paragraph) -> bool {
    let text = paragraph_plain_text(paragraph);

    text == format!("{BASIC_TEXT_TAB_BEFORE}\t{BASIC_TEXT_TAB_AFTER}")
        && paragraph
            .inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Tab))
}

fn paragraph_plain_text(paragraph: &Paragraph) -> String {
    let mut text = String::new();

    for inline in &paragraph.inlines {
        match inline {
            Inline::Text(run) => text.push_str(&run.text),
            Inline::LineBreak => text.push('\n'),
            Inline::Tab => text.push('\t'),
            Inline::Link(link) => text.push_str(&link_plain_text(&link.inlines)),
            Inline::FootnoteRef { note_id } | Inline::EndnoteRef { note_id } => {
                text.push_str(note_id.as_str());
            }
            Inline::Unknown(unknown) => {
                if let Some(fallback_text) = &unknown.fallback_text {
                    text.push_str(fallback_text);
                }
            }
        }
    }

    text
}

fn link_plain_text(inlines: &[Inline]) -> String {
    let paragraph = Paragraph {
        inlines: inlines.to_vec(),
        ..Default::default()
    };

    paragraph_plain_text(&paragraph)
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
