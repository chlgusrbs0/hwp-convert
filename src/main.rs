use std::error::Error;

use hwp_convert::cli::{parse_args, print_usage};
use hwp_convert::exporter;

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

    let warning_count = report.warning_count();
    if warning_count > 0 {
        println!("변환 경고 수: {warning_count}");
        let mut printed_warning_count = 0usize;
        for file in report.converted_files() {
            for warning in &file.warnings {
                if printed_warning_count >= 5 {
                    break;
                }
                println!("경고: {}: {warning}", file.input_path.display());
                printed_warning_count += 1;
            }
            if printed_warning_count >= 5 {
                break;
            }
        }
        if warning_count > printed_warning_count {
            let remaining = warning_count - printed_warning_count;
            println!(
                "추가 경고 {remaining}개는 --manifest 결과의 warnings 필드에서 확인할 수 있습니다."
            );
        }
    }

    println!("변환 완료");

    Ok(())
}
