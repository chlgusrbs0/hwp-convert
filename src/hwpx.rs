use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::Path;

use zip::ZipArchive;

use crate::ir::{
    Alignment, Block, Color, Document, HeaderFooter, HeaderFooterPlacement, Image, ImageResource,
    Inline, LengthPt, LengthPx, Link, ListInfo, ListKind, Metadata, Note, NoteId, NoteKind,
    NoteStore, Paragraph, ParagraphStyle, Resource, ResourceId, ResourceStore, Section, StyleSheet,
    Table, TableCell, TableCellStyle, TableRow, TableStyle, TextRun, TextStyle, UnknownBlock,
    UnknownInline,
};

const PREVIEW_TEXT_PATH: &str = "Preview/PrvText.txt";
const HEADER_XML_PATH: &str = "Contents/header.xml";
const MAX_HWPX_IMAGE_RESOURCE_BYTES: u64 = 64 * 1024 * 1024;

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

    let mut context = read_hwpx_fallback_context(&mut archive).unwrap_or_default();
    let mut sections = Vec::new();
    for section_path in section_paths {
        let section_xml = read_zip_text_entry(&mut archive, &section_path)?;
        let section = extract_section_xml_section(&section_xml, &mut context);
        if !section.blocks.is_empty() || !section.headers.is_empty() || !section.footers.is_empty()
        {
            sections.push(section);
        }
    }

    if sections.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HWPX section XML did not contain recoverable document blocks",
        ));
    }

    let mut document = document_from_sections(sections);
    document.resources = context.resources;
    document.notes = context.notes;
    Ok(document)
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

#[derive(Clone, Debug, Default, PartialEq)]
struct HwpxFallbackContext {
    paragraph_styles: Vec<HwpxParagraphStyle>,
    text_styles: Vec<TextStyle>,
    font_faces: Vec<Vec<String>>,
    border_fill_backgrounds: Vec<Option<Color>>,
    image_items: BTreeMap<String, HwpxImageItem>,
    image_resource_ids: BTreeMap<String, ResourceId>,
    resources: ResourceStore,
    ordered_counts: BTreeMap<(u32, u8), u32>,
    notes: NoteStore,
    next_note_ordinal: u32,
}

impl HwpxFallbackContext {
    fn border_fill_background_color(&self, border_fill_id: u32) -> Option<Color> {
        self.border_fill_backgrounds
            .get(border_fill_id as usize)
            .copied()
            .flatten()
            .or_else(|| {
                border_fill_id
                    .checked_sub(1)
                    .and_then(|index| self.border_fill_backgrounds.get(index as usize))
                    .copied()
                    .flatten()
            })
    }

    fn text_style_for_run(&self, run_tag: &str) -> TextStyle {
        let Some(char_pr_id) = xml_attribute_value(run_tag, "charPrIDRef")
            .and_then(|value| value.parse::<usize>().ok())
        else {
            return TextStyle::default();
        };

        self.text_styles
            .get(char_pr_id)
            .cloned()
            .unwrap_or_default()
    }

    fn paragraph_style_for_paragraph(&self, paragraph_xml: &str) -> ParagraphStyle {
        let Some(para_pr_id) =
            first_xml_attribute_u32(paragraph_xml, "p", "paraPrIDRef").map(|id| id as usize)
        else {
            return ParagraphStyle::default();
        };

        self.paragraph_styles
            .get(para_pr_id)
            .map(|style| style.style.clone())
            .unwrap_or_default()
    }

    fn list_info_for_paragraph(&mut self, paragraph_xml: &str) -> Option<ListInfo> {
        let para_pr_id = first_xml_attribute_u32(paragraph_xml, "p", "paraPrIDRef")? as usize;
        let style = self.paragraph_styles.get(para_pr_id)?.clone();

        match style.kind {
            Some(ListKind::Ordered) => {
                let key = (style.list_id.unwrap_or(0), style.level);
                let number = self.ordered_counts.entry(key).or_insert(0);
                *number += 1;

                Some(ListInfo {
                    kind: ListKind::Ordered,
                    level: style.level,
                    marker: None,
                    number: Some(*number),
                })
            }
            Some(ListKind::Unordered) => Some(ListInfo {
                kind: ListKind::Unordered,
                level: style.level,
                marker: Some("•".to_string()),
                number: None,
            }),
            _ => None,
        }
    }

    fn store_note_from_hwpx_control(
        &mut self,
        note_kind: NoteKind,
        tag: &str,
        note_xml: &str,
    ) -> Inline {
        let note_prefix = match note_kind {
            NoteKind::Footnote => "footnote",
            NoteKind::Endnote => "endnote",
        };
        let mut requested_id = decoded_xml_attribute_value(tag, "instId")
            .or_else(|| decoded_xml_attribute_value(tag, "id"));
        let blocks = extract_section_xml_blocks(note_xml, self);

        let note_id = loop {
            let candidate = self.next_note_id(note_prefix, requested_id.take());
            let note = Note {
                id: candidate.clone(),
                kind: note_kind.clone(),
                blocks: blocks.clone(),
            };
            if self.notes.insert_unique(note).is_ok() {
                break candidate;
            }
        };

        match note_kind {
            NoteKind::Footnote => Inline::FootnoteRef { note_id },
            NoteKind::Endnote => Inline::EndnoteRef { note_id },
        }
    }

    fn ensure_image_resource(&mut self, binary_item_id_ref: &str) -> Option<ResourceId> {
        let image_key = self.resolve_image_item_key(binary_item_id_ref)?;
        if let Some(resource_id) = self.image_resource_ids.get(&image_key) {
            return Some(resource_id.clone());
        }

        let item = self.image_items.get(&image_key)?;
        let resource_id = ResourceId(item.id.clone());
        if self.resources.get(&resource_id).is_none() {
            self.resources
                .insert_unique(Resource::Image(ImageResource {
                    id: resource_id.clone(),
                    media_type: item.media_type.clone(),
                    extension: item.extension.clone(),
                    bytes: item.bytes.clone(),
                }))
                .ok()?;
        }

        self.image_resource_ids
            .insert(image_key, resource_id.clone());
        Some(resource_id)
    }

    fn resolve_image_item_key(&self, binary_item_id_ref: &str) -> Option<String> {
        let raw = binary_item_id_ref.trim();
        if raw.is_empty() {
            return None;
        }

        hwpx_image_item_lookup_keys(raw)
            .into_iter()
            .find(|key| self.image_items.contains_key(key))
    }

    fn next_note_id(&mut self, prefix: &str, raw_id: Option<String>) -> NoteId {
        let base = raw_id
            .map(|id| format!("{prefix}-{id}"))
            .unwrap_or_else(|| {
                self.next_note_ordinal += 1;
                format!("{prefix}-{}", self.next_note_ordinal)
            });

        if self.notes.get(&NoteId(base.clone())).is_none() {
            return NoteId(base);
        }

        let mut suffix = 2u32;
        loop {
            let candidate = NoteId(format!("{base}-{suffix}"));
            if self.notes.get(&candidate).is_none() {
                return candidate;
            }
            suffix += 1;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HwpxImageItem {
    id: String,
    media_type: Option<String>,
    extension: Option<String>,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct HwpxParagraphStyle {
    kind: Option<ListKind>,
    level: u8,
    list_id: Option<u32>,
    style: ParagraphStyle,
}

fn read_hwpx_fallback_context<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
) -> io::Result<HwpxFallbackContext> {
    let mut context = read_zip_text_entry(archive, HEADER_XML_PATH)
        .map(|header_xml| extract_hwpx_fallback_context(&header_xml))
        .unwrap_or_default();
    context.image_items = read_hwpx_image_items(archive)?;
    Ok(context)
}

fn extract_hwpx_fallback_context(header_xml: &str) -> HwpxFallbackContext {
    let mut context = HwpxFallbackContext::default();
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(header_xml, cursor) {
        if tag.is_closing {
            cursor = tag.end;
            continue;
        }

        match tag.name {
            "fontface" => {
                let Some(fontface_end) = find_matching_element_end(header_xml, &tag) else {
                    cursor = tag.end;
                    continue;
                };
                let fontface_xml = &header_xml[tag.start..fontface_end];
                context
                    .font_faces
                    .push(extract_hwpx_font_face(fontface_xml));
                cursor = fontface_end;
            }
            "charPr" => {
                let Some(id) = xml_attribute_value(tag.raw, "id")
                    .and_then(|value| value.parse::<usize>().ok())
                else {
                    cursor = tag.end;
                    continue;
                };
                let Some(char_end) = find_matching_element_end(header_xml, &tag) else {
                    cursor = tag.end;
                    continue;
                };

                if context.text_styles.len() <= id {
                    context.text_styles.resize_with(id + 1, TextStyle::default);
                }

                let char_xml = &header_xml[tag.start..char_end];
                context.text_styles[id] =
                    extract_hwpx_text_style(tag.raw, char_xml, &context.font_faces);
                cursor = char_end;
            }
            "borderFill" => {
                let Some(id) = xml_attribute_value(tag.raw, "id")
                    .and_then(|value| value.parse::<usize>().ok())
                else {
                    cursor = tag.end;
                    continue;
                };
                let Some(border_end) = find_matching_element_end(header_xml, &tag) else {
                    cursor = tag.end;
                    continue;
                };

                if context.border_fill_backgrounds.len() <= id {
                    context.border_fill_backgrounds.resize(id + 1, None);
                }

                let border_xml = &header_xml[tag.start..border_end];
                context.border_fill_backgrounds[id] =
                    extract_hwpx_border_fill_background_color(border_xml);
                cursor = border_end;
            }
            "paraPr" => {
                let Some(id) = xml_attribute_value(tag.raw, "id")
                    .and_then(|value| value.parse::<usize>().ok())
                else {
                    cursor = tag.end;
                    continue;
                };
                let Some(para_end) = find_matching_element_end(header_xml, &tag) else {
                    cursor = tag.end;
                    continue;
                };

                if context.paragraph_styles.len() <= id {
                    context
                        .paragraph_styles
                        .resize_with(id + 1, HwpxParagraphStyle::default);
                }

                let para_xml = &header_xml[tag.start..para_end];
                context.paragraph_styles[id] = extract_hwpx_paragraph_style(para_xml);
                cursor = para_end;
            }
            _ => {
                cursor = tag.end;
            }
        }
    }

    context
}

fn read_hwpx_image_items<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
) -> io::Result<BTreeMap<String, HwpxImageItem>> {
    let Ok(content_xml) = read_zip_text_entry(archive, "Contents/content.hpf") else {
        return Ok(BTreeMap::new());
    };

    let mut items = BTreeMap::new();
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(&content_xml, cursor) {
        if tag.name != "item" || tag.is_closing {
            cursor = tag.end;
            continue;
        }

        let id = decoded_xml_attribute_value(tag.raw, "id");
        let href = decoded_xml_attribute_value(tag.raw, "href");
        let media_type = decoded_xml_attribute_value(tag.raw, "media-type");
        let Some(id) = id else {
            cursor = tag.end;
            continue;
        };
        let Some(href) = href else {
            cursor = tag.end;
            continue;
        };
        if !is_hwpx_image_manifest_item(&href, media_type.as_deref()) {
            cursor = tag.end;
            continue;
        }

        if let Ok(Some(bytes)) = read_hwpx_binary_entry(archive, &href) {
            let extension = path_extension(&href);
            let media_type = media_type.or_else(|| {
                extension
                    .as_deref()
                    .and_then(media_type_for_extension)
                    .map(ToOwned::to_owned)
            });
            let item = HwpxImageItem {
                id: id.clone(),
                media_type,
                extension,
                bytes,
            };

            for key in hwpx_image_item_lookup_keys(&id) {
                items.insert(key, item.clone());
            }
            if let Some(stem) = path_file_stem(&href) {
                for key in hwpx_image_item_lookup_keys(&stem) {
                    items.insert(key, item.clone());
                }
            }
        }

        cursor = tag.end;
    }

    Ok(items)
}

fn read_hwpx_binary_entry<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
    href: &str,
) -> io::Result<Option<Vec<u8>>> {
    for path in hwpx_binary_entry_candidates(href) {
        match read_zip_binary_entry(archive, &path) {
            Ok(bytes) => return Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }

    Ok(None)
}

fn hwpx_binary_entry_candidates(href: &str) -> Vec<String> {
    let normalized = href.replace('\\', "/");
    let mut candidates = vec![normalized.clone()];
    if !normalized.starts_with("Contents/") {
        candidates.push(format!("Contents/{normalized}"));
    }
    candidates
}

fn is_hwpx_image_manifest_item(href: &str, media_type: Option<&str>) -> bool {
    media_type.is_some_and(|media_type| media_type.starts_with("image/"))
        || href.replace('\\', "/").contains("BinData/")
            && path_extension(href)
                .as_deref()
                .and_then(media_type_for_extension)
                .is_some_and(|media_type| media_type.starts_with("image/"))
}

fn hwpx_image_item_lookup_keys(value: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return keys;
    }

    keys.push(trimmed.to_string());
    let digits = trimmed
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if !digits.is_empty() {
        keys.push(digits.clone());
        keys.push(format!("image{digits}"));
    }
    keys.sort();
    keys.dedup();
    keys
}

fn path_extension(path: &str) -> Option<String> {
    let file_name = path.replace('\\', "/").rsplit('/').next()?.to_string();
    let extension = file_name.rsplit_once('.')?.1;
    non_empty_string_owned(extension.to_ascii_lowercase())
}

fn path_file_stem(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let file_name = normalized.rsplit('/').next()?;
    let stem = file_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(file_name);
    non_empty_string_owned(stem.to_string())
}

fn media_type_for_extension(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn extract_hwpx_font_face(fontface_xml: &str) -> Vec<String> {
    let mut fonts = Vec::new();
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(fontface_xml, cursor) {
        if tag.name == "font"
            && !tag.is_closing
            && let Some(id) =
                xml_attribute_value(tag.raw, "id").and_then(|value| value.parse::<usize>().ok())
            && let Some(face) = xml_attribute_value(tag.raw, "face")
        {
            if fonts.len() <= id {
                fonts.resize(id + 1, String::new());
            }
            fonts[id] = face.to_string();
        }
        cursor = tag.end;
    }

    fonts
}

fn extract_hwpx_text_style(
    char_pr_tag: &str,
    char_pr_xml: &str,
    font_faces: &[Vec<String>],
) -> TextStyle {
    let mut style = TextStyle {
        font_size_pt: xml_attribute_hwp_units_to_pt(char_pr_tag, "height"),
        color: xml_attribute_value(char_pr_tag, "textColor").and_then(parse_hwpx_hex_color),
        background_color: xml_attribute_value(char_pr_tag, "shadeColor")
            .and_then(parse_hwpx_hex_color),
        ..Default::default()
    };
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(char_pr_xml, cursor) {
        if !tag.is_closing {
            match tag.name {
                "bold" => style.bold = true,
                "italic" => style.italic = true,
                "underline" => style.underline = true,
                "strikeout" | "strikeOut" => style.strike = true,
                "fontRef" => {
                    style.font_family = font_ref_family(tag.raw, font_faces);
                }
                _ => {}
            }
        }
        cursor = tag.end;
    }

    style
}

fn extract_hwpx_border_fill_background_color(border_fill_xml: &str) -> Option<Color> {
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(border_fill_xml, cursor) {
        if !tag.is_closing
            && let Some(color) =
                ["faceColor", "backgroundColor", "color"]
                    .iter()
                    .find_map(|attribute| {
                        xml_attribute_value(tag.raw, attribute).and_then(parse_hwpx_hex_color)
                    })
        {
            return Some(color);
        }
        cursor = tag.end;
    }

    None
}

fn font_ref_family(font_ref_tag: &str, font_faces: &[Vec<String>]) -> Option<String> {
    for (group_index, attribute) in [
        "hangul", "latin", "hanja", "japanese", "other", "symbol", "user",
    ]
    .iter()
    .enumerate()
    {
        let Some(font_id) = xml_attribute_value(font_ref_tag, attribute)
            .and_then(|value| value.parse::<usize>().ok())
        else {
            continue;
        };
        let Some(group) = font_faces.get(group_index) else {
            continue;
        };
        let Some(face) = group.get(font_id).filter(|face| !face.is_empty()) else {
            continue;
        };
        return Some(face.clone());
    }

    None
}

fn extract_hwpx_paragraph_style(para_xml: &str) -> HwpxParagraphStyle {
    let mut cursor = 0usize;
    let mut paragraph_style = HwpxParagraphStyle::default();

    while let Some(tag) = next_xml_tag(para_xml, cursor) {
        if !tag.is_closing {
            match tag.name {
                "heading" => {
                    paragraph_style.kind = match xml_attribute_value(tag.raw, "type") {
                        Some("NUMBER") => Some(ListKind::Ordered),
                        Some("BULLET") => Some(ListKind::Unordered),
                        _ => None,
                    };
                    paragraph_style.list_id =
                        xml_attribute_value(tag.raw, "idRef").and_then(|value| value.parse().ok());
                    paragraph_style.level = xml_attribute_value(tag.raw, "level")
                        .and_then(|value| value.parse::<u8>().ok())
                        .unwrap_or(0);
                }
                "align" => {
                    paragraph_style.style.alignment =
                        xml_attribute_value(tag.raw, "horizontal").and_then(map_hwpx_alignment);
                }
                "intent" => {
                    paragraph_style.style.indent.first_line_pt =
                        xml_attribute_hwp_units_to_pt(tag.raw, "value");
                }
                "left" => {
                    paragraph_style.style.indent.left_pt =
                        xml_attribute_hwp_units_to_pt(tag.raw, "value");
                }
                "right" => {
                    paragraph_style.style.indent.right_pt =
                        xml_attribute_hwp_units_to_pt(tag.raw, "value");
                }
                "prev" => {
                    paragraph_style.style.spacing.before_pt =
                        xml_attribute_hwp_units_to_pt(tag.raw, "value");
                }
                "next" => {
                    paragraph_style.style.spacing.after_pt =
                        xml_attribute_hwp_units_to_pt(tag.raw, "value");
                }
                "lineSpacing"
                    if !matches!(xml_attribute_value(tag.raw, "type"), Some("PERCENT")) =>
                {
                    paragraph_style.style.spacing.line_pt =
                        xml_attribute_hwp_units_to_pt(tag.raw, "value");
                }
                _ => {}
            }
        }

        cursor = tag.end;
    }

    paragraph_style
}

fn document_from_sections(sections: Vec<Section>) -> Document {
    Document {
        ir_version: crate::ir::IR_VERSION,
        metadata: Metadata::default(),
        sections,
        resources: ResourceStore::default(),
        styles: StyleSheet::default(),
        notes: NoteStore::default(),
        warnings: Vec::new(),
    }
}

fn extract_section_xml_blocks(xml: &str, context: &mut HwpxFallbackContext) -> Vec<Block> {
    let mut headers = Vec::new();
    let mut footers = Vec::new();
    extract_section_xml_blocks_with_metadata(xml, context, &mut headers, &mut footers)
}

fn extract_section_xml_section(xml: &str, context: &mut HwpxFallbackContext) -> Section {
    let mut headers = Vec::new();
    let mut footers = Vec::new();
    let blocks = extract_section_xml_blocks_with_metadata(xml, context, &mut headers, &mut footers);

    Section {
        blocks,
        headers,
        footers,
    }
}

fn extract_section_xml_blocks_with_metadata(
    xml: &str,
    context: &mut HwpxFallbackContext,
    headers: &mut Vec<HeaderFooter>,
    footers: &mut Vec<HeaderFooter>,
) -> Vec<Block> {
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
                if let Some(table) = extract_table_from_xml(table_xml, context) {
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
                blocks.extend(extract_blocks_from_paragraph_xml_with_metadata(
                    paragraph_xml,
                    context,
                    headers,
                    footers,
                ));
                cursor = paragraph_end;
            }
            _ => {
                cursor = tag.end;
            }
        }
    }

    blocks
}

fn extract_table_from_xml(table_xml: &str, context: &mut HwpxFallbackContext) -> Option<Table> {
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
        let cells = extract_table_cells_from_row_xml(row_xml, context);
        if !cells.is_empty() {
            rows.push(TableRow { cells });
        }
        cursor = row_end;
    }

    if rows.is_empty() {
        return None;
    }

    let background_color = first_xml_attribute_u32(table_xml, "tbl", "borderFillIDRef")
        .and_then(|border_fill_id| context.border_fill_background_color(border_fill_id));

    Some(Table {
        rows,
        style: TableStyle { background_color },
    })
}

fn extract_table_cells_from_row_xml(
    row_xml: &str,
    context: &mut HwpxFallbackContext,
) -> Vec<TableCell> {
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
        cells.push(extract_table_cell_from_xml(cell_xml, context));
        cursor = cell_end;
    }

    cells
}

fn extract_table_cell_from_xml(cell_xml: &str, context: &mut HwpxFallbackContext) -> TableCell {
    let background_color = first_xml_attribute_u32(cell_xml, "tc", "borderFillIDRef")
        .and_then(|border_fill_id| context.border_fill_background_color(border_fill_id));

    TableCell {
        row_span: first_xml_attribute_u32(cell_xml, "cellSpan", "rowSpan").unwrap_or(1),
        col_span: first_xml_attribute_u32(cell_xml, "cellSpan", "colSpan").unwrap_or(1),
        blocks: extract_section_xml_blocks(cell_xml, context),
        style: TableCellStyle { background_color },
    }
}

fn extract_blocks_from_paragraph_xml_with_metadata(
    paragraph_xml: &str,
    context: &mut HwpxFallbackContext,
    headers: &mut Vec<HeaderFooter>,
    footers: &mut Vec<HeaderFooter>,
) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut fragment_start = 0usize;
    let mut cursor = 0usize;
    let paragraph_style = context.paragraph_style_for_paragraph(paragraph_xml);
    let mut pending_list = context.list_info_for_paragraph(paragraph_xml);

    while let Some(tag) = next_xml_tag(paragraph_xml, cursor) {
        if tag.is_closing {
            cursor = tag.end;
            continue;
        }

        let object_kind = hwpx_fallback_object_kind(tag.name);
        let structural_kind = hwpx_fallback_structural_control_kind(tag.name);
        if object_kind.is_none() && structural_kind.is_none() {
            cursor = tag.end;
            continue;
        };

        let object_end = if tag.is_self_closing {
            tag.end
        } else {
            let Some(end) = find_matching_element_end(paragraph_xml, &tag) else {
                cursor = tag.end;
                continue;
            };
            end
        };

        push_paragraph_text_fragment_as_block(
            &mut blocks,
            &paragraph_xml[fragment_start..tag.start],
            pending_list.take(),
            paragraph_style.clone(),
            context,
        );

        if object_kind == Some("table") {
            let table_xml = &paragraph_xml[tag.start..object_end];
            if let Some(table) = extract_table_from_xml(table_xml, context) {
                blocks.push(Block::Table(table));
            }
        } else if object_kind == Some("image") {
            let object_xml = &paragraph_xml[tag.start..object_end];
            if let Some(image) = extract_hwpx_image_from_pic_xml(object_xml, context) {
                blocks.push(Block::Image(image));
            } else {
                blocks.push(Block::Unknown(unknown_hwpx_object_block(
                    "image", object_xml, context,
                )));
            }
        } else if let Some(object_kind) = object_kind {
            let object_xml = &paragraph_xml[tag.start..object_end];
            blocks.push(Block::Unknown(unknown_hwpx_object_block(
                object_kind,
                object_xml,
                context,
            )));
        } else if let Some(structural_kind) = structural_kind {
            let control_xml = &paragraph_xml[tag.start..object_end];
            push_hwpx_structural_control(structural_kind, control_xml, context, headers, footers);
        }

        fragment_start = object_end;
        cursor = object_end;
    }

    push_paragraph_text_fragment_as_block(
        &mut blocks,
        &paragraph_xml[fragment_start..],
        pending_list.take(),
        paragraph_style,
        context,
    );
    blocks
}

fn hwpx_fallback_object_kind(tag_name: &str) -> Option<&'static str> {
    match tag_name {
        "tbl" => Some("table"),
        "pic" => Some("image"),
        "equation" => Some("equation"),
        "line" | "rect" | "ellipse" | "arc" | "polygon" | "curve" | "connectLine" | "container" => {
            Some("shape")
        }
        "chart" => Some("chart"),
        _ => None,
    }
}

fn hwpx_fallback_structural_control_kind(tag_name: &str) -> Option<&'static str> {
    match tag_name {
        "header" => Some("header"),
        "footer" => Some("footer"),
        _ => None,
    }
}

fn push_hwpx_structural_control(
    control_kind: &str,
    control_xml: &str,
    context: &mut HwpxFallbackContext,
    headers: &mut Vec<HeaderFooter>,
    footers: &mut Vec<HeaderFooter>,
) {
    let blocks = extract_section_xml_blocks(control_xml, context);
    if blocks.is_empty() {
        return;
    }

    let header_footer = HeaderFooter {
        placement: hwpx_header_footer_placement(control_xml),
        blocks,
    };

    match control_kind {
        "header" => headers.push(header_footer),
        "footer" => footers.push(header_footer),
        _ => {}
    }
}

fn hwpx_header_footer_placement(control_xml: &str) -> HeaderFooterPlacement {
    match first_xml_attribute_value(control_xml, "applyPageType") {
        Some("EVEN") => HeaderFooterPlacement::EvenPage,
        Some("ODD") => HeaderFooterPlacement::OddPage,
        Some("FIRST" | "FIRST_PAGE") => HeaderFooterPlacement::FirstPage,
        _ => HeaderFooterPlacement::Default,
    }
}

fn extract_hwpx_image_from_pic_xml(
    pic_xml: &str,
    context: &mut HwpxFallbackContext,
) -> Option<Image> {
    let binary_item_id_ref = first_xml_attribute_value(pic_xml, "binaryItemIDRef")?;
    let resource_id = context.ensure_image_resource(binary_item_id_ref)?;

    Some(Image {
        resource_id,
        alt: first_non_empty_string([
            decoded_first_xml_attribute_value(pic_xml, "description"),
            decoded_first_xml_attribute_value(pic_xml, "desc"),
            decoded_first_xml_attribute_value(pic_xml, "name"),
        ]),
        caption: None,
        width: hwpx_object_dimension_to_px(pic_xml, "width"),
        height: hwpx_object_dimension_to_px(pic_xml, "height"),
    })
}

fn hwpx_object_dimension_to_px(pic_xml: &str, attribute_name: &str) -> Option<LengthPx> {
    first_xml_attribute_value(pic_xml, attribute_name)
        .and_then(|value| value.parse::<u32>().ok())
        .and_then(hwp_units_to_px_option)
}

fn unknown_hwpx_object_block(
    object_kind: &str,
    object_xml: &str,
    context: &mut HwpxFallbackContext,
) -> UnknownBlock {
    let object_text =
        inlines_to_plain_text(&extract_inlines_from_xml_fragment(object_xml, context));
    let fallback_text = if object_text.is_empty() {
        format!("[{object_kind}]")
    } else {
        format!("[{object_kind}]\n{object_text}")
    };

    UnknownBlock {
        kind: format!("hwpx:{object_kind}"),
        fallback_text: Some(fallback_text),
        message: Some(
            "HWPX section XML fallback preserved an unsupported object placeholder.".to_string(),
        ),
        source: Some("Contents/section*.xml".to_string()),
    }
}

fn push_paragraph_text_fragment_as_block(
    blocks: &mut Vec<Block>,
    xml: &str,
    list: Option<ListInfo>,
    style: ParagraphStyle,
    context: &mut HwpxFallbackContext,
) {
    let inlines = extract_inlines_from_xml_fragment(xml, context);
    if inlines.is_empty() {
        return;
    }

    blocks.push(Block::Paragraph(Paragraph {
        role: Default::default(),
        inlines,
        style,
        style_ref: None,
        list,
    }));
}

#[derive(Clone, Debug, PartialEq)]
struct HwpxActiveField {
    id: Option<String>,
    field_type: String,
    name: Option<String>,
    url: Option<String>,
    command: Option<String>,
    inlines: Vec<Inline>,
}

fn extract_inlines_from_xml_fragment(xml: &str, context: &mut HwpxFallbackContext) -> Vec<Inline> {
    let mut inlines = Vec::new();
    let mut text_buffer = String::new();
    let mut cursor = 0usize;
    let mut text_depth = 0usize;
    let mut current_style = TextStyle::default();
    let mut active_field: Option<HwpxActiveField> = None;

    while let Some(tag) = next_xml_tag(xml, cursor) {
        if text_depth > 0 && tag.start > cursor {
            text_buffer.push_str(&decode_xml_text(&xml[cursor..tag.start]));
        }

        match tag.name {
            "run" if !tag.is_closing => {
                push_text_buffer_to_hwpx_inline_target(
                    &mut inlines,
                    &mut active_field,
                    &mut text_buffer,
                    &current_style,
                );
                current_style = context.text_style_for_run(tag.raw);
            }
            "run" if tag.is_closing => {
                push_text_buffer_to_hwpx_inline_target(
                    &mut inlines,
                    &mut active_field,
                    &mut text_buffer,
                    &current_style,
                );
                current_style = TextStyle::default();
            }
            "t" if tag.is_closing => {
                text_depth = text_depth.saturating_sub(1);
            }
            "t" if !tag.is_closing && !tag.is_self_closing => {
                text_depth += 1;
            }
            "lineBreak" => {
                push_text_buffer_to_hwpx_inline_target(
                    &mut inlines,
                    &mut active_field,
                    &mut text_buffer,
                    &current_style,
                );
                push_hwpx_inline(&mut inlines, &mut active_field, Inline::LineBreak);
            }
            "tab" => {
                push_text_buffer_to_hwpx_inline_target(
                    &mut inlines,
                    &mut active_field,
                    &mut text_buffer,
                    &current_style,
                );
                push_hwpx_inline(&mut inlines, &mut active_field, Inline::Tab);
            }
            "bookmark" if !tag.is_closing => {
                push_text_buffer_to_hwpx_inline_target(
                    &mut inlines,
                    &mut active_field,
                    &mut text_buffer,
                    &current_style,
                );
                if let Some(bookmark) = hwpx_bookmark_inline(tag.raw) {
                    push_hwpx_inline(&mut inlines, &mut active_field, bookmark);
                }
            }
            "fieldBegin" if !tag.is_closing => {
                push_text_buffer_to_hwpx_inline_target(
                    &mut inlines,
                    &mut active_field,
                    &mut text_buffer,
                    &current_style,
                );
                if let Some(field) = active_field.take() {
                    inlines.push(finalize_hwpx_field(field));
                }

                let field_end = if tag.is_self_closing {
                    tag.end
                } else {
                    find_matching_element_end(xml, &tag).unwrap_or(tag.end)
                };
                active_field = Some(extract_hwpx_field_begin(
                    tag.raw,
                    &xml[tag.start..field_end],
                ));
                cursor = field_end;
                continue;
            }
            "fieldEnd" if !tag.is_closing => {
                push_text_buffer_to_hwpx_inline_target(
                    &mut inlines,
                    &mut active_field,
                    &mut text_buffer,
                    &current_style,
                );
                if let Some(field) = active_field.take() {
                    let begin_id = decoded_xml_attribute_value(tag.raw, "beginIDRef");
                    if field.id.as_deref() == begin_id.as_deref() || begin_id.is_none() {
                        inlines.push(finalize_hwpx_field(field));
                    } else {
                        inlines.push(finalize_hwpx_field(field));
                        inlines.push(unknown_hwpx_field_end_inline(tag.raw));
                    }
                } else {
                    inlines.push(unknown_hwpx_field_end_inline(tag.raw));
                }
            }
            "footNote" | "endNote" if !tag.is_closing => {
                push_text_buffer_to_hwpx_inline_target(
                    &mut inlines,
                    &mut active_field,
                    &mut text_buffer,
                    &current_style,
                );
                let note_end = if tag.is_self_closing {
                    tag.end
                } else {
                    find_matching_element_end(xml, &tag).unwrap_or(tag.end)
                };
                let note_kind = if tag.name == "footNote" {
                    NoteKind::Footnote
                } else {
                    NoteKind::Endnote
                };
                let note_ref = context.store_note_from_hwpx_control(
                    note_kind,
                    tag.raw,
                    &xml[tag.start..note_end],
                );
                push_hwpx_inline(&mut inlines, &mut active_field, note_ref);
                cursor = note_end;
                continue;
            }
            _ => {}
        }

        cursor = tag.end;
    }

    if text_depth > 0 && cursor < xml.len() {
        text_buffer.push_str(&decode_xml_text(&xml[cursor..]));
    }

    push_text_buffer_to_hwpx_inline_target(
        &mut inlines,
        &mut active_field,
        &mut text_buffer,
        &current_style,
    );
    if let Some(field) = active_field.take() {
        inlines.push(finalize_hwpx_field(field));
    }
    trim_trailing_empty_break_inlines(&mut inlines);
    inlines
}

fn push_text_buffer_to_hwpx_inline_target(
    inlines: &mut Vec<Inline>,
    active_field: &mut Option<HwpxActiveField>,
    text_buffer: &mut String,
    style: &TextStyle,
) {
    if let Some(field) = active_field {
        push_text_buffer_as_inline(&mut field.inlines, text_buffer, style);
    } else {
        push_text_buffer_as_inline(inlines, text_buffer, style);
    }
}

fn push_hwpx_inline(
    inlines: &mut Vec<Inline>,
    active_field: &mut Option<HwpxActiveField>,
    inline: Inline,
) {
    if let Some(field) = active_field {
        field.inlines.push(inline);
    } else {
        inlines.push(inline);
    }
}

fn extract_hwpx_field_begin(tag: &str, field_xml: &str) -> HwpxActiveField {
    let field_type =
        decoded_xml_attribute_value(tag, "type").unwrap_or_else(|| "UNKNOWN".to_string());
    let name = decoded_xml_attribute_value(tag, "name");
    let command = first_non_empty_string([
        decoded_xml_attribute_value(tag, "command"),
        hwpx_field_parameter_value(
            field_xml,
            &["command", "Command", "cmd", "hyperlink", "Hyperlink"],
        ),
    ]);
    let url = first_non_empty_string([
        decoded_xml_attribute_value(tag, "href"),
        decoded_xml_attribute_value(tag, "url"),
        command.clone().filter(|value| is_hwpx_url_like(value)),
        hwpx_field_parameter_value(
            field_xml,
            &["url", "URL", "href", "HRef", "target", "Target"],
        ),
        name.clone().filter(|value| is_hwpx_url_like(value)),
    ]);

    HwpxActiveField {
        id: decoded_xml_attribute_value(tag, "id"),
        field_type,
        name,
        url,
        command,
        inlines: Vec::new(),
    }
}

fn finalize_hwpx_field(field: HwpxActiveField) -> Inline {
    if field.field_type.eq_ignore_ascii_case("HYPERLINK")
        && let Some(url) = field.url.clone()
    {
        let label = first_non_empty_string([
            non_empty_string_owned(inlines_to_plain_text(&field.inlines)),
            field.name.clone().filter(|value| !is_hwpx_url_like(value)),
            Some(url.clone()),
        ])
        .unwrap_or_else(|| url.clone());
        let inlines = if field.inlines.is_empty() {
            vec![Inline::Text(TextRun {
                text: label,
                style: TextStyle::default(),
                style_ref: None,
            })]
        } else {
            field.inlines
        };

        return Inline::Link(Link {
            url,
            title: field.name.filter(|value| !is_hwpx_url_like(value)),
            inlines,
        });
    }

    let kind = hwpx_field_unknown_kind(&field.field_type);
    let fallback_text = first_non_empty_string([
        non_empty_string_owned(inlines_to_plain_text(&field.inlines)),
        field.name.clone(),
        field.command.clone(),
        field.url.clone(),
    ])
    .unwrap_or_else(|| format!("[{}]", kind));

    Inline::Unknown(UnknownInline {
        kind,
        fallback_text: Some(fallback_text),
        message: Some("HWPX section XML fallback preserved a field as fallback text.".to_string()),
        source: Some("Contents/section*.xml".to_string()),
    })
}

fn hwpx_bookmark_inline(tag: &str) -> Option<Inline> {
    let name = decoded_xml_attribute_value(tag, "name")?;
    Some(Inline::Anchor {
        id: crate::util::plain_text::sanitize_anchor_id(&name),
    })
}

fn unknown_hwpx_field_end_inline(tag: &str) -> Inline {
    let fallback_text = decoded_xml_attribute_value(tag, "beginIDRef")
        .map(|id| format!("[field_end:{id}]"))
        .unwrap_or_else(|| "[field_end]".to_string());

    Inline::Unknown(UnknownInline {
        kind: "hwpx:field_end".to_string(),
        fallback_text: Some(fallback_text),
        message: Some("HWPX fieldEnd appeared without a matching fieldBegin.".to_string()),
        source: Some("Contents/section*.xml".to_string()),
    })
}

fn hwpx_field_unknown_kind(field_type: &str) -> String {
    let mut normalized = String::new();
    for ch in field_type.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
        } else if ch == '_' || ch == '-' || ch.is_whitespace() {
            normalized.push('_');
        }
    }

    if normalized.is_empty() {
        "hwpx:field:unknown".to_string()
    } else {
        format!("hwpx:field:{normalized}")
    }
}

fn hwpx_field_parameter_value(field_xml: &str, names: &[&str]) -> Option<String> {
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(field_xml, cursor) {
        if tag.is_closing || !tag.name.ends_with("Param") || tag.name == "listParam" {
            cursor = tag.end;
            continue;
        }

        let Some(parameter_name) = xml_attribute_value(tag.raw, "name") else {
            cursor = tag.end;
            continue;
        };
        if !names
            .iter()
            .any(|name| parameter_name.eq_ignore_ascii_case(name))
        {
            cursor = tag.end;
            continue;
        }

        if let Some(value) = decoded_xml_attribute_value(tag.raw, "value")
            && !value.trim().is_empty()
        {
            return Some(value);
        }

        if !tag.is_self_closing
            && let Some(parameter_end) = find_matching_element_end(field_xml, &tag)
            && let Some(value) = simple_xml_element_text(&field_xml[tag.end..parameter_end])
        {
            return Some(value);
        }

        cursor = tag.end;
    }

    None
}

fn simple_xml_element_text(xml: &str) -> Option<String> {
    let text = xml
        .rsplit_once("</")
        .map(|(before_close, _)| before_close)
        .unwrap_or(xml);
    non_empty_string_owned(decode_xml_text(text))
}

fn decoded_xml_attribute_value(tag: &str, attribute_name: &str) -> Option<String> {
    xml_attribute_value(tag, attribute_name)
        .map(decode_xml_text)
        .and_then(non_empty_string_owned)
}

fn decoded_first_xml_attribute_value(xml: &str, attribute_name: &str) -> Option<String> {
    first_xml_attribute_value(xml, attribute_name)
        .map(decode_xml_text)
        .and_then(non_empty_string_owned)
}

fn first_non_empty_string(values: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

fn non_empty_string_owned(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn is_hwpx_url_like(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with('#')
        || trimmed.contains("://")
        || trimmed.starts_with("mailto:")
        || trimmed.starts_with("tel:")
}

fn inlines_to_plain_text(inlines: &[Inline]) -> String {
    let mut text = String::new();

    for inline in inlines {
        match inline {
            Inline::Text(run) => text.push_str(&run.text),
            Inline::LineBreak => text.push('\n'),
            Inline::Tab => text.push('\t'),
            Inline::Link(link) => text.push_str(&inlines_to_plain_text(&link.inlines)),
            Inline::FootnoteRef { note_id } | Inline::EndnoteRef { note_id } => {
                text.push_str(note_id.as_str());
            }
            Inline::Unknown(unknown) => {
                if let Some(fallback_text) = &unknown.fallback_text {
                    text.push_str(fallback_text);
                }
            }
            Inline::Anchor { .. } => {}
        }
    }

    text.trim_end().to_string()
}

fn push_text_buffer_as_inline(
    inlines: &mut Vec<Inline>,
    text_buffer: &mut String,
    style: &TextStyle,
) {
    if text_buffer.is_empty() {
        return;
    }

    inlines.push(Inline::Text(TextRun {
        text: std::mem::take(text_buffer),
        style: style.clone(),
        style_ref: None,
    }));
}

fn trim_trailing_empty_break_inlines(inlines: &mut Vec<Inline>) {
    while matches!(inlines.last(), Some(Inline::LineBreak | Inline::Tab)) {
        inlines.pop();
    }
}

fn first_xml_attribute_u32(xml: &str, tag_name: &str, attribute_name: &str) -> Option<u32> {
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(xml, cursor) {
        if tag.name == tag_name
            && !tag.is_closing
            && let Some(value) = xml_attribute_value(tag.raw, attribute_name)
        {
            return value.parse().ok();
        }
        cursor = tag.end;
    }

    None
}

fn first_xml_attribute_value<'a>(xml: &'a str, attribute_name: &str) -> Option<&'a str> {
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(xml, cursor) {
        if !tag.is_closing
            && let Some(value) = xml_attribute_value(tag.raw, attribute_name)
        {
            return Some(value);
        }
        cursor = tag.end;
    }

    None
}

fn map_hwpx_alignment(value: &str) -> Option<Alignment> {
    Some(match value {
        "LEFT" => Alignment::Left,
        "CENTER" => Alignment::Center,
        "RIGHT" => Alignment::Right,
        "JUSTIFY" | "DISTRIBUTE" | "DISTRIBUTE_SPACE" => Alignment::Justify,
        _ => return None,
    })
}

fn xml_attribute_hwp_units_to_pt(tag: &str, attribute_name: &str) -> Option<LengthPt> {
    xml_attribute_value(tag, attribute_name)
        .and_then(|value| value.parse::<i32>().ok())
        .and_then(hwp_units_to_pt_option)
}

fn hwp_units_to_pt_option(value: i32) -> Option<LengthPt> {
    if value == 0 {
        None
    } else {
        Some(LengthPt(value as f32 / 100.0))
    }
}

fn hwp_units_to_px_option(value: u32) -> Option<LengthPx> {
    if value == 0 {
        None
    } else {
        Some(LengthPx(value as f32 / 75.0))
    }
}

fn parse_hwpx_hex_color(value: &str) -> Option<Color> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }

    Some(Color {
        r: u8::from_str_radix(&hex[0..2], 16).ok()?,
        g: u8::from_str_radix(&hex[2..4], 16).ok()?,
        b: u8::from_str_radix(&hex[4..6], 16).ok()?,
        a: 255,
    })
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

fn read_zip_binary_entry<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
) -> io::Result<Vec<u8>> {
    let mut file = archive.by_name(path).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("{path} entry was not found"),
        )
    })?;
    if file.size() > MAX_HWPX_IMAGE_RESOURCE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{path} is larger than the HWPX fallback image limit"),
        ));
    }

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| {
        io::Error::new(error.kind(), format!("{path} could not be read: {error}"))
    })?;

    Ok(bytes)
}

fn is_section_xml_path(path: &str) -> bool {
    section_xml_index(path).is_some()
}

fn section_xml_index(path: &str) -> Option<u32> {
    let file_name = path.strip_prefix("Contents/section")?;

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
            Some("p") if is_closing && paragraph_depth > 0 => {
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
            Some("lineBreak") if paragraph_depth > 0 => current.push('\n'),
            Some("tab") if paragraph_depth > 0 => current.push('\t'),
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
    fn extracts_paragraph_breaks_and_tabs_outside_text_nodes() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p><hp:run><hp:t>line one</hp:t><hp:lineBreak/><hp:t>line two</hp:t></hp:run></hp:p>
              <hp:p><hp:run><hp:t>tab one</hp:t><hp:tab width="4000"/><hp:t>tab two</hp:t></hp:run></hp:p>
            </hs:sec>
        "#;

        let paragraphs = extract_section_xml_paragraphs(xml);

        assert_eq!(
            paragraphs,
            vec![
                "line one\nline two".to_string(),
                "tab one\ttab two".to_string()
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

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);

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
    fn preserves_hwpx_fallback_object_placeholders() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p>
                <hp:run><hp:t>before image</hp:t></hp:run>
                <hp:ctrl><hp:pic><hp:imgRect/></hp:pic></hp:ctrl>
                <hp:run><hp:t>after image</hp:t></hp:run>
                <hp:ctrl><hp:equation/></hp:ctrl>
              </hp:p>
            </hs:sec>
        "#;

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);

        assert_eq!(blocks.len(), 4);
        match &blocks[0] {
            Block::Paragraph(paragraph) => match &paragraph.inlines[0] {
                crate::ir::Inline::Text(run) => assert_eq!(run.text, "before image"),
                other => panic!("expected text inline, got {other:?}"),
            },
            other => panic!("expected paragraph block, got {other:?}"),
        }
        assert!(matches!(
            &blocks[1],
            Block::Unknown(unknown)
                if unknown.kind == "hwpx:image"
                    && unknown.fallback_text.as_deref() == Some("[image]")
        ));
        match &blocks[2] {
            Block::Paragraph(paragraph) => match &paragraph.inlines[0] {
                crate::ir::Inline::Text(run) => assert_eq!(run.text, "after image"),
                other => panic!("expected text inline, got {other:?}"),
            },
            other => panic!("expected paragraph block, got {other:?}"),
        }
        assert!(matches!(
            &blocks[3],
            Block::Unknown(unknown)
                if unknown.kind == "hwpx:equation"
                    && unknown.fallback_text.as_deref() == Some("[equation]")
        ));
    }

    #[test]
    fn preserves_text_inside_hwpx_unsupported_object_placeholders() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p>
                <hp:ctrl>
                  <hp:rect>
                    <hp:run><hp:t>shape text</hp:t></hp:run>
                  </hp:rect>
                </hp:ctrl>
              </hp:p>
            </hs:sec>
        "#;

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);

        assert!(matches!(
            &blocks[0],
            Block::Unknown(unknown)
                if unknown.kind == "hwpx:shape"
                    && unknown.fallback_text.as_deref() == Some("[shape]\nshape text")
        ));
    }

    #[test]
    fn recovers_hwpx_hyperlink_field_as_link_inline() {
        let xml = r#"
            <hp:p xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:ctrl>
                <hp:fieldBegin id="7" type="HYPERLINK" name="Example">
                  <hp:parameters cnt="1">
                    <hp:stringParam name="URL">https://example.com</hp:stringParam>
                  </hp:parameters>
                </hp:fieldBegin>
              </hp:ctrl>
              <hp:run><hp:t>Example Site</hp:t></hp:run>
              <hp:ctrl><hp:fieldEnd beginIDRef="7"/></hp:ctrl>
            </hp:p>
        "#;

        let mut context = HwpxFallbackContext::default();
        let inlines = extract_inlines_from_xml_fragment(xml, &mut context);

        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            Inline::Link(link) => {
                assert_eq!(link.url, "https://example.com");
                assert_eq!(link.title.as_deref(), Some("Example"));
                assert_eq!(inlines_to_plain_text(&link.inlines), "Example Site");
            }
            other => panic!("expected link inline, got {other:?}"),
        }
    }

    #[test]
    fn preserves_hwpx_bookmark_as_anchor_inline() {
        let xml = r#"
            <hp:p xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:ctrl><hp:bookmark name="Target Bookmark"/></hp:ctrl>
              <hp:run><hp:t>target text</hp:t></hp:run>
            </hp:p>
        "#;

        let mut context = HwpxFallbackContext::default();
        let inlines = extract_inlines_from_xml_fragment(xml, &mut context);

        assert_eq!(inlines.len(), 2);
        assert!(matches!(
            &inlines[0],
            Inline::Anchor { id } if id == "Target-Bookmark"
        ));
        assert!(matches!(
            &inlines[1],
            Inline::Text(run) if run.text == "target text"
        ));
    }

    #[test]
    fn preserves_hwpx_non_link_field_as_unknown_inline() {
        let xml = r#"
            <hp:p xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:ctrl>
                <hp:fieldBegin id="9" type="DATE" name="created date"/>
              </hp:ctrl>
              <hp:run><hp:t>2026-06-13</hp:t></hp:run>
              <hp:ctrl><hp:fieldEnd beginIDRef="9"/></hp:ctrl>
            </hp:p>
        "#;

        let mut context = HwpxFallbackContext::default();
        let inlines = extract_inlines_from_xml_fragment(xml, &mut context);

        assert_eq!(inlines.len(), 1);
        assert!(matches!(
            &inlines[0],
            Inline::Unknown(unknown)
                if unknown.kind == "hwpx:field:date"
                    && unknown.fallback_text.as_deref() == Some("2026-06-13")
        ));
    }

    #[test]
    fn recovers_list_info_from_hwpx_header_paragraph_properties() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[
            (
                HEADER_XML_PATH,
                r#"
                <hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head">
                  <hh:refList>
                    <hh:paraProperties>
                      <hh:paraPr id="0"><hh:heading type="BULLET" idRef="1" level="0"/></hh:paraPr>
                      <hh:paraPr id="1"><hh:heading type="NUMBER" idRef="1" level="0"/></hh:paraPr>
                      <hh:paraPr id="2"><hh:heading type="NUMBER" idRef="2" level="0"/></hh:paraPr>
                    </hh:paraProperties>
                  </hh:refList>
                </hh:head>
                "#,
            ),
            (
                "Contents/section0.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p paraPrIDRef="0"><hp:run><hp:t>bullet item</hp:t></hp:run></hp:p>
                  <hp:p paraPrIDRef="1"><hp:run><hp:t>first item</hp:t></hp:run></hp:p>
                  <hp:p paraPrIDRef="1"><hp:run><hp:t>second item</hp:t></hp:run></hp:p>
                  <hp:p paraPrIDRef="2"><hp:run><hp:t>new list first item</hp:t></hp:run></hp:p>
                </hs:sec>
                "#,
            ),
        ])?;

        let document = read_section_document_from_archive(&bytes)?;
        let paragraphs = document.sections[0]
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(paragraph) => Some(paragraph),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(paragraphs.len(), 4);
        assert_eq!(
            paragraphs[0].list.as_ref().map(|list| &list.kind),
            Some(&ListKind::Unordered)
        );
        assert_eq!(
            paragraphs[0]
                .list
                .as_ref()
                .and_then(|list| list.marker.as_deref()),
            Some("•")
        );
        assert_eq!(
            paragraphs[1]
                .list
                .as_ref()
                .map(|list| (&list.kind, list.number)),
            Some((&ListKind::Ordered, Some(1)))
        );
        assert_eq!(
            paragraphs[2]
                .list
                .as_ref()
                .map(|list| (&list.kind, list.number)),
            Some((&ListKind::Ordered, Some(2)))
        );
        assert_eq!(
            paragraphs[3]
                .list
                .as_ref()
                .map(|list| (&list.kind, list.number)),
            Some((&ListKind::Ordered, Some(1)))
        );

        Ok(())
    }

    #[test]
    fn recovers_paragraph_style_from_hwpx_header_paragraph_properties() -> Result<(), Box<dyn Error>>
    {
        let bytes = create_archive_bytes(&[
            (
                HEADER_XML_PATH,
                r#"
                <hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head">
                  <hh:refList>
                    <hh:paraProperties>
                      <hh:paraPr id="0">
                        <hh:align horizontal="CENTER"/>
                        <hh:margin>
                          <hh:intent unit="HWPUNIT" value="100"/>
                          <hh:left unit="HWPUNIT" value="200"/>
                          <hh:right unit="HWPUNIT" value="300"/>
                          <hh:prev unit="HWPUNIT" value="400"/>
                          <hh:next unit="HWPUNIT" value="500"/>
                        </hh:margin>
                        <hh:lineSpacing type="FIXED" value="600" unit="HWPUNIT"/>
                      </hh:paraPr>
                    </hh:paraProperties>
                  </hh:refList>
                </hh:head>
                "#,
            ),
            (
                "Contents/section0.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p paraPrIDRef="0"><hp:run><hp:t>styled paragraph</hp:t></hp:run></hp:p>
                </hs:sec>
                "#,
            ),
        ])?;

        let document = read_section_document_from_archive(&bytes)?;
        let paragraph = match &document.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => paragraph,
            other => panic!("expected paragraph block, got {other:?}"),
        };

        assert_eq!(paragraph.style.alignment, Some(Alignment::Center));
        assert_eq!(paragraph.style.indent.first_line_pt, Some(LengthPt(1.0)));
        assert_eq!(paragraph.style.indent.left_pt, Some(LengthPt(2.0)));
        assert_eq!(paragraph.style.indent.right_pt, Some(LengthPt(3.0)));
        assert_eq!(paragraph.style.spacing.before_pt, Some(LengthPt(4.0)));
        assert_eq!(paragraph.style.spacing.after_pt, Some(LengthPt(5.0)));
        assert_eq!(paragraph.style.spacing.line_pt, Some(LengthPt(6.0)));

        Ok(())
    }

    #[test]
    fn recovers_text_style_from_hwpx_header_char_properties() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[
            (
                HEADER_XML_PATH,
                r##"
                <hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head">
                  <hh:refList>
                    <hh:fontfaces>
                      <hh:fontface lang="HANGUL"><hh:font id="0" face="Noto Sans KR"/></hh:fontface>
                    </hh:fontfaces>
                    <hh:charProperties>
                      <hh:charPr id="7" height="1200" textColor="#010203" shadeColor="#040506">
                        <hh:fontRef hangul="0"/>
                        <hh:bold/>
                        <hh:italic/>
                        <hh:underline/>
                        <hh:strikeout/>
                      </hh:charPr>
                    </hh:charProperties>
                  </hh:refList>
                </hh:head>
                "##,
            ),
            (
                "Contents/section0.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p><hp:run charPrIDRef="7"><hp:t>styled text</hp:t></hp:run></hp:p>
                </hs:sec>
                "#,
            ),
        ])?;

        let document = read_section_document_from_archive(&bytes)?;
        let text_run = match &document.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => match &paragraph.inlines[0] {
                Inline::Text(run) => run,
                other => panic!("expected text run, got {other:?}"),
            },
            other => panic!("expected paragraph block, got {other:?}"),
        };

        assert_eq!(text_run.text, "styled text");
        assert_eq!(text_run.style.font_family.as_deref(), Some("Noto Sans KR"));
        assert_eq!(text_run.style.font_size_pt, Some(LengthPt(12.0)));
        assert_eq!(
            text_run.style.color,
            Some(Color {
                r: 1,
                g: 2,
                b: 3,
                a: 255,
            })
        );
        assert_eq!(
            text_run.style.background_color,
            Some(Color {
                r: 4,
                g: 5,
                b: 6,
                a: 255,
            })
        );
        assert!(text_run.style.bold);
        assert!(text_run.style.italic);
        assert!(text_run.style.underline);
        assert!(text_run.style.strike);

        Ok(())
    }

    #[test]
    fn recovers_table_and_cell_background_from_hwpx_border_fill() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[
            (
                HEADER_XML_PATH,
                r##"
                <hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head"
                         xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core">
                  <hh:refList>
                    <hh:borderFills>
                      <hh:borderFill id="3"><hc:fillBrush><hc:winBrush faceColor="#112233"/></hc:fillBrush></hh:borderFill>
                      <hh:borderFill id="4"><hc:fillBrush><hc:winBrush faceColor="#445566"/></hc:fillBrush></hh:borderFill>
                    </hh:borderFills>
                  </hh:refList>
                </hh:head>
                "##,
            ),
            (
                "Contents/section0.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:tbl borderFillIDRef="3">
                    <hp:tr>
                      <hp:tc borderFillIDRef="4">
                        <hp:subList>
                          <hp:p><hp:run><hp:t>cell</hp:t></hp:run></hp:p>
                        </hp:subList>
                      </hp:tc>
                    </hp:tr>
                  </hp:tbl>
                </hs:sec>
                "#,
            ),
        ])?;

        let document = read_section_document_from_archive(&bytes)?;
        let table = match &document.sections[0].blocks[0] {
            Block::Table(table) => table,
            other => panic!("expected table block, got {other:?}"),
        };

        assert_eq!(
            table.style.background_color,
            Some(Color {
                r: 0x11,
                g: 0x22,
                b: 0x33,
                a: 255,
            })
        );
        assert_eq!(
            table.rows[0].cells[0].style.background_color,
            Some(Color {
                r: 0x44,
                g: 0x55,
                b: 0x66,
                a: 255,
            })
        );

        Ok(())
    }

    #[test]
    fn recovers_hwpx_image_resource_from_manifest() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[
            (
                "Contents/content.hpf",
                r#"
                <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
                  <opf:manifest>
                    <opf:item id="image1" href="BinData/image1.png" media-type="image/png"/>
                    <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
                  </opf:manifest>
                  <opf:spine><opf:itemref idref="section0"/></opf:spine>
                </opf:package>
                "#,
            ),
            (
                "Contents/section0.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"
                        xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core">
                  <hp:p>
                    <hp:run><hp:t>before image</hp:t></hp:run>
                    <hp:ctrl>
                      <hp:pic description="sample image">
                        <hp:sz width="7500" height="3750"/>
                        <hp:img><hc:img binaryItemIDRef="image1"/></hp:img>
                      </hp:pic>
                    </hp:ctrl>
                    <hp:run><hp:t>after image</hp:t></hp:run>
                  </hp:p>
                </hs:sec>
                "#,
            ),
            ("BinData/image1.png", "fake-png-bytes"),
        ])?;

        let document = read_section_document_from_archive(&bytes)?;

        assert_eq!(document.resources.entries.len(), 1);
        let image = match &document.sections[0].blocks[1] {
            Block::Image(image) => image,
            other => panic!("expected image block, got {other:?}"),
        };
        assert_eq!(image.resource_id.as_str(), "image1");
        assert_eq!(image.alt.as_deref(), Some("sample image"));
        assert_eq!(image.width, Some(LengthPx(100.0)));
        assert_eq!(image.height, Some(LengthPx(50.0)));

        match document.resources.get(&ResourceId("image1".to_string())) {
            Some(Resource::Image(resource)) => {
                assert_eq!(resource.media_type.as_deref(), Some("image/png"));
                assert_eq!(resource.extension.as_deref(), Some("png"));
                assert_eq!(resource.bytes, b"fake-png-bytes");
            }
            other => panic!("expected image resource, got {other:?}"),
        }

        Ok(())
    }

    #[test]
    fn preserves_section_boundaries_in_hwpx_section_fallback() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[
            (
                "Contents/section0.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p><hp:run><hp:t>first section</hp:t></hp:run></hp:p>
                </hs:sec>
                "#,
            ),
            (
                "Contents/section1.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p><hp:run><hp:t>second section</hp:t></hp:run></hp:p>
                </hs:sec>
                "#,
            ),
        ])?;

        let document = read_section_document_from_archive(&bytes)?;

        assert_eq!(document.sections.len(), 2);
        assert_eq!(
            section_first_paragraph_text(&document.sections[0]),
            Some("first section".to_string())
        );
        assert_eq!(
            section_first_paragraph_text(&document.sections[1]),
            Some("second section".to_string())
        );

        Ok(())
    }

    #[test]
    fn recovers_header_footer_controls_from_hwpx_section_fallback() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[(
            "Contents/section0.xml",
            r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p>
                    <hp:run><hp:t>body text</hp:t></hp:run>
                    <hp:ctrl>
                      <hp:header applyPageType="ODD">
                        <hp:subList>
                          <hp:p><hp:run><hp:t>header text</hp:t></hp:run></hp:p>
                        </hp:subList>
                      </hp:header>
                    </hp:ctrl>
                    <hp:ctrl>
                      <hp:footer applyPageType="EVEN">
                        <hp:subList>
                          <hp:p><hp:run><hp:t>footer text</hp:t></hp:run></hp:p>
                        </hp:subList>
                      </hp:footer>
                    </hp:ctrl>
                  </hp:p>
                </hs:sec>
                "#,
        )])?;

        let document = read_section_document_from_archive(&bytes)?;
        let section = &document.sections[0];

        assert_eq!(section.blocks.len(), 1);
        assert_eq!(section.headers.len(), 1);
        assert_eq!(section.footers.len(), 1);
        assert_eq!(section.headers[0].placement, HeaderFooterPlacement::OddPage);
        assert_eq!(
            section.footers[0].placement,
            HeaderFooterPlacement::EvenPage
        );
        assert_eq!(
            section_first_paragraph_text(&crate::ir::Section {
                blocks: section.headers[0].blocks.clone(),
                ..Default::default()
            }),
            Some("header text".to_string())
        );
        assert_eq!(
            section_first_paragraph_text(&crate::ir::Section {
                blocks: section.footers[0].blocks.clone(),
                ..Default::default()
            }),
            Some("footer text".to_string())
        );
        assert_eq!(
            section_first_paragraph_text(section),
            Some("body text".to_string())
        );

        Ok(())
    }

    #[test]
    fn recovers_note_controls_from_hwpx_section_fallback() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[(
            "Contents/section0.xml",
            r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p>
                    <hp:run><hp:t>before</hp:t></hp:run>
                    <hp:ctrl>
                      <hp:footNote instId="3">
                        <hp:subList>
                          <hp:p><hp:run><hp:t>footnote text</hp:t></hp:run></hp:p>
                        </hp:subList>
                      </hp:footNote>
                    </hp:ctrl>
                    <hp:run><hp:t>after</hp:t></hp:run>
                    <hp:ctrl>
                      <hp:endNote instId="4">
                        <hp:subList>
                          <hp:p><hp:run><hp:t>endnote text</hp:t></hp:run></hp:p>
                        </hp:subList>
                      </hp:endNote>
                    </hp:ctrl>
                  </hp:p>
                </hs:sec>
                "#,
        )])?;

        let document = read_section_document_from_archive(&bytes)?;
        let paragraph = match &document.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => paragraph,
            other => panic!("expected paragraph block, got {other:?}"),
        };

        assert_eq!(document.notes.notes.len(), 2);
        assert!(matches!(
            &paragraph.inlines[0],
            Inline::Text(run) if run.text == "before"
        ));
        assert!(matches!(
            &paragraph.inlines[1],
            Inline::FootnoteRef { note_id } if note_id.as_str() == "footnote-3"
        ));
        assert!(matches!(
            &paragraph.inlines[2],
            Inline::Text(run) if run.text == "after"
        ));
        assert!(matches!(
            &paragraph.inlines[3],
            Inline::EndnoteRef { note_id } if note_id.as_str() == "endnote-4"
        ));

        let footnote = document
            .notes
            .get(&NoteId("footnote-3".to_string()))
            .expect("footnote should be stored");
        let endnote = document
            .notes
            .get(&NoteId("endnote-4".to_string()))
            .expect("endnote should be stored");
        assert_eq!(footnote.kind, NoteKind::Footnote);
        assert_eq!(endnote.kind, NoteKind::Endnote);
        assert_eq!(
            blocks_first_paragraph_text(&footnote.blocks),
            Some("footnote text".to_string())
        );
        assert_eq!(
            blocks_first_paragraph_text(&endnote.blocks),
            Some("endnote text".to_string())
        );

        Ok(())
    }

    #[test]
    fn keeps_duplicate_hwpx_note_ids_without_dropping_notes() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[(
            "Contents/section0.xml",
            r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p>
                    <hp:run><hp:t>a</hp:t></hp:run>
                    <hp:ctrl><hp:footNote instId="3"><hp:subList><hp:p><hp:run><hp:t>first note</hp:t></hp:run></hp:p></hp:subList></hp:footNote></hp:ctrl>
                    <hp:run><hp:t>b</hp:t></hp:run>
                    <hp:ctrl><hp:footNote instId="3"><hp:subList><hp:p><hp:run><hp:t>second note</hp:t></hp:run></hp:p></hp:subList></hp:footNote></hp:ctrl>
                  </hp:p>
                </hs:sec>
                "#,
        )])?;

        let document = read_section_document_from_archive(&bytes)?;

        assert_eq!(document.notes.notes.len(), 2);
        assert!(
            document
                .notes
                .get(&NoteId("footnote-3".to_string()))
                .is_some()
        );
        assert!(
            document
                .notes
                .get(&NoteId("footnote-3-2".to_string()))
                .is_some()
        );

        Ok(())
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

    fn section_first_paragraph_text(section: &crate::ir::Section) -> Option<String> {
        blocks_first_paragraph_text(&section.blocks)
    }

    fn blocks_first_paragraph_text(blocks: &[Block]) -> Option<String> {
        let Block::Paragraph(paragraph) = blocks.first()? else {
            return None;
        };

        paragraph.inlines.iter().find_map(|inline| match inline {
            Inline::Text(run) => Some(run.text.clone()),
            _ => None,
        })
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
