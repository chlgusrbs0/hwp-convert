mod cli;
mod exporter;
mod hwpx;

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

    let output_path = exporter::export(&args)?;

    println!("hwp-convert");
    println!("입력 파일: {}", args.input_path.display());
    println!("출력 형식: {}", args.format);
    println!("출력 파일: {}", output_path.display());
    println!("변환 완료");

    Ok(())
}
