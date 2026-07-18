use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io::{self, BufWriter, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bridge;
use crate::cli::{CliArgs, OutputFormat};
use crate::ir::{
    Alignment, Block, Border, BorderStyle, Chart, Color, Document, Equation, EquationKind,
    FillStyle, HeaderFooter, HeaderFooterPlacement, Image, ImageFillMode, Inline, Link, ListInfo,
    ListKind, MasterPage, Note, NoteId, NoteKind, Paragraph, ParagraphRole, ParagraphStyle,
    Resource, ResourceId, ResourceStore, Section, Shape, ShapeGeometry, ShapeShadow, Table,
    TableCell, TableCellStyle, TableCellTextDirection, TableRow, TableStyle, TableZone,
    TextDecorationStyle, TextRun, TextShadow, TextStyle, UnknownBlock, UnknownInline,
    VerticalAlign,
};
use crate::util::plain_text;

#[cfg(test)]
const DEFAULT_IMAGE_ASSET_PUBLIC_PREFIX: &str = "document_assets/images";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedFile {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub warnings: Vec<String>,
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

    pub fn warning_count(&self) -> usize {
        self.converted_files
            .iter()
            .map(|file| file.warnings.len())
            .sum()
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
            warning_count: file.warnings.len(),
            warnings: file.warnings.clone(),
        });
    }

    for file in &report.skipped_files {
        files.push(ManifestFileEntry {
            input_path: file.input_path.display().to_string(),
            output_path: Some(file.output_path.display().to_string()),
            status: "skipped",
            error: None,
            warning_count: 0,
            warnings: Vec::new(),
        });
    }

    for file in &report.failed_files {
        files.push(ManifestFileEntry {
            input_path: file.input_path.display().to_string(),
            output_path: None,
            status: "failed",
            error: Some(file.error_message.clone()),
            warning_count: 0,
            warnings: Vec::new(),
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

    if let Some(parent) = manifest_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
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
    let warnings = conversion_warnings_for_document(&document);

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
        warnings,
    }))
}

fn conversion_warnings_for_document(document: &Document) -> Vec<String> {
    let mut warnings = Vec::new();
    for warning in &document.warnings {
        push_warning_once(&mut warnings, warning.message.clone());
    }

    collect_ir_unknown_warnings(document, &mut warnings);
    warnings
}

fn collect_ir_unknown_warnings(document: &Document, warnings: &mut Vec<String>) {
    for section in &document.sections {
        collect_block_unknown_warnings(&section.blocks, warnings);

        for header in &section.headers {
            collect_block_unknown_warnings(&header.blocks, warnings);
        }

        for footer in &section.footers {
            collect_block_unknown_warnings(&footer.blocks, warnings);
        }

        for master_page in &section.master_pages {
            collect_block_unknown_warnings(&master_page.blocks, warnings);
        }
    }

    for note in &document.notes.notes {
        collect_block_unknown_warnings(&note.blocks, warnings);
    }
}

fn collect_block_unknown_warnings(blocks: &[Block], warnings: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                collect_inline_unknown_warnings(&paragraph.inlines, warnings);
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        collect_block_unknown_warnings(&cell.blocks, warnings);
                    }
                }
            }
            Block::Shape(shape) => {
                collect_block_unknown_warnings(&shape.content, warnings);
                collect_block_unknown_warnings(&shape.children, warnings);
            }
            Block::Unknown(unknown) => {
                if let Some(message) = &unknown.message {
                    push_warning_once(
                        warnings,
                        format!("IR unknown block `{}`: {message}", unknown.kind),
                    );
                }
            }
            Block::ColumnLayout(_)
            | Block::DocumentControl(_)
            | Block::Image(_)
            | Block::Equation(_)
            | Block::Chart(_) => {}
        }
    }
}

fn collect_inline_unknown_warnings(inlines: &[Inline], warnings: &mut Vec<String>) {
    for inline in inlines {
        match inline {
            Inline::Link(link) => collect_inline_unknown_warnings(&link.inlines, warnings),
            Inline::Field(_) => {}
            Inline::Unknown(unknown) => {
                if let Some(message) = &unknown.message {
                    push_warning_once(
                        warnings,
                        format!("IR unknown inline `{}`: {message}", unknown.kind),
                    );
                }
            }
            Inline::Text(_)
            | Inline::LineBreak
            | Inline::Tab
            | Inline::Anchor { .. }
            | Inline::FootnoteRef { .. }
            | Inline::EndnoteRef { .. } => {}
        }
    }
}

fn push_warning_once(warnings: &mut Vec<String>, message: String) {
    if !warnings.iter().any(|warning| warning == &message) {
        warnings.push(message);
    }
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
    let file = fs::File::create(output_path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, document).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to serialize JSON output: {error}"),
        )
    })?;
    writer.flush()
}

fn write_html_output(
    input_path: &Path,
    output_path: &Path,
    document: &Document,
) -> Result<(), io::Error> {
    let assets = document_asset_paths(output_path);
    write_resource_assets(&assets, &document.resources)?;
    let html =
        render_html_document_with_asset_prefix(input_path, document, &assets.image_public_prefix);
    fs::write(output_path, html)
}

fn write_markdown_output(output_path: &Path, document: &Document) -> Result<(), io::Error> {
    let assets = document_asset_paths(output_path);
    write_resource_assets(&assets, &document.resources)?;
    let markdown =
        render_markdown_document_with_asset_prefix(document, &assets.image_public_prefix);
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

#[cfg(test)]
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
      .title, .heading {{\n\
        color: #111827;\n\
        line-height: 1.35;\n\
      }}\n\
      .caption {{\n\
        color: #4b5563;\n\
        font-size: 14px;\n\
      }}\n\
      table {{\n\
        width: 100%;\n\
        border-collapse: collapse;\n\
        margin: 0 0 1em;\n\
      }}\n\
      td, th {{\n\
        border: 1px solid #e5e7eb;\n\
        padding: 12px 14px;\n\
        vertical-align: top;\n\
      }}\n\
      th {{\n\
        background: #f9fafb;\n\
        text-align: left;\n\
      }}\n\
      p {{\n\
        margin: 0 0 1em;\n\
        line-height: 1.8;\n\
        white-space: normal;\n\
      }}\n\
      p:last-child {{\n\
        margin-bottom: 0;\n\
      }}\n\
      .tab {{\n\
        white-space: pre;\n\
        tab-size: 4;\n\
      }}\n\
      li[data-marker]::marker {{\n\
        content: attr(data-marker) \" \";\n\
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

    for master_page in &section.master_pages {
        nodes.push_str(&render_html_master_page(
            master_page,
            resources,
            image_asset_prefix,
        ));
    }
    for header in &section.headers {
        nodes.push_str(&render_html_header_footer(
            "header",
            header,
            resources,
            image_asset_prefix,
        ));
    }
    nodes.push_str(&render_html_blocks(
        &section.blocks,
        resources,
        image_asset_prefix,
    ));
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

fn render_html_master_page(
    master_page: &MasterPage,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let content = render_html_blocks(&master_page.blocks, resources, image_asset_prefix);
    let placement = escape_html(header_footer_placement_name(&master_page.placement));

    format!(
        "<section class=\"master-page\" data-placement=\"{placement}\" data-extension=\"{}\" data-overlap=\"{}\" data-extension-flags=\"{}\" data-text-width-px=\"{}\" data-text-height-px=\"{}\" data-text-reference-mask=\"{}\" data-number-reference-mask=\"{}\">{content}</section>\n",
        master_page.is_extension,
        master_page.overlap,
        master_page.raw_extension_flags,
        master_page.text_width.0,
        master_page.text_height.0,
        master_page.text_reference_mask,
        master_page.number_reference_mask,
    )
}

fn render_html_header_footer(
    tag_name: &str,
    header_footer: &HeaderFooter,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let content = render_html_blocks(&header_footer.blocks, resources, image_asset_prefix);
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
    let content = render_html_blocks(&note.blocks, resources, image_asset_prefix);

    format!("<li id=\"{id}\" data-kind=\"{kind}\">{content}</li>\n")
}

fn render_html_blocks(
    blocks: &[Block],
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let mut html = String::new();
    let mut index = 0;

    while index < blocks.len() {
        if let Block::Paragraph(paragraph) = &blocks[index]
            && matches!(paragraph.role, ParagraphRole::Body)
            && let Some(list) = &paragraph.list
        {
            let (list_html, next_index) =
                render_html_list(blocks, index, list, resources, image_asset_prefix);
            html.push_str(&list_html);
            index = next_index;
            continue;
        }

        html.push_str(&render_html_block(
            &blocks[index],
            resources,
            image_asset_prefix,
        ));
        index += 1;
    }

    html
}

fn render_html_list(
    blocks: &[Block],
    start_index: usize,
    first_list: &ListInfo,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> (String, usize) {
    render_html_list_level(
        blocks,
        start_index,
        html_list_tag_name(&first_list.kind),
        first_list.level,
        resources,
        image_asset_prefix,
    )
}

fn render_html_list_level(
    blocks: &[Block],
    start_index: usize,
    tag_name: &str,
    level: u8,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> (String, usize) {
    let mut html = format!("<{tag_name}>\n");
    let mut index = start_index;

    while let Some(Block::Paragraph(paragraph)) = blocks.get(index) {
        if !matches!(paragraph.role, ParagraphRole::Body) {
            break;
        }
        let Some(list) = &paragraph.list else {
            break;
        };
        let current_tag_name = html_list_tag_name(&list.kind);
        if list.level < level || (list.level == level && current_tag_name != tag_name) {
            break;
        }
        if list.level > level {
            let (nested_html, next_index) = render_html_list_level(
                blocks,
                index,
                current_tag_name,
                list.level,
                resources,
                image_asset_prefix,
            );
            html.push_str(&nested_html);
            index = next_index;
            continue;
        }

        html.push_str(&render_html_list_item_open(
            paragraph,
            list,
            resources,
            image_asset_prefix,
        ));
        index += 1;

        while let Some(Block::Paragraph(next_paragraph)) = blocks.get(index) {
            if !matches!(next_paragraph.role, ParagraphRole::Body) {
                break;
            }
            let Some(next_list) = &next_paragraph.list else {
                break;
            };
            if next_list.level <= level {
                break;
            }

            let (nested_html, next_index) = render_html_list_level(
                blocks,
                index,
                html_list_tag_name(&next_list.kind),
                next_list.level,
                resources,
                image_asset_prefix,
            );
            html.push_str(&nested_html);
            index = next_index;
        }

        html.push_str("</li>\n");
    }

    html.push_str(&format!("</{tag_name}>\n"));

    (html, index)
}

fn render_html_list_item_open(
    paragraph: &Paragraph,
    list: &ListInfo,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let value = if list.kind == ListKind::Ordered {
        list.number
            .map(|number| format!(" value=\"{number}\""))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let marker_format = if list.kind == ListKind::Ordered {
        list.marker_format
            .as_deref()
            .filter(|format| !format.is_empty())
            .map(|format| format!(" data-marker-format=\"{}\"", escape_html(format)))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let marker = list
        .marker
        .as_deref()
        .filter(|marker| !marker.is_empty())
        .map(|marker| format!(" data-marker=\"{}\"", escape_html(marker)))
        .unwrap_or_default();
    let marker_metadata = render_html_list_marker_metadata(list);
    let style = render_html_style_attr(&render_html_paragraph_style(&paragraph.style));
    let content = render_html_inlines(&paragraph.inlines, resources, image_asset_prefix);

    format!("<li{value}{marker_format}{marker}{marker_metadata}{style}>{content}")
}

fn render_html_list_marker_metadata(list: &ListInfo) -> String {
    let mut attributes = Vec::new();
    if let Some(definition_id) = list.source_definition_id {
        attributes.push(format!("data-source-definition-id=\"{definition_id}\""));
    }
    if let Some(layout) = &list.marker_layout {
        attributes.push(format!(
            "data-marker-attributes=\"{}\"",
            layout.raw_attributes
        ));
        attributes.push(format!(
            "data-marker-width-adjust=\"{}\"",
            layout.raw_width_adjust
        ));
        attributes.push(format!(
            "data-marker-text-distance=\"{}\"",
            layout.raw_text_distance
        ));
        if let Some(char_shape_id) = layout.source_char_shape_id {
            attributes.push(format!("data-marker-char-shape-id=\"{char_shape_id}\""));
        }
        if let Some(image_bullet_id) = layout.image_bullet_id {
            attributes.push(format!("data-image-bullet-id=\"{image_bullet_id}\""));
        }
        if layout.image_data != [0; 4] {
            attributes.push(format!(
                "data-image-bullet-metadata=\"{},{},{},{}\"",
                layout.image_data[0],
                layout.image_data[1],
                layout.image_data[2],
                layout.image_data[3]
            ));
        }
        if let Some(check_marker) = layout.check_marker.as_deref() {
            attributes.push(format!(
                "data-check-marker=\"{}\"",
                escape_html(check_marker)
            ));
        }
    }

    if attributes.is_empty() {
        String::new()
    } else {
        format!(" {}", attributes.join(" "))
    }
}

fn html_list_tag_name(kind: &ListKind) -> &'static str {
    match kind {
        ListKind::Ordered => "ol",
        ListKind::Unordered | ListKind::Unknown => "ul",
    }
}

fn render_html_block(block: &Block, resources: &ResourceStore, image_asset_prefix: &str) -> String {
    match block {
        Block::Paragraph(paragraph) => {
            render_html_paragraph(paragraph, resources, image_asset_prefix)
        }
        Block::ColumnLayout(_) => String::new(),
        Block::DocumentControl(control) => format!(
            "<p>{}</p>\n",
            render_html_fallback_text(control.fallback_text())
        ),
        Block::Table(table) => render_html_table(table, resources, image_asset_prefix),
        Block::Image(image) => render_html_image(image, resources, image_asset_prefix),
        Block::Equation(equation) => render_html_equation(equation),
        Block::Shape(shape) => render_html_shape(shape, resources, image_asset_prefix),
        Block::Chart(chart) => render_html_chart(chart),
        Block::Unknown(unknown) => {
            let content = render_html_fallback_text(&unknown_block_display_text(unknown));
            format!("<p>{content}</p>\n")
        }
    }
}

fn render_html_paragraph(
    paragraph: &Paragraph,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let mut content = String::new();
    if let Some(list) = &paragraph.list {
        content.push_str(&render_html_fallback_text(&list_prefix(list)));
    }
    content.push_str(&render_html_inlines(
        &paragraph.inlines,
        resources,
        image_asset_prefix,
    ));
    let style = render_html_style_attr(&render_html_paragraph_style(&paragraph.style));
    let class = render_html_paragraph_role_class(&paragraph.role);

    match &paragraph.role {
        ParagraphRole::Title => format!("<h1{class}{style}>{content}</h1>\n"),
        ParagraphRole::Heading { level } => {
            let level = (*level).clamp(1, 6);
            format!("<h{level}{class}{style}>{content}</h{level}>\n")
        }
        _ => format!("<p{class}{style}>{content}</p>\n"),
    }
}

fn render_html_paragraph_role_class(role: &ParagraphRole) -> String {
    paragraph_role_html_class(role)
        .map(|class| format!(" class=\"{class}\""))
        .unwrap_or_default()
}

fn paragraph_role_html_class(role: &ParagraphRole) -> Option<&'static str> {
    match role {
        ParagraphRole::Caption => Some("caption"),
        ParagraphRole::Title => Some("title"),
        ParagraphRole::Heading { .. } => Some("heading"),
        ParagraphRole::Unknown => Some("unknown-paragraph"),
        ParagraphRole::Body => None,
    }
}

fn render_html_inlines(
    inlines: &[Inline],
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let mut content = String::new();

    for inline in inlines {
        match inline {
            Inline::Text(run) => {
                content.push_str(&render_html_text_run(run, resources, image_asset_prefix))
            }
            Inline::LineBreak => content.push_str("<br />"),
            Inline::Tab => content.push_str("<span class=\"tab\">\t</span>"),
            Inline::Link(link) => {
                content.push_str(&render_html_link(link, resources, image_asset_prefix))
            }
            Inline::Field(field) => {
                content.push_str(&render_html_fallback_text(&field.fallback_text));
            }
            Inline::FootnoteRef { note_id } => {
                content.push_str(&render_html_note_ref(note_id, NoteKind::Footnote));
            }
            Inline::EndnoteRef { note_id } => {
                content.push_str(&render_html_note_ref(note_id, NoteKind::Endnote));
            }
            Inline::Anchor { id } => {
                let id = crate::util::plain_text::sanitize_anchor_id(id);
                content.push_str(&format!("<a id=\"{}\"></a>", escape_html(&id)));
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

fn render_html_text_run(
    run: &TextRun,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let content = render_html_fallback_text(&run.text);
    let style = render_html_style_attr(&render_html_text_style(
        &run.style,
        resources,
        image_asset_prefix,
    ));
    let shadow_metadata = render_html_text_shadow_metadata(&run.style);
    let emphasis_metadata = run
        .style
        .emphasis_mark_type
        .map(|kind| format!(" data-emphasis-mark-type=\"{kind}\""))
        .unwrap_or_default();
    let border_fill_metadata = run
        .style
        .border_fill
        .as_ref()
        .map(|border_fill| {
            render_html_border_fill_metadata(
                Some(border_fill.source_border_fill_id),
                border_fill.diagonal.as_ref(),
            )
        })
        .unwrap_or_default();

    if style.is_empty()
        && shadow_metadata.is_empty()
        && emphasis_metadata.is_empty()
        && border_fill_metadata.is_empty()
    {
        content
    } else {
        format!(
            "<span{shadow_metadata}{emphasis_metadata}{border_fill_metadata}{style}>{content}</span>"
        )
    }
}

fn render_html_text_shadow_metadata(style: &TextStyle) -> String {
    let Some(shadow) = style.shadow_details else {
        return String::new();
    };

    format!(
        " data-shadow-kind=\"{}\" data-shadow-offset-x-percent=\"{}\" data-shadow-offset-y-percent=\"{}\" data-shadow-color-raw=\"{}\"",
        shadow.kind, shadow.offset_x_percent, shadow.offset_y_percent, shadow.raw_color
    )
}

fn render_html_border_fill_metadata(
    source_id: Option<u16>,
    diagonal: Option<&crate::ir::BorderFillDiagonal>,
) -> String {
    let mut attributes = Vec::new();
    if let Some(source_id) = source_id {
        attributes.push(format!("data-border-fill-id=\"{source_id}\""));
    }
    if let Some(diagonal) = diagonal {
        attributes.push(format!(
            "data-diagonal-attributes=\"{}\"",
            diagonal.raw_attributes
        ));
        attributes.push(format!("data-diagonal-type=\"{}\"", diagonal.diagonal_type));
        attributes.push(format!(
            "data-diagonal-width-index=\"{}\"",
            diagonal.width_index
        ));
        attributes.push(format!(
            "data-diagonal-color-raw=\"{}\"",
            diagonal.raw_color
        ));
    }

    if attributes.is_empty() {
        String::new()
    } else {
        format!(" {}", attributes.join(" "))
    }
}

fn render_html_link(link: &Link, resources: &ResourceStore, image_asset_prefix: &str) -> String {
    let href = escape_html(&link.url);
    let title = link
        .title
        .as_deref()
        .map(escape_html)
        .map(|title| format!(" title=\"{title}\""))
        .unwrap_or_default();
    let content = render_html_link_label(link, resources, image_asset_prefix);

    format!("<a href=\"{href}\"{title}>{content}</a>")
}

fn render_html_link_label(
    link: &Link,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let content = render_html_inlines(&link.inlines, resources, image_asset_prefix);
    if content.is_empty() {
        escape_html(link.title.as_deref().unwrap_or(&link.url))
    } else {
        content
    }
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
    let mut declarations = Vec::new();
    if let Some(font_size) = equation.font_size_pt {
        declarations.push(format!("font-size: {}pt", font_size.0));
    }
    if let Some(color) = equation.color {
        declarations.push(format!("color: {}", render_css_color(color)));
    }
    if let Some(baseline) = equation.baseline_pt {
        declarations.push(format!("vertical-align: {}pt", baseline.0));
    }
    if let Some(font_family) = &equation.font_family
        && let Some(font_family) = sanitize_css_font_family(font_family)
    {
        declarations.push(format!("font-family: {font_family}"));
    }
    if let Some(width) = equation.width {
        declarations.push(format!("width: {}px", width.0));
    }
    if let Some(height) = equation.height {
        declarations.push(format!("height: {}px", height.0));
    }
    let style = render_html_style_attr(&declarations.join("; "));
    format!("<p><span class=\"equation\"{style}>{content}</span></p>\n")
}

fn render_html_shape(shape: &Shape, resources: &ResourceStore, image_asset_prefix: &str) -> String {
    // Shapes carry box geometry, so an inline span would ignore their width and
    // height. Keep the semantic placeholder while making its box CSS effective.
    let mut declarations = vec!["display: inline-block".to_string()];
    if let Some(fill) = &shape.fill {
        declarations.extend(render_html_fill_style(fill, resources, image_asset_prefix));
    } else if let Some(background_color) = shape.background_color {
        declarations.push(format!(
            "background-color: {}",
            render_css_color(background_color)
        ));
    }
    if let Some(border) = &shape.border {
        declarations.push(format!("border: {}", render_css_border(border)));
    }
    if let Some(shadow) = &shape.shadow
        && let Some(shadow) = render_css_shape_shadow(shadow)
    {
        declarations.push(format!("box-shadow: {shadow}"));
    }
    match &shape.geometry {
        Some(ShapeGeometry::Rectangle {
            round_rate_percent, ..
        }) if *round_rate_percent > 0 => {
            declarations.push(format!(
                "border-radius: {}%",
                (*round_rate_percent).min(100)
            ));
        }
        Some(ShapeGeometry::Ellipse { .. }) => {
            declarations.push("border-radius: 50%".to_string());
        }
        _ => {}
    }
    if let Some(transform) = render_css_transform(
        shape.rotation_degrees,
        shape.flip_horizontal,
        shape.flip_vertical,
    ) {
        declarations.push(format!("transform: {transform}"));
    }
    for (padding, property) in [
        (shape.padding_top, "padding-top"),
        (shape.padding_right, "padding-right"),
        (shape.padding_bottom, "padding-bottom"),
        (shape.padding_left, "padding-left"),
    ] {
        if let Some(padding) = padding {
            declarations.push(format!("{property}: {}px", padding.0));
        }
    }
    if let Some(vertical_align) = shape.text_vertical_align {
        declarations.push("display: inline-flex".to_string());
        declarations.push("flex-direction: column".to_string());
        let justify_content = match vertical_align {
            VerticalAlign::Top => "flex-start",
            VerticalAlign::Middle => "center",
            VerticalAlign::Bottom => "flex-end",
        };
        declarations.push(format!("justify-content: {justify_content}"));
    }
    if let Some(width) = shape.width {
        declarations.push(format!("width: {}px", width.0));
    }
    if let Some(height) = shape.height {
        declarations.push(format!("height: {}px", height.0));
    }
    if let Some(offset_x) = shape.offset_x {
        declarations.push(format!("margin-left: {}px", offset_x.0));
    }
    if let Some(offset_y) = shape.offset_y {
        declarations.push(format!("margin-top: {}px", offset_y.0));
    }
    let style = render_html_style_attr(&declarations.join("; "));
    if !shape.children.is_empty() {
        let content = render_html_blocks(&shape.children, resources, image_asset_prefix);
        format!("<div class=\"shape-group\"{style}>\n{content}</div>\n")
    } else if !shape.content.is_empty() {
        let content = render_html_blocks(&shape.content, resources, image_asset_prefix);
        format!("<div class=\"shape-placeholder shape-content\"{style}>\n{content}</div>\n")
    } else {
        let content = render_html_fallback_text(&shape_display_text(shape));
        format!("<p><span class=\"shape-placeholder\"{style}>{content}</span></p>\n")
    }
}

fn render_html_chart(chart: &Chart) -> String {
    let content = render_html_fallback_text(&chart_display_text(chart));
    format!("<p><span class=\"chart-placeholder\">{content}</span></p>\n")
}

fn render_html_text_style(
    style: &TextStyle,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let mut declarations = Vec::new();

    if style.bold {
        declarations.push("font-weight: bold".to_string());
    }
    if style.italic {
        declarations.push("font-style: italic".to_string());
    }

    let mut decorations = Vec::new();
    if style.underline {
        decorations.push(if style.underline_above {
            "overline"
        } else {
            "underline"
        });
    }
    if style.strike {
        decorations.push("line-through");
    }
    if !decorations.is_empty() {
        declarations.push(format!("text-decoration: {}", decorations.join(" ")));
        // CSS text-decoration-color is a single value, so prefer the underline
        // color when underlined, otherwise fall back to the strike color.
        let decoration_color = style
            .underline
            .then_some(style.underline_color)
            .flatten()
            .or_else(|| style.strike.then_some(style.strike_color).flatten());
        if let Some(color) = decoration_color {
            declarations.push(format!(
                "text-decoration-color: {}",
                render_css_color(color)
            ));
        }
        let decoration_style = if style.underline {
            style.underline_style.as_ref()
        } else {
            style.strike_style.as_ref()
        };
        if let Some(decoration_style) = decoration_style {
            declarations.push(format!(
                "text-decoration-style: {}",
                text_decoration_style_to_css(decoration_style)
            ));
        }
    }

    if style.superscript {
        declarations.push("vertical-align: super".to_string());
        declarations.push("font-size: smaller".to_string());
    } else if style.subscript {
        declarations.push("vertical-align: sub".to_string());
        declarations.push("font-size: smaller".to_string());
    }
    if let Some(marker) = style.emphasis_mark_type.and_then(text_emphasis_marker) {
        declarations.push(format!("text-emphasis: \"{marker}\""));
    } else if style.emphasis_dot {
        declarations.push("text-emphasis: dot".to_string());
    }
    if style.outline {
        declarations.push("-webkit-text-stroke: 1px currentColor".to_string());
    }
    // Emboss and engrave remain generic approximations. A structured HWP text
    // shadow is applied only when those mutually competing effects are absent.
    if style.emboss {
        declarations.push(
            "text-shadow: -1px -1px 0 rgba(255,255,255,0.7), 1px 1px 1px rgba(0,0,0,0.4)"
                .to_string(),
        );
    } else if style.engrave {
        declarations.push(
            "text-shadow: 1px 1px 0 rgba(255,255,255,0.7), -1px -1px 1px rgba(0,0,0,0.4)"
                .to_string(),
        );
    } else if let Some(shadow) = style
        .shadow_details
        .as_ref()
        .and_then(render_css_text_shadow)
    {
        declarations.push(format!("text-shadow: {shadow}"));
    } else if style.shadow && style.shadow_details.is_none() {
        declarations.push("text-shadow: 1px 1px 2px rgba(0,0,0,0.5)".to_string());
    }

    let mut font_families = Vec::new();
    let font_fallback = style.font_fallback.as_deref();
    for family in [
        style.font_family.as_deref(),
        font_fallback.and_then(|font| font.alternate_family.as_deref()),
        font_fallback.and_then(|font| font.default_family.as_deref()),
    ]
    .into_iter()
    .flatten()
    .flat_map(|families| families.split(','))
    .filter_map(sanitize_css_font_family)
    {
        if !font_families.contains(&family) {
            font_families.push(family);
        }
    }
    if !font_families.is_empty() {
        declarations.push(format!("font-family: {}", font_families.join(", ")));
    }
    if let Some(font_size_pt) = style.font_size_pt {
        declarations.push(format!("font-size: {}pt", font_size_pt.0));
    }
    if let Some(font_width_percent) = style.font_width_percent
        && font_width_percent.0.is_finite()
        && (1.0..=1000.0).contains(&font_width_percent.0)
    {
        declarations.push(format!("font-stretch: {}%", font_width_percent.0));
    }
    if let Some(letter_spacing_percent) = style.letter_spacing_percent
        && letter_spacing_percent.0.is_finite()
        && (-100.0..=1000.0).contains(&letter_spacing_percent.0)
    {
        declarations.push(format!(
            "letter-spacing: {}em",
            letter_spacing_percent.0 / 100.0
        ));
    }
    if !style.superscript
        && !style.subscript
        && let Some(vertical_offset_percent) = style.vertical_offset_percent
        && vertical_offset_percent.0.is_finite()
        && (-1000.0..=1000.0).contains(&vertical_offset_percent.0)
    {
        declarations.push(format!(
            "vertical-align: {}em",
            vertical_offset_percent.0 / 100.0
        ));
    }
    if style.kerning {
        declarations.push("font-kerning: normal".to_string());
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
    if let Some(border_fill) = &style.border_fill {
        if let Some(fill) = &border_fill.fill {
            declarations.extend(render_html_fill_style(fill, resources, image_asset_prefix));
        }
        for (border, property) in [
            (&border_fill.border_top, "border-top"),
            (&border_fill.border_right, "border-right"),
            (&border_fill.border_bottom, "border-bottom"),
            (&border_fill.border_left, "border-left"),
        ] {
            if let Some(border) = border {
                declarations.push(format!("{property}: {}", render_css_border(border)));
            }
        }
    }

    declarations.join("; ")
}

fn render_css_text_shadow(shadow: &TextShadow) -> Option<String> {
    let color = shadow.color?;
    Some(format!(
        "{}em {}em 0 {}",
        f32::from(shadow.offset_x_percent) / 100.0,
        f32::from(shadow.offset_y_percent) / 100.0,
        render_css_color(color)
    ))
}

fn text_emphasis_marker(kind: u8) -> Option<&'static str> {
    match kind {
        1 => Some("●"),
        2 => Some("○"),
        3 => Some("ˇ"),
        4 => Some("˜"),
        5 => Some("･"),
        6 => Some(":"),
        _ => None,
    }
}

fn text_decoration_style_to_css(style: &TextDecorationStyle) -> &'static str {
    match style {
        TextDecorationStyle::Solid => "solid",
        TextDecorationStyle::Dashed => "dashed",
        TextDecorationStyle::Dotted => "dotted",
        TextDecorationStyle::Double => "double",
        TextDecorationStyle::Wavy => "wavy",
    }
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
    if let Some(line_pt) = style.spacing.line_pt {
        declarations.push(format!("line-height: {}pt", line_pt.0));
    } else if let Some(line_percent) = style.spacing.line_percent {
        declarations.push(format!("line-height: {}%", line_percent.0));
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
    if let Some(background_color) = style.background_color {
        declarations.push(format!(
            "background-color: {}",
            render_css_color(background_color)
        ));
    }
    for (value, property) in [
        (style.padding_top_pt, "padding-top"),
        (style.padding_right_pt, "padding-right"),
        (style.padding_bottom_pt, "padding-bottom"),
        (style.padding_left_pt, "padding-left"),
    ] {
        if let Some(value) = value {
            declarations.push(format!("{property}: {}pt", value.0));
        }
    }
    for (border, property) in [
        (&style.border_top, "border-top"),
        (&style.border_right, "border-right"),
        (&style.border_bottom, "border-bottom"),
        (&style.border_left, "border-left"),
    ] {
        if let Some(border) = border {
            declarations.push(format!("{property}: {}", render_css_border(border)));
        }
    }
    if style.widow_orphan {
        declarations.push("orphans: 2".to_string());
        declarations.push("widows: 2".to_string());
    }
    if style.keep_with_next {
        declarations.push("break-after: avoid-page".to_string());
    }
    if style.keep_lines {
        declarations.push("break-inside: avoid".to_string());
    }
    if style.page_break_before {
        declarations.push("break-before: page".to_string());
    }

    declarations.join("; ")
}

fn render_html_table_style(
    style: &TableStyle,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let mut declarations = Vec::new();
    if let Some(fill) = &style.fill {
        declarations.extend(render_html_fill_style(fill, resources, image_asset_prefix));
    } else if let Some(color) = style.background_color.map(render_css_color) {
        declarations.push(format!("background-color: {color}"));
    }
    for (border, property) in [
        (&style.border_top, "border-top"),
        (&style.border_right, "border-right"),
        (&style.border_bottom, "border-bottom"),
        (&style.border_left, "border-left"),
    ] {
        if let Some(border) = border {
            declarations.push(format!("{property}: {}", render_css_border(border)));
        }
    }
    if let Some(width) = style.width {
        declarations.push(format!("width: {}px", width.0));
    }
    if let Some(height) = style.height {
        declarations.push(format!("height: {}px", height.0));
    }
    if let Some(cell_spacing) = style.cell_spacing {
        declarations.push("border-collapse: separate".to_string());
        declarations.push(format!("border-spacing: {}px", cell_spacing.0));
    }
    for (property, value) in [
        ("margin-top", style.margin_top),
        ("margin-right", style.margin_right),
        ("margin-bottom", style.margin_bottom),
        ("margin-left", style.margin_left),
    ] {
        if let Some(value) = value {
            declarations.push(format!("{property}: {}px", value.0));
        }
    }
    if matches!(style.page_break, Some(crate::ir::TablePageBreak::Row))
        || style
            .placement
            .is_some_and(|placement| placement.prevent_page_break)
    {
        declarations.push("break-inside: avoid".to_string());
    }
    declarations.join("; ")
}

fn render_html_table_cell_style(
    style: &TableCellStyle,
    fallback_fill: Option<&FillStyle>,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let mut declarations = Vec::new();

    if let Some(fill) = &style.fill {
        declarations.extend(render_html_fill_style(fill, resources, image_asset_prefix));
    } else if let Some(fill) = fallback_fill {
        declarations.extend(render_html_fill_style(fill, resources, image_asset_prefix));
    } else if let Some(background_color) = style.background_color {
        declarations.push(format!(
            "background-color: {}",
            render_css_color(background_color)
        ));
    }
    if let Some(vertical_align) = &style.vertical_align {
        declarations.push(format!(
            "vertical-align: {}",
            vertical_align_to_css(vertical_align)
        ));
    }
    if let Some(text_direction) = style.text_direction {
        match text_direction {
            TableCellTextDirection::Horizontal => {
                declarations.push("writing-mode: horizontal-tb".to_string());
            }
            TableCellTextDirection::VerticalLatinRotated => {
                declarations.push("writing-mode: vertical-rl".to_string());
                declarations.push("text-orientation: mixed".to_string());
            }
            TableCellTextDirection::VerticalLatinUpright => {
                declarations.push("writing-mode: vertical-rl".to_string());
                declarations.push("text-orientation: upright".to_string());
            }
            TableCellTextDirection::Unknown(_) => {}
        }
    }
    if let Some(width) = style.width {
        declarations.push(format!("width: {}px", width.0));
    }
    if let Some(height) = style.height {
        declarations.push(format!("height: {}px", height.0));
    }
    for (value, property) in [
        (style.padding_top, "padding-top"),
        (style.padding_right, "padding-right"),
        (style.padding_bottom, "padding-bottom"),
        (style.padding_left, "padding-left"),
    ] {
        if let Some(value) = value {
            declarations.push(format!("{property}: {}px", value.0));
        }
    }
    for (border, property) in [
        (&style.border_top, "border-top"),
        (&style.border_right, "border-right"),
        (&style.border_bottom, "border-bottom"),
        (&style.border_left, "border-left"),
    ] {
        if let Some(border) = border {
            declarations.push(format!("{property}: {}", render_css_border(border)));
        }
    }

    declarations.join("; ")
}

fn render_html_fill_style(
    fill: &FillStyle,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> Vec<String> {
    match fill {
        FillStyle::Solid {
            background_color,
            alpha,
            ..
        } => background_color
            .map(|color| {
                format!(
                    "background-color: {}",
                    render_css_color(apply_fill_alpha(color, *alpha))
                )
            })
            .into_iter()
            .collect(),
        FillStyle::Gradient {
            gradient_type,
            angle,
            center_x,
            center_y,
            colors,
            positions,
            alpha,
            ..
        } => {
            let color_stops = colors
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    let color = apply_fill_alpha(entry.color?, *alpha);
                    let position = positions
                        .get(index)
                        .filter(|position| (0..=100).contains(*position))
                        .map(|position| format!(" {position}%"))
                        .unwrap_or_default();
                    Some(format!("{}{position}", render_css_color(color)))
                })
                .collect::<Vec<_>>();
            if color_stops.len() < 2 {
                return Vec::new();
            }

            let stops = color_stops.join(", ");
            let background = match gradient_type {
                2 | 4 => format!(
                    "radial-gradient(at {}% {}%, {stops})",
                    center_x.clamp(&0, &100),
                    center_y.clamp(&0, &100)
                ),
                3 => format!(
                    "conic-gradient(from {angle}deg at {}% {}%, {stops})",
                    center_x.clamp(&0, &100),
                    center_y.clamp(&0, &100)
                ),
                _ => format!("linear-gradient({angle}deg, {stops})"),
            };
            vec![format!("background-image: {background}")]
        }
        FillStyle::Image {
            mode, resource_id, ..
        } => {
            let Some(resource_id) = resource_id else {
                return Vec::new();
            };
            let path = resource_public_path(resources, resource_id, image_asset_prefix);
            let mut declarations = vec![format!(
                "background-image: url('{}')",
                escape_css_url(&path)
            )];
            let (repeat, position, size) = image_fill_css(*mode);
            declarations.push(format!("background-repeat: {repeat}"));
            declarations.push(format!("background-position: {position}"));
            if let Some(size) = size {
                declarations.push(format!("background-size: {size}"));
            }
            declarations
        }
    }
}

fn apply_fill_alpha(mut color: Color, alpha: u8) -> Color {
    let alpha = if alpha == 0 { 255 } else { alpha };
    color.a = ((u16::from(color.a) * u16::from(alpha)) / 255) as u8;
    color
}

fn image_fill_css(mode: ImageFillMode) -> (&'static str, &'static str, Option<&'static str>) {
    match mode {
        ImageFillMode::TileAll => ("repeat", "left top", None),
        ImageFillMode::TileHorizontalTop => ("repeat-x", "left top", None),
        ImageFillMode::TileHorizontalBottom => ("repeat-x", "left bottom", None),
        ImageFillMode::TileVerticalLeft => ("repeat-y", "left top", None),
        ImageFillMode::TileVerticalRight => ("repeat-y", "right top", None),
        ImageFillMode::FitToSize => ("no-repeat", "center", Some("100% 100%")),
        ImageFillMode::Center => ("no-repeat", "center", None),
        ImageFillMode::CenterTop => ("no-repeat", "center top", None),
        ImageFillMode::CenterBottom => ("no-repeat", "center bottom", None),
        ImageFillMode::LeftCenter => ("no-repeat", "left center", None),
        ImageFillMode::LeftTop => ("no-repeat", "left top", None),
        ImageFillMode::LeftBottom => ("no-repeat", "left bottom", None),
        ImageFillMode::RightCenter => ("no-repeat", "right center", None),
        ImageFillMode::RightTop => ("no-repeat", "right top", None),
        ImageFillMode::RightBottom => ("no-repeat", "right bottom", None),
        ImageFillMode::None => ("no-repeat", "left top", None),
    }
}

fn escape_css_url(path: &str) -> String {
    path.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace(['\r', '\n'], "")
}

fn render_css_border(border: &Border) -> String {
    let style = match border.style {
        BorderStyle::Solid => "solid",
        BorderStyle::Dashed => "dashed",
        BorderStyle::Dotted => "dotted",
        BorderStyle::Double => "double",
    };
    match border.color {
        Some(color) => format!("{}px {} {}", border.width.0, style, render_css_color(color)),
        None => format!("{}px {}", border.width.0, style),
    }
}

fn vertical_align_to_css(vertical_align: &VerticalAlign) -> &'static str {
    match vertical_align {
        VerticalAlign::Top => "top",
        VerticalAlign::Middle => "middle",
        VerticalAlign::Bottom => "bottom",
    }
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
        ListKind::Ordered => list
            .marker
            .as_deref()
            .map(marker_with_trailing_space)
            .unwrap_or_else(|| format!("{}. ", list.number.unwrap_or(1))),
        ListKind::Unordered | ListKind::Unknown => {
            marker_with_trailing_space(list.marker.as_deref().unwrap_or("-"))
        }
    };

    format!("{indent}{marker}")
}

fn marker_with_trailing_space(marker: &str) -> String {
    if marker.chars().last().is_some_and(char::is_whitespace) {
        marker.to_string()
    } else {
        format!("{marker} ")
    }
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

fn render_css_shape_shadow(shadow: &ShapeShadow) -> Option<String> {
    let mut color = shadow.color?;
    let opacity = 255u16.saturating_sub(u16::from(shadow.transparency));
    color.a = ((u16::from(color.a) * opacity) / 255) as u8;
    Some(format!(
        "{}px {}px {}",
        shadow.offset_x.0,
        shadow.offset_y.0,
        render_css_color(color)
    ))
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
    let mut declarations = Vec::new();
    if let Some(border) = &image.border {
        declarations.push(format!("border: {}", render_css_border(border)));
    }
    if image.grayscale {
        declarations.push("filter: grayscale(100%)".to_string());
    }
    if let Some(opacity) = image
        .opacity
        .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
    {
        declarations.push(format!("opacity: {opacity}"));
    }
    for (padding, property) in [
        (image.padding_top, "padding-top"),
        (image.padding_right, "padding-right"),
        (image.padding_bottom, "padding-bottom"),
        (image.padding_left, "padding-left"),
    ] {
        if let Some(padding) = padding {
            declarations.push(format!("{property}: {}px", padding.0));
        }
    }
    if let Some(transform) = render_css_transform(
        image.rotation_degrees,
        image.flip_horizontal,
        image.flip_vertical,
    ) {
        declarations.push(format!("transform: {transform}"));
    }
    let tag = render_html_cropped_image(image, &src, &alt, &declarations).unwrap_or_else(|| {
        let style = render_html_style_attr(&declarations.join("; "));
        format!("<img src=\"{src}\" alt=\"{alt}\"{width}{height}{style} />")
    });

    if let Some(caption) = &image.caption {
        let caption = format!(
            "<figcaption>{}</figcaption>",
            render_html_fallback_text(caption)
        );
        return match image.caption_placement {
            Some(crate::ir::CaptionPlacement::Top) => {
                format!("<figure>{caption}{tag}</figure>\n")
            }
            Some(crate::ir::CaptionPlacement::Left) => format!(
                "<figure style=\"display: inline-flex; align-items: center; gap: 0.5em\">{caption}{tag}</figure>\n"
            ),
            Some(crate::ir::CaptionPlacement::Right) => format!(
                "<figure style=\"display: inline-flex; align-items: center; gap: 0.5em\">{tag}{caption}</figure>\n"
            ),
            Some(crate::ir::CaptionPlacement::Bottom) | None => {
                format!("<figure>{tag}{caption}</figure>\n")
            }
        };
    }

    format!("{tag}\n")
}

fn render_html_cropped_image(
    image: &Image,
    src: &str,
    alt: &str,
    declarations: &[String],
) -> Option<String> {
    let crop = image.crop?;
    let display_width = image.width?.0;
    let display_height = image.height?.0;
    let source_width = crop.source_width?.0;
    let source_height = crop.source_height?.0;
    let crop_width = crop.right.0 - crop.left.0;
    let crop_height = crop.bottom.0 - crop.top.0;
    let values = [
        display_width,
        display_height,
        source_width,
        source_height,
        crop.left.0,
        crop.top.0,
        crop_width,
        crop_height,
    ];
    if values.iter().any(|value| !value.is_finite())
        || display_width <= 0.0
        || display_height <= 0.0
        || source_width <= 0.0
        || source_height <= 0.0
        || crop.left.0 < 0.0
        || crop.top.0 < 0.0
        || crop_width <= 0.0
        || crop_height <= 0.0
        || crop.right.0 > source_width
        || crop.bottom.0 > source_height
    {
        return None;
    }

    let scale_x = display_width / crop_width;
    let scale_y = display_height / crop_height;
    let mut outer_declarations = declarations.to_vec();
    outer_declarations.push("display: inline-block".to_string());
    let outer_style = render_html_style_attr(&outer_declarations.join("; "));
    Some(format!(
        "<span{outer_style}><span style=\"position: relative; display: inline-block; overflow: hidden; width: {display_width}px; height: {display_height}px\"><img src=\"{src}\" alt=\"{alt}\" style=\"position: absolute; max-width: none; left: {}px; top: {}px; width: {}px; height: {}px\" /></span></span>",
        -crop.left.0 * scale_x,
        -crop.top.0 * scale_y,
        source_width * scale_x,
        source_height * scale_y,
    ))
}

fn render_css_transform(
    rotation_degrees: Option<f32>,
    flip_horizontal: Option<bool>,
    flip_vertical: Option<bool>,
) -> Option<String> {
    let mut transforms = Vec::new();
    if let Some(rotation) = rotation_degrees.filter(|value| value.is_finite()) {
        transforms.push(format!("rotate({rotation}deg)"));
    }
    if flip_horizontal == Some(true) {
        transforms.push("scaleX(-1)".to_string());
    }
    if flip_vertical == Some(true) {
        transforms.push("scaleY(-1)".to_string());
    }
    (!transforms.is_empty()).then(|| transforms.join(" "))
}

fn render_html_table(table: &Table, resources: &ResourceStore, image_asset_prefix: &str) -> String {
    let border_fill_metadata = render_html_border_fill_metadata(
        table.style.source_border_fill_id,
        table.style.diagonal.as_ref(),
    );
    let mut html = format!(
        "<table{border_fill_metadata}{}>\n",
        render_html_style_attr(&render_html_table_style(
            &table.style,
            resources,
            image_asset_prefix,
        ))
    );

    for (index, row) in table.rows.iter().enumerate() {
        if index == 0 && table.style.repeat_header {
            html.push_str("<thead>\n");
            html.push_str(&render_html_table_row(
                table,
                row,
                index as u32,
                resources,
                image_asset_prefix,
            ));
            html.push_str("</thead>\n");
        } else {
            html.push_str(&render_html_table_row(
                table,
                row,
                index as u32,
                resources,
                image_asset_prefix,
            ));
        }
    }

    html.push_str("</table>\n");
    html
}

fn render_html_table_row(
    table: &Table,
    row: &TableRow,
    inferred_row: u32,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let style = row
        .height
        .map(|height| format!("height: {}px", height.0))
        .map(|style| render_html_style_attr(&style))
        .unwrap_or_default();
    let mut html = format!("<tr{style}>\n");

    let mut inferred_column = 0u32;
    for cell in &row.cells {
        let source_row = cell.source_row.unwrap_or(inferred_row);
        let source_column = cell.source_column.unwrap_or(inferred_column);
        let fallback_zone = table_zone_for_cell(&table.zones, source_row, source_column);
        html.push_str(&render_html_table_cell(
            cell,
            fallback_zone,
            resources,
            image_asset_prefix,
        ));
        inferred_column = source_column.saturating_add(cell.col_span.max(1));
    }

    html.push_str("</tr>\n");
    html
}

fn table_zone_for_cell(
    zones: &[TableZone],
    source_row: u32,
    source_column: u32,
) -> Option<&TableZone> {
    zones.iter().rev().find(|zone| {
        (zone.start_row..=zone.end_row).contains(&source_row)
            && (zone.start_column..=zone.end_column).contains(&source_column)
    })
}

fn render_html_table_cell(
    cell: &TableCell,
    fallback_zone: Option<&TableZone>,
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
    let style = render_html_style_attr(&render_html_table_cell_style(
        &cell.style,
        fallback_zone.and_then(|zone| zone.fill.as_ref()),
        resources,
        image_asset_prefix,
    ));
    let diagonal = cell
        .style
        .diagonal
        .as_ref()
        .or_else(|| fallback_zone.and_then(|zone| zone.diagonal.as_ref()));
    let source_border_fill_id = cell
        .style
        .source_border_fill_id
        .or_else(|| fallback_zone.map(|zone| zone.source_border_fill_id));
    let border_fill_metadata = render_html_border_fill_metadata(source_border_fill_id, diagonal);
    let text_direction_metadata = cell
        .style
        .text_direction
        .map(|direction| format!(" data-text-direction=\"{}\"", direction.source_value()))
        .unwrap_or_default();
    let source_list_metadata = cell
        .source_list_header_width_ref
        .map(|value| format!(" data-list-header-width-ref=\"{value}\""))
        .unwrap_or_default();
    let protection_metadata = if cell.is_protected {
        " data-cell-protected=\"true\""
    } else {
        ""
    };
    let tag = if cell.is_header { "th" } else { "td" };

    format!(
        "<{tag}{rowspan}{colspan}{border_fill_metadata}{text_direction_metadata}{source_list_metadata}{protection_metadata}{style}>{content}</{tag}>\n"
    )
}

fn render_html_table_cell_blocks(
    blocks: &[Block],
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    render_html_blocks(blocks, resources, image_asset_prefix)
}

#[cfg(test)]
fn render_markdown_document(document: &Document) -> String {
    render_markdown_document_with_asset_prefix(document, DEFAULT_IMAGE_ASSET_PUBLIC_PREFIX)
}

fn render_markdown_document_with_asset_prefix(
    document: &Document,
    image_asset_prefix: &str,
) -> String {
    let mut blocks = Vec::new();

    for section in &document.sections {
        for master_page in &section.master_pages {
            blocks.push(render_markdown_master_page(
                master_page,
                &document.resources,
                image_asset_prefix,
            ));
        }
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

fn render_markdown_master_page(
    master_page: &MasterPage,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let content = master_page
        .blocks
        .iter()
        .map(|block| render_markdown_block(block, resources, image_asset_prefix))
        .collect::<Vec<_>>()
        .join("\n\n");
    let label = render_markdown_text("[바탕쪽]");

    if content.is_empty() {
        label
    } else {
        format!("{label}\n\n{content}")
    }
}

fn render_markdown_block(
    block: &Block,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    match block {
        Block::Paragraph(paragraph) => render_markdown_paragraph(paragraph),
        Block::ColumnLayout(_) => String::new(),
        Block::DocumentControl(control) => render_markdown_text(control.fallback_text()),
        Block::Table(table) => render_markdown_table(table),
        Block::Image(image) => render_markdown_image(image, resources, image_asset_prefix),
        Block::Equation(equation) => render_markdown_equation(equation),
        Block::Shape(shape) => render_markdown_shape(shape, resources, image_asset_prefix),
        Block::Chart(chart) => render_markdown_chart(chart),
        Block::Unknown(unknown) => render_markdown_unknown_block(unknown),
    }
}

fn render_markdown_unknown_block(unknown: &UnknownBlock) -> String {
    render_markdown_text(&unknown_block_display_text(unknown))
}

fn render_markdown_paragraph(paragraph: &Paragraph) -> String {
    let mut content = render_markdown_inlines(&paragraph.inlines);
    if let Some(list) = &paragraph.list {
        content = format!("{}{}", list_prefix(list), content);
    }

    match &paragraph.role {
        ParagraphRole::Title => format!("# {content}"),
        ParagraphRole::Heading { level } => {
            let level = (*level).clamp(1, 6);
            format!("{} {content}", "#".repeat(level as usize))
        }
        _ => content,
    }
}

fn render_markdown_inlines(inlines: &[Inline]) -> String {
    let mut content = String::new();

    for inline in inlines {
        match inline {
            Inline::Text(run) => content.push_str(&render_markdown_text_run(run)),
            Inline::LineBreak => content.push_str("  \n"),
            Inline::Tab => content.push('\t'),
            Inline::Link(link) => content.push_str(&render_markdown_link(link)),
            Inline::Field(field) => {
                content.push_str(&render_markdown_text(&field.fallback_text));
            }
            Inline::FootnoteRef { note_id } => content.push_str(&render_markdown_note_ref(note_id)),
            Inline::EndnoteRef { note_id } => content.push_str(&render_markdown_note_ref(note_id)),
            Inline::Anchor { id } => {
                let id = crate::util::plain_text::sanitize_anchor_id(id);
                content.push_str(&format!("<a id=\"{}\"></a>", escape_html(&id)));
            }
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

    let has_header_row = table.rows[0].cells.iter().all(|cell| cell.is_header);
    let mut lines = Vec::with_capacity(table.rows.len() + usize::from(!has_header_row) + 1);
    if has_header_row {
        lines.push(render_markdown_table_row(&table.rows[0]));
    } else {
        lines.push(format!("| {} |", vec![""; column_count].join(" | ")));
    }
    lines.push(format!("| {} |", vec!["---"; column_count].join(" | ")));

    for row in table.rows.iter().skip(usize::from(has_header_row)) {
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

    let first_row_is_header = first_row.cells.iter().all(|cell| cell.is_header);
    let first_row_has_no_headers = first_row.cells.iter().all(|cell| !cell.is_header);
    if (!first_row_is_header && !first_row_has_no_headers)
        || table
            .rows
            .iter()
            .skip(1)
            .any(|row| row.cells.iter().any(|cell| cell.is_header))
    {
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
    let path = escape_markdown_link_destination(&resource_public_path(
        resources,
        &image.resource_id,
        image_asset_prefix,
    ));

    let mut markdown = format!("![{alt}]({path})");
    if let Some(caption) = image
        .caption
        .as_deref()
        .filter(|caption| !caption.is_empty())
    {
        markdown.push_str("\n\n");
        markdown.push_str(&render_markdown_text(caption));
    }
    markdown
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DocumentAssetPaths {
    image_output_dir: PathBuf,
    image_public_prefix: String,
    binary_output_dir: PathBuf,
}

fn write_resource_assets(
    assets: &DocumentAssetPaths,
    resources: &ResourceStore,
) -> Result<(), io::Error> {
    let image_file_names =
        resource_file_name_map(resources, |resource| matches!(resource, Resource::Image(_)));
    let image_resources = resources
        .entries
        .iter()
        .filter_map(|resource| match resource {
            Resource::Image(image) => Some(image),
            Resource::Binary(_) => None,
        })
        .collect::<Vec<_>>();

    if !image_resources.is_empty() {
        fs::create_dir_all(&assets.image_output_dir)?;

        for image in image_resources {
            let file_name = image_file_names
                .get(&image.id)
                .cloned()
                .unwrap_or_else(|| sanitized_resource_file_name(resources, &image.id));
            fs::write(assets.image_output_dir.join(file_name), &image.bytes)?;
        }
    }

    let binary_file_names = resource_file_name_map(
        resources,
        |resource| matches!(resource, Resource::Binary(binary) if binary.kind != crate::ir::BinaryResourceKind::Link),
    );
    let binary_resources = resources
        .entries
        .iter()
        .filter_map(|resource| match resource {
            Resource::Binary(binary) if binary.kind != crate::ir::BinaryResourceKind::Link => {
                Some(binary)
            }
            Resource::Binary(_) | Resource::Image(_) => None,
        })
        .collect::<Vec<_>>();

    if !binary_resources.is_empty() {
        fs::create_dir_all(&assets.binary_output_dir)?;
        for binary in binary_resources {
            let file_name = binary_file_names
                .get(&binary.id)
                .cloned()
                .unwrap_or_else(|| sanitized_resource_file_name(resources, &binary.id));
            fs::write(assets.binary_output_dir.join(file_name), &binary.bytes)?;
        }
    }

    Ok(())
}

#[cfg(test)]
fn image_asset_dir(output_path: &Path) -> PathBuf {
    document_asset_paths(output_path).image_output_dir
}

#[cfg(test)]
fn image_asset_public_prefix(output_path: &Path) -> String {
    document_asset_paths(output_path).image_public_prefix
}

#[cfg(test)]
fn binary_asset_dir(output_path: &Path) -> PathBuf {
    document_asset_paths(output_path).binary_output_dir
}

fn document_asset_paths(output_path: &Path) -> DocumentAssetPaths {
    let asset_root = format!("{}_assets", sanitized_output_file_stem(output_path));
    let output_root = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(&asset_root);

    DocumentAssetPaths {
        image_output_dir: output_root.join("images"),
        image_public_prefix: format!("{asset_root}/images"),
        binary_output_dir: output_root.join("files"),
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
    if equation.kind == EquationKind::Latex
        && let Some(content) = &equation.content
    {
        return format!("$${content}$$");
    }

    render_markdown_text(&equation_display_text(equation))
}

fn render_markdown_shape(
    shape: &Shape,
    resources: &ResourceStore,
    image_asset_prefix: &str,
) -> String {
    let nested_blocks = if !shape.content.is_empty() {
        &shape.content
    } else {
        &shape.children
    };
    if nested_blocks.is_empty() {
        return render_markdown_text(&shape_display_text(shape));
    }

    nested_blocks
        .iter()
        .map(|block| render_markdown_block(block, resources, image_asset_prefix))
        .filter(|block| !block.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_markdown_chart(chart: &Chart) -> String {
    render_markdown_text(&chart_display_text(chart))
}

fn render_markdown_link(link: &Link) -> String {
    let label = render_markdown_link_label(link);
    let url = escape_markdown_link_destination(&link.url);
    let title = link
        .title
        .as_deref()
        .map(escape_markdown_title)
        .unwrap_or_default();

    if title.is_empty() {
        format!("[{label}]({url})")
    } else {
        format!("[{label}]({url} {title})")
    }
}

fn escape_markdown_title(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

fn render_markdown_link_label(link: &Link) -> String {
    let label = render_markdown_link_label_inlines(&link.inlines);
    if label.is_empty() {
        escape_markdown_image_alt(link.title.as_deref().unwrap_or(&link.url))
    } else {
        label
    }
}

fn render_markdown_link_label_inlines(inlines: &[Inline]) -> String {
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
        Inline::Link(link) => render_markdown_link_label(link),
        Inline::Field(field) => escape_markdown_image_alt(&field.fallback_text),
        Inline::FootnoteRef { note_id } => render_markdown_note_ref(note_id),
        Inline::EndnoteRef { note_id } => render_markdown_note_ref(note_id),
        Inline::Anchor { id } => {
            let id = crate::util::plain_text::sanitize_anchor_id(id);
            format!("<a id=\"{}\"></a>", escape_html(&id))
        }
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
        text = format!("~~{text}~~");
    }

    // Markdown has no native syntax for these, but inline HTML is widely
    // supported and keeps the distinction instead of dropping it silently.
    if run.style.superscript {
        text = format!("<sup>{text}</sup>");
    } else if run.style.subscript {
        text = format!("<sub>{text}</sub>");
    }

    text
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
        .replace(' ', "%20")
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
    resource_file_name_map(resources, |resource| matches!(resource, Resource::Image(_)))
        .get(resource_id)
        .cloned()
        .unwrap_or_else(|| sanitized_resource_file_name(resources, resource_id))
}

fn resource_file_name_map(
    resources: &ResourceStore,
    include: impl Fn(&Resource) -> bool,
) -> HashMap<ResourceId, String> {
    let mut names = HashMap::new();
    let mut used_names = HashMap::new();

    for resource in resources
        .entries
        .iter()
        .filter(|resource| include(resource))
    {
        let candidate = sanitized_resource_file_name(resources, resource.id());
        let unique_name = unique_resource_file_name(&candidate, &mut used_names);
        names.insert(resource.id().clone(), unique_name);
    }

    names
}

fn sanitized_resource_file_name(resources: &ResourceStore, resource_id: &ResourceId) -> String {
    let base = resource_id.as_str();
    let extension = resource_extension(resources, resource_id)
        .map(ToOwned::to_owned)
        .or_else(|| path_extension_from_resource_id(base))
        .unwrap_or_else(|| match resources.get(resource_id) {
            Some(Resource::Binary(_)) => "bin".to_string(),
            Some(Resource::Image(_)) | None => "png".to_string(),
        });
    let extension = sanitize_asset_extension(&extension);
    let stem = resource_file_stem(base, &extension);
    let stem = sanitize_asset_path_segment(stem);

    format!("{stem}.{extension}")
}

fn unique_resource_file_name(candidate: &str, used_names: &mut HashMap<String, usize>) -> String {
    if !used_names.contains_key(candidate) {
        used_names.insert(candidate.to_string(), 1);
        return candidate.to_string();
    }

    let mut suffix = used_names.get(candidate).copied().unwrap_or(1) + 1;
    loop {
        let unique_name = append_file_name_suffix(candidate, suffix);
        if !used_names.contains_key(&unique_name) {
            used_names.insert(candidate.to_string(), suffix);
            used_names.insert(unique_name.clone(), 1);
            return unique_name;
        }
        suffix += 1;
    }
}

fn append_file_name_suffix(file_name: &str, suffix: usize) -> String {
    file_name
        .rsplit_once('.')
        .map(|(stem, extension)| format!("{stem}-{suffix}.{extension}"))
        .unwrap_or_else(|| format!("{file_name}-{suffix}"))
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

fn path_extension_from_resource_id(resource_id: &str) -> Option<String> {
    resource_id
        .rsplit(['/', '\\'])
        .next()
        .and_then(|file_name| file_name.rsplit_once('.'))
        .map(|(_, extension)| extension)
        .filter(|extension| !extension.is_empty())
        .map(ToOwned::to_owned)
}

fn resource_file_stem<'a>(resource_id: &'a str, extension: &str) -> &'a str {
    let file_name = resource_id
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(resource_id);
    file_name
        .rsplit_once('.')
        .filter(|(_, candidate_extension)| candidate_extension.eq_ignore_ascii_case(extension))
        .map(|(stem, _)| stem)
        .unwrap_or(resource_id)
}

fn sanitize_asset_extension(extension: &str) -> String {
    let sanitized = extension
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();

    if sanitized.is_empty() {
        "bin".to_string()
    } else {
        sanitized
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
    #[serde(skip_serializing_if = "is_zero")]
    warning_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
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
        Alignment, BinaryResource, BinaryResourceKind, Border, BorderFillDiagonal, BorderStyle,
        Chart, Color, ConversionWarning, Equation, EquationKind, FillStyle, HeaderFooter,
        HeaderFooterPlacement, IR_VERSION, Image, ImageCrop, ImageResource, Indent, LengthPt,
        LengthPx, Link, ListInfo, ListKind, ListMarkerLayout, MasterPage, Metadata, Note, NoteId,
        NoteKind, NoteStore, Paragraph, ParagraphRole, ParagraphStyle, Percent, Resource,
        ResourceId, ResourceStore, Section, Shape, ShapeKind, Spacing, StyleSheet, Table,
        TableCell, TableCellStyle, TableCellTextDirection, TableRow, TableStyle, TextBorderFill,
        TextRun, TextShadow, TextStyle, UnknownInline, WarningCode,
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
    fn reports_conversion_warnings_for_converted_files() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("converted-warning-report");
        fs::create_dir_all(&root)?;
        write_preview_hwpx(&root.join("alpha.hwpx"), "first line")?;

        let args = CliArgs {
            input_path: root.join("alpha.hwpx"),
            format: OutputFormat::Json,
            recursive: false,
            manifest_path: None,
            resume_manifest_path: None,
            continue_on_error: false,
            output_dir: None,
            skip_existing: false,
        };

        let report = export(&args)?;

        assert_eq!(report.converted_files().len(), 1);
        assert_eq!(report.warning_count(), 1);
        assert_eq!(report.converted_files()[0].warnings.len(), 1);
        assert!(
            report.converted_files()[0].warnings[0].contains("Used HWPX preview fallback"),
            "expected preview fallback warning, got {:?}",
            report.converted_files()[0].warnings
        );

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    #[test]
    fn reports_ir_unknown_messages_as_conversion_warnings() {
        let document = document_with_blocks(vec![
            Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![
                    Inline::Unknown(UnknownInline {
                        kind: "field".to_string(),
                        fallback_text: Some("[field]".to_string()),
                        message: Some("field fallback preserved".to_string()),
                        source: None,
                    }),
                    Inline::Link(Link {
                        url: "https://example.com".to_string(),
                        title: None,
                        inlines: vec![Inline::Unknown(UnknownInline {
                            kind: "field".to_string(),
                            fallback_text: Some("[field]".to_string()),
                            message: Some("field fallback preserved".to_string()),
                            source: None,
                        })],
                    }),
                ],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: None,
            }),
            Block::Table(Table {
                rows: vec![TableRow {
                    cells: vec![TableCell {
                        row_span: 1,
                        col_span: 1,
                        is_header: false,
                        blocks: vec![Block::Unknown(UnknownBlock {
                            kind: "cell_field".to_string(),
                            fallback_text: Some("[cell field]".to_string()),
                            message: Some("cell field fallback preserved".to_string()),
                            source: None,
                        })],
                        style: TableCellStyle::default(),
                        ..Default::default()
                    }],
                    height: None,
                }],
                style: TableStyle::default(),
                ..Default::default()
            }),
        ]);

        let warnings = conversion_warnings_for_document(&document);

        assert_eq!(
            warnings
                .iter()
                .filter(|warning| warning.contains("IR unknown inline `field`"))
                .count(),
            1
        );
        assert!(warnings.iter().any(|warning| {
            warning.contains("IR unknown block `cell_field`: cell field fallback preserved")
        }));
    }

    #[test]
    fn deduplicates_document_conversion_warnings() {
        let mut document = document_with_blocks(Vec::new());
        document.warnings = vec![
            ConversionWarning {
                code: WarningCode::Unknown,
                message: "same warning".to_string(),
            },
            ConversionWarning {
                code: WarningCode::Unknown,
                message: "same warning".to_string(),
            },
        ];

        let warnings = conversion_warnings_for_document(&document);

        assert_eq!(warnings, vec!["same warning".to_string()]);
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
                .contains("HWPX preview fallback")
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
        assert!(error.to_string().contains("HWPX preview fallback"));

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
    fn renders_empty_paragraphs_across_text_exports() {
        let document = Document::from_paragraphs(vec![
            "before".to_string(),
            String::new(),
            "after".to_string(),
        ]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);
        let markdown = render_markdown_document(&document);
        let svg = render_svg_document(
            Path::new("sample.hwpx"),
            &plain_text::collect_block_texts(&document),
        );

        assert!(html.contains("<p>before</p>\n<p></p>\n<p>after</p>"));
        assert_eq!(markdown, "before\n\n\n\nafter");
        assert!(svg.contains(">before</text>"));
        assert!(svg.contains("> </text>"));
        assert!(svg.contains(">after</text>"));
    }

    #[test]
    fn serializes_json_output_as_document_ir() {
        let document = Document::from_paragraphs(vec![
            "first paragraph".to_string(),
            "second paragraph".to_string(),
        ]);

        let content = serde_json::to_string_pretty(&document).unwrap();

        assert!(content.contains(&format!("\"ir_version\": {IR_VERSION}")));
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
                    Inline::Tab,
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
        assert!(html.contains(
            "<p>&amp; &lt; &gt; &quot; &apos;<br /><span class=\"tab\">\t</span>second line + extra</p>"
        ));
        assert!(html.contains(".tab {"));
        assert!(html.contains("<p>fallback block</p>"));
    }

    #[test]
    fn renders_html_caption_paragraph_role_with_class() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Caption,
            inlines: vec![Inline::Text(TextRun {
                text: "Table caption".to_string(),
                style: TextStyle::default(),
                style_ref: None,
            })],
            style: ParagraphStyle::default(),
            style_ref: None,
            list: None,
        })]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("<p class=\"caption\">Table caption</p>"));
    }

    #[test]
    fn renders_html_title_and_heading_paragraph_roles_as_headings() {
        let document = document_with_blocks(vec![
            Block::Paragraph(Paragraph {
                role: ParagraphRole::Title,
                inlines: vec![Inline::Text(TextRun {
                    text: "Document title".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: None,
            }),
            Block::Paragraph(Paragraph {
                role: ParagraphRole::Heading { level: 3 },
                inlines: vec![Inline::Text(TextRun {
                    text: "Section heading".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: None,
            }),
        ]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("<h1 class=\"title\">Document title</h1>"));
        assert!(html.contains("<h3 class=\"heading\">Section heading</h3>"));
    }

    #[test]
    fn renders_html_list_paragraphs_as_semantic_lists() {
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
                    kind: ListKind::Ordered,
                    level: 0,
                    marker: None,
                    marker_format: None,
                    number: Some(1),
                    ..Default::default()
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
                    marker_format: None,
                    number: Some(2),
                    ..Default::default()
                }),
            }),
        ]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(
            html.contains("<ol>\n<li value=\"1\">first</li>\n<li value=\"2\">second</li>\n</ol>")
        );
        assert!(!html.contains("1. first"));
    }

    #[test]
    fn preserves_and_escapes_ordered_list_marker_format_in_html() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![Inline::Text(TextRun {
                text: "chapter".to_string(),
                style: TextStyle::default(),
                style_ref: None,
            })],
            style: ParagraphStyle::default(),
            style_ref: None,
            list: Some(ListInfo {
                kind: ListKind::Ordered,
                level: 0,
                marker: Some("chapter 3 & \"appendix\"".to_string()),
                marker_format: Some("chapter ^1 & \"appendix\"".to_string()),
                number: Some(3),
                source_definition_id: Some(4),
                marker_layout: Some(ListMarkerLayout {
                    raw_attributes: 5,
                    raw_width_adjust: 4,
                    raw_text_distance: 8,
                    source_char_shape_id: Some(6),
                    image_bullet_id: None,
                    image_data: [0; 4],
                    check_marker: None,
                }),
            }),
        })]);

        let html = render_html_document(Path::new("sample.hwp"), &document);
        let markdown = render_markdown_document(&document);

        assert!(html.contains(
            "<li value=\"3\" data-marker-format=\"chapter ^1 &amp; &quot;appendix&quot;\" data-marker=\"chapter 3 &amp; &quot;appendix&quot;\" data-source-definition-id=\"4\" data-marker-attributes=\"5\" data-marker-width-adjust=\"4\" data-marker-text-distance=\"8\" data-marker-char-shape-id=\"6\">chapter</li>"
        ));
        assert!(html.contains("li[data-marker]::marker"));
        assert_eq!(markdown, "chapter 3 & \"appendix\" chapter");
    }

    #[test]
    fn renders_html_nested_list_levels_as_nested_lists() {
        let document = document_with_blocks(vec![
            Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![Inline::Text(TextRun {
                    text: "parent".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: Some(ListInfo {
                    kind: ListKind::Unordered,
                    level: 0,
                    marker: Some("-".to_string()),
                    marker_format: None,
                    number: None,
                    ..Default::default()
                }),
            }),
            Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![Inline::Text(TextRun {
                    text: "child".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: Some(ListInfo {
                    kind: ListKind::Unordered,
                    level: 1,
                    marker: Some("-".to_string()),
                    marker_format: None,
                    number: None,
                    ..Default::default()
                }),
            }),
            Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![Inline::Text(TextRun {
                    text: "sibling".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: Some(ListInfo {
                    kind: ListKind::Unordered,
                    level: 0,
                    marker: Some("-".to_string()),
                    marker_format: None,
                    number: None,
                    ..Default::default()
                }),
            }),
        ]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains(
            "<ul>\n<li data-marker=\"-\">parent<ul>\n<li data-marker=\"-\">child</li>\n</ul>\n</li>\n<li data-marker=\"-\">sibling</li>\n</ul>"
        ));
    }

    #[test]
    fn renders_markdown_title_and_heading_paragraph_roles() {
        let document = document_with_blocks(vec![
            Block::Paragraph(Paragraph {
                role: ParagraphRole::Title,
                inlines: vec![Inline::Text(TextRun {
                    text: "Document title".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: None,
            }),
            Block::Paragraph(Paragraph {
                role: ParagraphRole::Heading { level: 3 },
                inlines: vec![Inline::Text(TextRun {
                    text: "Section heading".to_string(),
                    style: TextStyle::default(),
                    style_ref: None,
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: None,
            }),
        ]);

        let markdown = render_markdown_document(&document);

        assert!(markdown.contains("# Document title"));
        assert!(markdown.contains("### Section heading"));
    }

    #[test]
    fn renders_numbered_heading_roles_as_headings() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Heading { level: 2 },
            inlines: vec![Inline::Text(TextRun {
                text: "Numbered heading".to_string(),
                style: TextStyle::default(),
                style_ref: None,
            })],
            style: ParagraphStyle::default(),
            style_ref: None,
            list: Some(ListInfo {
                kind: ListKind::Ordered,
                level: 0,
                marker: None,
                marker_format: None,
                number: Some(3),
                ..Default::default()
            }),
        })]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);
        let markdown = render_markdown_document(&document);

        assert!(html.contains("<h2 class=\"heading\">3. Numbered heading</h2>"));
        assert!(markdown.contains("## 3. Numbered heading"));
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
    fn renders_multiline_unknown_fallbacks_across_text_exports() {
        let document = document_with_blocks(vec![Block::Unknown(UnknownBlock {
            kind: "hwpx:image".to_string(),
            fallback_text: Some("[image]\nmissing image alt".to_string()),
            message: None,
            source: None,
        })]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);
        let markdown = render_markdown_document(&document);
        let text = plain_text::to_plain_text(&document);

        assert!(html.contains("<p>[image]<br />missing image alt</p>"));
        assert!(markdown.contains("[image]  \nmissing image alt"));
        assert!(text.contains("[image]\nmissing image alt"));
    }

    #[test]
    fn renders_markdown_image_from_document_ir() {
        let document = document_with_image_block("image-1", Some("로고"), Some("png"));

        let asset_prefix = image_asset_public_prefix(Path::new("sample.md"));
        let markdown = render_markdown_document_with_asset_prefix(&document, &asset_prefix);

        assert!(markdown.contains("![로고](sample_assets/images/image-1.png)"));
    }

    #[test]
    fn renders_markdown_image_caption_from_document_ir() {
        let mut document = document_with_image_block("image-1", Some("logo"), Some("png"));
        if let Block::Image(image) = &mut document.sections[0].blocks[0] {
            image.caption = Some("Image caption".to_string());
        }

        let asset_prefix = image_asset_public_prefix(Path::new("sample.md"));
        let markdown = render_markdown_document_with_asset_prefix(&document, &asset_prefix);

        assert!(markdown.contains("![logo](sample_assets/images/image-1.png)"));
        assert!(markdown.contains("Image caption"));
    }

    #[test]
    fn escapes_markdown_image_asset_path() {
        let document = document_with_image_block("image-1", Some("logo"), Some("png"));

        let markdown =
            render_markdown_document_with_asset_prefix(&document, "assets (draft)/images");

        assert!(markdown.contains("![logo](assets%20\\(draft\\)/images/image-1.png)"));
    }

    #[test]
    fn renders_html_image_from_document_ir() {
        let mut document = document_with_image_block("image-1", Some("로고"), Some("png"));
        if let Block::Image(image) = &mut document.sections[0].blocks[0] {
            image.width = Some(LengthPx(200.0));
            image.height = Some(LengthPx(100.0));
            image.crop = Some(ImageCrop {
                left: LengthPx(10.0),
                top: LengthPx(20.0),
                right: LengthPx(90.0),
                bottom: LengthPx(70.0),
                source_width: Some(LengthPx(100.0)),
                source_height: Some(LengthPx(80.0)),
            });
        }

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("<img src=\"sample_assets/images/image-1.png\" alt=\"로고\""));
        assert!(html.contains("overflow: hidden; width: 200px; height: 100px"));
        assert!(html.contains("left: -25px; top: -40px; width: 250px; height: 160px"));
    }

    #[test]
    fn renders_html_image_caption_line_breaks() {
        let mut document = document_with_image_block("image-1", Some("logo"), Some("png"));
        if let Block::Image(image) = &mut document.sections[0].blocks[0] {
            image.caption = Some("first line\nsecond line".to_string());
            image.caption_placement = Some(crate::ir::CaptionPlacement::Top);
        }

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("<figcaption>first line<br />second line</figcaption>"));
        assert!(
            html.find("<figcaption>").expect("caption should render")
                < html.find("<img ").expect("image should render")
        );
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
    fn writes_binary_assets_without_materializing_external_links() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("binary-assets");
        fs::create_dir_all(&root)?;
        let mut document = document_with_blocks(Vec::new());
        document.resources.entries = vec![
            Resource::Binary(BinaryResource {
                id: ResourceId("attachment".to_string()),
                media_type: Some("application/octet-stream".to_string()),
                extension: Some("dat".to_string()),
                bytes: b"attachment-bytes".to_vec(),
                kind: BinaryResourceKind::Embedded,
                ..Default::default()
            }),
            Resource::Binary(BinaryResource {
                id: ResourceId("storage-without-extension".to_string()),
                bytes: b"storage-bytes".to_vec(),
                kind: BinaryResourceKind::Storage,
                ..Default::default()
            }),
            Resource::Binary(BinaryResource {
                id: ResourceId("linked.pdf".to_string()),
                extension: Some("pdf".to_string()),
                kind: BinaryResourceKind::Link,
                absolute_path: Some("https://example.com/linked.pdf".to_string()),
                ..Default::default()
            }),
        ];
        let html_path = root.join("sample.html");

        write_html_output(Path::new("sample.hwpx"), &html_path, &document)?;

        let files_dir = root.join("sample_assets").join("files");
        assert_eq!(
            fs::read(files_dir.join("attachment.dat"))?,
            b"attachment-bytes"
        );
        assert_eq!(
            fs::read(files_dir.join("storage-without-extension.bin"))?,
            b"storage-bytes"
        );
        assert!(!files_dir.join("linked.pdf").exists());

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    #[test]
    fn sanitizes_image_asset_file_names() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("unsafe-image-assets");
        fs::create_dir_all(&root)?;
        let document =
            document_with_image_block("../BinData/unsafe image.png", Some("logo"), Some("png"));
        let html_path = root.join("sample.html");

        write_html_output(Path::new("sample.hwpx"), &html_path, &document)?;

        let html = fs::read_to_string(&html_path)?;
        assert!(html.contains("src=\"sample_assets/images/unsafe_image.png\""));
        assert!(
            root.join("sample_assets")
                .join("images")
                .join("unsafe_image.png")
                .exists()
        );

        fs::remove_dir_all(&root)?;

        Ok(())
    }

    #[test]
    fn avoids_sanitized_image_asset_file_name_collisions() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("colliding-image-assets");
        fs::create_dir_all(&root)?;
        let first_id = ResourceId("same image.png".to_string());
        let second_id = ResourceId("same?image.png".to_string());
        let document = Document {
            ir_version: IR_VERSION,
            metadata: Metadata::default(),
            sections: vec![Section {
                blocks: vec![
                    Block::Image(Image {
                        resource_id: first_id.clone(),
                        alt: Some("first".to_string()),
                        ..Default::default()
                    }),
                    Block::Image(Image {
                        resource_id: second_id.clone(),
                        alt: Some("second".to_string()),
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            }],
            resources: ResourceStore {
                entries: vec![
                    Resource::Image(ImageResource {
                        id: first_id,
                        media_type: Some("image/png".to_string()),
                        extension: Some("png".to_string()),
                        bytes: vec![1],
                    }),
                    Resource::Image(ImageResource {
                        id: second_id,
                        media_type: Some("image/png".to_string()),
                        extension: Some("png".to_string()),
                        bytes: vec![2],
                    }),
                ],
            },
            styles: StyleSheet::default(),
            notes: NoteStore::default(),
            warnings: Vec::<ConversionWarning>::new(),
        };
        let html_path = root.join("sample.html");

        write_html_output(Path::new("sample.hwpx"), &html_path, &document)?;

        let html = fs::read_to_string(&html_path)?;
        assert!(html.contains("src=\"sample_assets/images/same_image.png\""));
        assert!(html.contains("src=\"sample_assets/images/same_image-2.png\""));
        assert_eq!(
            fs::read(
                root.join("sample_assets")
                    .join("images")
                    .join("same_image.png")
            )?,
            vec![1]
        );
        assert_eq!(
            fs::read(
                root.join("sample_assets")
                    .join("images")
                    .join("same_image-2.png")
            )?,
            vec![2]
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
        assert_eq!(
            binary_asset_dir(&output_path),
            Path::new("out").join("my_report_assets").join("files")
        );
    }

    #[test]
    fn renders_html_table_from_document_ir() {
        let mut table = match simple_table_block() {
            Block::Table(table) => table,
            other => panic!("expected table block, got {other:?}"),
        };
        table.rows[0].cells[0].style.source_border_fill_id = Some(7);
        table.rows[0].cells[0].style.diagonal = Some(BorderFillDiagonal {
            raw_attributes: 8,
            diagonal_type: 1,
            width_index: 3,
            color: Some(Color {
                r: 0x44,
                g: 0x55,
                b: 0x66,
                a: 255,
            }),
            raw_color: 0x00665544,
        });
        let document = document_with_blocks(vec![Block::Table(table)]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("<table>"));
        assert!(html.contains("<tr>"));
        assert!(html.contains("<td data-border-fill-id=\"7\""));
        assert!(html.contains("data-diagonal-attributes=\"8\""));
        assert!(html.contains("data-diagonal-type=\"1\""));
        assert!(html.contains("data-diagonal-width-index=\"3\""));
        assert!(html.contains("data-diagonal-color-raw=\"6706500\""));
        assert!(html.contains("<p>cell1</p>\n</td>"));
        assert!(html.contains("<td><p>cell4</p>\n</td>"));
    }

    #[test]
    fn renders_markdown_table_from_document_ir() {
        let document = document_with_blocks(vec![simple_table_block()]);

        let markdown = render_markdown_document(&document);

        assert_eq!(
            markdown,
            "|  |  |\n| --- | --- |\n| cell1 | cell2 |\n| cell3 | cell4 |"
        );
    }

    #[test]
    fn renders_markdown_table_header_from_header_cells() {
        let mut table = match simple_table_block() {
            Block::Table(table) => table,
            other => panic!("expected table block, got {other:?}"),
        };
        for cell in &mut table.rows[0].cells {
            cell.is_header = true;
        }
        let document = document_with_blocks(vec![Block::Table(table)]);

        let markdown = render_markdown_document(&document);

        assert_eq!(
            markdown,
            "| cell1 | cell2 |\n| --- | --- |\n| cell3 | cell4 |"
        );
    }

    #[test]
    fn renders_plain_text_table_fallback_for_txt_exporter() {
        let document = document_with_blocks(vec![simple_table_block()]);

        let text = plain_text::to_plain_text(&document);

        assert_eq!(text, "[표]\ncell1 | cell2\ncell3 | cell4");
    }

    #[test]
    fn renders_html_text_style_decorations_and_border_fill() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![Inline::Text(TextRun {
                text: "styled".to_string(),
                style: TextStyle {
                    bold: true,
                    italic: true,
                    underline: true,
                    strike: true,
                    underline_style: Some(TextDecorationStyle::Wavy),
                    underline_above: true,
                    underline_color: Some(Color {
                        r: 17,
                        g: 34,
                        b: 51,
                        a: 255,
                    }),
                    border_fill: Some(Box::new(TextBorderFill {
                        source_border_fill_id: 3,
                        fill: Some(FillStyle::Solid {
                            background_color: Some(Color {
                                r: 68,
                                g: 85,
                                b: 102,
                                a: 255,
                            }),
                            background_color_raw: 0x00665544,
                            pattern_color: None,
                            pattern_color_raw: 0,
                            pattern_type: 0,
                            alpha: 255,
                        }),
                        border_left: Some(Border {
                            width: LengthPx(1.0),
                            style: BorderStyle::Dashed,
                            color: Some(Color {
                                r: 119,
                                g: 136,
                                b: 153,
                                a: 255,
                            }),
                        }),
                        ..Default::default()
                    })),
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
        assert!(html.contains("text-decoration: overline line-through"));
        assert!(html.contains("text-decoration-color: #112233"));
        assert!(html.contains("text-decoration-style: wavy"));
        assert!(html.contains("data-border-fill-id=\"3\""));
        assert!(html.contains("background-color: #445566"));
        assert!(html.contains("border-left: 1px dashed #778899"));
    }

    #[test]
    fn renders_html_advanced_text_style_decorations() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![
                Inline::Text(TextRun {
                    text: "x".to_string(),
                    style: TextStyle {
                        superscript: true,
                        emphasis_dot: true,
                        emphasis_mark_type: Some(2),
                        emboss: true,
                        outline: true,
                        ..Default::default()
                    },
                    style_ref: None,
                }),
                Inline::Text(TextRun {
                    text: "shadow".to_string(),
                    style: TextStyle {
                        shadow: true,
                        shadow_details: Some(TextShadow {
                            kind: 2,
                            offset_x_percent: 25,
                            offset_y_percent: -50,
                            color: Some(Color {
                                r: 0x11,
                                g: 0x22,
                                b: 0x33,
                                a: 255,
                            }),
                            raw_color: 0x00332211,
                        }),
                        ..Default::default()
                    },
                    style_ref: None,
                }),
            ],
            style: ParagraphStyle::default(),
            style_ref: None,
            list: None,
        })]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("vertical-align: super"));
        assert!(html.contains("text-emphasis: &quot;○&quot;"));
        assert!(html.contains("data-emphasis-mark-type=\"2\""));
        assert!(html.contains("-webkit-text-stroke: 1px currentColor"));
        assert!(html.contains("text-shadow:"));
        assert!(html.contains("text-shadow: 0.25em -0.5em 0 #112233"));
        assert!(html.contains("data-shadow-kind=\"2\""));
        assert!(html.contains("data-shadow-offset-x-percent=\"25\""));
        assert!(html.contains("data-shadow-offset-y-percent=\"-50\""));
        assert!(html.contains("data-shadow-color-raw=\"3351057\""));
    }

    #[test]
    fn renders_html_typographic_metrics() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![Inline::Text(TextRun {
                text: "metrics".to_string(),
                style: TextStyle {
                    font_width_percent: Some(Percent(95.0)),
                    letter_spacing_percent: Some(Percent(-5.0)),
                    relative_size_percent: Some(Percent(80.0)),
                    vertical_offset_percent: Some(Percent(10.0)),
                    kerning: true,
                    ..Default::default()
                },
                style_ref: None,
            })],
            style: ParagraphStyle::default(),
            style_ref: None,
            list: None,
        })]);

        let html = render_html_document(Path::new("sample.hwp"), &document);

        assert!(html.contains("font-stretch: 95%"));
        assert!(html.contains("letter-spacing: -0.05em"));
        assert!(html.contains("vertical-align: 0.1em"));
        assert!(html.contains("font-kerning: normal"));
    }

    #[test]
    fn renders_table_header_cell_and_vertical_align() {
        let document = document_with_blocks(vec![Block::Table(Table {
            rows: vec![TableRow {
                cells: vec![TableCell {
                    is_header: true,
                    is_protected: true,
                    source_list_header_width_ref: Some(0x06),
                    blocks: vec![Block::Paragraph(Paragraph::from_plain_text(
                        "H".to_string(),
                    ))],
                    style: TableCellStyle {
                        vertical_align: Some(VerticalAlign::Middle),
                        text_direction: Some(TableCellTextDirection::VerticalLatinUpright),
                        width: Some(LengthPx(100.0)),
                        height: Some(LengthPx(20.0)),
                        padding_left: Some(LengthPx(2.0)),
                        border_top: Some(Border {
                            width: LengthPx(2.0),
                            style: BorderStyle::Solid,
                            color: Some(Color {
                                r: 17,
                                g: 34,
                                b: 51,
                                a: 255,
                            }),
                        }),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                height: Some(LengthPx(36.0)),
            }],
            style: TableStyle {
                source_border_fill_id: Some(9),
                diagonal: Some(BorderFillDiagonal {
                    raw_attributes: 8,
                    diagonal_type: 1,
                    width_index: 2,
                    color: None,
                    raw_color: 0,
                }),
                fill: Some(FillStyle::Solid {
                    background_color: Some(Color {
                        r: 0xEE,
                        g: 0xFF,
                        b: 0xEE,
                        a: 255,
                    }),
                    background_color_raw: 0x00EEFFEE,
                    pattern_color: None,
                    pattern_color_raw: 0,
                    pattern_type: 0,
                    alpha: 0,
                }),
                border_left: Some(Border {
                    width: LengthPx(1.0),
                    style: BorderStyle::Solid,
                    color: Some(Color {
                        r: 0x22,
                        g: 0x33,
                        b: 0x44,
                        a: 255,
                    }),
                }),
                cell_spacing: Some(LengthPx(3.0)),
                repeat_header: true,
                page_break: Some(crate::ir::TablePageBreak::Row),
                ..Default::default()
            },
            ..Default::default()
        })]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("<th"));
        assert!(html.contains("<table data-border-fill-id=\"9\""));
        assert!(html.contains("data-diagonal-attributes=\"8\""));
        assert!(html.contains("background-color: #eeffee"));
        assert!(html.contains("border-left: 1px solid #223344"));
        assert!(html.contains("vertical-align: middle"));
        assert!(html.contains("data-text-direction=\"2\""));
        assert!(html.contains("data-list-header-width-ref=\"6\""));
        assert!(html.contains("data-cell-protected=\"true\""));
        assert!(html.contains("writing-mode: vertical-rl"));
        assert!(html.contains("text-orientation: upright"));
        assert!(html.contains("width: 100px"));
        assert!(html.contains("height: 20px"));
        assert!(html.contains("padding-left: 2px"));
        assert!(html.contains("border-top: 2px solid #112233"));
        assert!(html.contains("<tr style=\"height: 36px\">"));
        assert!(html.contains("<thead>\n<tr style=\"height: 36px\">"));
        assert!(html.contains("break-inside: avoid"));
        assert!(html.contains("border-collapse: separate"));
        assert!(html.contains("border-spacing: 3px"));
        assert!(!html.contains("<td"));
    }

    #[test]
    fn renders_table_cell_gradient_and_image_fills() {
        let image_id = ResourceId("fill.png".to_string());
        let mut document = document_with_blocks(vec![Block::Table(Table {
            rows: vec![TableRow {
                cells: vec![
                    TableCell {
                        blocks: vec![Block::Paragraph(Paragraph::from_plain_text(
                            "gradient".to_string(),
                        ))],
                        style: TableCellStyle {
                            fill: Some(FillStyle::Gradient {
                                gradient_type: 1,
                                angle: 45,
                                center_x: 50,
                                center_y: 50,
                                blur: 0,
                                colors: vec![
                                    crate::ir::GradientColor {
                                        color: Some(Color {
                                            r: 255,
                                            g: 0,
                                            b: 0,
                                            a: 255,
                                        }),
                                        raw: 0x000000FF,
                                    },
                                    crate::ir::GradientColor {
                                        color: Some(Color {
                                            r: 0,
                                            g: 0,
                                            b: 255,
                                            a: 255,
                                        }),
                                        raw: 0x00FF0000,
                                    },
                                ],
                                positions: vec![0, 100],
                                alpha: 128,
                            }),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    TableCell {
                        blocks: vec![Block::Paragraph(Paragraph::from_plain_text(
                            "image".to_string(),
                        ))],
                        style: TableCellStyle {
                            fill: Some(FillStyle::Image {
                                mode: ImageFillMode::TileHorizontalBottom,
                                brightness: 0,
                                contrast: 0,
                                effect: 0,
                                source_bin_data_id: 1,
                                resource_id: Some(image_id.clone()),
                                alpha: 255,
                            }),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                ],
                height: None,
            }],
            ..Default::default()
        })]);
        document
            .resources
            .entries
            .push(Resource::Image(ImageResource {
                id: image_id,
                media_type: Some("image/png".to_string()),
                extension: Some("png".to_string()),
                bytes: vec![137, 80, 78, 71],
            }));

        let html = render_html_document(Path::new("sample.hwp"), &document);

        assert!(html.contains("linear-gradient(45deg"));
        assert!(html.contains("rgba(255, 0, 0"));
        assert!(html.contains(" 0%"));
        assert!(html.contains(" 100%"));
        assert!(html.contains("background-image: url(&apos;sample_assets/images/fill.png&apos;)"));
        assert!(html.contains("background-repeat: repeat-x"));
        assert!(html.contains("background-position: left bottom"));
    }

    #[test]
    fn applies_table_zone_fill_without_overriding_cell_fill() {
        let zone_fill = FillStyle::Solid {
            background_color: Some(Color {
                r: 0,
                g: 255,
                b: 0,
                a: 255,
            }),
            background_color_raw: 0x0000FF00,
            pattern_color: None,
            pattern_color_raw: 0xFFFFFFFF,
            pattern_type: 0,
            alpha: 255,
        };
        let cell_fill = FillStyle::Solid {
            background_color: Some(Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            }),
            background_color_raw: 0x000000FF,
            pattern_color: None,
            pattern_color_raw: 0xFFFFFFFF,
            pattern_type: 0,
            alpha: 255,
        };
        let document = document_with_blocks(vec![Block::Table(Table {
            rows: vec![TableRow {
                cells: vec![
                    TableCell {
                        source_row: Some(0),
                        source_column: Some(0),
                        blocks: vec![Block::Paragraph(Paragraph::from_plain_text(
                            "zone".to_string(),
                        ))],
                        ..Default::default()
                    },
                    TableCell {
                        source_row: Some(0),
                        source_column: Some(1),
                        blocks: vec![Block::Paragraph(Paragraph::from_plain_text(
                            "cell".to_string(),
                        ))],
                        style: TableCellStyle {
                            fill: Some(cell_fill),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                ],
                height: None,
            }],
            zones: vec![TableZone {
                start_row: 0,
                start_column: 0,
                end_row: 0,
                end_column: 1,
                source_border_fill_id: 3,
                fill: Some(zone_fill),
                ..Default::default()
            }],
            ..Default::default()
        })]);

        let html = render_html_document(Path::new("sample.hwp"), &document);

        assert_eq!(html.matches("background-color: #00ff00").count(), 1);
        assert_eq!(html.matches("background-color: #ff0000").count(), 1);
    }

    #[test]
    fn renders_html_image_border_and_grayscale() {
        let document = document_with_blocks(vec![Block::Image(Image {
            resource_id: ResourceId("img-1".to_string()),
            border: Some(Border {
                width: LengthPx(2.0),
                style: BorderStyle::Solid,
                color: Some(Color {
                    r: 17,
                    g: 34,
                    b: 51,
                    a: 255,
                }),
            }),
            grayscale: true,
            opacity: Some(0.5),
            rotation_degrees: Some(90.0),
            flip_horizontal: Some(true),
            flip_vertical: Some(true),
            padding_top: Some(LengthPx(1.0)),
            padding_right: Some(LengthPx(2.0)),
            padding_bottom: Some(LengthPx(3.0)),
            padding_left: Some(LengthPx(4.0)),
            ..Default::default()
        })]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("border: 2px solid #112233"));
        assert!(html.contains("filter: grayscale(100%)"));
        assert!(html.contains("opacity: 0.5"));
        assert!(html.contains("transform: rotate(90deg) scaleX(-1) scaleY(-1)"));
        assert!(html.contains("padding-top: 1px"));
        assert!(html.contains("padding-right: 2px"));
        assert!(html.contains("padding-bottom: 3px"));
        assert!(html.contains("padding-left: 4px"));
    }

    #[test]
    fn renders_html_shape_simple_border_and_fill() {
        let document = document_with_blocks(vec![Block::Shape(Shape {
            kind: ShapeKind::Rectangle,
            fallback_text: Some("note".to_string()),
            border: Some(Border {
                width: LengthPx(2.0),
                style: BorderStyle::Dotted,
                color: Some(Color {
                    r: 17,
                    g: 34,
                    b: 51,
                    a: 255,
                }),
            }),
            background_color: Some(Color {
                r: 68,
                g: 85,
                b: 102,
                a: 255,
            }),
            shadow: Some(ShapeShadow {
                kind: 4,
                color: Some(Color {
                    r: 17,
                    g: 34,
                    b: 51,
                    a: 255,
                }),
                raw_color: 0x00332211,
                offset_x: LengthPx(2.0),
                offset_y: LengthPx(-1.0),
                transparency: 64,
            }),
            rotation_degrees: Some(90.0),
            flip_horizontal: Some(true),
            flip_vertical: Some(true),
            text_vertical_align: Some(crate::ir::VerticalAlign::Middle),
            padding_top: Some(LengthPx(1.0)),
            padding_right: Some(LengthPx(2.0)),
            padding_bottom: Some(LengthPx(3.0)),
            padding_left: Some(LengthPx(4.0)),
            geometry: Some(ShapeGeometry::Rectangle {
                corners: Vec::new(),
                round_rate_percent: 25,
            }),
            content: vec![Block::Paragraph(Paragraph::from_plain_text(
                "inside shape".to_string(),
            ))],
            ..Default::default()
        })]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);
        let markdown = render_markdown_document(&document);
        let text = plain_text::to_plain_text(&document);

        assert!(html.contains("background-color: #445566"));
        assert!(html.contains("border: 2px dotted #112233"));
        assert!(html.contains("box-shadow: 2px -1px rgba(17, 34, 51,"));
        assert!(html.contains("transform: rotate(90deg) scaleX(-1) scaleY(-1)"));
        assert!(html.contains("display: inline-block"));
        assert!(html.contains("display: inline-flex"));
        assert!(html.contains("padding-top: 1px"));
        assert!(html.contains("padding-right: 2px"));
        assert!(html.contains("padding-bottom: 3px"));
        assert!(html.contains("padding-left: 4px"));
        assert!(html.contains("justify-content: center"));
        assert!(html.contains("border-radius: 25%"));
        assert!(html.contains("class=\"shape-placeholder shape-content\""));
        assert!(html.contains("inside shape"));
        assert!(markdown.contains("inside shape"));
        assert_eq!(text, "inside shape");
    }

    #[test]
    fn renders_html_shape_gradient_fill() {
        let document = document_with_blocks(vec![Block::Shape(Shape {
            kind: ShapeKind::Ellipse,
            fallback_text: Some("gradient shape".to_string()),
            fill: Some(FillStyle::Gradient {
                gradient_type: 2,
                angle: 0,
                center_x: 25,
                center_y: 75,
                blur: 0,
                colors: vec![
                    crate::ir::GradientColor {
                        color: Some(Color {
                            r: 255,
                            g: 255,
                            b: 255,
                            a: 255,
                        }),
                        raw: 0x00FFFFFF,
                    },
                    crate::ir::GradientColor {
                        color: Some(Color {
                            r: 0,
                            g: 0,
                            b: 0,
                            a: 255,
                        }),
                        raw: 0,
                    },
                ],
                positions: vec![0, 100],
                alpha: 255,
            }),
            ..Default::default()
        })]);

        let html = render_html_document(Path::new("sample.hwp"), &document);

        assert!(html.contains("radial-gradient(at 25% 75%, #ffffff 0%, #000000 100%)"));
    }

    #[test]
    fn renders_structured_shape_group_children_across_exporters() {
        let document = document_with_blocks(vec![Block::Shape(Shape {
            kind: ShapeKind::Group,
            fallback_text: Some("group".to_string()),
            width: Some(LengthPx(120.0)),
            children: vec![
                Block::Shape(Shape {
                    kind: ShapeKind::Rectangle,
                    fallback_text: Some("inner shape".to_string()),
                    ..Default::default()
                }),
                Block::Unknown(UnknownBlock {
                    kind: "group_child".to_string(),
                    fallback_text: Some("inner fallback".to_string()),
                    message: Some("nested group fallback preserved".to_string()),
                    source: Some("rhwp".to_string()),
                }),
            ],
            ..Default::default()
        })]);

        let html = render_html_document(Path::new("sample.hwp"), &document);
        let markdown = render_markdown_document(&document);
        let text = plain_text::to_plain_text(&document);
        let warnings = conversion_warnings_for_document(&document);

        assert!(html.contains("class=\"shape-group\""));
        assert!(html.contains("width: 120px"));
        assert!(html.contains("inner shape"));
        assert!(html.contains("inner fallback"));
        assert!(markdown.contains("inner shape"));
        assert!(markdown.contains("inner fallback"));
        assert!(text.contains("[shape: inner shape]"));
        assert!(text.contains("inner fallback"));
        assert!(warnings.iter().any(|warning| {
            warning.contains("IR unknown block `group_child`: nested group fallback preserved")
        }));
    }

    #[test]
    fn renders_html_text_style_visual_properties() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![Inline::Text(TextRun {
                text: "styled".to_string(),
                style: TextStyle {
                    font_family: Some("Noto Sans KR, Malgun Gothic; color:red".to_string()),
                    font_fallback: Some(Box::new(crate::ir::FontFallback {
                        alternate_type: Some(1),
                        alternate_family: Some("Arial".to_string()),
                        default_family: Some("Noto Sans KR".to_string()),
                    })),
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

        assert!(html.contains("font-family: Noto Sans KR, Malgun Gothic colorred, Arial"));
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
                    line_percent: None,
                },
                indent: Indent {
                    left_pt: Some(LengthPt(10.0)),
                    right_pt: Some(LengthPt(12.0)),
                    first_line_pt: Some(LengthPt(18.0)),
                },
                ..Default::default()
            },
            style_ref: None,
            list: None,
        })]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("text-align: center"));
        assert!(html.contains("margin-top: 6pt"));
        assert!(html.contains("margin-bottom: 8pt"));
        assert!(html.contains("line-height: 14pt"));
        assert!(html.contains("text-indent: 18pt"));
        assert!(html.contains("margin-left: 10pt"));
        assert!(html.contains("margin-right: 12pt"));
    }

    #[test]
    fn renders_html_paragraph_border_background_and_padding() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![Inline::Text(TextRun {
                text: "framed paragraph".to_string(),
                style: TextStyle::default(),
                style_ref: None,
            })],
            style: ParagraphStyle {
                background_color: Some(Color {
                    r: 17,
                    g: 34,
                    b: 51,
                    a: 255,
                }),
                padding_top_pt: Some(LengthPt(1.0)),
                padding_right_pt: Some(LengthPt(2.0)),
                padding_bottom_pt: Some(LengthPt(3.0)),
                padding_left_pt: Some(LengthPt(4.0)),
                border_top: Some(Border {
                    width: LengthPx(2.0),
                    style: BorderStyle::Dashed,
                    color: Some(Color {
                        r: 68,
                        g: 85,
                        b: 102,
                        a: 255,
                    }),
                }),
                widow_orphan: true,
                keep_with_next: true,
                keep_lines: true,
                page_break_before: true,
                ..Default::default()
            },
            style_ref: None,
            list: None,
        })]);

        let html = render_html_document(Path::new("sample.hwp"), &document);

        assert!(html.contains("background-color: #112233"));
        assert!(html.contains("padding-top: 1pt"));
        assert!(html.contains("padding-right: 2pt"));
        assert!(html.contains("padding-bottom: 3pt"));
        assert!(html.contains("padding-left: 4pt"));
        assert!(html.contains("border-top: 2px dashed #445566"));
        assert!(html.contains("orphans: 2"));
        assert!(html.contains("widows: 2"));
        assert!(html.contains("break-after: avoid-page"));
        assert!(html.contains("break-inside: avoid"));
        assert!(html.contains("break-before: page"));
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
    fn renders_markdown_superscript_and_subscript() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![
                Inline::Text(TextRun {
                    text: "2".to_string(),
                    style: TextStyle {
                        superscript: true,
                        ..Default::default()
                    },
                    style_ref: None,
                }),
                Inline::Text(TextRun {
                    text: "n".to_string(),
                    style: TextStyle {
                        subscript: true,
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

        assert!(markdown.contains("<sup>2</sup>"));
        assert!(markdown.contains("<sub>n</sub>"));
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
    fn renders_empty_link_label_with_visible_fallback() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![Inline::Link(Link {
                url: "https://example.com/empty".to_string(),
                title: None,
                inlines: Vec::new(),
            })],
            style: ParagraphStyle::default(),
            style_ref: None,
            list: None,
        })]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);
        let markdown = render_markdown_document(&document);

        assert!(
            html.contains("<a href=\"https://example.com/empty\">https://example.com/empty</a>")
        );
        assert!(markdown.contains("[https://example.com/empty](https://example.com/empty)"));
    }

    #[test]
    fn escapes_markdown_note_ref_inside_link_labels() {
        let document = document_with_notes(
            vec![Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![Inline::Link(Link {
                    url: "https://example.com/note".to_string(),
                    title: None,
                    inlines: vec![
                        Inline::Text(TextRun {
                            text: "See note ".to_string(),
                            style: TextStyle::default(),
                            style_ref: None,
                        }),
                        Inline::FootnoteRef {
                            note_id: NoteId("fn 1/가".to_string()),
                        },
                    ],
                })],
                style: ParagraphStyle::default(),
                style_ref: None,
                list: None,
            })],
            vec![Note {
                id: NoteId("fn 1/가".to_string()),
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

        let markdown = render_markdown_document(&document);

        assert!(markdown.contains("[See note [^fn-1--]](https://example.com/note)"));
        assert!(markdown.contains("[^fn-1--]: note body"));
    }

    #[test]
    fn escapes_markdown_link_title_quotes_and_backslashes() {
        let document = document_with_blocks(vec![Block::Paragraph(Paragraph {
            role: ParagraphRole::Body,
            inlines: vec![Inline::Link(Link {
                url: "https://example.com/docs".to_string(),
                title: Some(r#"quoted "title" \ path"#.to_string()),
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

        assert!(
            markdown.contains(r#"[Docs](https://example.com/docs "quoted \"title\" \\ path")"#)
        );
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
                    marker_format: None,
                    number: None,
                    ..Default::default()
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
                    marker_format: None,
                    number: Some(2),
                    ..Default::default()
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
                master_pages: vec![MasterPage {
                    placement: HeaderFooterPlacement::OddPage,
                    is_extension: true,
                    overlap: true,
                    raw_extension_flags: 0x1234,
                    text_width: LengthPx(100.0),
                    text_height: LengthPx(20.0),
                    text_reference_mask: 3,
                    number_reference_mask: 5,
                    raw_list_header: vec![7, 8, 9],
                    blocks: vec![Block::Paragraph(Paragraph {
                        role: ParagraphRole::Body,
                        inlines: vec![Inline::Text(TextRun {
                            text: "master".to_string(),
                            style: TextStyle::default(),
                            style_ref: None,
                        })],
                        style: ParagraphStyle::default(),
                        style_ref: None,
                        list: None,
                    })],
                }],
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
                layout: None,
            }],
            resources: ResourceStore::default(),
            styles: StyleSheet::default(),
            notes: NoteStore::default(),
            warnings: Vec::new(),
        };

        let html = render_html_document(Path::new("sample.hwpx"), &document);
        let markdown = render_markdown_document(&document);

        assert!(html.contains("<section class=\"master-page\" data-placement=\"odd_page\" data-extension=\"true\" data-overlap=\"true\" data-extension-flags=\"4660\" data-text-width-px=\"100\" data-text-height-px=\"20\" data-text-reference-mask=\"3\" data-number-reference-mask=\"5\"><p>master</p>"));
        assert!(html.contains("<header data-placement=\"default\"><p>header</p>"));
        assert!(html.contains("<footer data-placement=\"even_page\"><p>footer</p>"));
        assert!(markdown.contains("[바탕쪽]\n\nmaster"));
    }

    #[test]
    fn renders_markdown_latex_equation_from_document_ir() {
        let document = document_with_blocks(vec![Block::Equation(Equation {
            kind: EquationKind::Latex,
            content: Some(r"\frac{a}{b}".to_string()),
            fallback_text: None,
            resource_id: None,
            ..Default::default()
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
                ..Default::default()
            }),
            Block::Shape(Shape {
                kind: ShapeKind::Rectangle,
                fallback_text: None,
                description: Some("callout box".to_string()),
                ..Default::default()
            }),
            Block::Chart(Chart {
                title: Some("Sales".to_string()),
                fallback_text: None,
                resource_id: None,
            }),
        ]);

        let html = render_html_document(Path::new("sample.hwpx"), &document);

        assert!(html.contains("<span class=\"equation\">[equation: x + y]</span>"));
        assert!(html.contains("<span class=\"shape-placeholder\" style=\"display: inline-block\">[shape: callout box]</span>"));
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
                warnings: vec![
                    "Used HWPX preview fallback. Preview/PrvText.txt only recovers plain text."
                        .to_string(),
                ],
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
        assert!(content.contains("\"warning_count\": 1"));
        assert!(content.contains("Used HWPX preview fallback"));

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
                    ..Default::default()
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
                    height: None,
                },
                TableRow {
                    cells: vec![table_cell("cell3"), table_cell("cell4")],
                    height: None,
                },
            ],
            style: TableStyle::default(),
            ..Default::default()
        })
    }

    fn table_cell(text: &str) -> TableCell {
        TableCell {
            row_span: 1,
            col_span: 1,
            is_header: false,
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
            ..Default::default()
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
