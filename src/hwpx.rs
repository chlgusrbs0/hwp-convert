use std::error::Error;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::Path;

use zip::ZipArchive;

use crate::ir::{
    Block, Document, Metadata, NoteStore, Paragraph, ResourceStore, Section, StyleSheet, Table,
    TableCell, TableCellStyle, TableRow, TableStyle,
};

const PREVIEW_TEXT_PATH: &str = "Preview/PrvText.txt";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputKind {
    Hwp,
    Hwpx,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HwpxTextFallbackSource {
    SectionXml,
    PreviewText,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HwpxTextFallback {
    pub paragraphs: Vec<String>,
    pub source: HwpxTextFallbackSource,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HwpxDocumentFallback {
    pub document: Document,
    pub source: HwpxTextFallbackSource,
}

#[allow(dead_code)]
pub fn read_preview_text(input_path: &Path) -> Result<String, Box<dyn Error>> {
    let paragraphs = read_paragraphs(input_path)?;
    Ok(paragraphs.join("\n"))
}

/// Legacy paragraph-only extraction path.
///
/// This flattens the parsed document into plain paragraph strings, so table,
/// image, and style structure is intentionally discarded here.
pub fn read_paragraphs(input_path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let (input_kind, bytes) = read_input_bytes(input_path)?;

    resolve_paragraphs(input_kind, &bytes, read_paragraphs_with_rhwp(&bytes)).map_err(Into::into)
}

pub(crate) fn read_input_bytes(input_path: &Path) -> io::Result<(InputKind, Vec<u8>)> {
    let input_kind = detect_input_kind(input_path)?;
    let bytes = fs::read(input_path)?;
    Ok((input_kind, bytes))
}

pub(crate) fn detect_input_kind(input_path: &Path) -> io::Result<InputKind> {
    let Some(extension) = input_path
        .extension()
        .and_then(|extension| extension.to_str())
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "지원하지 않는 입력 형식입니다. .hwp 또는 .hwpx 파일만 처리할 수 있습니다: {}",
                input_path.display()
            ),
        ));
    };

    if extension.eq_ignore_ascii_case("hwp") {
        Ok(InputKind::Hwp)
    } else if extension.eq_ignore_ascii_case("hwpx") {
        Ok(InputKind::Hwpx)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "지원하지 않는 입력 형식입니다. .hwp 또는 .hwpx 파일만 처리할 수 있습니다: {}",
                input_path.display()
            ),
        ))
    }
}

fn resolve_paragraphs(
    input_kind: InputKind,
    bytes: &[u8],
    rhwp_result: io::Result<Vec<String>>,
) -> io::Result<Vec<String>> {
    match input_kind {
        InputKind::Hwp => rhwp_result,
        InputKind::Hwpx => match rhwp_result {
            Ok(paragraphs) => Ok(paragraphs),
            Err(rhwp_error) => read_text_fallback_from_archive(bytes)
                .map(|fallback| fallback.paragraphs)
                .map_err(|fallback_error| combine_hwpx_errors(&rhwp_error, &fallback_error)),
        },
    }
}

pub(crate) fn combine_hwpx_errors(rhwp_error: &io::Error, fallback_error: &io::Error) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{rhwp_error}; HWPX text fallback 실패: {fallback_error}"),
    )
}

pub(crate) fn read_text_fallback_from_archive(bytes: &[u8]) -> io::Result<HwpxTextFallback> {
    match read_section_text_from_archive(bytes) {
        Ok(paragraphs) => Ok(HwpxTextFallback {
            paragraphs,
            source: HwpxTextFallbackSource::SectionXml,
        }),
        Err(section_error) => read_preview_text_from_archive(bytes)
            .map(|paragraphs| HwpxTextFallback {
                paragraphs,
                source: HwpxTextFallbackSource::PreviewText,
            })
            .map_err(|preview_error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "HWPX section XML fallback 실패: {section_error}; HWPX preview fallback 실패: {preview_error}"
                    ),
                )
            }),
    }
}

pub(crate) fn read_document_fallback_from_archive(
    bytes: &[u8],
) -> io::Result<HwpxDocumentFallback> {
    match read_section_document_from_archive(bytes) {
        Ok(document) => Ok(HwpxDocumentFallback {
            document,
            source: HwpxTextFallbackSource::SectionXml,
        }),
        Err(section_error) => read_preview_text_from_archive(bytes)
            .map(|paragraphs| HwpxDocumentFallback {
                document: Document::from_paragraphs(paragraphs),
                source: HwpxTextFallbackSource::PreviewText,
            })
            .map_err(|preview_error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "HWPX section XML fallback failed: {section_error}; HWPX preview fallback failed: {preview_error}"
                    ),
                )
            }),
    }
}

fn read_paragraphs_with_rhwp(bytes: &[u8]) -> io::Result<Vec<String>> {
    let document = rhwp::parse_document(bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("rhwp 파싱 실패: {error}"),
        )
    })?;

    let paragraphs = extract_body_paragraphs(&document);
    if paragraphs.is_empty() {
        return Err(empty_paragraphs_error());
    }

    Ok(paragraphs)
}

fn empty_paragraphs_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "rhwp 문단 추출 결과가 비어 있습니다",
    )
}

fn extract_body_paragraphs(document: &rhwp::model::document::Document) -> Vec<String> {
    let mut paragraphs = Vec::new();

    for section in &document.sections {
        for paragraph in &section.paragraphs {
            let text = normalize_newlines(&paragraph.text);
            if !text.is_empty() {
                paragraphs.push(text);
            }
        }
    }

    paragraphs
}

/// Recover text directly from `Contents/section*.xml` when the rHWP bridge
/// cannot produce semantic blocks. This is still text-only, but it is usually
/// more faithful than `Preview/PrvText.txt` because it can preserve actual
/// paragraph text, inline line breaks, and tabs from the HWPX body XML.
pub(crate) fn read_section_text_from_archive(bytes: &[u8]) -> io::Result<Vec<String>> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("HWPX 아카이브를 열 수 없습니다: {error}"),
        )
    })?;

    let mut section_paths = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("HWPX 아카이브 항목을 읽을 수 없습니다: {error}"),
            )
        })?;
        let name = entry.name().to_string();
        if is_section_xml_path(&name) {
            section_paths.push(name);
        }
    }
    section_paths.sort_by_key(|path| section_xml_index(path).unwrap_or(u32::MAX));

    if section_paths.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Contents/section*.xml 항목이 없습니다",
        ));
    }

    let mut paragraphs = Vec::new();
    for section_path in section_paths {
        let section_xml = read_zip_text_entry(&mut archive, &section_path)?;
        paragraphs.extend(extract_section_xml_paragraphs(&section_xml));
    }

    if paragraphs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HWPX section XML에서 문단 텍스트를 찾을 수 없습니다",
        ));
    }

    Ok(paragraphs)
}

pub(crate) fn read_section_document_from_archive(bytes: &[u8]) -> io::Result<Document> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("HWPX archive could not be opened: {error}"),
        )
    })?;

    let mut section_paths = collect_section_xml_paths(&mut archive)?;
    section_paths.sort_by_key(|path| section_xml_index(path).unwrap_or(u32::MAX));

    if section_paths.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Contents/section*.xml entries were not found",
        ));
    }

    let mut blocks = Vec::new();
    for section_path in section_paths {
        let section_xml = read_zip_text_entry(&mut archive, &section_path)?;
        blocks.extend(extract_section_xml_blocks(&section_xml));
    }

    if blocks.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HWPX section XML did not contain recoverable document blocks",
        ));
    }

    Ok(document_from_blocks(blocks))
}

fn collect_section_xml_paths<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
) -> io::Result<Vec<String>> {
    let mut section_paths = Vec::new();

    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("HWPX archive entry could not be read: {error}"),
            )
        })?;
        let name = entry.name().to_string();
        if is_section_xml_path(&name) {
            section_paths.push(name);
        }
    }

    Ok(section_paths)
}

fn document_from_blocks(blocks: Vec<Block>) -> Document {
    Document {
        ir_version: crate::ir::IR_VERSION,
        metadata: Metadata::default(),
        sections: vec![Section {
            blocks,
            ..Default::default()
        }],
        resources: ResourceStore::default(),
        styles: StyleSheet::default(),
        notes: NoteStore::default(),
        warnings: Vec::new(),
    }
}

fn extract_section_xml_blocks(xml: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(xml, cursor) {
        if tag.is_closing {
            cursor = tag.end;
            continue;
        }

        match tag.name {
            "tbl" => {
                let Some(table_end) = find_matching_element_end(xml, &tag) else {
                    cursor = tag.end;
                    continue;
                };
                let table_xml = &xml[tag.start..table_end];
                if let Some(table) = extract_table_from_xml(table_xml) {
                    blocks.push(Block::Table(table));
                }
                cursor = table_end;
            }
            "p" => {
                let Some(paragraph_end) = find_matching_element_end(xml, &tag) else {
                    cursor = tag.end;
                    continue;
                };
                let paragraph_xml = &xml[tag.start..paragraph_end];
                blocks.extend(extract_blocks_from_paragraph_xml(paragraph_xml));
                cursor = paragraph_end;
            }
            _ => {
                cursor = tag.end;
            }
        }
    }

    blocks
}

fn extract_table_from_xml(table_xml: &str) -> Option<Table> {
    let mut rows = Vec::new();
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(table_xml, cursor) {
        if tag.name != "tr" || tag.is_closing {
            cursor = tag.end;
            continue;
        }

        let Some(row_end) = find_matching_element_end(table_xml, &tag) else {
            cursor = tag.end;
            continue;
        };
        let row_xml = &table_xml[tag.start..row_end];
        let cells = extract_table_cells_from_row_xml(row_xml);
        if !cells.is_empty() {
            rows.push(TableRow { cells });
        }
        cursor = row_end;
    }

    if rows.is_empty() {
        return None;
    }

    Some(Table {
        rows,
        style: TableStyle::default(),
    })
}

fn extract_table_cells_from_row_xml(row_xml: &str) -> Vec<TableCell> {
    let mut cells = Vec::new();
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(row_xml, cursor) {
        if tag.name != "tc" || tag.is_closing {
            cursor = tag.end;
            continue;
        }

        let Some(cell_end) = find_matching_element_end(row_xml, &tag) else {
            cursor = tag.end;
            continue;
        };
        let cell_xml = &row_xml[tag.start..cell_end];
        cells.push(extract_table_cell_from_xml(cell_xml));
        cursor = cell_end;
    }

    cells
}

fn extract_table_cell_from_xml(cell_xml: &str) -> TableCell {
    TableCell {
        row_span: first_xml_attribute_u32(cell_xml, "cellSpan", "rowSpan").unwrap_or(1),
        col_span: first_xml_attribute_u32(cell_xml, "cellSpan", "colSpan").unwrap_or(1),
        blocks: extract_section_xml_blocks(cell_xml),
        style: TableCellStyle::default(),
    }
}

fn extract_blocks_from_paragraph_xml(paragraph_xml: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut fragment_start = 0usize;
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(paragraph_xml, cursor) {
        if tag.name != "tbl" || tag.is_closing {
            cursor = tag.end;
            continue;
        }

        let Some(table_end) = find_matching_element_end(paragraph_xml, &tag) else {
            cursor = tag.end;
            continue;
        };

        push_paragraph_text_fragment_as_block(
            &mut blocks,
            &paragraph_xml[fragment_start..tag.start],
        );
        let table_xml = &paragraph_xml[tag.start..table_end];
        if let Some(table) = extract_table_from_xml(table_xml) {
            blocks.push(Block::Table(table));
        }

        fragment_start = table_end;
        cursor = table_end;
    }

    push_paragraph_text_fragment_as_block(&mut blocks, &paragraph_xml[fragment_start..]);
    blocks
}

fn push_paragraph_text_fragment_as_block(blocks: &mut Vec<Block>, xml: &str) {
    let text = extract_text_from_xml_fragment(xml);
    if !text.is_empty() {
        blocks.push(Block::Paragraph(Paragraph::from_plain_text(text)));
    }
}

fn extract_text_from_xml_fragment(xml: &str) -> String {
    let mut current = String::new();
    let mut cursor = 0usize;
    let mut text_depth = 0usize;

    while let Some(relative_tag_start) = xml[cursor..].find('<') {
        let tag_start = cursor + relative_tag_start;
        if text_depth > 0 && tag_start > cursor {
            current.push_str(&decode_xml_text(&xml[cursor..tag_start]));
        }

        let Some(relative_tag_end) = xml[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + relative_tag_end;
        let tag = &xml[tag_start + 1..tag_end];
        let tag_name = xml_tag_local_name(tag);
        let is_closing = is_xml_closing_tag(tag);
        let is_self_closing = is_xml_self_closing_tag(tag);

        match tag_name {
            Some("t") if is_closing => {
                text_depth = text_depth.saturating_sub(1);
            }
            Some("t") if !is_closing && !is_self_closing => {
                text_depth += 1;
            }
            Some("lineBreak") if text_depth > 0 => current.push('\n'),
            Some("tab") if text_depth > 0 => current.push('\t'),
            _ => {}
        }

        cursor = tag_end + 1;
    }

    if text_depth > 0 && cursor < xml.len() {
        current.push_str(&decode_xml_text(&xml[cursor..]));
    }

    current.trim_end().to_string()
}

fn first_xml_attribute_u32(xml: &str, tag_name: &str, attribute_name: &str) -> Option<u32> {
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(xml, cursor) {
        if tag.name == tag_name && !tag.is_closing {
            if let Some(value) = xml_attribute_value(tag.raw, attribute_name) {
                return value.parse().ok();
            }
        }
        cursor = tag.end;
    }

    None
}

#[derive(Clone, Copy, Debug)]
struct XmlTag<'a> {
    start: usize,
    end: usize,
    raw: &'a str,
    name: &'a str,
    is_closing: bool,
    is_self_closing: bool,
}

fn next_xml_tag(xml: &str, cursor: usize) -> Option<XmlTag<'_>> {
    let relative_start = xml.get(cursor..)?.find('<')?;
    let start = cursor + relative_start;
    let relative_end = xml.get(start..)?.find('>')?;
    let end = start + relative_end + 1;
    let raw = xml.get(start + 1..end - 1)?;
    let name = xml_tag_local_name(raw)?;

    Some(XmlTag {
        start,
        end,
        raw,
        name,
        is_closing: is_xml_closing_tag(raw),
        is_self_closing: is_xml_self_closing_tag(raw),
    })
}

fn find_matching_element_end(xml: &str, start_tag: &XmlTag<'_>) -> Option<usize> {
    if start_tag.is_closing {
        return None;
    }

    if start_tag.is_self_closing {
        return Some(start_tag.end);
    }

    let mut cursor = start_tag.end;
    let mut depth = 1usize;

    while let Some(tag) = next_xml_tag(xml, cursor) {
        if tag.name == start_tag.name {
            if tag.is_closing {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(tag.end);
                }
            } else if !tag.is_self_closing {
                depth += 1;
            }
        }

        cursor = tag.end;
    }

    None
}

fn xml_attribute_value<'a>(tag: &'a str, attribute_name: &str) -> Option<&'a str> {
    let mut search_start = 0usize;

    while let Some(relative_attr_start) = tag.get(search_start..)?.find(attribute_name) {
        let attr_start = search_start + relative_attr_start;
        let attr_end = attr_start + attribute_name.len();
        if !is_xml_attribute_boundary(tag, attr_start, attr_end) {
            search_start = attr_end;
            continue;
        }

        let after_name = tag.get(attr_end..)?.trim_start();
        let after_equals = after_name.strip_prefix('=')?.trim_start();
        let quote = after_equals.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }

        let value_start = quote.len_utf8();
        let value_end = after_equals.get(value_start..)?.find(quote)?;
        return after_equals.get(value_start..value_start + value_end);
    }

    None
}

fn is_xml_attribute_boundary(tag: &str, attr_start: usize, attr_end: usize) -> bool {
    let before_ok = attr_start == 0
        || tag
            .as_bytes()
            .get(attr_start.saturating_sub(1))
            .is_some_and(|byte| byte.is_ascii_whitespace());
    let after_ok = tag
        .as_bytes()
        .get(attr_end)
        .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'=');

    before_ok && after_ok
}

/// HWPX preview fallback is text-only.
///
/// `Preview/PrvText.txt` can recover plain text, but it cannot reconstruct
/// table, image, or style structure.
pub(crate) fn read_preview_text_from_archive(bytes: &[u8]) -> io::Result<Vec<String>> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("HWPX 아카이브를 열 수 없습니다: {error}"),
        )
    })?;

    let preview_text = read_zip_text_entry(&mut archive, PREVIEW_TEXT_PATH)?;
    Ok(split_preview_text_to_paragraphs(&preview_text))
}

fn read_zip_text_entry<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
) -> io::Result<String> {
    let mut file = archive
        .by_name(path)
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, format!("{path} 항목이 없습니다")))?;

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| {
        io::Error::new(error.kind(), format!("{path}를 읽을 수 없습니다: {error}"))
    })?;

    String::from_utf8(bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{path}가 UTF-8이 아닙니다: {error}"),
        )
    })
}

fn is_section_xml_path(path: &str) -> bool {
    section_xml_index(path).is_some()
}

fn section_xml_index(path: &str) -> Option<u32> {
    let Some(file_name) = path.strip_prefix("Contents/section") else {
        return None;
    };

    let index = file_name.strip_suffix(".xml")?;
    if index.is_empty() || !index.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    index.parse().ok()
}

fn extract_section_xml_paragraphs(xml: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current = String::new();
    let mut cursor = 0usize;
    let mut paragraph_depth = 0usize;
    let mut text_depth = 0usize;

    while let Some(relative_tag_start) = xml[cursor..].find('<') {
        let tag_start = cursor + relative_tag_start;
        if paragraph_depth > 0 && text_depth > 0 && tag_start > cursor {
            current.push_str(&decode_xml_text(&xml[cursor..tag_start]));
        }

        let Some(relative_tag_end) = xml[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + relative_tag_end;
        let tag = &xml[tag_start + 1..tag_end];
        let tag_name = xml_tag_local_name(tag);
        let is_closing = is_xml_closing_tag(tag);
        let is_self_closing = is_xml_self_closing_tag(tag);

        match tag_name {
            Some("p") if is_closing => {
                if paragraph_depth > 0 {
                    paragraph_depth -= 1;
                    if paragraph_depth == 0 {
                        let paragraph = current.trim_end().to_string();
                        if !paragraph.is_empty() {
                            paragraphs.push(paragraph);
                        }
                        current.clear();
                        text_depth = 0;
                    }
                }
            }
            Some("p") if !is_closing => {
                if paragraph_depth == 0 {
                    current.clear();
                    text_depth = 0;
                }
                if !is_self_closing {
                    paragraph_depth += 1;
                }
            }
            Some("t") if paragraph_depth > 0 && is_closing => {
                text_depth = text_depth.saturating_sub(1);
            }
            Some("t") if paragraph_depth > 0 && !is_closing && !is_self_closing => {
                text_depth += 1;
            }
            Some("lineBreak") if paragraph_depth > 0 && text_depth > 0 => current.push('\n'),
            Some("tab") if paragraph_depth > 0 && text_depth > 0 => current.push('\t'),
            _ => {}
        }

        cursor = tag_end + 1;
    }

    if paragraph_depth > 0 && text_depth > 0 && cursor < xml.len() {
        current.push_str(&decode_xml_text(&xml[cursor..]));
    }

    paragraphs
}

fn xml_tag_local_name(tag: &str) -> Option<&str> {
    let trimmed = tag.trim();
    if trimmed.is_empty() || trimmed.starts_with('?') || trimmed.starts_with('!') {
        return None;
    }

    let trimmed = trimmed
        .trim_start_matches('/')
        .trim()
        .trim_end_matches('/')
        .trim();
    let qualified_name = trimmed.split_whitespace().next()?;

    Some(
        qualified_name
            .rsplit_once(':')
            .map(|(_, local_name)| local_name)
            .unwrap_or(qualified_name),
    )
}

fn is_xml_closing_tag(tag: &str) -> bool {
    tag.trim_start().starts_with('/')
}

fn is_xml_self_closing_tag(tag: &str) -> bool {
    tag.trim_end().ends_with('/')
}

fn decode_xml_text(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }

    let mut decoded = String::new();
    let mut cursor = 0usize;

    while let Some(relative_ampersand) = text[cursor..].find('&') {
        let ampersand = cursor + relative_ampersand;
        decoded.push_str(&text[cursor..ampersand]);

        let Some(relative_semicolon) = text[ampersand..].find(';') else {
            decoded.push_str(&text[ampersand..]);
            return decoded;
        };
        let semicolon = ampersand + relative_semicolon;
        let entity = &text[ampersand + 1..semicolon];

        if let Some(ch) = decode_xml_entity(entity) {
            decoded.push(ch);
        } else {
            decoded.push('&');
            decoded.push_str(entity);
            decoded.push(';');
        }

        cursor = semicolon + 1;
    }

    decoded.push_str(&text[cursor..]);
    decoded
}

fn decode_xml_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => decode_numeric_xml_entity(entity),
    }
}

fn decode_numeric_xml_entity(entity: &str) -> Option<char> {
    let value = if let Some(hex) = entity
        .strip_prefix("#x")
        .or_else(|| entity.strip_prefix("#X"))
    {
        u32::from_str_radix(hex, 16).ok()?
    } else if let Some(decimal) = entity.strip_prefix('#') {
        decimal.parse::<u32>().ok()?
    } else {
        return None;
    };

    char::from_u32(value)
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn split_preview_text_to_paragraphs(text: &str) -> Vec<String> {
    let mut paragraphs = normalize_newlines(text)
        .split('\n')
        .map(str::trim_end)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    while paragraphs.last().is_some_and(|line| line.is_empty()) {
        paragraphs.pop();
    }

    paragraphs
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
            vec![
                "first paragraph".to_string(),
                "second paragraph".to_string(),
                "third paragraph".to_string()
            ]
        );
    }

    #[test]
    fn preserves_internal_blank_lines_in_preview_text() {
        let paragraphs = split_preview_text_to_paragraphs("first line\r\n\r\nthird line\r\n");

        assert_eq!(
            paragraphs,
            vec![
                "first line".to_string(),
                "".to_string(),
                "third line".to_string()
            ]
        );
    }

    #[test]
    fn trims_trailing_blank_lines_in_preview_text() {
        let paragraphs = split_preview_text_to_paragraphs("first line\nsecond line\n\n\n");

        assert_eq!(
            paragraphs,
            vec!["first line".to_string(), "second line".to_string()]
        );
    }

    #[test]
    fn extracts_paragraphs_from_section_xml_text() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p><hp:run><hp:t>first &amp; second</hp:t></hp:run></hp:p>
              <hp:p><hp:run><hp:t>line 앞<hp:lineBreak/>line 뒤</hp:t></hp:run></hp:p>
              <hp:p><hp:run><hp:t>tab 앞<hp:tab width="4000"/>tab 뒤</hp:t></hp:run></hp:p>
              <hp:p><hp:run><hp:t></hp:t></hp:run></hp:p>
            </hs:sec>
        "#;

        let paragraphs = extract_section_xml_paragraphs(xml);

        assert_eq!(
            paragraphs,
            vec![
                "first & second".to_string(),
                "line 앞\nline 뒤".to_string(),
                "tab 앞\ttab 뒤".to_string(),
            ]
        );
    }

    #[test]
    fn extracts_simple_table_from_section_xml_blocks() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p><hp:run><hp:t>before table</hp:t></hp:run></hp:p>
              <hp:p>
                <hp:run><hp:t>table lead</hp:t></hp:run>
                <hp:ctrl>
                  <hp:tbl>
                    <hp:tr>
                      <hp:tc>
                        <hp:cellSpan rowSpan="1" colSpan="2"/>
                        <hp:subList>
                          <hp:p><hp:run><hp:t>left cell</hp:t></hp:run></hp:p>
                        </hp:subList>
                      </hp:tc>
                      <hp:tc>
                        <hp:cellSpan rowSpan="2" colSpan="1"/>
                        <hp:subList>
                          <hp:p><hp:run><hp:t>right cell</hp:t></hp:run></hp:p>
                        </hp:subList>
                      </hp:tc>
                    </hp:tr>
                  </hp:tbl>
                </hp:ctrl>
              </hp:p>
              <hp:p><hp:run><hp:t>after table</hp:t></hp:run></hp:p>
            </hs:sec>
        "#;

        let blocks = extract_section_xml_blocks(xml);

        assert_eq!(blocks.len(), 4);
        match &blocks[0] {
            Block::Paragraph(paragraph) => match &paragraph.inlines[0] {
                crate::ir::Inline::Text(run) => assert_eq!(run.text, "before table"),
                other => panic!("expected text inline, got {other:?}"),
            },
            other => panic!("expected paragraph block, got {other:?}"),
        }
        match &blocks[1] {
            Block::Paragraph(paragraph) => match &paragraph.inlines[0] {
                crate::ir::Inline::Text(run) => assert_eq!(run.text, "table lead"),
                other => panic!("expected text inline, got {other:?}"),
            },
            other => panic!("expected paragraph block, got {other:?}"),
        }
        match &blocks[2] {
            Block::Table(table) => {
                assert_eq!(table.rows.len(), 1);
                assert_eq!(table.rows[0].cells.len(), 2);
                assert_eq!(table.rows[0].cells[0].col_span, 2);
                assert_eq!(table.rows[0].cells[1].row_span, 2);
                match &table.rows[0].cells[0].blocks[0] {
                    Block::Paragraph(paragraph) => match &paragraph.inlines[0] {
                        crate::ir::Inline::Text(run) => assert_eq!(run.text, "left cell"),
                        other => panic!("expected text inline, got {other:?}"),
                    },
                    other => panic!("expected paragraph in table cell, got {other:?}"),
                }
            }
            other => panic!("expected table block, got {other:?}"),
        }
        match &blocks[3] {
            Block::Paragraph(paragraph) => match &paragraph.inlines[0] {
                crate::ir::Inline::Text(run) => assert_eq!(run.text, "after table"),
                other => panic!("expected text inline, got {other:?}"),
            },
            other => panic!("expected paragraph block, got {other:?}"),
        }
    }

    #[test]
    fn prefers_section_xml_text_fallback_before_preview_text() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[
            (
                "Contents/section0.xml",
                r#"<hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"><hp:p><hp:run><hp:t>section text</hp:t></hp:run></hp:p></hs:sec>"#,
            ),
            (PREVIEW_TEXT_PATH, "preview text"),
        ])?;

        let fallback = read_text_fallback_from_archive(&bytes)?;

        assert_eq!(fallback.source, HwpxTextFallbackSource::SectionXml);
        assert_eq!(fallback.paragraphs, vec!["section text".to_string()]);

        Ok(())
    }

    #[test]
    fn falls_back_to_preview_archive_entry_for_hwpx_parse_failure() -> Result<(), Box<dyn Error>> {
        let path = temp_fixture_path("preview-fallback", "hwpx");
        write_preview_archive(&path, "first line\r\nsecond line")?;

        let paragraphs = read_paragraphs(&path)?;
        fs::remove_file(&path)?;

        assert_eq!(
            paragraphs,
            vec!["first line".to_string(), "second line".to_string()]
        );

        Ok(())
    }

    #[test]
    fn falls_back_to_preview_archive_entry_for_hwpx_empty_rhwp_result() -> Result<(), Box<dyn Error>>
    {
        let bytes = create_preview_archive_bytes("first line\r\n\r\nthird line")?;

        let paragraphs =
            resolve_paragraphs(InputKind::Hwpx, &bytes, Err(empty_paragraphs_error()))?;

        assert_eq!(
            paragraphs,
            vec![
                "first line".to_string(),
                "".to_string(),
                "third line".to_string()
            ]
        );

        Ok(())
    }

    #[test]
    fn does_not_fall_back_to_preview_archive_for_hwp() -> Result<(), Box<dyn Error>> {
        let path = temp_fixture_path("no-preview-fallback", "hwp");
        write_preview_archive(&path, "preview text that should not be used")?;

        let error = read_paragraphs(&path).unwrap_err();
        fs::remove_file(&path)?;

        let message = error.to_string();
        assert!(message.contains("rhwp 파싱 실패:"));
        assert!(!message.contains("HWPX text fallback 실패:"));

        Ok(())
    }

    #[test]
    fn combines_rhwp_and_preview_errors_for_hwpx_when_both_fail() -> Result<(), Box<dyn Error>> {
        let path = temp_fixture_path("combined-error", "hwpx");
        fs::write(&path, "not a valid hwpx file")?;

        let error = read_paragraphs(&path).unwrap_err();
        fs::remove_file(&path)?;

        let message = error.to_string();
        assert!(message.contains("rhwp 파싱 실패:"));
        assert!(message.contains("HWPX text fallback 실패:"));
        assert!(message.contains("HWPX preview fallback 실패:"));

        Ok(())
    }

    fn create_preview_archive_bytes(preview_text: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        create_archive_bytes(&[(PREVIEW_TEXT_PATH, preview_text)])
    }

    fn create_archive_bytes(entries: &[(&str, &str)]) -> Result<Vec<u8>, Box<dyn Error>> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);

        for (path, content) in entries {
            writer.start_file(*path, SimpleFileOptions::default())?;
            writer.write_all(content.as_bytes())?;
        }

        let cursor = writer.finish()?;
        Ok(cursor.into_inner())
    }

    fn write_preview_archive(path: &Path, preview_text: &str) -> Result<(), Box<dyn Error>> {
        let file = File::create(path)?;
        let mut writer = ZipWriter::new(file);

        writer.start_file(PREVIEW_TEXT_PATH, SimpleFileOptions::default())?;
        writer.write_all(preview_text.as_bytes())?;
        writer.finish()?;

        Ok(())
    }

    fn temp_fixture_path(label: &str, extension: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "hwp-convert-{label}-{}-{nanos}.{extension}",
            std::process::id()
        ))
    }
}
