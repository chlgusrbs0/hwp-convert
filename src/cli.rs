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
}

impl OutputFormat {
    pub fn parse(value: &str) -> Result<Self, io::Error> {
        match value {
            "txt" => Ok(Self::Txt),
            "svg" => Ok(Self::Svg),
            "json" => Ok(Self::Json),
            "html" => Ok(Self::Html),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("지원하지 않는 출력 형식입니다: {value}. 지원 형식: txt, svg, json, html"),
            )),
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Svg => "svg",
            Self::Json => "json",
            Self::Html => "html",
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
    let Some(flag) = args.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "출력 형식을 지정해야 합니다. 예: hwp-convert sample.hwpx --to svg",
        ));
    };
    let Some(format) = args.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "출력 형식을 지정해야 합니다. 예: hwp-convert sample.hwpx --to svg",
        ));
    };

    if args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "인자가 너무 많습니다. 예: hwp-convert sample.hwpx --to svg",
        ));
    }

    if flag.to_string_lossy() != "--to" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "두 번째 인자는 --to 여야 합니다.",
        ));
    }

    let format = OutputFormat::parse(&format.to_string_lossy())?;

    Ok(Some(CliArgs { input_path, format }))
}

pub fn print_usage() {
    println!("사용법");
    println!("  hwp-convert <입력 파일> --to <출력 형식>");
    println!();
    println!("지원 형식");
    println!("  txt");
    println!("  svg");
    println!("  json");
    println!("  html");
    println!();
    println!("예시");
    println!("  hwp-convert sample.hwpx --to txt");
    println!("  hwp-convert sample.hwpx --to svg");
    println!("  hwp-convert sample.hwpx --to json");
    println!("  hwp-convert sample.hwpx --to html");
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
}
