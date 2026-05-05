use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bridge;
use crate::cli::{CliArgs, OutputFormat};
use crate::ir::{
    Alignment, Block, Chart, Color, Document, Equation, EquationKind, HeaderFooter,
    HeaderFooterPlacement, Image, Inline, Link, ListInfo, ListKind, Note, NoteId, NoteKind,
    Paragraph, ParagraphStyle, Resource, ResourceId, ResourceStore, Section, Shape, Table,
    TableCell, TableCellStyle, TableRow, TableStyle, TextRun, TextStyle, UnknownBlock,
    UnknownInline,
};
use crate::util::plain_text;

const DEFAULT_IMAGE_ASSET_PUBLIC_PREFIX: &str = "document_assets/images";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedFile {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFile {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedFile {
    pub input_path: PathBuf,
    pub error_message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    converted_files: Vec<ExportedFile>,
    skipped_files: Vec<SkippedFile>,
    failed_files: Vec<FailedFile>,
}

impl ExportReport {
    pub fn converted_files(&self) -> &[ExportedFile] {
        &self.converted_files
    }

    pub fn skipped_files(&self) -> &[SkippedFile] {
        &self.skipped_files
    }

    pub fn failed_files(&self) -> &[FailedFile] {
        &self.failed_files
    }
}

enum ExportOutcome {
    Converted(ExportedFile),
    Skipped(SkippedFile),
}

pub fn write_manifest(
    manifest_path: &Path,
    args: &CliArgs,
    report: &ExportReport,
) -> Result<(), Box<dyn Error>> {
    let mut files = Vec::with_capacity(
        report.converted_files.len() + report.skipped_files.len() + report.failed_files.len(),
    );

    for file in &report.converted_files {
        files.push(ManifestFileEntry {
            input_path: file.input_path.display().to_string(),
            output_path: Some(file.output_path.display().to_string()),
            status: "success",
            error: None,
        });
    }

    for file in &report.skipped_files {
        files.push(ManifestFileEntry {
            input_path: file.input_path.display().to_string(),
            output_path: Some(file.output_path.display().to_string()),
            status: "skipped",
            error: None,
        });
    }

    for file in &report.failed_files {
        files.push(ManifestFileEntry {
            input_path: file.input_path.display().to_string(),
            output_path: None,
            status: "failed",
            error: Some(file.error_message.clone()),
        });
    }

    let manifest = ManifestExport {
        input_path: args.input_path.display().to_string(),
        format: args.format.to_string(),
        recursive: args.recursive,
        continue_on_error: args.continue_on_error,
        skip_existing: args.skip_existing,
        resume_manifest: args
            .resume_manifest_path
            .as_ref()
            .map(|path| path.display().to_string()),
        output_dir: args
            .output_dir
            .as_ref()
            .map(|path| path.display().to_string()),
        converted_count: report.converted_files.len(),
        skipped_count: report.skipped_files.len(),
        failed_count: report.failed_files.len(),
        files,
    };

    let content = serde_json::to_string_pretty(&manifest).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to serialize manifest output: {error}"),
        )
    })?;

    if let Some(parent) = manifest_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    fs::write(manifest_path, content)?;

    Ok(())
}

pub fn export(args: &CliArgs) -> Result<ExportReport, Box<dyn Error>> {
    validate_input_path(&args.input_path, args.recursive)?;
    let resume_outputs = load_resume_outputs(args.resume_manifest_path.as_deref())?;

    if args.input_path.is_dir() {
        export_directory_recursively(
            &args.input_path,
            args.format,
            args.continue_on_error,
            args.output_dir.as_deref(),
            args.skip_existing,
            &resume_outputs,
        )
    } else {
        export_single_input(args, &resume_outputs)
    }
}

fn export_single_input(
    args: &CliArgs,
    resume_outputs: &HashMap<String, Option<PathBuf>>,
) -> Result<ExportReport, Box<dyn Error>> {
    match export_file(
        &args.input_path,
        &args.input_path,
        args.format,
        args.output_dir.as_deref(),
        args.skip_existing,
        resume_outputs,
    ) {
        Ok(ExportOutcome::Converted(exported_file)) => Ok(ExportReport {
            converted_files: vec![exported_file],
            skipped_files: Vec::new(),
            failed_files: Vec::new(),
        }),
        Ok(ExportOutcome::Skipped(skipped_file)) => Ok(ExportReport {
            converted_files: Vec::new(),
            skipped_files: vec![skipped_file],
            failed_files: Vec::new(),
        }),
        Err(error) if args.continue_on_error => Ok(ExportReport {
            converted_files: Vec::new(),
            skipped_files: Vec::new(),
            failed_files: vec![FailedFile {
                input_path: args.input_path.clone(),
                error_message: error.to_string(),
            }],
        }),
        Err(error) => Err(error),
    }
}

fn export_directory_recursively(
    input_dir: &Path,
    format: OutputFormat,
    continue_on_error: bool,
    output_dir: Option<&Path>,
    skip_existing: bool,
    resume_outputs: &HashMap<String, Option<PathBuf>>,
) -> Result<ExportReport, Box<dyn Error>> {
    let mut input_files = Vec::new();
    collect_supported_input_files(input_dir, &mut input_files)?;

    if input_files.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "디렉토리 안에서 변환할 .hwp 또는 .hwpx 파일을 찾을 수 없습니다: {}",
                input_dir.display()
            ),
        )
        .into());
    }

    let mut converted_files = Vec::with_capacity(input_files.len());
    let mut skipped_files = Vec::new();
    let mut failed_files = Vec::new();

    for input_path in input_files {
        match export_file(
            &input_path,
            input_dir,
            format,
            output_dir,
            skip_existing,
            resume_outputs,
        ) {
            Ok(ExportOutcome::Converted(exported_file)) => converted_files.push(exported_file),
            Ok(ExportOutcome::Skipped(skipped_file)) => skipped_files.push(skipped_file),
            Err(error) if continue_on_error => failed_files.push(FailedFile {
                input_path,
                error_message: error.to_string(),
            }),
            Err(error) => return Err(error),
        }
    }

    Ok(ExportReport {
        converted_files,
        skipped_files,
        failed_files,
    })
}

fn export_file(
    input_path: &Path,
    input_root: &Path,
    format: OutputFormat,
    output_dir: Option<&Path>,
    skip_existing: bool,
    resume_outputs: &HashMap<String, Option<PathBuf>>,
) -> Result<ExportOutcome, Box<dyn Error>> {
    validate_supported_file(input_path)?;

    let output_path = create_output_path(input_path, input_root, output_dir, format)?;
    let resume_key = create_resume_key(input_path);
    if let Some(previous_output_path) = resume_outputs.get(&resume_key) {
        return Ok(ExportOutcome::Skipped(SkippedFile {
            input_path: input_path.to_path_buf(),
            output_path: previous_output_path
                .clone()
                .unwrap_or_else(|| output_path.clone()),
        }));
    }

    if skip_existing && output_path.exists() {
        return Ok(ExportOutcome::Skipped(SkippedFile {
            input_path: input_path.to_path_buf(),
            output_path,
        }));
    }

    let document = bridge::rhwp::read_document(input_path)?;

    match format {
        OutputFormat::Txt => {
            let document_text = plain_text::to_plain_text(&document);
            write_txt_output(&output_path, &document_text)?;
        }
        OutputFormat::Svg => {
            write_svg_output(input_path, &output_path, &document)?;
        }
        OutputFormat::Json => {
            write_json_output(&output_path, &document)?;
        }
        OutputFormat::Html => {
            write_html_output(input_path, &output_path, &document)?;
        }
        OutputFormat::Markdown => {
            write_markdown_output(&output_path, &document)?;
        }
    }

    Ok(ExportOutcome::Converted(ExportedFile {
        input_path: input_path.to_path_buf(),
        output_path,
    }))
}

fn validate_input_path(input_path: &Path, recursive: bool) -> Result<(), io::Error> {
    if !input_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("입력 경로를 찾을 수 없습니다: {}", input_path.display()),
        ));
    }

    if input_path.is_dir() {
        if !recursive {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "디렉토리 입력은 --recursive와 함께 사용해야 합니다.",
            ));
        }

        return Ok(());
    }

    if !input_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "입력 경로가 파일 또는 디렉토리가 아닙니다: {}",
                input_path.display()
            ),
        ));
    }

    validate_supported_file(input_path)
}

fn validate_supported_file(input_path: &Path) -> Result<(), io::Error> {
    if !has_supported_input_extension(input_path) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "현재 버전은 .hwp, .hwpx 파일만 지원합니다.",
        ));
    }

    Ok(())
}

fn collect_supported_input_files(
    input_dir: &Path,
    input_files: &mut Vec<PathBuf>,
) -> Result<(), io::Error> {
    let mut entries = fs::read_dir(input_dir)?.collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_supported_input_files(&path, input_files)?;
        } else if path.is_file() && has_supported_input_extension(&path) {
            input_files.push(path);
        }
    }

    Ok(())
}

fn has_supported_input_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("hwp") || extension.eq_ignore_ascii_case("hwpx")
        })
}

fn create_output_path(
    input_path: &Path,
    input_root: &Path,
    output_dir: Option<&Path>,
    format: OutputFormat,
) -> Result<PathBuf, io::Error> {
    if let Some(output_dir) = output_dir {
        let relative_path = if input_root.is_dir() {
            input_path.strip_prefix(input_root).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("failed to build relative output path: {error}"),
                )
            })?
        } else {
            Path::new(input_path.file_name().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("missing file name for input path: {}", input_path.display()),
                )
            })?)
        };

        let mut output_path = output_dir.join(relative_path);
        output_path.set_extension(format.extension());

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        return Ok(output_path);
    }

    Ok(input_path.with_extension(format.extension()))
}

fn write_txt_output(output_path: &Path, document_text: &str) -> Result<(), io::Error> {
    fs::write(output_path, document_text)
}

fn write_svg_output(
    input_path: &Path,
    output_path: &Path,
    document: &Document,
) -> Result<(), io::Error> {
    let paragraphs = plain_text::collect_block_texts(document);
    let svg = render_svg_document(input_path, &paragraphs);
    fs::write(output_path, svg)
}

fn write_json_output(output_path: &Path, document: &Document) -> Result<(), io::Error> {
    let content = serde_json::to_string_pretty(document).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to serialize JSON output: {error}"),
        )
    })?;

    fs::write(output_path, content)
}

fn write_html_output(
    input_path: &Path,
    output_path: &Path,
    document: &Document,
) -> Result<(), io::Error> {
    let image_assets = image_asset_paths(output_path);
    write_image_assets(&image_assets, &document.resources)?;
    let html =
        render_html_document_with_asset_prefix(input_path, document, &image_assets.public_prefix);
    fs::write(output_path, html)
}

fn write_markdown_output(output_path: &Path, document: &Document) -> Result<(), io::Error> {
    let image_assets = image_asset_paths(output_path);
    write_image_assets(&image_assets, &document.resources)?;
    let markdown =
        render_markdown_document_with_asset_prefix(document, &image_assets.public_prefix);
    fs::write(output_path, markdown)
}

fn render_svg_document(input_path: &Path, paragraphs: &[String]) -> String {
    let lines = collect_render_lines(paragraphs);
    let padding_x = 40_u32;
    let padding_top = 40_u32;
    let padding_bottom = 40_u32;
    let line_height = 28_u32;
    let paragraph_gap = 16_u32;
    let font_size = 18_u32;
    let longest_line = lines
        .iter()
        .map(|line| line.content.chars().count() as u32)
        .max()
        .unwrap_or(0);
    let width = (padding_x * 2 + longest_line.saturating_mul(10)).clamp(640, 1600);
    let content_height = lines.iter().fold(font_size, |height, line| {
        height
            + if line.add_paragraph_gap {
                line_height + paragraph_gap
            } else {
                line_height
            }
    });
    let height = padding_top + padding_bottom + content_height;
    let file_name = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document.hwpx");
    let title = escape_xml(&format!("{file_name} text export"));
    let desc = escape_xml("Generated from rhwp paragraph text by hwp-convert");

    let mut text_nodes = String::new();
    let mut y = padding_top + font_size;
    for line in &lines {
        let content = if line.content.is_empty() {
            " "
        } else {
            &line.content
        };
        text_nodes.push_str(&format!(
            "    <text x=\"{padding_x}\" y=\"{y}\" xml:space=\"preserve\">{}</text>\n",
            escape_xml(content)
        ));
        y += if line.add_paragraph_gap {
            line_height + paragraph_gap
        } else {
            line_height
        };
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n\
  <title>{title}</title>\n\
  <desc>{desc}</desc>\n\
  <rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n\
  <g fill=\"#111827\" font-family=\"Noto Sans KR, Malgun Gothic, Apple SD Gothic Neo, sans-serif\" font-size=\"{font_size}\">\n\
{text_nodes}  </g>\n\
</svg>\n"
    )
}

fn render_html_document(input_path: &Path, document: &Document) -> String {
    let output_path = input_path.with_extension("html");
    let asset_prefix = image_asset_public_prefix(&output_path);
    render_html_document_with_asset_prefix(input_path, document, &asset_prefix)
}

fn render_html_document_with_asset_prefix(
    input_path: &Path,
    document: &Document,
    image_asset_prefix: &str,
) -> String {
    let file_name = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document");
    let title = escape_html(&format!("{file_name} text export"));

    let document_nodes = render_html_sections(document, image_asset_prefix);
    let note_nodes = render_html_notes(document, image_asset_prefix);

    format!(
        "<!DOCTYPE html>\n\
<html lang=\"ko\">\n\
  <head>\n\
    <meta charset=\"UTF-8\" />\n\
    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\" />\n\
    <title>{title}</title>\n\
    <style>\n\
      :root {{\n\
        color-scheme: light;\n\
      }}\n\
      body {{\n\
        margin: 0;\n\
        background: #f8fafc;\n\
        color: #111827;\n\
        font-family: \"Noto Sans KR\", \"Malgun Gothic\", \"Apple SD Gothic Neo\", sans-serif;\n\
      }}\n\
      main {{\n\
        max-width: 920px;\n\
        margin: 0 auto;\n\
        padding: 48px 32px 64px;\n\
      }}\n\
      h1 {{\n\
        margin: 0 0 24px;\n\
        font-size: 28px;\n\
      }}\n\
      article {{\n\
        background: #ffffff;\n\
        border: 1px solid #e5e7eb;\n\
        border-radius: 16px;\n\
        padding: 32px;\n\
        box-shadow: 0 18px 45px rgba(15, 23, 42, 0.08);\n\
      }}\n\
      figure {{\n\
        margin: 0 0 1em;\n\
      }}\n\
      img {{\n\
        display: block;\n\
        max-width: 100%;\n\
      }}\n\
      figcaption {{\n\
        margin-top: 0.5em;\n\
        color: #4b5563;\n\
        font-size: 14px;\n\
      }}\n\
      table {{\n\
        width: 100%;\n\
        border-collapse: collapse;\n\
        margin: 0 0 1em;\n\
      }}\n\
      td {{\n\
        border: 1px solid #e5e7eb;\n\
        padding: 12px 14px;\n\
        vertical-align: top;\n\
      }}\n\
      p {{\n\
        margin: 0 0 1em;\n\
        line-height: 1.8;\n\
        white-space: normal;\n\
      }}\n\
      p:last-child {{\n\
        margin-bottom: 0;\n\
      }}\n\
      td p {{\n\
        margin-bottom: 0.75em;\n\
      }}\n\
      td p:last-child {{\n\
        margin-bottom: 0;\n\
      }}\n\
      header, footer {{\n\
        margin: 0 0 1em;\n\
        padding: 12px 14px;\n\
        border: 1px dashed #cbd5e1;\n\
        border-radius: 12px;\n\
        background: #f8fafc;\n\
      }}\n\
      .notes {{\n\
        margin-top: 2rem;\n\
        padding-top: 1.5rem;\n\
        border-top: 1px solid #e5e7eb;\n\
      }}\n\
      .notes ol {{\n\
        margin: 0;\n\
        padding-left: 1.5rem;\n\
      }}\n\
      .notes li {{\n\
        margin-bottom: 1rem;\n\
      }}\n\
      .note-ref {{\n\
        font-size: 0.875em;\n\
      }}\n\
      .equation {{\n\
        font-family: \"Times New Roman\", serif;\n\
      }}\n\
      .shape-placeholder,\n\
      .chart-placeholder {{\n\
        display: inline-block;\n\
        padding: 0.1em 0.45em;\n\
        border: 1px solid #cbd5e1;\n\
        border-radius: 999px;\n\
        color: #475569;\n\
        background: #f8fafc;\n\
      }}\n\
      article > *:last-child {{\n\
        margin-bottom: 0;\n\
      }}\n\
    </style>\n\
  </head>\n\
  <body>\n\
    <main>\n\
      <h1>{title}</h1>\n\
      <article>\n\
{document_nodes}{note_nodes}      </article>\n\
    </main>\n\
  </body>\n\
</html>\n"
    )
}

fn render_html_sections(document: &Document, image_asset_prefix: &str) -> String {
    let mut document_nodes = String::new();

    for section in &document.sections {
        document_nodes.push_str(&render_html_section(
            section,
            &document.resources,
            image_asset_prefix,
        ));
    }

    if document_nodes.is_empty() {
        document_nodes.push_str("    <p></p>\n");
    }

    document_nodes
}

fn render_html_section(
    section: &Section,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let mut nodes = String::new();

    for header in &section.headers {
        nodes.push_str(&render_html_header_footer(
            "header",
            header,
            resources,
            image_asset_prefix,
        ));
    }
    for block in &section.blocks {
        nodes.push_str(&render_html_block(block, resources, image_asset_prefix));
    }
    for footer in &section.footers {
        nodes.push_str(&render_html_header_footer(
            "footer",
            footer,
            resources,
            image_asset_prefix,
        ));
    }

    nodes
}

fn render_html_header_footer(
    tag_name: &str,
    header_footer: &HeaderFooter,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let content = header_footer
        .blocks
        .iter()
        .map(|block| render_html_block(block, resources, image_asset_prefix))
        .collect::<Vec<_>>()
        .join("");
    let placement = escape_html(header_footer_placement_name(&header_footer.placement));

    format!("<{tag_name} data-placement=\"{placement}\">{content}</{tag_name}>\n")
}

fn render_html_notes(document: &Document, image_asset_prefix: &str) -> String {
    if document.notes.notes.is_empty() {
        return String::new();
    }

    let note_items = document
        .notes
        .notes
        .iter()
        .map(|note| render_html_note(note, &document.resources, image_asset_prefix))
        .collect::<Vec<_>>()
        .join("");

    format!("<section class=\"notes\">\n<h2>주석</h2>\n<ol>\n{note_items}</ol>\n</section>\n")
}

fn render_html_note(note: &Note, resources: &ResourceStore, image_asset_prefix: &str) -> String {
    let id = escape_html(&note_html_anchor_id(&note.id));
    let kind = escape_html(note_kind_name(&note.kind));
    let content = note
        .blocks
        .iter()
        .map(|block| render_html_block(block, resources, image_asset_prefix))
        .collect::<Vec<_>>()
        .join("");

    format!("<li id=\"{id}\" data-kind=\"{kind}\">{content}</li>\n")
}

fn render_html_block(block: &Block, resources: &ResourceStore, image_asset_prefix: &str) -> String {
    match block {
        Block::Paragraph(paragraph) => render_html_paragraph(paragraph),
        Block::Table(table) => render_html_table(table, resources, image_asset_prefix),
        Block::Image(image) => render_html_image(image, resources, image_asset_prefix),
        Block::Equation(equation) => render_html_equation(equation),
        Block::Shape(shape) => render_html_shape(shape),
        Block::Chart(chart) => render_html_chart(chart),
        Block::Unknown(unknown) => {
            let content = render_html_fallback_text(&unknown_block_display_text(unknown));
            format!("<p>{content}</p>\n")
        }
    }
}

fn render_html_paragraph(paragraph: &Paragraph) -> String {
    let mut content = String::new();
    if let Some(list) = &paragraph.list {
        content.push_str(&render_html_fallback_text(&list_prefix(list)));
    }
    content.push_str(&render_html_inlines(&paragraph.inlines));
    let style = render_html_style_attr(&render_html_paragraph_style(&paragraph.style));

    format!("<p{style}>{content}</p>\n")
}

fn render_html_inlines(inlines: &[Inline]) -> String {
    let mut content = String::new();

    for inline in inlines {
        match inline {
            Inline::Text(run) => content.push_str(&render_html_text_run(run)),
            Inline::LineBreak => content.push_str("<br />"),
            Inline::Tab => content.push('\t'),
            Inline::Link(link) => content.push_str(&render_html_link(link)),
            Inline::FootnoteRef { note_id } => {
                content.push_str(&render_html_note_ref(note_id, NoteKind::Footnote));
            }
            Inline::EndnoteRef { note_id } => {
                content.push_str(&render_html_note_ref(note_id, NoteKind::Endnote));
            }
            Inline::Unknown(unknown) => {
                content.push_str(&render_html_fallback_text(&unknown_inline_display_text(
                    unknown,
                )));
            }
        }
    }

    content
}

fn render_html_fallback_text(text: &str) -> String {
    escape_html(text).replace('\n', "<br />")
}

fn render_html_text_run(run: &TextRun) -> String {
    let content = render_html_fallback_text(&run.text);
    let style = render_html_style_attr(&render_html_text_style(&run.style));

    if style.is_empty() {
        content
    } else {
        format!("<span{style}>{content}</span>")
    }
}

fn render_html_link(link: &Link) -> String {
    let href = escape_html(&link.url);
    let title = link
        .title
        .as_deref()
        .map(escape_html)
        .map(|title| format!(" title=\"{title}\""))
        .unwrap_or_default();
    let content = render_html_inlines(&link.inlines);

    format!("<a href=\"{href}\"{title}>{content}</a>")
}

fn render_html_note_ref(note_id: &NoteId, kind: NoteKind) -> String {
    let note_anchor = escape_html(&note_html_anchor_id(note_id));
    let label = match kind {
        NoteKind::Footnote => "각주",
        NoteKind::Endnote => "미주",
    };
    let text = escape_html(note_id.as_str());

    format!("<sup class=\"note-ref\"><a href=\"#{note_anchor}\">[{label}: {text}]</a></sup>")
}

fn render_html_equation(equation: &Equation) -> String {
    let content = render_html_fallback_text(&equation_display_text(equation));
    format!("<p><span class=\"equation\">{content}</span></p>\n")
}

fn render_html_shape(shape: &Shape) -> String {
    let content = render_html_fallback_text(&shape_display_text(shape));
    format!("<p><span class=\"shape-placeholder\">{content}</span></p>\n")
}

fn render_html_chart(chart: &Chart) -> String {
    let content = render_html_fallback_text(&chart_display_text(chart));
    format!("<p><span class=\"chart-placeholder\">{content}</span></p>\n")
}

fn render_html_text_style(style: &TextStyle) -> String {
    let mut declarations = Vec::new();

    if style.bold {
        declarations.push("font-weight: bold".to_string());
    }
    if style.italic {
        declarations.push("font-style: italic".to_string());
    }

    let mut decorations = Vec::new();
    if style.underline {
        decorations.push("underline");
    }
    if style.strike {
        decorations.push("line-through");
    }
    if !decorations.is_empty() {
        declarations.push(format!("text-decoration: {}", decorations.join(" ")));
    }

    if let Some(font_family) = style
        .font_family
        .as_deref()
        .and_then(sanitize_css_font_family)
    {
        declarations.push(format!("font-family: {font_family}"));
    }
    if let Some(font_size_pt) = style.font_size_pt {
        declarations.push(format!("font-size: {}pt", font_size_pt.0));
    }
    if let Some(color) = style.color {
        declarations.push(format!("color: {}", render_css_color(color)));
    }
    if let Some(background_color) = style.background_color {
        declarations.push(format!(
            "background-color: {}",
            render_css_color(background_color)
        ));
    }

    declarations.join("; ")
}

fn render_html_paragraph_style(style: &ParagraphStyle) -> String {
    let mut declarations = Vec::new();

    if let Some(alignment) = &style.alignment {
        declarations.push(format!("text-align: {}", alignment_to_css(alignment)));
    }
    if let Some(before_pt) = style.spacing.before_pt {
        declarations.push(format!("margin-top: {}pt", before_pt.0));
    }
    if let Some(after_pt) = style.spacing.after_pt {
        declarations.push(format!("margin-bottom: {}pt", after_pt.0));
    }
    if let Some(first_line_pt) = style.indent.first_line_pt {
        declarations.push(format!("text-indent: {}pt", first_line_pt.0));
    }
    if let Some(left_pt) = style.indent.left_pt {
        declarations.push(format!("margin-left: {}pt", left_pt.0));
    }
    if let Some(right_pt) = style.indent.right_pt {
        declarations.push(format!("margin-right: {}pt", right_pt.0));
    }

    declarations.join("; ")
}

fn render_html_table_style(style: &TableStyle) -> String {
    style
        .background_color
        .map(render_css_color)
        .map(|color| format!("background-color: {color}"))
        .unwrap_or_default()
}

fn render_html_table_cell_style(style: &TableCellStyle) -> String {
    style
        .background_color
        .map(render_css_color)
        .map(|color| format!("background-color: {color}"))
        .unwrap_or_default()
}

fn render_html_style_attr(style: &str) -> String {
    if style.is_empty() {
        String::new()
    } else {
        format!(" style=\"{}\"", escape_html(style))
    }
}

fn list_prefix(list: &ListInfo) -> String {
    let indent = "  ".repeat(list.level as usize);
    let marker = match list.kind {
        ListKind::Ordered => format!("{}. ", list.number.unwrap_or(1)),
        ListKind::Unordered | ListKind::Unknown => {
            format!("{} ", list.marker.as_deref().unwrap_or("-"))
        }
    };

    format!("{indent}{marker}")
}

fn note_html_anchor_id(note_id: &NoteId) -> String {
    format!("note-{}", sanitize_html_anchor(note_id.as_str()))
}

fn sanitize_html_anchor(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "note".to_string()
    } else {
        sanitized
    }
}

fn note_kind_name(kind: &NoteKind) -> &'static str {
    match kind {
        NoteKind::Footnote => "footnote",
        NoteKind::Endnote => "endnote",
    }
}

fn header_footer_placement_name(placement: &HeaderFooterPlacement) -> &'static str {
    match placement {
        HeaderFooterPlacement::Default => "default",
        HeaderFooterPlacement::FirstPage => "first_page",
        HeaderFooterPlacement::OddPage => "odd_page",
        HeaderFooterPlacement::EvenPage => "even_page",
    }
}

fn equation_display_text(equation: &Equation) -> String {
    let text = equation
        .fallback_text
        .as_deref()
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            if matches!(
                equation.kind,
                EquationKind::PlainText | EquationKind::Latex | EquationKind::MathMl
            ) {
                equation
                    .content
                    .as_deref()
                    .filter(|text| !text.is_empty())
                    .map(ToOwned::to_owned)
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unsupported".to_string());

    format!("[equation: {text}]")
}

fn shape_display_text(shape: &Shape) -> String {
    let text = shape
        .fallback_text
        .as_deref()
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            shape
                .description
                .as_deref()
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "unsupported".to_string());

    format!("[shape: {text}]")
}

fn chart_display_text(chart: &Chart) -> String {
    let text = chart
        .fallback_text
        .as_deref()
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            chart
                .title
                .as_deref()
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "unsupported".to_string());

    format!("[chart: {text}]")
}

fn unknown_block_display_text(unknown: &UnknownBlock) -> String {
    unknown
        .fallback_text
        .as_deref()
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("[unknown: {}]", unknown.kind))
}

fn unknown_inline_display_text(unknown: &UnknownInline) -> String {
    unknown
        .fallback_text
        .as_deref()
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("[unknown: {}]", unknown.kind))
}

fn alignment_to_css(alignment: &Alignment) -> &'static str {
    match alignment {
        Alignment::Left => "left",
        Alignment::Center => "center",
        Alignment::Right => "right",
        Alignment::Justify => "justify",
    }
}

fn render_css_color(color: Color) -> String {
    if color.a == 255 {
        return format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b);
    }

    format!(
        "rgba({}, {}, {}, {})",
        color.r,
        color.g,
        color.b,
        color.a as f32 / 255.0
    )
}

fn sanitize_css_font_family(font_family: &str) -> Option<String> {
    let sanitized = font_family
        .split(',')
        .map(|family| {
            family
                .chars()
                .filter(|ch| ch.is_alphanumeric() || ch.is_whitespace() || matches!(ch, '-' | '_'))
                .collect::<String>()
        })
        .map(|family| family.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|family| !family.is_empty())
        .collect::<Vec<_>>();

    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized.join(", "))
    }
}

fn render_html_image(image: &Image, resources: &ResourceStore, image_asset_prefix: &str) -> String {
    let src = escape_html(&resource_public_path(
        resources,
        &image.resource_id,
        image_asset_prefix,
    ));
    let alt = escape_html(image.alt.as_deref().unwrap_or(image.resource_id.as_str()));
    let width = image
        .width
        .map(|width| format!(" width=\"{}\"", width.0))
        .unwrap_or_default();
    let height = image
        .height
        .map(|height| format!(" height=\"{}\"", height.0))
        .unwrap_or_default();
    let tag = format!("<img src=\"{src}\" alt=\"{alt}\"{width}{height} />");

    if let Some(caption) = &image.caption {
        return format!(
            "<figure>{tag}<figcaption>{}</figcaption></figure>\n",
            escape_html(caption)
        );
    }

    format!("{tag}\n")
}

fn render_html_table(table: &Table, resources: &ResourceStore, image_asset_prefix: &str) -> String {
    let mut html = format!(
        "<table{}>\n",
        render_html_style_attr(&render_html_table_style(&table.style))
    );

    for row in &table.rows {
        html.push_str(&render_html_table_row(row, resources, image_asset_prefix));
    }

    html.push_str("</table>\n");
    html
}

fn render_html_table_row(
    row: &TableRow,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let mut html = String::from("<tr>\n");

    for cell in &row.cells {
        html.push_str(&render_html_table_cell(cell, resources, image_asset_prefix));
    }

    html.push_str("</tr>\n");
    html
}

fn render_html_table_cell(
    cell: &TableCell,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let rowspan = if cell.row_span > 1 {
        format!(" rowspan=\"{}\"", cell.row_span)
    } else {
        String::new()
    };
    let colspan = if cell.col_span > 1 {
        format!(" colspan=\"{}\"", cell.col_span)
    } else {
        String::new()
    };
    let content = render_html_table_cell_blocks(&cell.blocks, resources, image_asset_prefix);
    let style = render_html_style_attr(&render_html_table_cell_style(&cell.style));

    format!("<td{rowspan}{colspan}{style}>{content}</td>\n")
}

fn render_html_table_cell_blocks(
    blocks: &[Block],
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    blocks
        .iter()
        .map(|block| render_html_block(block, resources, image_asset_prefix))
        .collect::<Vec<_>>()
        .join("")
}

fn render_markdown_document(document: &Document) -> String {
    render_markdown_document_with_asset_prefix(document, DEFAULT_IMAGE_ASSET_PUBLIC_PREFIX)
}

fn render_markdown_document_with_asset_prefix(
    document: &Document,
    image_asset_prefix: &str,
) -> String {
    let mut blocks = Vec::new();

    for section in &document.sections {
        for header in &section.headers {
            blocks.push(render_markdown_header_footer(
                "머리말",
                header,
                &document.resources,
                image_asset_prefix,
            ));
        }
        for block in &section.blocks {
            blocks.push(render_markdown_block(
                block,
                &document.resources,
                image_asset_prefix,
            ));
        }
        for footer in &section.footers {
            blocks.push(render_markdown_header_footer(
                "꼬리말",
                footer,
                &document.resources,
                image_asset_prefix,
            ));
        }
    }

    if !document.notes.notes.is_empty() {
        for note in &document.notes.notes {
            blocks.push(render_markdown_note(
                note,
                &document.resources,
                image_asset_prefix,
            ));
        }
    }

    blocks.join("\n\n")
}

fn render_markdown_block(
    block: &Block,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    match block {
        Block::Paragraph(paragraph) => render_markdown_paragraph(paragraph),
        Block::Table(table) => render_markdown_table(table),
        Block::Image(image) => render_markdown_image(image, resources, image_asset_prefix),
        Block::Equation(equation) => render_markdown_equation(equation),
        Block::Shape(shape) => render_markdown_shape(shape),
        Block::Chart(chart) => render_markdown_chart(chart),
        Block::Unknown(unknown) => render_markdown_unknown_block(unknown),
    }
}

fn render_markdown_unknown_block(unknown: &UnknownBlock) -> String {
    render_markdown_text(&unknown_block_display_text(unknown))
}

fn render_markdown_paragraph(paragraph: &Paragraph) -> String {
    let content = render_markdown_inlines(&paragraph.inlines);

    if let Some(list) = &paragraph.list {
        return format!("{}{}", list_prefix(list), content);
    }

    content
}

fn render_markdown_inlines(inlines: &[Inline]) -> String {
    let mut content = String::new();

    for inline in inlines {
        match inline {
            Inline::Text(run) => content.push_str(&render_markdown_text_run(run)),
            Inline::LineBreak => content.push_str("  \n"),
            Inline::Tab => content.push('\t'),
            Inline::Link(link) => content.push_str(&render_markdown_link(link)),
            Inline::FootnoteRef { note_id } => content.push_str(&render_markdown_note_ref(note_id)),
            Inline::EndnoteRef { note_id } => content.push_str(&render_markdown_note_ref(note_id)),
            Inline::Unknown(unknown) => {
                content.push_str(&render_markdown_text(&unknown_inline_display_text(unknown)));
            }
        }
    }

    content
}

fn render_markdown_table(table: &Table) -> String {
    if !can_render_markdown_table(table) {
        return render_markdown_text(&plain_text::table_to_plain_text(table));
    }

    let column_count = table.rows.first().map(|row| row.cells.len()).unwrap_or(0);
    if column_count == 0 {
        return render_markdown_text("[표]");
    }

    let mut lines = Vec::with_capacity(table.rows.len() + 1);
    lines.push(render_markdown_table_row(&table.rows[0]));
    lines.push(format!("| {} |", vec!["---"; column_count].join(" | ")));

    for row in table.rows.iter().skip(1) {
        lines.push(render_markdown_table_row(row));
    }

    lines.join("\n")
}

fn can_render_markdown_table(table: &Table) -> bool {
    let Some(first_row) = table.rows.first() else {
        return false;
    };

    let column_count = first_row.cells.len();
    if column_count == 0 {
        return false;
    }

    table.rows.iter().all(|row| {
        row.cells.len() == column_count
            && row.cells.iter().all(|cell| {
                cell.row_span == 1
                    && cell.col_span == 1
                    && cell.blocks.iter().all(is_markdown_simple_block)
            })
    })
}

fn is_markdown_simple_block(block: &Block) -> bool {
    matches!(block, Block::Paragraph(_) | Block::Unknown(_))
}

fn render_markdown_table_row(row: &TableRow) -> String {
    format!(
        "| {} |",
        row.cells
            .iter()
            .map(render_markdown_table_cell)
            .collect::<Vec<_>>()
            .join(" | ")
    )
}

fn render_markdown_table_cell(cell: &TableCell) -> String {
    escape_markdown_table_cell(&plain_text::blocks_to_plain_text(&cell.blocks).replace('\n', " "))
}

fn render_markdown_image(
    image: &Image,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let alt = escape_markdown_image_alt(image.alt.as_deref().unwrap_or(image.resource_id.as_str()));
    let path = resource_public_path(resources, &image.resource_id, image_asset_prefix);

    format!("![{alt}]({path})")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ImageAssetPaths {
    output_dir: PathBuf,
    public_prefix: String,
}

fn write_image_assets(
    image_assets: &ImageAssetPaths,
    resources: &ResourceStore,
) -> Result<(), io::Error> {
    let image_resources = resources
        .entries
        .iter()
        .filter_map(|resource| match resource {
            Resource::Image(image) => Some(image),
            Resource::Binary(_) => None,
        })
        .collect::<Vec<_>>();

    if image_resources.is_empty() {
        return Ok(());
    }

    let asset_dir = &image_assets.output_dir;
    fs::create_dir_all(&asset_dir)?;

    for image in image_resources {
        let file_name = resource_file_name(resources, &image.id);
        fs::write(asset_dir.join(file_name), &image.bytes)?;
    }

    Ok(())
}

fn image_asset_dir(output_path: &Path) -> PathBuf {
    image_asset_paths(output_path).output_dir
}

fn image_asset_public_prefix(output_path: &Path) -> String {
    image_asset_paths(output_path).public_prefix
}

fn image_asset_paths(output_path: &Path) -> ImageAssetPaths {
    let asset_root = format!("{}_assets", sanitized_output_file_stem(output_path));
    let output_dir = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(&asset_root)
        .join("images");

    ImageAssetPaths {
        output_dir,
        public_prefix: format!("{asset_root}/images"),
    }
}

fn sanitized_output_file_stem(output_path: &Path) -> String {
    let stem = output_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("document");
    sanitize_asset_path_segment(stem)
}

fn sanitize_asset_path_segment(segment: &str) -> String {
    let mut sanitized = String::new();
    let mut previous_was_separator = false;

    for ch in segment.chars() {
        if ch.is_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            sanitized.push(ch);
            previous_was_separator = false;
        } else if !previous_was_separator {
            sanitized.push('_');
            previous_was_separator = true;
        }
    }

    let sanitized = sanitized.trim_matches(|ch| ch == '_' || ch == '.');
    if sanitized.is_empty() {
        "document".to_string()
    } else {
        sanitized.to_string()
    }
}

fn render_markdown_equation(equation: &Equation) -> String {
    if equation.kind == EquationKind::Latex {
        if let Some(content) = &equation.content {
            return format!("$${content}$$");
        }
    }

    render_markdown_text(&equation_display_text(equation))
}

fn render_markdown_shape(shape: &Shape) -> String {
    render_markdown_text(&shape_display_text(shape))
}

fn render_markdown_chart(chart: &Chart) -> String {
    render_markdown_text(&chart_display_text(chart))
}

fn render_markdown_link(link: &Link) -> String {
    let label = render_markdown_link_label(&link.inlines);
    let url = escape_markdown_link_destination(&link.url);

    format!("[{label}]({url})")
}

fn render_markdown_link_label(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(markdown_link_label_inline)
        .collect::<Vec<_>>()
        .join("")
}

fn markdown_link_label_inline(inline: &Inline) -> String {
    match inline {
        Inline::Text(run) => escape_markdown_image_alt(&run.text),
        Inline::LineBreak => " ".to_string(),
        Inline::Tab => "\t".to_string(),
        Inline::Link(link) => render_markdown_link_label(&link.inlines),
        Inline::FootnoteRef { note_id } => format!("[^{}]", note_id.as_str()),
        Inline::EndnoteRef { note_id } => format!("[^{}]", note_id.as_str()),
        Inline::Unknown(unknown) => {
            escape_markdown_image_alt(&unknown_inline_display_text(unknown))
        }
    }
}

fn render_markdown_note_ref(note_id: &NoteId) -> String {
    format!("[^{}]", escape_markdown_note_id(note_id.as_str()))
}

fn render_markdown_note(
    note: &Note,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let note_id = escape_markdown_note_id(note.id.as_str());
    let content = note
        .blocks
        .iter()
        .map(|block| render_markdown_block(block, resources, image_asset_prefix))
        .collect::<Vec<_>>()
        .join(" ");

    format!("[^{note_id}]: {}", content.trim())
}

fn render_markdown_header_footer(
    label: &str,
    header_footer: &HeaderFooter,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let content = header_footer
        .blocks
        .iter()
        .map(|block| render_markdown_block(block, resources, image_asset_prefix))
        .collect::<Vec<_>>()
        .join("\n\n");

    if content.is_empty() {
        render_markdown_text(&format!("[{label}]"))
    } else {
        format!(
            "{}\n\n{content}",
            render_markdown_text(&format!("[{label}]"))
        )
    }
}

fn render_markdown_text_run(run: &TextRun) -> String {
    let mut text = render_markdown_text(&run.text);

    text = match (run.style.bold, run.style.italic) {
        (true, true) => format!("***{text}***"),
        (true, false) => format!("**{text}**"),
        (false, true) => format!("*{text}*"),
        (false, false) => text,
    };

    if run.style.strike {
        format!("~~{text}~~")
    } else {
        text
    }
}

fn escape_markdown_table_cell(text: &str) -> String {
    text.replace('\\', "\\\\").replace('|', "\\|")
}

fn escape_markdown_image_alt(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn escape_markdown_link_destination(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace(')', "\\)")
        .replace('(', "\\(")
}

fn escape_markdown_note_id(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn render_markdown_text(text: &str) -> String {
    text.split('\n')
        .map(escape_markdown_line)
        .collect::<Vec<_>>()
        .join("  \n")
}

fn resource_public_path(
    resources: &ResourceStore,
    resource_id: &ResourceId,
    image_asset_prefix: &str,
) -> String {
    let file_name = resource_file_name(resources, resource_id);
    format!("{image_asset_prefix}/{file_name}")
}

fn resource_file_name(resources: &ResourceStore, resource_id: &ResourceId) -> String {
    let base = resource_id.as_str();
    if base.contains('.') {
        return base.to_string();
    }

    let extension = resource_extension(resources, resource_id).unwrap_or("png");
    format!("{base}.{extension}")
}

fn resource_extension<'a>(
    resources: &'a ResourceStore,
    resource_id: &ResourceId,
) -> Option<&'a str> {
    match resources.get(resource_id) {
        Some(Resource::Image(resource)) => resource.extension.as_deref(),
        Some(Resource::Binary(resource)) => resource.extension.as_deref(),
        None => None,
    }
}

fn collect_render_lines(paragraphs: &[String]) -> Vec<RenderLine> {
    if paragraphs.is_empty() {
        return vec![RenderLine {
            content: String::new(),
            add_paragraph_gap: false,
        }];
    }

    let mut lines = Vec::new();
    for (paragraph_index, paragraph) in paragraphs.iter().enumerate() {
        let paragraph_lines: Vec<&str> = paragraph.split('\n').collect();
        for (line_index, line) in paragraph_lines.iter().enumerate() {
            lines.push(RenderLine {
                content: (*line).to_string(),
                add_paragraph_gap: paragraph_index + 1 < paragraphs.len()
                    && line_index + 1 == paragraph_lines.len(),
            });
        }
    }

    lines
}

struct RenderLine {
    content: String,
    add_paragraph_gap: bool,
}

#[derive(Serialize)]
struct ManifestExport {
    input_path: String,
    format: String,
    recursive: bool,
    continue_on_error: bool,
    skip_existing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    resume_manifest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_dir: Option<String>,
    converted_count: usize,
    skipped_count: usize,
    failed_count: usize,
    files: Vec<ManifestFileEntry>,
}

#[derive(Serialize)]
struct ManifestFileEntry {
    input_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_path: Option<String>,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Deserialize)]
struct ResumeManifest {
    files: Vec<ResumeManifestFileEntry>,
}

#[derive(Deserialize)]
struct ResumeManifestFileEntry {
    input_path: String,
    output_path: Option<String>,
    status: String,
}

fn create_resume_key(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

fn load_resume_outputs(
    resume_manifest_path: Option<&Path>,
) -> Result<HashMap<String, Option<PathBuf>>, Box<dyn Error>> {
    let Some(resume_manifest_path) = resume_manifest_path else {
        return Ok(HashMap::new());
    };

    let content = fs::read_to_string(resume_manifest_path)?;
    let manifest: ResumeManifest = serde_json::from_str(&content).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to parse resume manifest: {error}"),
        )
    })?;

    let mut completed_outputs = HashMap::new();
    for file in manifest.files {
        if file.status == "success" || file.status == "skipped" {
            completed_outputs.insert(
                create_resume_key(Path::new(&file.input_path)),
                file.output_path.map(PathBuf::from),
            );
        }
    }

    Ok(completed_outputs)
}

fn escape_xml(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(ch),
        }
    }

    escaped
}

fn escape_html(value: &str) -> String {
    escape_xml(value)
}

fn escape_markdown_line(line: &str) -> String {
    let mut escaped = line.replace('\\', "\\\\");
    let trimmed = escaped.trim_start();
    if trimmed.is_empty() {
        return escaped;
    }

    let needs_escape = matches!(
        trimmed.chars().next(),
        Some('#' | '>' | '-' | '+' | '*' | '|')
    ) || starts_with_ordered_list_marker(trimmed);

    if needs_escape {
        let indent_len = escaped.len() - trimmed.len();
        escaped.insert(indent_len, '\\');
    }

    escaped
}

fn starts_with_ordered_list_marker(line: &str) -> bool {
    let digit_count = line.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        return false;
    }

    matches!(line.chars().nth(digit_count), Some('.') | Some(')'))
        && line.chars().nth(digit_count + 1) == Some(' ')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        Alignment, Chart, Color, ConversionWarning, Equation, EquationKind, HeaderFooter,
        HeaderFooterPlacement, IR_VERSION, Image, ImageResource, Indent, LengthPt, Link, ListInfo,
        ListKind, Metadata, Note, NoteId, NoteKind, NoteStore, Paragraph, ParagraphRole,
        ParagraphStyle, Resource, ResourceId, ResourceStore, Section, Shape, ShapeKind, Spacing,
        StyleSheet, Table, TableCell, TableCellStyle, TableRow, TableStyle, TextRun, TextStyle,
        UnknownInline,
    };
    use std::fs::File;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    #[test]
    fn rejects_unsupported_extension() {
        assert!(!has_supported_input_extension(Path::new("sample.txt")));
        assert!(has_supported_input_extension(Path::new("sample.hwp")));
        assert!(has_supported_input_extension(Path::new("sample.HWP")));
        assert!(has_supported_input_extension(Path::new("sample.hwpx")));
        assert!(has_supported_input_extension(Path::new("sample.HWPX")));
    }

    #[test]
    fn collects_supported_files_recursively_in_sorted_order() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("collect");
        fs::create_dir_all(root.join("b"))?;
        fs::create_dir_all(root.join("a"))?;
        fs::write(root.join("ignore.txt"), "ignore")?;
        fs::write(root.join("b").join("two.hwp"), "binary placeholder")?;
        fs::write(root.join("a").join("one.hwpx"), "zip placeholder")?;

        let mut input_files = Vec::new();
        collect_supported_input_files(&root, &mut input_files)?;

        assert_eq!(
            input_files,
            vec![
                root.join("a").join("one.hwpx"),
                root.join("b").join("two.hwp")
            ]
        );

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    #[test]
    fn exports_directory_recursively_to_txt() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("recursive-export");
        fs::create_dir_all(root.join("nested"))?;
        write_preview_hwpx(&root.join("alpha.hwpx"), "first line")?;
        write_preview_hwpx(&root.join("nested").join("beta.hwpx"), "second line")?;
        fs::write(root.join("nested").join("ignore.txt"), "ignore")?;

        let args = CliArgs {
            input_path: root.clone(),
            format: OutputFormat::Txt,
            recursive: true,
            manifest_path: None,
            resume_manifest_path: None,
            continue_on_error: false,
            output_dir: None,
            skip_existing: false,
        };

        let report = export(&args)?;

        assert_eq!(report.converted_files().len(), 2);
        assert_eq!(report.skipped_files().len(), 0);
        assert_eq!(report.failed_files().len(), 0);
        assert_eq!(fs::read_to_string(root.join("alpha.txt"))?, "first line");
        assert_eq!(
            fs::read_to_string(root.join("nested").join("beta.txt"))?,
            "second line"
        );

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    #[test]
    fn skips_single_file_when_output_exists() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("skip-single");
        let output_dir = root.join("out");
        fs::create_dir_all(&output_dir)?;
        write_preview_hwpx(&root.join("alpha.hwpx"), "first line")?;
        fs::write(output_dir.join("alpha.txt"), "existing")?;

        let args = CliArgs {
            input_path: root.join("alpha.hwpx"),
            format: OutputFormat::Txt,
            recursive: false,
            manifest_path: None,
            resume_manifest_path: None,
            continue_on_error: false,
            output_dir: Some(output_dir.clone()),
            skip_existing: true,
        };

        let report = export(&args)?;

        assert_eq!(report.converted_files().len(), 0);
        assert_eq!(report.skipped_files().len(), 1);
        assert_eq!(report.failed_files().len(), 0);
        assert_eq!(
            report.skipped_files()[0].output_path,
            output_dir.join("alpha.txt")
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("alpha.txt"))?,
            "existing"
        );

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    #[test]
    fn skips_existing_outputs_in_directory_export() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("skip-recursive");
        let output_dir = root.join("out");
        fs::create_dir_all(root.join("nested"))?;
        fs::create_dir_all(output_dir.join("nested"))?;
        write_preview_hwpx(&root.join("alpha.hwpx"), "first line")?;
        write_preview_hwpx(&root.join("nested").join("beta.hwpx"), "second line")?;
        fs::write(output_dir.join("alpha.txt"), "existing alpha")?;

        let args = CliArgs {
            input_path: root.clone(),
            format: OutputFormat::Txt,
            recursive: true,
            manifest_path: None,
            resume_manifest_path: None,
            continue_on_error: false,
            output_dir: Some(output_dir.clone()),
            skip_existing: true,
        };

        let report = export(&args)?;

        assert_eq!(report.converted_files().len(), 1);
        assert_eq!(report.skipped_files().len(), 1);
        assert_eq!(report.failed_files().len(), 0);
        assert_eq!(
            fs::read_to_string(output_dir.join("alpha.txt"))?,
            "existing alpha"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("nested").join("beta.txt"))?,
            "second line"
        );

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    #[test]
    fn resumes_directory_export_from_previous_manifest() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("resume-recursive");
        fs::create_dir_all(&root)?;
        write_preview_hwpx(&root.join("alpha.hwpx"), "first line")?;
        write_preview_hwpx(&root.join("beta.hwpx"), "second line")?;

        let previous_manifest_path = root.join("previous-manifest.json");
        let previous_alpha_output = root.join("alpha.txt");
        let previous_manifest = serde_json::json!({
            "files": [
                {
                    "input_path": root.join("alpha.hwpx").display().to_string(),
                    "output_path": previous_alpha_output.display().to_string(),
                    "status": "success"
                },
                {
                    "input_path": root.join("beta.hwpx").display().to_string(),
                    "output_path": null,
                    "status": "failed"
                }
            ]
        });
        fs::write(
            &previous_manifest_path,
            serde_json::to_string_pretty(&previous_manifest)?,
        )?;

        let args = CliArgs {
            input_path: root.clone(),
            format: OutputFormat::Txt,
            recursive: true,
            manifest_path: None,
            resume_manifest_path: Some(previous_manifest_path),
            continue_on_error: false,
            output_dir: None,
            skip_existing: false,
        };

        let report = export(&args)?;

        assert_eq!(report.converted_files().len(), 1);
        assert_eq!(report.skipped_files().len(), 1);
        assert_eq!(report.failed_files().len(), 0);
        assert_eq!(
            report.converted_files()[0].input_path,
            root.join("beta.hwpx")
        );
        assert_eq!(
            report.skipped_files()[0],
            SkippedFile {
                input_path: root.join("alpha.hwpx"),
                output_path: previous_alpha_output,
            }
        );
        assert_eq!(fs::read_to_string(root.join("beta.txt"))?, "second line");
        assert!(!root.join("alpha.txt").exists());

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    #[test]
    fn resumes_directory_export_with_relative_input_path() -> Result<(), Box<dyn Error>> {
        let root = workspace_fixture_dir("resume-relative");
        fs::create_dir_all(&root)?;
        write_preview_hwpx(&root.join("alpha.hwpx"), "first line")?;
        write_preview_hwpx(&root.join("beta.hwpx"), "second line")?;

        let previous_manifest_path = root.join("previous-manifest.json");
        let previous_manifest = serde_json::json!({
            "files": [
                {
                    "input_path": fs::canonicalize(root.join("alpha.hwpx"))?.display().to_string(),
                    "output_path": fs::canonicalize(&root)
                        .unwrap_or_else(|_| root.clone())
                        .join("alpha.txt")
                        .display()
                        .to_string(),
                    "status": "success"
                }
            ]
        });
        fs::write(
            &previous_manifest_path,
            serde_json::to_string_pretty(&previous_manifest)?,
        )?;

        let args = CliArgs {
            input_path: root.clone(),
            format: OutputFormat::Txt,
            recursive: true,
            manifest_path: None,
            resume_manifest_path: Some(previous_manifest_path),
            continue_on_error: false,
            output_dir: None,
            skip_existing: false,
        };

        let report = export(&args)?;

        assert_eq!(report.converted_files().len(), 1);
        assert_eq!(report.skipped_files().len(), 1);
        assert_eq!(report.failed_files().len(), 0);
        assert_eq!(
            report.skipped_files()[0].input_path,
            root.join("alpha.hwpx")
        );
        assert_eq!(
            report.converted_files()[0].input_path,
            root.join("beta.hwpx")
        );
        assert_eq!(fs::read_to_string(root.join("beta.txt"))?, "second line");
        assert!(!root.join("alpha.txt").exists());

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    #[test]
    fn exports_single_file_to_output_dir() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("single-output-dir");
        let output_dir = root.join("out");
        fs::create_dir_all(&root)?;
        write_preview_hwpx(&root.join("alpha.hwpx"), "first line")?;

        let args = CliArgs {
            input_path: root.join("alpha.hwpx"),
            format: OutputFormat::Txt,
            recursive: false,
            manifest_path: None,
            resume_manifest_path: None,
            continue_on_error: false,
            output_dir: Some(output_dir.clone()),
            skip_existing: false,
        };

        let report = export(&args)?;

        assert_eq!(report.converted_files().len(), 1);
        assert_eq!(report.skipped_files().len(), 0);
        assert_eq!(report.failed_files().len(), 0);
        assert_eq!(
            report.converted_files()[0].output_path,
            output_dir.join("alpha.txt")
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("alpha.txt"))?,
            "first line"
        );

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    #[test]
    fn exports_directory_recursively_to_output_dir() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("recursive-output-dir");
        let output_dir = root.join("out");
        fs::create_dir_all(root.join("nested"))?;
        write_preview_hwpx(&root.join("alpha.hwpx"), "first line")?;
        write_preview_hwpx(&root.join("nested").join("beta.hwpx"), "second line")?;

        let args = CliArgs {
            input_path: root.clone(),
            format: OutputFormat::Txt,
            recursive: true,
            manifest_path: None,
            resume_manifest_path: None,
            continue_on_error: false,
            output_dir: Some(output_dir.clone()),
            skip_existing: false,
        };

        let report = export(&args)?;

        assert_eq!(report.converted_files().len(), 2);
        assert_eq!(report.skipped_files().len(), 0);
        assert_eq!(
            fs::read_to_string(output_dir.join("alpha.txt"))?,
            "first line"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("nested").join("beta.txt"))?,
            "second line"
        );

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    #[test]
    fn continues_directory_export_after_failure() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("continue-on-error");
        fs::create_dir_all(root.join("nested"))?;
        write_preview_hwpx(&root.join("alpha.hwpx"), "first line")?;
        fs::write(
            root.join("nested").join("broken.hwpx"),
            "not a valid hwpx file",
        )?;

        let args = CliArgs {
            input_path: root.clone(),
            format: OutputFormat::Txt,
            recursive: true,
            manifest_path: None,
            resume_manifest_path: None,
            continue_on_error: true,
            output_dir: None,
            skip_existing: false,
        };

        let report = export(&args)?;

        assert_eq!(report.converted_files().len(), 1);
        assert_eq!(report.skipped_files().len(), 0);
        assert_eq!(report.failed_files().len(), 1);
        assert_eq!(
            report.failed_files()[0].input_path,
            root.join("nested").join("broken.hwpx")
        );
        assert!(
            report.failed_files()[0]
                .error_message
                .contains("rhwp 파싱 실패:")
        );
        assert!(
            report.failed_files()[0]
                .error_message
                .contains("HWPX preview fallback 실패:")
        );
        assert_eq!(fs::read_to_string(root.join("alpha.txt"))?, "first line");
        assert!(!root.join("nested").join("broken.txt").exists());

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    #[test]
    fn stops_directory_export_without_continue_on_error() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("stop-on-error");
        fs::create_dir_all(&root)?;
        fs::write(root.join("broken.hwpx"), "not a valid hwpx file")?;

        let args = CliArgs {
            input_path: root.clone(),
            format: OutputFormat::Txt,
            recursive: true,
            manifest_path: None,
            resume_manifest_path: None,
            continue_on_error: false,
            output_dir: None,
            skip_existing: false,
        };

        let error = export(&args).unwrap_err();

        assert!(error.to_string().contains("rhwp 파싱 실패:"));
        assert!(error.to_string().contains("HWPX preview fallback 실패:"));

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    #[test]
    fn rejects_directory_input_without_recursive_flag() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("dir-validation");
        fs::create_dir_all(&root)?;

        let error = validate_input_path(&root, false).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    #[test]
    fn renders_svg_with_escaped_text() {
        let svg = render_svg_document(Path::new("sample.hwpx"), &[String::from("& < > \" '")]);

        assert!(svg.contains("&amp; &lt; &gt; &quot; &apos;"));
        assert!(svg.contains("<svg"));
        assert!(svg.contains("sample.hwpx text export"));
    }

    #[test]
    fn adds_paragraph_spacing_between_svg_lines() {
        let lines = collect_render_lines(&[
            String::from("first paragraph"),
            String::from("second paragraph"),
        ]);

        assert_eq!(lines.len(), 2);
        assert!(lines[0].add_paragraph_gap);
        assert!(!lines[1].add_paragraph_gap);
    }

    #[test]
    fn serializes_json_output_as_document_ir() {
        let document = Document::from_paragraphs(vec![
            "first paragraph".to_string(),
            "second paragraph".to_string(),
        ]);

        let content = serde_json::to_string_pretty(&document).unwrap();

        assert!(content.contains("\"ir_version\": 6"));
        assert!(content.contains("\"sections\": ["));
        assert!(content.contains("\"resources\": {"));
        assert!(content.contains("\"styles\": {"));
        assert!(content.contains("\"notes\": {"));
        assert!(content.contains("\"type\": \"paragraph\""));
        assert!(content.contains("\"role\": \"body\""));
        assert!(content.contains("first paragraph"));
        assert!(content.contains("second paragraph"));
    }

    #[test]
    fn renders_html_from_document_ir_blocks_and_inlines() {
        let document = document_with_blocks(vec![
            Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![
                    Inline::Text(TextRun {
                        text: "& < > \" '".to_string(),
                        style: TextStyle::default(),
                        style_ref: None,
                    }),
                    Inline::LineBreak,
                    Inline::Text(TextRun {
                        text: "second line".to_string(),
                        style: TextStyle::default(),
                        style_ref: None,
                    }),
                    Inline::Unknown(UnknownInline {
                        kind: "opaque_inline".to_string(),
                        fallback_text: Some(" + extra".to_string()),
                        message: None,
                        source: None,
                    }),
                ],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: None,
            }),
            Block::Unknown(UnknownBlock {
                kind: "opaque_block".to_string(),
                fallback_text: Some("fallback block".to_string()),
                message: None,
                source: None,
            }),
        ]);
        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<title>sample.hwpx text export</title>"));
        assert!(html.contains("<p>&amp; &lt; &gt; &quot; &apos;<br />second line + extra</p>"));
        assert!(html.contains("<p>fallback block</p>"));
    }

    #[test]
    fn renders_markdown_from_document_ir_blocks_and_inlines() {
        let document = document_with_blocks(vec![
            Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![
                    Inline::Text(TextRun {
                        text: "# heading".to_string(),
                        style: TextStyle::default(),
                        style_ref: None,
                    }),
                    Inline::LineBreak,
                    Inline::Text(TextRun {
                        text: "line one\nline two".to_string(),
                        style: TextStyle::default(),
                        style_ref: None,
                    }),
                ],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: None,
            }),
            Block::Unknown(UnknownBlock {
                kind: "opaque_block".to_string(),
                fallback_text: Some("1. ordered".to_string()),
                message: None,
                source: None,
            }),
        ]);
        let markdown = render_markdown_document(&document);

        assert!(markdown.contains("\\# heading"));
        assert!(markdown.contains("line one  \nline two"));
        assert!(markdown.contains("\\1. ordered"));
    }

    #[test]
    fn renders_unknown_fallback_labels_when_text_is_missing() {
        let document = document_with_blocks(vec![
            Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![Inline::Unknown(UnknownInline {
                    kind: "opaque_inline".to_string(),
                    fallback_text: None,
                    message: None,
                    source: None,
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: None,
            }),
            Block::Unknown(UnknownBlock {
                kind: "opaque_block".to_string(),
                fallback_text: None,
                message: None,
                source: None,
            }),
        ]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);
        let markdown = render_markdown_document(&document);
        let text = plain_text::to_plain_text(&document);

        assert!(html.contains("<p>[unknown: opaque_inline]</p>"));
        assert!(html.contains("<p>[unknown: opaque_block]</p>"));
        assert!(markdown.contains("[unknown: opaque_inline]"));
        assert!(markdown.contains("[unknown: opaque_block]"));
        assert!(text.contains("[unknown: opaque_inline]"));
        assert!(text.contains("[unknown: opaque_block]"));
    }

    #[test]
    fn renders_markdown_image_from_document_ir() {
        let document = document_with_image_block("image-1", Some("로고"), Some("png"));

        let asset_prefix = image_asset_public_prefix(Path::new("sample.md"));
        let markdown = render_markdown_document_with_asset_prefix(&document, &asset_prefix);

        assert!(markdown.contains("![로고](sample_assets/images/image-1.png)"));
    }

    #[test]
    fn renders_html_image_from_document_ir() {
        let document = document_with_image_block("image-1", Some("로고"), Some("png"));

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("<img src=\"sample_assets/images/image-1.png\" alt=\"로고\""));
    }

    #[test]
    fn writes_image_assets_to_document_scoped_directory() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("image-assets");
        fs::create_dir_all(&root)?;
        let document = document_with_image_block("image-1", Some("로고"), Some("png"));
        let html_path = root.join("sample.html");
        let markdown_path = root.join("sample.md");

        write_html_output(Path::new("sample.hwpx"), &html_path, &document)?;
        write_markdown_output(&markdown_path, &document)?;

        let asset_path = root
            .join("sample_assets")
            .join("images")
            .join("image-1.png");
        assert_eq!(fs::read(asset_path)?, vec![137, 80, 78, 71]);
        assert!(
            fs::read_to_string(&html_path)?.contains("src=\"sample_assets/images/image-1.png\"")
        );
        assert!(
            fs::read_to_string(&markdown_path)?.contains("](sample_assets/images/image-1.png)")
        );

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    #[test]
    fn sanitizes_document_scoped_asset_directory_names() {
        let output_path = Path::new("out").join("my report!.html");

        assert_eq!(
            image_asset_public_prefix(&output_path),
            "my_report_assets/images"
        );
        assert_eq!(
            image_asset_dir(&output_path),
            Path::new("out").join("my_report_assets").join("images")
        );
    }

    #[test]
    fn renders_html_table_from_document_ir() {
        let document = document_with_blocks(vec![simple_table_block()]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("<table>"));
        assert!(html.contains("<tr>"));
        assert!(html.contains("<td><p>cell1</p>\n</td>"));
        assert!(html.contains("<td><p>cell4</p>\n</td>"));
    }

    #[test]
    fn renders_markdown_table_from_document_ir() {
        let document = document_with_blocks(vec![simple_table_block()]);

        let markdown = render_markdown_document(&document);

        assert!(markdown.contains("| cell1 | cell2 |"));
        assert!(markdown.contains("| --- | --- |"));
        assert!(markdown.contains("| cell3 | cell4 |"));
    }

    #[test]
    fn renders_plain_text_table_fallback_for_txt_exporter() {
        let document = document_with_blocks(vec![simple_table_block()]);

        let text = plain_text::to_plain_text(&document);

        assert_eq!(text, "[표]\ncell1 | cell2\ncell3 | cell4");
    }

    #[test]
    fn renders_html_text_style_decorations() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![Inline::Text(TextRun {
                text: "styled".to_string(),
                style: TextStyle {
                    bold: true,
                    italic: true,
                    underline: true,
                    strike: true,
                    ..Default::default()
                },
                style_ref: None,
            })],
            style: ParagraphStyle::default(),
            style_ref: None,
            list: None,
        })]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("font-weight: bold"));
        assert!(html.contains("font-style: italic"));
        assert!(html.contains("text-decoration: underline line-through"));
    }

    #[test]
    fn renders_html_text_style_visual_properties() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![Inline::Text(TextRun {
                text: "styled".to_string(),
                style: TextStyle {
                    font_family: Some("Noto Sans KR, Malgun Gothic; color:red".to_string()),
                    font_size_pt: Some(LengthPt(12.5)),
                    color: Some(Color {
                        r: 17,
                        g: 34,
                        b: 51,
                        a: 255,
                    }),
                    background_color: Some(Color {
                        r: 68,
                        g: 85,
                        b: 102,
                        a: 255,
                    }),
                    ..Default::default()
                },
                style_ref: None,
            })],
            style: ParagraphStyle::default(),
            style_ref: None,
            list: None,
        })]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("font-family: Noto Sans KR, Malgun Gothic colorred"));
        assert!(html.contains("font-size: 12.5pt"));
        assert!(html.contains("color: #112233"));
        assert!(html.contains("background-color: #445566"));
    }

    #[test]
    fn renders_html_paragraph_alignment_and_spacing() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![Inline::Text(TextRun {
                text: "paragraph".to_string(),
                style: TextStyle::default(),
                style_ref: None,
            })],
            style: ParagraphStyle {
                alignment: Some(Alignment::Center),
                spacing: Spacing {
                    before_pt: Some(LengthPt(6.0)),
                    after_pt: Some(LengthPt(8.0)),
                    line_pt: Some(LengthPt(14.0)),
                },
                indent: Indent {
                    left_pt: Some(LengthPt(10.0)),
                    right_pt: Some(LengthPt(12.0)),
                    first_line_pt: Some(LengthPt(18.0)),
                },
            },
            style_ref: None,
            list: None,
        })]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("text-align: center"));
        assert!(html.contains("margin-top: 6pt"));
        assert!(html.contains("margin-bottom: 8pt"));
        assert!(html.contains("text-indent: 18pt"));
        assert!(html.contains("margin-left: 10pt"));
        assert!(html.contains("margin-right: 12pt"));
    }

    #[test]
    fn renders_markdown_text_style_from_text_style() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![
                Inline::Text(TextRun {
                    text: "bold".to_string(),
                    style: TextStyle {
                        bold: true,
                        ..Default::default()
                    },
                    style_ref: None,
                }),
                Inline::Text(TextRun {
                    text: " ".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                }),
                Inline::Text(TextRun {
                    text: "italic".to_string(),
                    style: TextStyle {
                        italic: true,
                        ..Default::default()
                    },
                    style_ref: None,
                }),
                Inline::Text(TextRun {
                    text: " ".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                }),
                Inline::Text(TextRun {
                    text: "both".to_string(),
                    style: TextStyle {
                        bold: true,
                        italic: true,
                        ..Default::default()
                    },
                    style_ref: None,
                }),
                Inline::Text(TextRun {
                    text: " ".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                }),
                Inline::Text(TextRun {
                    text: "strike".to_string(),
                    style: TextStyle {
                        strike: true,
                        ..Default::default()
                    },
                    style_ref: None,
                }),
            ],
            style: ParagraphStyle::default(),
            style_ref: None,
            list: None,
        })]);

        let markdown = render_markdown_document(&document);

        assert!(markdown.contains("**bold**"));
        assert!(markdown.contains("*italic*"));
        assert!(markdown.contains("***both***"));
        assert!(markdown.contains("~~strike~~"));
    }

    #[test]
    fn renders_html_link_inline_from_document_ir() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![Inline::Link(Link {
                url: "https://example.com?q=1&lang=ko".to_string(),
                title: Some("Example".to_string()),
                inlines: vec![Inline::Text(TextRun {
                    text: "Open".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
            })],
            style: ParagraphStyle::default(),
            style_ref: None,
            list: None,
        })]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains(
            "<a href=\"https://example.com?q=1&amp;lang=ko\" title=\"Example\">Open</a>"
        ));
    }

    #[test]
    fn renders_markdown_link_inline_from_document_ir() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![Inline::Link(Link {
                url: "https://example.com/docs".to_string(),
                title: None,
                inlines: vec![Inline::Text(TextRun {
                    text: "Docs".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
            })],
            style: ParagraphStyle::default(),
            style_ref: None,
            list: None,
        })]);

        let markdown = render_markdown_document(&document);

        assert!(markdown.contains("[Docs](https://example.com/docs)"));
    }

    #[test]
    fn renders_note_refs_in_html_and_markdown() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![
                Inline::Text(TextRun {
                    text: "body".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                }),
                Inline::FootnoteRef {
                    note_id: NoteId("fn-1".to_string()),
                },
                Inline::EndnoteRef {
                    note_id: NoteId("en-1".to_string()),
                },
            ],
            style: ParagraphStyle::default(),
            style_ref: None,
            list: None,
        })]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);
        let markdown = render_markdown_document(&document);

        assert!(html.contains("href=\"#note-fn-1\">[각주: fn-1]</a>"));
        assert!(html.contains("href=\"#note-en-1\">[미주: en-1]</a>"));
        assert!(markdown.contains("[^fn-1]"));
        assert!(markdown.contains("[^en-1]"));
    }

    #[test]
    fn renders_notes_at_end_in_html_and_markdown() {
        let document = document_with_notes(
            vec![Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![Inline::Text(TextRun {
                    text: "body".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: None,
            })],
            vec![Note {
                id: NoteId("fn-1".to_string()),
                kind: NoteKind::Footnote,
                blocks: vec![Block::Paragraph(Paragraph {
                    role: ParagraphRole::Body,
                    inlines: vec![Inline::Text(TextRun {
                        text: "note body".to_string(),
                        style: TextStyle::default(),
                        style_ref: None,
                    })],
                    style: ParagraphStyle::default(),
                    style_ref: None,
                    list: None,
                })],
            }],
        );

        let html = render_html_document(Path::new("sample.hwpx"), &document);
        let markdown = render_markdown_document(&document);

        assert!(html.contains("<section class=\"notes\">"));
        assert!(html.contains("<li id=\"note-fn-1\" data-kind=\"footnote\"><p>note body</p>"));
        assert!(markdown.contains("[^fn-1]: note body"));
    }

    #[test]
    fn renders_markdown_list_prefixes() {
        let document = document_with_blocks(vec![
            Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![Inline::Text(TextRun {
                    text: "first".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: Some(ListInfo {
                    kind: ListKind::Unordered,
                    level: 0,
                    marker: Some("-".to_string()),
                    number: None,
                }),
            }),
            Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![Inline::Text(TextRun {
                    text: "second".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: Some(ListInfo {
                    kind: ListKind::Ordered,
                    level: 0,
                    marker: None,
                    number: Some(2),
                }),
            }),
        ]);

        let markdown = render_markdown_document(&document);

        assert!(markdown.contains("- first"));
        assert!(markdown.contains("2. second"));
    }

    #[test]
    fn renders_headers_and_footers_in_html() {
        let document = Document {
            ir_version: IR_VERSION,
            metadata: Metadata::default(),
            sections: vec![Section {
                headers: vec![HeaderFooter {
                    placement: HeaderFooterPlacement::Default,
                    blocks: vec![Block::Paragraph(Paragraph {
                        role: ParagraphRole::Body,
                        inlines: vec![Inline::Text(TextRun {
                            text: "header".to_string(),
                            style: TextStyle::default(),
                            style_ref: None,
                        })],
                        style: ParagraphStyle::default(),
                        style_ref: None,
                        list: None,
                    })],
                }],
                blocks: vec![Block::Paragraph(Paragraph {
                    role: ParagraphRole::Body,
                    inlines: vec![Inline::Text(TextRun {
                        text: "body".to_string(),
                        style: TextStyle::default(),
                        style_ref: None,
                    })],
                    style: ParagraphStyle::default(),
                    style_ref: None,
                    list: None,
                })],
                footers: vec![HeaderFooter {
                    placement: HeaderFooterPlacement::EvenPage,
                    blocks: vec![Block::Paragraph(Paragraph {
                        role: ParagraphRole::Body,
                        inlines: vec![Inline::Text(TextRun {
                            text: "footer".to_string(),
                            style: TextStyle::default(),
                            style_ref: None,
                        })],
                        style: ParagraphStyle::default(),
                        style_ref: None,
                        list: None,
                    })],
                }],
            }],
            resources: ResourceStore::default(),
            styles: StyleSheet::default(),
            notes: NoteStore::default(),
            warnings: Vec::new(),
        };

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("<header data-placement=\"default\"><p>header</p>"));
        assert!(html.contains("<footer data-placement=\"even_page\"><p>footer</p>"));
    }

    #[test]
    fn renders_markdown_latex_equation_from_document_ir() {
        let document = document_with_blocks(vec![Block::Equation(Equation {
            kind: EquationKind::Latex,
            content: Some(r"\frac{a}{b}".to_string()),
            fallback_text: None,
            resource_id: None,
        })]);

        let markdown = render_markdown_document(&document);

        assert!(markdown.contains("$$\\frac{a}{b}$$"));
    }

    #[test]
    fn renders_html_equation_shape_and_chart_placeholders() {
        let document = document_with_blocks(vec![
            Block::Equation(Equation {
                kind: EquationKind::PlainText,
                content: Some("x + y".to_string()),
                fallback_text: None,
                resource_id: None,
            }),
            Block::Shape(Shape {
                kind: ShapeKind::Rectangle,
                fallback_text: None,
                description: Some("callout box".to_string()),
            }),
            Block::Chart(Chart {
                title: Some("Sales".to_string()),
                fallback_text: None,
                resource_id: None,
            }),
        ]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("<span class=\"equation\">[equation: x + y]</span>"));
        assert!(html.contains("<span class=\"shape-placeholder\">[shape: callout box]</span>"));
        assert!(html.contains("<span class=\"chart-placeholder\">[chart: Sales]</span>"));
    }

    #[test]
    fn writes_manifest_with_success_skip_and_failure_entries() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("manifest");
        fs::create_dir_all(&root)?;
        let manifest_path = root.join("manifest.json");
        let report = ExportReport {
            converted_files: vec![ExportedFile {
                input_path: PathBuf::from("docs/alpha.hwpx"),
                output_path: PathBuf::from("docs/alpha.svg"),
            }],
            skipped_files: vec![SkippedFile {
                input_path: PathBuf::from("docs/existing.hwpx"),
                output_path: PathBuf::from("out/existing.svg"),
            }],
            failed_files: vec![FailedFile {
                input_path: PathBuf::from("docs/nested/beta.hwp"),
                error_message: "parse failed".to_string(),
            }],
        };
        let args = CliArgs {
            input_path: PathBuf::from("docs"),
            format: OutputFormat::Svg,
            recursive: true,
            manifest_path: Some(manifest_path.clone()),
            resume_manifest_path: Some(PathBuf::from("previous-manifest.json")),
            continue_on_error: true,
            output_dir: Some(PathBuf::from("out")),
            skip_existing: true,
        };

        write_manifest(&manifest_path, &args, &report)?;

        let content = fs::read_to_string(&manifest_path)?;
        assert!(content.contains("\"input_path\": \"docs\""));
        assert!(content.contains("\"format\": \"svg\""));
        assert!(content.contains("\"recursive\": true"));
        assert!(content.contains("\"continue_on_error\": true"));
        assert!(content.contains("\"skip_existing\": true"));
        assert!(content.contains("\"output_dir\": \"out\""));
        assert!(content.contains("\"converted_count\": 1"));
        assert!(content.contains("\"skipped_count\": 1"));
        assert!(content.contains("\"failed_count\": 1"));
        assert!(content.contains("\"status\": \"success\""));
        assert!(content.contains("\"status\": \"skipped\""));
        assert!(content.contains("\"status\": \"failed\""));
        assert!(content.contains("\"error\": \"parse failed\""));

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    fn write_preview_hwpx(path: &Path, preview_text: &str) -> Result<(), Box<dyn Error>> {
        let file = File::create(path)?;
        let mut writer = ZipWriter::new(file);

        writer.start_file("Preview/PrvText.txt", SimpleFileOptions::default())?;
        writer.write_all(preview_text.as_bytes())?;
        writer.finish()?;

        Ok(())
    }

    fn document_with_blocks(blocks: Vec<Block>) -> Document {
        Document {
            ir_version: IR_VERSION,
            metadata: Metadata::default(),
            sections: vec![Section {
                blocks,
                ..Default::default()
            }],
            resources: ResourceStore::default(),
            styles: StyleSheet::default(),
            notes: NoteStore::default(),
            warnings: Vec::<ConversionWarning>::new(),
        }
    }

    fn document_with_notes(blocks: Vec<Block>, notes: Vec<Note>) -> Document {
        Document {
            notes: NoteStore { notes },
            ..document_with_blocks(blocks)
        }
    }

    fn document_with_image_block(
        resource_id: &str,
        alt: Option<&str>,
        extension: Option<&str>,
    ) -> Document {
        Document {
            ir_version: IR_VERSION,
            metadata: Metadata::default(),
            sections: vec![Section {
                blocks: vec![Block::Image(Image {
                    resource_id: ResourceId(resource_id.to_string()),
                    alt: alt.map(ToOwned::to_owned),
                    caption: None,
                    width: None,
                    height: None,
                })],
                ..Default::default()
            }],
            resources: ResourceStore {
                entries: vec![Resource::Image(ImageResource {
                    id: ResourceId(resource_id.to_string()),
                    media_type: Some("image/png".to_string()),
                    extension: extension.map(ToOwned::to_owned),
                    bytes: vec![137, 80, 78, 71],
                })],
            },
            styles: StyleSheet::default(),
            notes: NoteStore::default(),
            warnings: Vec::<ConversionWarning>::new(),
        }
    }

    fn simple_table_block() -> Block {
        Block::Table(Table {
            rows: vec![
                TableRow {
                    cells: vec![table_cell("cell1"), table_cell("cell2")],
                },
                TableRow {
                    cells: vec![table_cell("cell3"), table_cell("cell4")],
                },
            ],
            style: TableStyle::default(),
        })
    }

    fn table_cell(text: &str) -> TableCell {
        TableCell {
            row_span: 1,
            col_span: 1,
            blocks: vec![Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![Inline::Text(TextRun {
                    text: text.to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: None,
            })],
            style: TableCellStyle::default(),
        }
    }

    fn temp_fixture_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "hwp-convert-exporter-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn workspace_fixture_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();

        PathBuf::from("target").join(format!(
            "hwp-convert-exporter-{label}-{}-{nanos}",
            std::process::id()
        ))
    }
}
