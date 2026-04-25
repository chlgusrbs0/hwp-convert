use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cli::{CliArgs, OutputFormat};
use crate::hwpx;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedFile {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    converted_files: Vec<ExportedFile>,
}

impl ExportReport {
    pub fn converted_files(&self) -> &[ExportedFile] {
        &self.converted_files
    }
}

pub fn write_manifest(
    manifest_path: &Path,
    args: &CliArgs,
    report: &ExportReport,
) -> Result<(), Box<dyn Error>> {
    let manifest = ManifestExport {
        input_path: args.input_path.display().to_string(),
        format: args.format.to_string(),
        recursive: args.recursive,
        converted_count: report.converted_files.len(),
        files: report
            .converted_files
            .iter()
            .map(|file| ManifestFileEntry {
                input_path: file.input_path.display().to_string(),
                output_path: file.output_path.display().to_string(),
                status: "success",
            })
            .collect(),
    };

    let content = serde_json::to_string_pretty(&manifest).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to serialize manifest output: {error}"),
        )
    })?;

    fs::write(manifest_path, content)?;

    Ok(())
}

pub fn export(args: &CliArgs) -> Result<ExportReport, Box<dyn Error>> {
    validate_input_path(&args.input_path, args.recursive)?;

    if args.input_path.is_dir() {
        export_directory_recursively(&args.input_path, args.format)
    } else {
        let exported_file = export_file(&args.input_path, args.format)?;
        Ok(ExportReport {
            converted_files: vec![exported_file],
        })
    }
}

fn export_directory_recursively(
    input_dir: &Path,
    format: OutputFormat,
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
    for input_path in input_files {
        converted_files.push(export_file(&input_path, format)?);
    }

    Ok(ExportReport { converted_files })
}

fn export_file(input_path: &Path, format: OutputFormat) -> Result<ExportedFile, Box<dyn Error>> {
    validate_supported_file(input_path)?;

    let output_path = create_output_path(input_path, format);

    match format {
        OutputFormat::Txt => {
            let document_text = hwpx::read_preview_text(input_path)?;
            write_txt_output(&output_path, &document_text)?;
        }
        OutputFormat::Svg => {
            let paragraphs = hwpx::read_paragraphs(input_path)?;
            write_svg_output(input_path, &output_path, &paragraphs)?;
        }
        OutputFormat::Json => {
            let paragraphs = hwpx::read_paragraphs(input_path)?;
            write_json_output(input_path, &output_path, &paragraphs)?;
        }
        OutputFormat::Html => {
            let paragraphs = hwpx::read_paragraphs(input_path)?;
            write_html_output(input_path, &output_path, &paragraphs)?;
        }
        OutputFormat::Markdown => {
            let paragraphs = hwpx::read_paragraphs(input_path)?;
            write_markdown_output(&output_path, &paragraphs)?;
        }
    }

    Ok(ExportedFile {
        input_path: input_path.to_path_buf(),
        output_path,
    })
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

fn create_output_path(input_path: &Path, format: OutputFormat) -> PathBuf {
    input_path.with_extension(format.extension())
}

fn write_txt_output(output_path: &Path, document_text: &str) -> Result<(), io::Error> {
    fs::write(output_path, document_text)
}

fn write_svg_output(
    input_path: &Path,
    output_path: &Path,
    paragraphs: &[String],
) -> Result<(), io::Error> {
    let svg = render_svg_document(input_path, paragraphs);
    fs::write(output_path, svg)
}

fn write_json_output(
    input_path: &Path,
    output_path: &Path,
    paragraphs: &[String],
) -> Result<(), io::Error> {
    let file_name = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document");
    let json = JsonExport {
        input_file: file_name,
        paragraph_count: paragraphs.len(),
        paragraphs,
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
    paragraphs: &[String],
) -> Result<(), io::Error> {
    let html = render_html_document(input_path, paragraphs);
    fs::write(output_path, html)
}

fn write_markdown_output(output_path: &Path, paragraphs: &[String]) -> Result<(), io::Error> {
    let markdown = render_markdown_document(paragraphs);
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
    converted_count: usize,
    files: Vec<ManifestFileEntry>,
}

#[derive(Serialize)]
struct ManifestFileEntry {
    input_path: String,
    output_path: String,
    status: &'static str,
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
        };

        let report = export(&args)?;

        assert_eq!(report.converted_files().len(), 2);
        assert_eq!(fs::read_to_string(root.join("alpha.txt"))?, "first line");
        assert_eq!(
            fs::read_to_string(root.join("nested").join("beta.txt"))?,
            "second line"
        );

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
    fn writes_manifest_with_converted_files() -> Result<(), Box<dyn Error>> {
        let root = temp_fixture_dir("manifest");
        fs::create_dir_all(&root)?;
        let manifest_path = root.join("manifest.json");
        let report = ExportReport {
            converted_files: vec![
                ExportedFile {
                    input_path: PathBuf::from("docs/alpha.hwpx"),
                    output_path: PathBuf::from("docs/alpha.svg"),
                },
                ExportedFile {
                    input_path: PathBuf::from("docs/nested/beta.hwp"),
                    output_path: PathBuf::from("docs/nested/beta.svg"),
                },
            ],
        };
        let args = CliArgs {
            input_path: PathBuf::from("docs"),
            format: OutputFormat::Svg,
            recursive: true,
            manifest_path: Some(manifest_path.clone()),
        };

        write_manifest(&manifest_path, &args, &report)?;

        let content = fs::read_to_string(&manifest_path)?;
        assert!(content.contains("\"input_path\": \"docs\""));
        assert!(content.contains("\"format\": \"svg\""));
        assert!(content.contains("\"recursive\": true"));
        assert!(content.contains("\"converted_count\": 2"));
        assert!(content.contains("\"status\": \"success\""));
        assert!(content.contains("docs/alpha.hwpx"));
        assert!(content.contains("docs/nested/beta.svg"));

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
}
