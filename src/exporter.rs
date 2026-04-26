use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bridge;
use crate::cli::{CliArgs, OutputFormat};
use crate::ir::Document;
use crate::util::plain_text;

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
            write_json_output(input_path, &output_path, &document)?;
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
    let paragraphs = plain_text::collect_paragraph_texts(document);
    let svg = render_svg_document(input_path, &paragraphs);
    fs::write(output_path, svg)
}

fn write_json_output(
    input_path: &Path,
    output_path: &Path,
    document: &Document,
) -> Result<(), io::Error> {
    let paragraphs = plain_text::collect_paragraph_texts(document);
    let file_name = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document");
    let json = JsonExport {
        input_file: file_name,
        paragraph_count: paragraphs.len(),
        paragraphs: &paragraphs,
        text: paragraphs.join("\n"),
    };

    let content = serde_json::to_string_pretty(&json).map_err(|error| {
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
    let paragraphs = plain_text::collect_paragraph_texts(document);
    let html = render_html_document(input_path, &paragraphs);
    fs::write(output_path, html)
}

fn write_markdown_output(output_path: &Path, document: &Document) -> Result<(), io::Error> {
    let paragraphs = plain_text::collect_paragraph_texts(document);
    let markdown = render_markdown_document(&paragraphs);
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

fn render_html_document(input_path: &Path, paragraphs: &[String]) -> String {
    let file_name = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document");
    let title = escape_html(&format!("{file_name} text export"));

    let mut paragraph_nodes = String::new();
    if paragraphs.is_empty() {
        paragraph_nodes.push_str("    <p></p>\n");
    } else {
        for paragraph in paragraphs {
            let content = if paragraph.is_empty() {
                String::new()
            } else {
                escape_html(paragraph).replace('\n', "<br />")
            };
            paragraph_nodes.push_str(&format!("    <p>{content}</p>\n"));
        }
    }

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
      p {{\n\
        margin: 0 0 1em;\n\
        line-height: 1.8;\n\
        white-space: normal;\n\
      }}\n\
      p:last-child {{\n\
        margin-bottom: 0;\n\
      }}\n\
    </style>\n\
  </head>\n\
  <body>\n\
    <main>\n\
      <h1>{title}</h1>\n\
      <article>\n\
{paragraph_nodes}      </article>\n\
    </main>\n\
  </body>\n\
</html>\n"
    )
}

fn render_markdown_document(paragraphs: &[String]) -> String {
    paragraphs
        .iter()
        .map(|paragraph| {
            paragraph
                .split('\n')
                .map(escape_markdown_line)
                .collect::<Vec<_>>()
                .join("  \n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
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
struct JsonExport<'a> {
    input_file: &'a str,
    paragraph_count: usize,
    paragraphs: &'a [String],
    text: String,
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
    fn serializes_json_output_with_paragraphs() {
        let path = Path::new("sample.hwpx");
        let paragraphs = vec![
            "first paragraph".to_string(),
            "second paragraph".to_string(),
        ];
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("document");
        let json = JsonExport {
            input_file: file_name,
            paragraph_count: paragraphs.len(),
            paragraphs: &paragraphs,
            text: paragraphs.join("\n"),
        };

        let content = serde_json::to_string_pretty(&json).unwrap();

        assert!(content.contains("\"input_file\": \"sample.hwpx\""));
        assert!(content.contains("\"paragraph_count\": 2"));
        assert!(content.contains("\"paragraphs\": ["));
        assert!(content.contains("first paragraph"));
        assert!(content.contains("second paragraph"));
    }

    #[test]
    fn renders_html_with_escaped_paragraphs() {
        let html = render_html_document(
            Path::new("sample.hwpx"),
            &[String::from("& < > \" '"), String::from("second line")],
        );

        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<title>sample.hwpx text export</title>"));
        assert!(html.contains("<p>&amp; &lt; &gt; &quot; &apos;</p>"));
        assert!(html.contains("<p>second line</p>"));
    }

    #[test]
    fn renders_markdown_with_escaped_prefixes() {
        let markdown = render_markdown_document(&[
            String::from("# heading"),
            String::from("1. ordered"),
            String::from("line one\nline two"),
        ]);

        assert!(markdown.contains("\\# heading"));
        assert!(markdown.contains("\\1. ordered"));
        assert!(markdown.contains("line one  \nline two"));
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
