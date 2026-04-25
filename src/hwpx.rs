use std::error::Error;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::Path;

use zip::ZipArchive;

const PREVIEW_TEXT_PATH: &str = "Preview/PrvText.txt";

pub fn read_preview_text(input_path: &Path) -> Result<String, Box<dyn Error>> {
    let bytes = fs::read(input_path)?;

    match read_preview_text_with_rhwp(&bytes) {
        Ok(Some(text)) => Ok(text),
        Ok(None) | Err(_) => read_preview_text_from_archive(&bytes),
    }
}

fn read_preview_text_with_rhwp(bytes: &[u8]) -> Result<Option<String>, Box<dyn Error>> {
    let document = rhwp::parse_document(bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to parse document with rhwp: {error}"),
        )
    })?;

    Ok(document
        .preview
        .and_then(|preview| preview.text)
        .map(|text| normalize_newlines(&text)))
}

fn read_preview_text_from_archive(bytes: &[u8]) -> Result<String, Box<dyn Error>> {
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

    Ok(normalize_newlines(&preview_text))
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    #[test]
    fn falls_back_to_preview_archive_entry() -> Result<(), Box<dyn Error>> {
        let path = temp_fixture_path("preview-text");
        let file = File::create(&path)?;
        let mut writer = ZipWriter::new(file);

        writer.start_file(PREVIEW_TEXT_PATH, SimpleFileOptions::default())?;
        writer.write_all(b"first line\r\nsecond line")?;
        writer.finish()?;

        let preview_text = read_preview_text(&path)?;
        fs::remove_file(&path)?;

        assert_eq!(preview_text, "first line\nsecond line");

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
