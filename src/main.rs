use std::env;
use std::error::Error;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use zip::ZipArchive;

fn main() {
    if let Err(error) = run() {
        eprintln!("오류: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() == 1 {
        print_usage();
        return Ok(());
    }

    if args.len() != 4 {
        print_usage();
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "인자 형식이 올바르지 않습니다.",
        )
        .into());
    }

    let input_file = &args[1];
    let option = &args[2];
    let format = &args[3];

    if option != "--to" {
        print_usage();
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "두 번째 인자는 --to 여야 합니다.",
        )
        .into());
    }

    if format != "txt" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "현재는 txt 형식만 지원합니다.",
        )
        .into());
    }

    let input_path = PathBuf::from(input_file);

    if !input_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("입력 파일을 찾을 수 없습니다: {}", input_path.display()),
        )
        .into());
    }

    let output_path = create_output_path(&input_path, format);
    let preview_text = read_hwpx_preview_text(&input_path)?;
    write_txt_output(&input_path, &output_path, format, &preview_text)?;

    println!("hwp-convert 실행");
    println!("입력 파일: {}", input_path.display());
    println!("출력 형식: {format}");
    println!("출력 파일: {}", output_path.display());
    println!("변환 완료");

    Ok(())
}

fn create_output_path(input_path: &Path, format: &str) -> PathBuf {
    input_path.with_extension(format)
}

fn read_hwpx_preview_text(input_path: &Path) -> Result<String, Box<dyn Error>> {
    let file = File::open(input_path)?;
    let mut archive = ZipArchive::new(file)?;

    let mut preview_file = archive.by_name("Preview/PrvText.txt")?;
    let mut bytes = Vec::new();

    preview_file.read_to_end(&mut bytes)?;

    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn write_txt_output(
    input_path: &Path,
    output_path: &Path,
    format: &str,
    preview_text: &str,
) -> Result<(), Box<dyn Error>> {
    let input_file_name = input_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| input_path.display().to_string());

    let content = format!(
        "hwp-convert 변환 결과\n입력 파일: {input_file_name}\n출력 형식: {format}\n\n{preview_text}"
    );

    fs::write(output_path, content)?;

    Ok(())
}

fn print_usage() {
    println!("사용법:");
    println!("  hwp-convert <입력 파일> --to <출력 형식>");
    println!();
    println!("예시:");
    println!("  hwp-convert sample.hwpx --to txt");
}