use std::error::Error;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use zip::ZipArchive;

const PREVIEW_TEXT_PATH: &str = "Preview/PrvText.txt";

pub fn read_preview_text(input_path: &Path) -> Result<String, Box<dyn Error>> {
    let file = File::open(input_path)?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("HWPX(zip) 파일을 열 수 없습니다: {error}"),
        )
    })?;

    let mut preview_file = archive.by_name(PREVIEW_TEXT_PATH).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("{PREVIEW_TEXT_PATH} 항목을 찾을 수 없습니다."),
        )
    })?;

    let mut bytes = Vec::new();
    preview_file.read_to_end(&mut bytes)?;

    let preview_text = String::from_utf8(bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("미리보기 텍스트가 UTF-8이 아닙니다: {error}"),
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
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    #[test]
    fn reads_preview_text_from_hwpx_archive() -> Result<(), Box<dyn Error>> {
        let path = temp_fixture_path("preview-text");
        let file = File::create(&path)?;
        let mut writer = ZipWriter::new(file);

        writer.start_file(PREVIEW_TEXT_PATH, SimpleFileOptions::default())?;
        writer.write_all("첫 줄\r\n둘째 줄".as_bytes())?;
        writer.finish()?;

        let preview_text = read_preview_text(&path)?;
        fs::remove_file(&path)?;

        assert_eq!(preview_text, "첫 줄\n둘째 줄");

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
