mod bridge;
mod cli;
mod exporter;
mod hwpx;
mod ir;
mod render;
mod util;

use std::error::Error;

use cli::{parse_args, print_usage};

fn main() {
    if let Err(error) = run() {
        eprintln!("오류: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = match parse_args(std::env::args_os()) {
        Ok(Some(args)) => args,
        Ok(None) => {
            print_usage();
            return Ok(());
        }
        Err(error) => {
            print_usage();
            return Err(error.into());
        }
    };

    let report = exporter::export(&args)?;

    if let Some(manifest_path) = &args.manifest_path {
        exporter::write_manifest(manifest_path, &args, &report)?;
    }

    println!("hwp-convert");
    println!("입력 경로: {}", args.input_path.display());
    println!("출력 형식: {}", args.format);
    if let Some(output_dir) = &args.output_dir {
        println!("출력 디렉토리: {}", output_dir.display());
    }

    if report.converted_files().len() == 1
        && report.skipped_files().is_empty()
        && report.failed_files().is_empty()
    {
        let exported_file = &report.converted_files()[0];
        println!("출력 파일: {}", exported_file.output_path.display());
    } else if report.skipped_files().len() == 1
        && report.converted_files().is_empty()
        && report.failed_files().is_empty()
    {
        let skipped_file = &report.skipped_files()[0];
        println!("건너뛴 파일: {}", skipped_file.output_path.display());
    } else {
        println!("변환 성공 수: {}", report.converted_files().len());
        println!("건너뛴 수: {}", report.skipped_files().len());
        println!("변환 실패 수: {}", report.failed_files().len());
    }

    if let Some(manifest_path) = &args.manifest_path {
        println!("manifest 파일: {}", manifest_path.display());
    }

    println!("변환 완료");

    Ok(())
}
