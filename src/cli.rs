use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Txt,
    Svg,
    Json,
    Html,
    Markdown,
}

impl OutputFormat {
    pub fn parse(value: &str) -> Result<Self, io::Error> {
        match value {
            "txt" => Ok(Self::Txt),
            "svg" => Ok(Self::Svg),
            "json" => Ok(Self::Json),
            "html" => Ok(Self::Html),
            "markdown" | "md" => Ok(Self::Markdown),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "지원하지 않는 출력 형식입니다: {value}. 지원 형식: txt, svg, json, html, markdown"
                ),
            )),
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Svg => "svg",
            Self::Json => "json",
            Self::Html => "html",
            Self::Markdown => "md",
        }
    }
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.extension())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliArgs {
    pub input_path: PathBuf,
    pub format: OutputFormat,
    pub recursive: bool,
    pub manifest_path: Option<PathBuf>,
    pub continue_on_error: bool,
}

pub fn parse_args<I, T>(args: I) -> Result<Option<CliArgs>, io::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut args = args.into_iter().map(Into::into);
    let _program_name = args.next();

    let Some(first) = args.next() else {
        return Ok(None);
    };

    let first_text = first.to_string_lossy();
    if first_text == "--help" || first_text == "-h" {
        return Ok(None);
    }

    let input_path = PathBuf::from(first);
    let mut format = None;
    let mut recursive = false;
    let mut manifest_path = None;
    let mut continue_on_error = false;

    while let Some(arg) = args.next() {
        match arg.to_string_lossy().as_ref() {
            "--to" => {
                if format.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "--to는 한 번만 사용할 수 있습니다.",
                    ));
                }

                let Some(value) = args.next() else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "출력 형식을 지정해야 합니다. 예: hwp-convert sample.hwpx --to svg",
                    ));
                };

                format = Some(OutputFormat::parse(&value.to_string_lossy())?);
            }
            "--recursive" => {
                if recursive {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "--recursive는 한 번만 사용할 수 있습니다.",
                    ));
                }

                recursive = true;
            }
            "--manifest" => {
                if manifest_path.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "--manifest는 한 번만 사용할 수 있습니다.",
                    ));
                }

                let Some(value) = args.next() else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "manifest 파일 경로를 지정해야 합니다. 예: --manifest manifest.json",
                    ));
                };

                manifest_path = Some(PathBuf::from(value));
            }
            "--continue-on-error" => {
                if continue_on_error {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "--continue-on-error는 한 번만 사용할 수 있습니다.",
                    ));
                }

                continue_on_error = true;
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "알 수 없는 인자입니다. 예: hwp-convert sample.hwpx --to svg [--recursive] [--manifest manifest.json] [--continue-on-error]",
                ));
            }
        }
    }

    let Some(format) = format else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "출력 형식을 지정해야 합니다. 예: hwp-convert sample.hwpx --to svg",
        ));
    };

    Ok(Some(CliArgs {
        input_path,
        format,
        recursive,
        manifest_path,
        continue_on_error,
    }))
}

pub fn print_usage() {
    println!("사용법");
    println!(
        "  hwp-convert <입력 파일> --to <출력 형식> [--manifest <manifest.json>] [--continue-on-error]"
    );
    println!(
        "  hwp-convert <입력 디렉토리> --to <출력 형식> --recursive [--manifest <manifest.json>] [--continue-on-error]"
    );
    println!();
    println!("지원 형식");
    println!("  txt");
    println!("  svg");
    println!("  json");
    println!("  html");
    println!("  markdown");
    println!();
    println!("예시");
    println!("  hwp-convert sample.hwpx --to txt");
    println!("  hwp-convert sample.hwpx --to svg --manifest manifest.json");
    println!("  hwp-convert ./documents --to svg --recursive --continue-on-error");
    println!(
        "  hwp-convert ./documents --to json --recursive --manifest manifest.json --continue-on-error"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_txt_export_arguments() {
        let args = parse_args(["hwp-convert", "sample.hwpx", "--to", "txt"]).unwrap();

        assert_eq!(
            args,
            Some(CliArgs {
                input_path: PathBuf::from("sample.hwpx"),
                format: OutputFormat::Txt,
                recursive: false,
                manifest_path: None,
                continue_on_error: false,
            })
        );
    }

    #[test]
    fn parses_svg_export_arguments() {
        let args = parse_args(["hwp-convert", "sample.hwpx", "--to", "svg"]).unwrap();

        assert_eq!(
            args,
            Some(CliArgs {
                input_path: PathBuf::from("sample.hwpx"),
                format: OutputFormat::Svg,
                recursive: false,
                manifest_path: None,
                continue_on_error: false,
            })
        );
    }

    #[test]
    fn parses_json_export_arguments() {
        let args = parse_args(["hwp-convert", "sample.hwpx", "--to", "json"]).unwrap();

        assert_eq!(
            args,
            Some(CliArgs {
                input_path: PathBuf::from("sample.hwpx"),
                format: OutputFormat::Json,
                recursive: false,
                manifest_path: None,
                continue_on_error: false,
            })
        );
    }

    #[test]
    fn parses_html_export_arguments() {
        let args = parse_args(["hwp-convert", "sample.hwpx", "--to", "html"]).unwrap();

        assert_eq!(
            args,
            Some(CliArgs {
                input_path: PathBuf::from("sample.hwpx"),
                format: OutputFormat::Html,
                recursive: false,
                manifest_path: None,
                continue_on_error: false,
            })
        );
    }

    #[test]
    fn parses_markdown_export_arguments() {
        let args = parse_args(["hwp-convert", "sample.hwpx", "--to", "markdown"]).unwrap();

        assert_eq!(
            args,
            Some(CliArgs {
                input_path: PathBuf::from("sample.hwpx"),
                format: OutputFormat::Markdown,
                recursive: false,
                manifest_path: None,
                continue_on_error: false,
            })
        );
    }

    #[test]
    fn parses_recursive_directory_arguments() {
        let args = parse_args(["hwp-convert", "documents", "--to", "svg", "--recursive"]).unwrap();

        assert_eq!(
            args,
            Some(CliArgs {
                input_path: PathBuf::from("documents"),
                format: OutputFormat::Svg,
                recursive: true,
                manifest_path: None,
                continue_on_error: false,
            })
        );
    }

    #[test]
    fn parses_manifest_arguments() {
        let args = parse_args([
            "hwp-convert",
            "documents",
            "--to",
            "svg",
            "--recursive",
            "--manifest",
            "manifest.json",
        ])
        .unwrap();

        assert_eq!(
            args,
            Some(CliArgs {
                input_path: PathBuf::from("documents"),
                format: OutputFormat::Svg,
                recursive: true,
                manifest_path: Some(PathBuf::from("manifest.json")),
                continue_on_error: false,
            })
        );
    }

    #[test]
    fn parses_continue_on_error_arguments() {
        let args = parse_args([
            "hwp-convert",
            "documents",
            "--to",
            "svg",
            "--recursive",
            "--continue-on-error",
        ])
        .unwrap();

        assert_eq!(
            args,
            Some(CliArgs {
                input_path: PathBuf::from("documents"),
                format: OutputFormat::Svg,
                recursive: true,
                manifest_path: None,
                continue_on_error: true,
            })
        );
    }

    #[test]
    fn returns_none_for_help() {
        let args = parse_args(["hwp-convert", "--help"]).unwrap();

        assert_eq!(args, None);
    }

    #[test]
    fn rejects_unknown_format() {
        let error = parse_args(["hwp-convert", "sample.hwpx", "--to", "pdf"]).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn rejects_missing_to_value() {
        let error = parse_args(["hwp-convert", "sample.hwpx", "--to"]).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn rejects_missing_manifest_value() {
        let error =
            parse_args(["hwp-convert", "sample.hwpx", "--to", "txt", "--manifest"]).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn rejects_unknown_flag() {
        let error =
            parse_args(["hwp-convert", "sample.hwpx", "--to", "txt", "--verbose"]).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
