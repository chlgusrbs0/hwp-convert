use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::cli::{CliArgs, OutputFormat};
use crate::hwpx;

pub fn export(args: &CliArgs) -> Result<PathBuf, Box<dyn Error>> {
    validate_input_path(&args.input_path)?;

    let output_path = create_output_path(&args.input_path, args.format);

    match args.format {
        OutputFormat::Txt => {
            let document_text = hwpx::read_preview_text(&args.input_path)?;
            write_txt_output(&output_path, &document_text)?;
        }
        OutputFormat::Svg => {
            let paragraphs = hwpx::read_paragraphs(&args.input_path)?;
            write_svg_output(&args.input_path, &output_path, &paragraphs)?;
        }
    }

    Ok(output_path)
}

fn validate_input_path(input_path: &Path) -> Result<(), io::Error> {
    if !input_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("입력 파일을 찾을 수 없습니다: {}", input_path.display()),
        ));
    }

    if !input_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("입력 경로가 파일이 아닙니다: {}", input_path.display()),
        ));
    }

    if !has_hwpx_extension(input_path) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "현재 버전은 .hwpx 파일만 지원합니다.",
        ));
    }

    Ok(())
}

fn has_hwpx_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("hwpx"))
}

fn create_output_path(input_path: &Path, format: OutputFormat) -> PathBuf {
    input_path.with_extension(format.extension())
}

fn write_txt_output(output_path: &Path, preview_text: &str) -> Result<(), io::Error> {
    fs::write(output_path, preview_text)
}

fn write_svg_output(
    input_path: &Path,
    output_path: &Path,
    paragraphs: &[String],
) -> Result<(), io::Error> {
    let svg = render_svg_document(input_path, paragraphs);
    fs::write(output_path, svg)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_hwpx_extension() {
        assert!(!has_hwpx_extension(Path::new("sample.txt")));
        assert!(has_hwpx_extension(Path::new("sample.hwpx")));
        assert!(has_hwpx_extension(Path::new("sample.HWPX")));
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
}
