use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use hwp_convert::bridge;
use hwp_convert::cli::{CliArgs, OutputFormat};
use hwp_convert::exporter;
use hwp_convert::ir::{
    Alignment, Block, Color, Document, HeaderFooterPlacement, Image, Inline, LengthPt, LengthPx,
    NoteKind, Paragraph, ParagraphStyle, Resource, Table, TableCell, TextStyle,
};
use serde::{Deserialize, Serialize};

const FIXTURE_ROOT: &str = "tests/fixtures";
const UPDATE_FIXTURE_STATS_ENV: &str = "HWP_CONVERT_UPDATE_FIXTURE_STATS";
const BASIC_TEXT_KOREAN_PARAGRAPH: &str = "기본 한글 문단";
const BASIC_TEXT_MIXED_PARAGRAPH: &str = "English 123 mixed text";
const BASIC_TEXT_LINE_BREAK_BEFORE: &str = "줄바꿈 앞";
const BASIC_TEXT_LINE_BREAK_AFTER: &str = "줄바꿈 뒤";
const BASIC_TEXT_TAB_BEFORE: &str = "탭 앞";
const BASIC_TEXT_TAB_AFTER: &str = "탭 뒤";
const FOOTNOTE_BODY_TEXT: &str = "body text";
const FOOTNOTE_ID: &str = "footnote-3";
const FOOTNOTE_NOTE_TEXT: &str = "note body";
const HEADER_TEXT: &str = "header text";
const FOOTER_TEXT: &str = "footer text";
const IMAGE_ALT_TEXT: &str = "sample image";
const IMAGE_RESOURCE_ID: &str = "image-1";
const IMAGE_PNG_SIGNATURE: &[u8] = &[137, 80, 78, 71, 13, 10, 26, 10];
const MERGED_TABLE_CELL_TEXTS: [&str; 7] = [
    "row span", "col span", "cell 2-2", "cell 2-3", "cell 3-1", "cell 3-2", "cell 3-3",
];
const STYLE_PARAGRAPH_TEXT: &str = "styled text";
const TABLE_CELL_TEXTS: [&str; 4] = ["cell 1-1", "cell 1-2", "cell 2-1", "cell 2-2"];

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
        let stats = DocumentStats::from_document(&document);

        assert!(
            stats.has_semantic_content(),
            "fixture {} parsed successfully but produced no semantic content: {stats:#?}",
            input.label,
        );
    }
}

#[test]
fn official_fixtures_export_all_current_formats() {
    let inputs = discover_fixture_inputs();
    if inputs.is_empty() {
        eprintln!(
            "no official fixture inputs found under {FIXTURE_ROOT}; export smoke test is armed"
        );
        return;
    }

    let output_root = temp_output_dir("fixture-export-smoke");

    for input in inputs {
        for format in current_output_formats() {
            let output_dir = output_root
                .join(&input.fixture_name)
                .join(format.extension());
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
                panic!("failed to export {} as {}: {error}", input.label, format)
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

            assert_fixture_export_artifacts(&input, format, output_path);
        }
    }
}

fn assert_fixture_export_artifacts(input: &FixtureInput, format: OutputFormat, output_path: &Path) {
    if input.fixture_name != "image" {
        return;
    }

    match format {
        OutputFormat::Html | OutputFormat::Markdown => {
            let asset_ref = "input_assets/images/image-1.png";
            let asset_path = output_path
                .parent()
                .expect("export output should have a parent directory")
                .join("input_assets")
                .join("images")
                .join("image-1.png");
            assert!(
                asset_path.is_file(),
                "fixture {} should write image asset for {} export: {}",
                input.label,
                format,
                asset_path.display()
            );
            let asset_bytes = fs::read(&asset_path).unwrap_or_else(|error| {
                panic!(
                    "fixture {} should allow reading exported image asset {}: {error}",
                    input.label,
                    asset_path.display()
                )
            });
            assert!(
                asset_bytes.starts_with(IMAGE_PNG_SIGNATURE),
                "fixture {} should export PNG resource bytes for {}",
                input.label,
                format
            );

            let output = fs::read_to_string(output_path).unwrap_or_else(|error| {
                panic!(
                    "fixture {} should allow reading exported {} output {}: {error}",
                    input.label,
                    format,
                    output_path.display()
                )
            });
            assert!(
                output.contains(asset_ref),
                "fixture {} should reference exported image asset in {} output",
                input.label,
                format
            );
        }
        _ => {}
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
            "footnote" => assert_footnote_fixture(&input, &document),
            "header_footer" => assert_header_footer_fixture(&input, &document),
            "image" => assert_image_fixture(&input, &document),
            "merged_table" => assert_merged_table_fixture(&input, &document),
            "style" => assert_style_fixture(&input, &document),
            "table" => assert_table_fixture(&input, &document),
            _ => {}
        }
    }
}

#[test]
fn official_fixtures_match_expected_bridge_stats() {
    let inputs = discover_fixture_inputs();
    if inputs.is_empty() {
        eprintln!(
            "no official fixture inputs found under {FIXTURE_ROOT}; bridge stats test is armed"
        );
        return;
    }

    let update_stats = env_flag(UPDATE_FIXTURE_STATS_ENV);
    let mut checked = 0usize;

    for input in inputs {
        let document = bridge::rhwp::read_document(&input.path)
            .unwrap_or_else(|error| panic!("failed to parse {}: {error}", input.label));
        let actual = DocumentStats::from_document(&document);

        if update_stats {
            let expected_path = input.bridge_stats_update_path();
            write_expected_bridge_stats(&expected_path, &actual);
            checked += 1;
            continue;
        }

        let Some(expected_path) = input.bridge_stats_expected_path() else {
            continue;
        };
        let expected = read_expected_bridge_stats(&expected_path);

        assert_eq!(
            actual,
            expected,
            "fixture {} bridge stats changed; update {} only after deciding the change is correct",
            input.label,
            expected_path.display()
        );
        checked += 1;
    }

    if checked == 0 {
        eprintln!(
            "no bridge stats expectation files found under {FIXTURE_ROOT}; bridge stats test is armed"
        );
    } else if update_stats {
        eprintln!("updated {checked} bridge stats expectation file(s)");
    }
}

#[derive(Debug, Clone)]
struct FixtureInput {
    fixture_name: String,
    label: String,
    path: PathBuf,
}

impl FixtureInput {
    fn bridge_stats_expected_path(&self) -> Option<PathBuf> {
        let fixture_dir = self.path.parent()?;
        let extension = self.path.extension()?.to_str()?;
        let expected_dir = fixture_dir.join("expected");
        let specific = expected_dir.join(format!("bridge-stats.{extension}.json"));
        if specific.is_file() {
            return Some(specific);
        }

        let shared = expected_dir.join("bridge-stats.json");
        if shared.is_file() {
            return Some(shared);
        }

        None
    }

    fn bridge_stats_update_path(&self) -> PathBuf {
        let fixture_dir = self
            .path
            .parent()
            .expect("fixture input should have a parent directory");
        let extension = self
            .path
            .extension()
            .and_then(|extension| extension.to_str())
            .expect("fixture input should have a UTF-8 extension");

        fixture_dir
            .join("expected")
            .join(format!("bridge-stats.{extension}.json"))
    }
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

fn read_expected_bridge_stats(path: &Path) -> DocumentStats {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));

    serde_json::from_str(&content)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

fn write_expected_bridge_stats(path: &Path, stats: &DocumentStats) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("failed to create {}: {error}", parent.display()));
    }

    let mut content = serde_json::to_string_pretty(stats)
        .unwrap_or_else(|error| panic!("failed to serialize bridge stats: {error}"));
    content.push('\n');
    fs::write(path, content)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
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

fn collect_images(document: &Document) -> Vec<&Image> {
    document
        .sections
        .iter()
        .flat_map(|section| &section.blocks)
        .filter_map(|block| match block {
            Block::Image(image) => Some(image),
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

fn assert_table_fixture(input: &FixtureInput, document: &Document) {
    let tables = collect_tables(document);
    assert_eq!(
        tables.len(),
        1,
        "fixture {} should preserve exactly one table",
        input.label
    );

    let table = tables[0];
    assert_eq!(
        table.rows.len(),
        2,
        "fixture {} should preserve two table rows",
        input.label
    );
    assert!(
        table.rows.iter().all(|row| row.cells.len() == 2),
        "fixture {} should preserve two cells in each table row",
        input.label
    );

    let cell_texts = table
        .rows
        .iter()
        .flat_map(|row| &row.cells)
        .map(table_cell_plain_text)
        .collect::<Vec<_>>();
    assert_eq!(
        cell_texts, TABLE_CELL_TEXTS,
        "fixture {} should preserve table cell text in row-major order",
        input.label
    );
}

fn assert_merged_table_fixture(input: &FixtureInput, document: &Document) {
    let tables = collect_tables(document);
    assert_eq!(
        tables.len(),
        1,
        "fixture {} should preserve exactly one merged table",
        input.label
    );

    let table = tables[0];
    assert_eq!(
        table.rows.len(),
        3,
        "fixture {} should preserve three table rows",
        input.label
    );
    assert!(
        table
            .rows
            .iter()
            .flat_map(|row| &row.cells)
            .any(|cell| cell.row_span == 2),
        "fixture {} should preserve a row-spanning cell",
        input.label
    );
    assert!(
        table
            .rows
            .iter()
            .flat_map(|row| &row.cells)
            .any(|cell| cell.col_span == 2),
        "fixture {} should preserve a column-spanning cell",
        input.label
    );

    let cell_texts = table
        .rows
        .iter()
        .flat_map(|row| &row.cells)
        .map(table_cell_plain_text)
        .collect::<Vec<_>>();
    assert_eq!(
        cell_texts, MERGED_TABLE_CELL_TEXTS,
        "fixture {} should preserve merged table cell text in row-major owner-cell order",
        input.label
    );
}

fn assert_image_fixture(input: &FixtureInput, document: &Document) {
    let images = collect_images(document);
    assert_eq!(
        images.len(),
        1,
        "fixture {} should preserve exactly one image block",
        input.label
    );

    let image = images[0];
    assert_eq!(
        image.resource_id.as_str(),
        IMAGE_RESOURCE_ID,
        "fixture {} should preserve the image resource reference",
        input.label
    );
    assert_eq!(
        image.alt.as_deref(),
        Some(IMAGE_ALT_TEXT),
        "fixture {} should preserve image alt/description text",
        input.label
    );
    assert_eq!(
        image.width,
        Some(LengthPx(96.0)),
        "fixture {} should preserve image display width",
        input.label
    );
    assert_eq!(
        image.height,
        Some(LengthPx(48.0)),
        "fixture {} should preserve image display height",
        input.label
    );

    let resource = document
        .resources
        .entries
        .iter()
        .find_map(|resource| match resource {
            Resource::Image(resource) if resource.id.as_str() == IMAGE_RESOURCE_ID => {
                Some(resource)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("fixture {} should preserve the image resource", input.label));

    assert_eq!(
        resource.extension.as_deref(),
        Some("png"),
        "fixture {} should preserve image extension",
        input.label
    );
    assert_eq!(
        resource.media_type.as_deref(),
        Some("image/png"),
        "fixture {} should preserve image media type",
        input.label
    );
    assert!(
        resource.bytes.starts_with(IMAGE_PNG_SIGNATURE),
        "fixture {} should preserve PNG resource bytes",
        input.label
    );
}

fn assert_header_footer_fixture(input: &FixtureInput, document: &Document) {
    let section = document
        .sections
        .first()
        .unwrap_or_else(|| panic!("fixture {} should preserve one section", input.label));

    assert!(
        section.blocks.is_empty(),
        "fixture {} should not leak header/footer controls into body blocks",
        input.label
    );
    assert_eq!(
        section.headers.len(),
        1,
        "fixture {} should preserve exactly one header",
        input.label
    );
    assert_eq!(
        section.footers.len(),
        1,
        "fixture {} should preserve exactly one footer",
        input.label
    );

    let header = &section.headers[0];
    assert_eq!(
        header.placement,
        HeaderFooterPlacement::Default,
        "fixture {} should preserve header placement",
        input.label
    );
    assert_eq!(
        block_paragraph_texts(&header.blocks),
        vec![HEADER_TEXT.to_string()],
        "fixture {} should preserve header text",
        input.label
    );

    let footer = &section.footers[0];
    assert_eq!(
        footer.placement,
        HeaderFooterPlacement::EvenPage,
        "fixture {} should preserve footer placement",
        input.label
    );
    assert_eq!(
        block_paragraph_texts(&footer.blocks),
        vec![FOOTER_TEXT.to_string()],
        "fixture {} should preserve footer text",
        input.label
    );
}

fn assert_footnote_fixture(input: &FixtureInput, document: &Document) {
    let note = document
        .notes
        .notes
        .first()
        .unwrap_or_else(|| panic!("fixture {} should preserve one footnote", input.label));
    assert_eq!(
        document.notes.notes.len(),
        1,
        "fixture {} should preserve exactly one note",
        input.label
    );
    assert_eq!(
        note.id.as_str(),
        FOOTNOTE_ID,
        "fixture {} should preserve the footnote id",
        input.label
    );
    assert_eq!(
        note.kind,
        NoteKind::Footnote,
        "fixture {} should preserve footnote kind",
        input.label
    );
    assert_eq!(
        block_paragraph_texts(&note.blocks),
        vec![FOOTNOTE_NOTE_TEXT.to_string()],
        "fixture {} should preserve footnote body text",
        input.label
    );
    assert!(
        document
            .warnings
            .iter()
            .any(|warning| warning.message.contains("footnote/endnote")),
        "fixture {} should report the current rhwp note-position limitation",
        input.label
    );

    let paragraph = collect_paragraphs(document)
        .into_iter()
        .find(|paragraph| {
            paragraph
                .inlines
                .iter()
                .any(|inline| matches!(inline, Inline::Text(run) if run.text == FOOTNOTE_BODY_TEXT))
        })
        .unwrap_or_else(|| {
            panic!(
                "fixture {} should preserve the paragraph with a footnote",
                input.label
            )
        });

    match paragraph.inlines.last() {
        Some(Inline::FootnoteRef { note_id }) => assert_eq!(
            note_id.as_str(),
            FOOTNOTE_ID,
            "fixture {} should append the current footnote reference",
            input.label
        ),
        other => panic!(
            "fixture {} should preserve a trailing footnote ref, got {other:?}",
            input.label
        ),
    }
}

fn assert_style_fixture(input: &FixtureInput, document: &Document) {
    let paragraphs = collect_paragraphs(document);
    let paragraph = paragraphs
        .iter()
        .copied()
        .find(|paragraph| paragraph_plain_text(paragraph) == STYLE_PARAGRAPH_TEXT)
        .unwrap_or_else(|| {
            panic!(
                "fixture {} should preserve the styled paragraph text",
                input.label
            )
        });

    assert_eq!(
        paragraph.style.alignment,
        Some(Alignment::Center),
        "fixture {} should preserve paragraph alignment",
        input.label
    );
    assert_eq!(
        paragraph.style.spacing.before_pt,
        Some(LengthPt(4.0)),
        "fixture {} should preserve paragraph before spacing",
        input.label
    );
    assert_eq!(
        paragraph.style.spacing.after_pt,
        Some(LengthPt(5.0)),
        "fixture {} should preserve paragraph after spacing",
        input.label
    );
    assert_eq!(
        paragraph.style.indent.left_pt,
        Some(LengthPt(3.0)),
        "fixture {} should preserve paragraph left indent",
        input.label
    );

    let text_run = paragraph
        .inlines
        .iter()
        .find_map(|inline| match inline {
            Inline::Text(run) => Some(run),
            _ => None,
        })
        .unwrap_or_else(|| panic!("fixture {} should preserve a styled text run", input.label));

    assert!(
        text_run.style.bold,
        "fixture {} should preserve bold style",
        input.label
    );
    assert!(
        text_run.style.italic,
        "fixture {} should preserve italic style",
        input.label
    );
    assert!(
        text_run.style.underline,
        "fixture {} should preserve underline style",
        input.label
    );
    assert!(
        text_run.style.strike,
        "fixture {} should preserve strike style",
        input.label
    );
    assert_eq!(
        text_run.style.font_family.as_deref(),
        Some("Noto Sans KR"),
        "fixture {} should preserve font family",
        input.label
    );
    assert_eq!(
        text_run.style.font_size_pt,
        Some(LengthPt(12.0)),
        "fixture {} should preserve font size",
        input.label
    );
    assert_eq!(
        text_run.style.color,
        Some(Color {
            r: 3,
            g: 2,
            b: 1,
            a: 255,
        }),
        "fixture {} should preserve text color",
        input.label
    );
    assert_eq!(
        text_run.style.background_color,
        Some(Color {
            r: 6,
            g: 5,
            b: 4,
            a: 255,
        }),
        "fixture {} should preserve text background color",
        input.label
    );
}

fn block_paragraph_texts(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph_plain_text(paragraph)),
            _ => None,
        })
        .collect()
}

fn collect_tables(document: &Document) -> Vec<&Table> {
    document
        .sections
        .iter()
        .flat_map(|section| &section.blocks)
        .filter_map(|block| match block {
            Block::Table(table) => Some(table),
            _ => None,
        })
        .collect()
}

fn table_cell_plain_text(cell: &TableCell) -> String {
    cell.blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph_plain_text(paragraph)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
struct DocumentStats {
    sections: usize,
    body_blocks: usize,
    headers: usize,
    footers: usize,
    notes: usize,
    resources: usize,
    image_resources: usize,
    binary_resources: usize,
    warnings: usize,
    paragraphs: usize,
    list_paragraphs: usize,
    styled_paragraphs: usize,
    tables: usize,
    table_rows: usize,
    table_cells: usize,
    table_cells_with_background: usize,
    images: usize,
    equations: usize,
    shapes: usize,
    charts: usize,
    unknown_blocks: usize,
    text_runs: usize,
    styled_text_runs: usize,
    line_breaks: usize,
    tabs: usize,
    links: usize,
    footnote_refs: usize,
    endnote_refs: usize,
    unknown_inlines: usize,
}

impl DocumentStats {
    fn from_document(document: &Document) -> Self {
        let mut stats = Self {
            sections: document.sections.len(),
            notes: document.notes.notes.len(),
            resources: document.resources.entries.len(),
            warnings: document.warnings.len(),
            ..Default::default()
        };

        for resource in &document.resources.entries {
            match resource {
                Resource::Image(_) => stats.image_resources += 1,
                Resource::Binary(_) => stats.binary_resources += 1,
            }
        }

        for section in &document.sections {
            stats.body_blocks += section.blocks.len();
            stats.headers += section.headers.len();
            stats.footers += section.footers.len();
            stats.count_blocks(&section.blocks);

            for header in &section.headers {
                stats.count_blocks(&header.blocks);
            }

            for footer in &section.footers {
                stats.count_blocks(&footer.blocks);
            }
        }

        for note in &document.notes.notes {
            stats.count_blocks(&note.blocks);
        }

        stats
    }

    fn has_semantic_content(&self) -> bool {
        self.paragraphs > 0
            || self.tables > 0
            || self.images > 0
            || self.equations > 0
            || self.shapes > 0
            || self.charts > 0
            || self.unknown_blocks > 0
            || self.notes > 0
            || self.headers > 0
            || self.footers > 0
    }

    fn count_blocks(&mut self, blocks: &[Block]) {
        for block in blocks {
            self.count_block(block);
        }
    }

    fn count_block(&mut self, block: &Block) {
        match block {
            Block::Paragraph(paragraph) => self.count_paragraph(paragraph),
            Block::Table(table) => {
                self.tables += 1;
                self.table_rows += table.rows.len();

                for row in &table.rows {
                    self.table_cells += row.cells.len();
                    for cell in &row.cells {
                        if cell.style.background_color.is_some() {
                            self.table_cells_with_background += 1;
                        }
                        self.count_blocks(&cell.blocks);
                    }
                }
            }
            Block::Image(_) => self.images += 1,
            Block::Equation(_) => self.equations += 1,
            Block::Shape(_) => self.shapes += 1,
            Block::Chart(_) => self.charts += 1,
            Block::Unknown(_) => self.unknown_blocks += 1,
        }
    }

    fn count_paragraph(&mut self, paragraph: &Paragraph) {
        self.paragraphs += 1;
        if paragraph.list.is_some() {
            self.list_paragraphs += 1;
        }
        if paragraph.style_ref.is_some() || paragraph.style != ParagraphStyle::default() {
            self.styled_paragraphs += 1;
        }
        self.count_inlines(&paragraph.inlines);
    }

    fn count_inlines(&mut self, inlines: &[Inline]) {
        for inline in inlines {
            match inline {
                Inline::Text(run) => {
                    self.text_runs += 1;
                    if run.style_ref.is_some() || run.style != TextStyle::default() {
                        self.styled_text_runs += 1;
                    }
                }
                Inline::LineBreak => self.line_breaks += 1,
                Inline::Tab => self.tabs += 1,
                Inline::Link(link) => {
                    self.links += 1;
                    self.count_inlines(&link.inlines);
                }
                Inline::FootnoteRef { .. } => self.footnote_refs += 1,
                Inline::EndnoteRef { .. } => self.endnote_refs += 1,
                Inline::Unknown(_) => self.unknown_inlines += 1,
            }
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
