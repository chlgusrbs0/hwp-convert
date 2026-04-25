use std::error::Error;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::Path;

use zip::ZipArchive;

const PREVIEW_TEXT_PATH: &str = "Preview/PrvText.txt";

pub fn read_preview_text(input_path: &Path) -> Result<String, Box<dyn Error>> {
    let paragraphs = read_paragraphs(input_path)?;
    Ok(paragraphs.join("\n"))
}

pub fn read_paragraphs(input_path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let bytes = fs::read(input_path)?;

    match read_paragraphs_with_rhwp(&bytes) {
        Ok(Some(paragraphs)) => Ok(paragraphs),
        Ok(None) | Err(_) => read_preview_text_from_archive(&bytes),
    }
}

fn read_paragraphs_with_rhwp(bytes: &[u8]) -> Result<Option<Vec<String>>, Box<dyn Error>> {
    let document = rhwp::parse_document(bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to parse document with rhwp: {error}"),
        )
    })?;

    if let Some(paragraphs) = extract_body_paragraphs(&document) {
        return Ok(Some(paragraphs));
    }

    Ok(document
        .preview
        .and_then(|preview| preview.text)
        .map(|text| split_preview_text_to_paragraphs(&text)))
}

fn extract_body_paragraphs(document: &rhwp::model::document::Document) -> Option<Vec<String>> {
    let mut paragraphs = Vec::new();

    for section in &document.sections {
        for paragraph in &section.paragraphs {
            let text = normalize_newlines(&paragraph.text);
            if !text.is_empty() {
                paragraphs.push(text);
            }
        }
    }

    if paragraphs.is_empty() {
        None
    } else {
        Some(paragraphs)
    }
}

fn read_preview_text_from_archive(bytes: &[u8]) -> Result<Vec<String>, Box<dyn Error>> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to open HWPX archive: {error}"),
        )
    })?;

    let mut preview_file = archive.by_name(PREVIEW_TEXT_PATH).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing {PREVIEW_TEXT_PATH} entry"),
        )
    })?;

    let mut preview_bytes = Vec::new();
    preview_file.read_to_end(&mut preview_bytes)?;

    let preview_text = String::from_utf8(preview_bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("preview text is not valid UTF-8: {error}"),
        )
    })?;

    Ok(split_preview_text_to_paragraphs(&preview_text))
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn split_preview_text_to_paragraphs(text: &str) -> Vec<String> {
    normalize_newlines(text)
        .split('\n')
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rhwp::model::document::{Document, Section};
    use rhwp::model::paragraph::Paragraph;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    #[test]
    fn extracts_body_paragraphs_from_rhwp_document() {
        let document = Document {
            sections: vec![
                Section {
                    paragraphs: vec![
                        Paragraph {
                            text: "first paragraph".to_string(),
                            ..Default::default()
                        },
                        Paragraph {
                            text: "second paragraph".to_string(),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                },
                Section {
                    paragraphs: vec![Paragraph {
                        text: "third paragraph".to_string(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let paragraphs = extract_body_paragraphs(&document);

        assert_eq!(
            paragraphs,
            Some(vec![
                "first paragraph".to_string(),
                "second paragraph".to_string(),
                "third paragraph".to_string()
            ])
        );
    }

    #[test]
    fn splits_preview_text_into_paragraphs() {
        let paragraphs = split_preview_text_to_paragraphs("first line\r\n\r\nsecond line\r\n");

        assert_eq!(
            paragraphs,
            vec!["first line".to_string(), "second line".to_string()]
        );
    }

    #[test]
    fn falls_back_to_preview_archive_entry() -> Result<(), Box<dyn Error>> {
        let path = temp_fixture_path("preview-text");
        let file = File::create(&path)?;
        let mut writer = ZipWriter::new(file);

        writer.start_file(PREVIEW_TEXT_PATH, SimpleFileOptions::default())?;
        writer.write_all(b"first line\r\nsecond line")?;
        writer.finish()?;

        let preview_text = read_paragraphs(&path)?;
        fs::remove_file(&path)?;

        assert_eq!(
            preview_text,
            vec!["first line".to_string(), "second line".to_string()]
        );

        Ok(())
    }

    fn temp_fixture_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "hwp-convert-{label}-{}-{nanos}.hwpx",
            std::process::id()
        ))
    }
}
