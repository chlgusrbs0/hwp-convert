use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::Path;

use zip::ZipArchive;

use rhwp::renderer::{NumberFormat as RhwpNumberFormat, format_number as format_rhwp_number};

use crate::ir::{
    Alignment, Block, Border, BorderStyle, Chart, Color, ConversionWarning, Document, Equation,
    EquationKind, HeaderFooter, HeaderFooterPlacement, Image, ImageResource, Inline, LengthPt,
    LengthPx, Link, ListInfo, ListKind, Metadata, Note, NoteId, NoteKind, NoteStore, Paragraph,
    ParagraphRole, ParagraphStyle, Percent, Resource, ResourceId, ResourceStore, Section, Shape,
    ShapeKind, StyleSheet, Table, TableCell, TableCellStyle, TableRow, TableStyle,
    TextDecorationStyle, TextRun, TextStyle, UnknownBlock, UnknownInline, VerticalAlign,
    WarningCode,
};

const PREVIEW_TEXT_PATH: &str = "Preview/PrvText.txt";
const CONTENT_HPF_PATH: &str = "Contents/content.hpf";
const HEADER_XML_PATH: &str = "Contents/header.xml";
const HWPX_BINARY_ITEM_ID_REF_ATTRIBUTES: &[&str] = &[
    "binaryItemIDRef",
    "binaryItemIdRef",
    "binaryItemIDREF",
    "binaryItemId",
    "binaryItemID",
    "binItem",
];
const HWPX_BOOKMARK_NAME_ATTRIBUTES: &[&str] = &["name", "id", "title"];
const HWPX_BORDER_COLOR_ATTRIBUTES: &[&str] = &["color", "lineColor"];
const HWPX_BORDER_FILL_ID_REF_ATTRIBUTES: &[&str] =
    &["borderFillIDRef", "borderFillIdRef", "borderFillIDREF"];
const HWPX_BORDER_STYLE_ATTRIBUTES: &[&str] = &["type", "style"];
const HWPX_BORDER_WIDTH_ATTRIBUTES: &[&str] = &["width", "w"];
const HWPX_BULLET_MARKER_ATTRIBUTES: &[&str] = &[
    "char",
    "bulletChar",
    "marker",
    "bulletMarker",
    "mark",
    "symbol",
];
const HWPX_CAPTION_PLACEMENT_ATTRIBUTES: &[&str] = &["side", "position", "pos", "placement"];
const HWPX_CHART_TITLE_ATTRIBUTES: &[&str] =
    &["title", "chartTitle", "name", "description", "desc"];
const HWPX_CHAR_PR_ID_REF_ATTRIBUTES: &[&str] = &["charPrIDRef", "charPrIdRef", "charPrIDREF"];
const HWPX_DECORATION_COLOR_ATTRIBUTES: &[&str] = &["color", "lineColor"];
const HWPX_DESCRIPTION_ATTRIBUTES: &[&str] =
    &["description", "desc", "alt", "altText", "name", "title"];
const HWPX_EQUATION_CONTENT_ATTRIBUTES: &[&str] = &["script", "formula", "text", "equation"];
const HWPX_FILL_COLOR_ATTRIBUTES: &[&str] = &["faceColor", "backgroundColor", "fillColor"];
const HWPX_FALLBACK_TEXT_ATTRIBUTES: &[&str] = &[
    "description",
    "desc",
    "alt",
    "altText",
    "name",
    "title",
    "text",
    "value",
];
const HWPX_FIELD_BEGIN_ID_REF_ATTRIBUTES: &[&str] = &["beginIDRef", "beginIdRef", "beginIDREF"];
const HWPX_FIELD_COMMAND_ATTRIBUTES: &[&str] = &["command", "cmd"];
const HWPX_FIELD_ID_ATTRIBUTES: &[&str] = &["id", "instId"];
const HWPX_FIELD_NAME_ATTRIBUTES: &[&str] = &["name", "title", "desc", "description"];
const HWPX_FIELD_PARAMETER_NAME_ATTRIBUTES: &[&str] = &["name", "paramName", "key"];
const HWPX_FIELD_PARAMETER_VALUE_ATTRIBUTES: &[&str] = &["value", "val", "text"];
const HWPX_FIELD_TYPE_ATTRIBUTES: &[&str] = &["type", "fieldType"];
const HWPX_FONT_FACE_ATTRIBUTES: &[&str] = &["face", "fontFace", "fontName", "name", "typeface"];
const HWPX_FONT_FACE_LANG_ATTRIBUTES: &[&str] = &["lang", "type", "script", "fontType"];
const HWPX_LINK_TITLE_ATTRIBUTES: &[&str] = &["title", "name", "desc", "description", "tooltip"];
const HWPX_LINK_URL_ATTRIBUTES: &[&str] = &["href", "url", "target", "address", "webAddress"];
const HWPX_HEADER_FOOTER_APPLY_PAGE_TYPE_ATTRIBUTES: &[&str] =
    &["applyPageType", "pageType", "applyTo"];
const HWPX_LIST_ID_REF_ATTRIBUTES: &[&str] = &["idRef", "idref", "idREF"];
const HWPX_LIST_LEVEL_ATTRIBUTES: &[&str] = &["level", "lvl", "outlineLevel"];
const HWPX_LIST_TYPE_ATTRIBUTES: &[&str] = &["type", "kind"];
const HWPX_LINE_SPACING_TYPE_ATTRIBUTES: &[&str] = &["type", "kind"];
const HWPX_MANIFEST_HREF_ATTRIBUTES: &[&str] = &["href", "full-path", "fullPath"];
const HWPX_MANIFEST_ID_ATTRIBUTES: &[&str] = &["id", "itemId", "itemID", "identifier"];
const HWPX_MANIFEST_ID_REF_ATTRIBUTES: &[&str] =
    &["idref", "idRef", "idREF", "itemIdRef", "itemIDRef"];
const HWPX_MANIFEST_MEDIA_TYPE_ATTRIBUTES: &[&str] = &["media-type", "mediaType"];
const HWPX_MARGIN_BOTTOM_ATTRIBUTES: &[&str] = &["bottom", "b"];
const HWPX_MARGIN_LEFT_ATTRIBUTES: &[&str] = &["left", "l"];
const HWPX_MARGIN_RIGHT_ATTRIBUTES: &[&str] = &["right", "r"];
const HWPX_MARGIN_TOP_ATTRIBUTES: &[&str] = &["top", "t"];
const HWPX_NOTE_ID_ATTRIBUTES: &[&str] = &["instId", "id"];
const HWPX_PARAGRAPH_PR_ID_REF_ATTRIBUTES: &[&str] = &["paraPrIDRef", "paraPrIdRef", "paraPrIDREF"];
const HWPX_IMAGE_ALPHA_ATTRIBUTES: &[&str] = &["alpha", "opacity"];
const HWPX_IMAGE_BRIGHTNESS_ATTRIBUTES: &[&str] = &["bright", "brightness"];
const HWPX_IMAGE_BORDER_COLOR_ATTRIBUTES: &[&str] = &["color", "lineColor"];
const HWPX_IMAGE_BORDER_STYLE_ATTRIBUTES: &[&str] = &["style", "type"];
const HWPX_IMAGE_BORDER_WIDTH_ATTRIBUTES: &[&str] = &["width", "w"];
const HWPX_IMAGE_CONTRAST_ATTRIBUTES: &[&str] = &["contrast", "contrastValue"];
const HWPX_IMAGE_CROP_BOTTOM_ATTRIBUTES: &[&str] = &["bottom", "b"];
const HWPX_IMAGE_EFFECT_ATTRIBUTES: &[&str] = &["effect", "pictureEffect"];
const HWPX_IMAGE_CROP_LEFT_ATTRIBUTES: &[&str] = &["left", "l"];
const HWPX_IMAGE_CROP_RIGHT_ATTRIBUTES: &[&str] = &["right", "r"];
const HWPX_IMAGE_CROP_TOP_ATTRIBUTES: &[&str] = &["top", "t"];
const HWPX_IMAGE_FLIP_HORIZONTAL_ATTRIBUTES: &[&str] = &["horizontal", "horizontalFlip", "flipH"];
const HWPX_IMAGE_FLIP_VERTICAL_ATTRIBUTES: &[&str] = &["vertical", "verticalFlip", "flipV"];
const HWPX_IMAGE_ROTATION_ANGLE_ATTRIBUTES: &[&str] = &["angle", "rotateAngle", "rotation"];
const HWPX_HWP_UNIT_VALUE_ATTRIBUTES: &[&str] = &["value", "val"];
const HWPX_PARAGRAPH_HORIZONTAL_ALIGN_ATTRIBUTES: &[&str] =
    &["horizontal", "horizontalAlign", "horzAlign"];
const HWPX_TEXT_BACKGROUND_COLOR_ATTRIBUTES: &[&str] = &["shadeColor", "backgroundColor"];
const HWPX_TEXT_COLOR_ATTRIBUTES: &[&str] = &["textColor", "color"];
const HWPX_TEXT_EMPHASIS_DOT_ATTRIBUTES: &[&str] = &["symMark", "symbolMark"];
const HWPX_TEXT_FONT_SIZE_ATTRIBUTES: &[&str] = &["height", "fontSize"];
const HWPX_PICTURE_HORIZONTAL_ALIGN_ATTRIBUTES: &[&str] = &["horzAlign", "horizontalAlign"];
const HWPX_PICTURE_HORIZONTAL_OFFSET_ATTRIBUTES: &[&str] = &["horzOffset", "horizontalOffset"];
const HWPX_PICTURE_HORIZONTAL_REL_TO_ATTRIBUTES: &[&str] = &["horzRelTo", "horizontalRelTo"];
const HWPX_TABLE_CELL_HEADER_ATTRIBUTES: &[&str] = &["header", "isHeader"];
const HWPX_PICTURE_TEXT_WRAP_ATTRIBUTES: &[&str] = &["textWrap", "wrap"];
const HWPX_PICTURE_TREAT_AS_CHAR_ATTRIBUTES: &[&str] = &["treatAsChar", "treat-as-char"];
const HWPX_PICTURE_VERTICAL_ALIGN_ATTRIBUTES: &[&str] = &["vertAlign", "verticalAlign"];
const HWPX_PICTURE_VERTICAL_OFFSET_ATTRIBUTES: &[&str] = &["vertOffset", "verticalOffset"];
const HWPX_PICTURE_VERTICAL_REL_TO_ATTRIBUTES: &[&str] = &["vertRelTo", "verticalRelTo"];
const HWPX_VERTICAL_ALIGN_ATTRIBUTES: &[&str] = &["vertAlign", "verticalAlign"];
const HWPX_TABLE_CELL_COL_ADDR_ATTRIBUTES: &[&str] = &["colAddr", "coladdr", "colIndex"];
const HWPX_TABLE_CELL_COL_SPAN_ATTRIBUTES: &[&str] = &["colSpan", "colspan", "col-span"];
const HWPX_TABLE_CELL_FIELD_NAME_ATTRIBUTES: &[&str] =
    &["fieldName", "field-name", "cellFieldName"];
const HWPX_TABLE_CELL_ROW_ADDR_ATTRIBUTES: &[&str] = &["rowAddr", "rowaddr", "rowIndex"];
const HWPX_TABLE_CELL_ROW_SPAN_ATTRIBUTES: &[&str] = &["rowSpan", "rowspan", "row-span"];
const HWPX_WIDTH_ATTRIBUTES: &[&str] = &["width", "w"];
const HWPX_HEIGHT_ATTRIBUTES: &[&str] = &["height", "h"];
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

    let section_paths = collect_section_xml_paths(&mut archive)?;
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

    let section_paths = collect_section_xml_paths(&mut archive)?;

    if section_paths.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Contents/section*.xml entries were not found",
        ));
    }

    let mut context = read_hwpx_fallback_context(&mut archive)?;
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
    document.warnings = context.warnings;
    Ok(document)
}

fn collect_section_xml_paths<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
) -> io::Result<Vec<String>> {
    if let Some(content_xml) = read_hwpx_content_hpf_xml(archive)? {
        let section_paths = resolve_existing_section_paths(
            archive,
            extract_section_paths_from_content_hpf(&content_xml),
        );
        if !section_paths.is_empty() {
            return Ok(section_paths);
        }
    }

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
    section_paths.sort_by_key(|path| section_xml_index(path).unwrap_or(u32::MAX));

    Ok(section_paths)
}

fn resolve_existing_section_paths<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
    hrefs: Vec<String>,
) -> Vec<String> {
    let mut paths = Vec::new();

    for href in hrefs {
        for candidate in hwpx_section_entry_candidates(&href) {
            if paths.contains(&candidate) {
                break;
            }
            if archive.by_name(&candidate).is_ok() {
                paths.push(candidate);
                break;
            }
            if let Ok(Some(actual_path)) = find_archive_entry_case_insensitive(archive, &candidate)
            {
                paths.push(actual_path);
                break;
            }
        }
    }

    paths
}

fn extract_section_paths_from_content_hpf(content_xml: &str) -> Vec<String> {
    let mut manifest_items = Vec::new();
    let mut spine_order = Vec::new();
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(content_xml, cursor) {
        if tag.name == "item" && !tag.is_closing {
            let id = decoded_xml_attribute_value_any(tag.raw, HWPX_MANIFEST_ID_ATTRIBUTES);
            let href = decoded_xml_attribute_value_any(tag.raw, HWPX_MANIFEST_HREF_ATTRIBUTES);

            if let (Some(id), Some(href)) = (id, href) {
                let media_type =
                    decoded_xml_attribute_value_any(tag.raw, HWPX_MANIFEST_MEDIA_TYPE_ATTRIBUTES);
                manifest_items.push((id, href, media_type));
            }
        } else if tag.name == "itemref"
            && !tag.is_closing
            && let Some(idref) =
                decoded_xml_attribute_value_any(tag.raw, HWPX_MANIFEST_ID_REF_ATTRIBUTES)
        {
            spine_order.push(idref);
        }

        cursor = tag.end;
    }

    let mut section_paths = Vec::new();
    for idref in spine_order {
        if let Some((_, href, media_type)) = manifest_items.iter().find(|(id, _, _)| id == &idref)
            && is_hwpx_section_manifest_item(href, media_type.as_deref())
        {
            section_paths.push(href.clone());
        }
    }

    for (_, href, media_type) in manifest_items {
        if is_hwpx_section_manifest_item(&href, media_type.as_deref())
            && !section_paths.contains(&href)
        {
            section_paths.push(href);
        }
    }

    section_paths
}

fn is_hwpx_section_manifest_item(href: &str, media_type: Option<&str>) -> bool {
    let normalized = href.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();

    lower.ends_with(".xml")
        && lower.contains("section")
        && media_type.is_none_or(is_hwpx_xml_media_type)
}

fn is_hwpx_xml_media_type(media_type: &str) -> bool {
    let base = media_type_base(media_type);
    base.eq_ignore_ascii_case("application/xml") || base.eq_ignore_ascii_case("text/xml")
}

fn hwpx_section_entry_candidates(href: &str) -> Vec<String> {
    let Some(normalized) = normalize_hwpx_archive_path(href) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    push_unique_candidate(&mut candidates, normalized.clone());

    if !normalized.starts_with("Contents/")
        && let Some(candidate) = normalize_hwpx_archive_path(&format!("Contents/{normalized}"))
    {
        push_unique_candidate(&mut candidates, candidate);
    }

    candidates
}

#[derive(Clone, Debug, Default, PartialEq)]
struct HwpxFallbackContext {
    paragraph_styles: Vec<HwpxParagraphStyle>,
    text_styles: Vec<TextStyle>,
    font_faces: Vec<Vec<String>>,
    border_fills: Vec<HwpxBorderFill>,
    bullet_markers: BTreeMap<u32, String>,
    numberings: BTreeMap<u32, HwpxNumbering>,
    image_items: BTreeMap<String, HwpxImageItem>,
    image_resource_ids: BTreeMap<String, ResourceId>,
    resources: ResourceStore,
    ordered_counts: BTreeMap<(u32, u8), u32>,
    notes: NoteStore,
    warnings: Vec<ConversionWarning>,
    next_note_ordinal: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct HwpxNumbering {
    levels: BTreeMap<u8, HwpxNumberingLevel>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HwpxNumberingLevel {
    start: u32,
    format: String,
}

impl HwpxFallbackContext {
    fn add_warning_once(&mut self, message: &str) {
        if self
            .warnings
            .iter()
            .any(|warning| warning.message == message)
        {
            return;
        }
        self.warnings.push(ConversionWarning {
            code: WarningCode::Unknown,
            message: message.to_string(),
        });
    }

    fn border_fill(&self, border_fill_id: u32) -> Option<&HwpxBorderFill> {
        self.border_fills.get(border_fill_id as usize).or_else(|| {
            border_fill_id
                .checked_sub(1)
                .and_then(|index| self.border_fills.get(index as usize))
        })
    }

    fn border_fill_background_color(&mut self, border_fill_id: u32) -> Option<Color> {
        match self.border_fill(border_fill_id) {
            Some(fill) => fill.background_color,
            None => {
                self.warn_missing_border_fill(border_fill_id);
                None
            }
        }
    }

    fn border_fill_borders(&mut self, border_fill_id: u32) -> [Option<Border>; 4] {
        match self.border_fill(border_fill_id) {
            Some(fill) => fill.borders.clone(),
            None => {
                self.warn_missing_border_fill(border_fill_id);
                Default::default()
            }
        }
    }

    fn warn_missing_border_fill(&mut self, border_fill_id: u32) {
        self.add_warning_once(&format!(
            "HWPX object referenced missing borderFill id {border_fill_id}; hwp-convert used default fill and border style."
        ));
    }

    fn text_style_for_run(&mut self, run_tag: &str) -> TextStyle {
        let Some(char_pr_id) = xml_attribute_value_any(run_tag, HWPX_CHAR_PR_ID_REF_ATTRIBUTES)
            .and_then(parse_trimmed::<usize>)
        else {
            return TextStyle::default();
        };

        match self.text_styles.get(char_pr_id) {
            Some(style) => style.clone(),
            None => {
                self.add_warning_once(&format!(
                    "HWPX run referenced missing charPr id {char_pr_id}; hwp-convert used default text style."
                ));
                TextStyle::default()
            }
        }
    }

    fn paragraph_style_for_paragraph(&mut self, paragraph_xml: &str) -> ParagraphStyle {
        self.hwpx_paragraph_style_for_paragraph(paragraph_xml).style
    }

    fn paragraph_role_for_paragraph(&mut self, paragraph_xml: &str) -> ParagraphRole {
        self.hwpx_paragraph_style_for_paragraph(paragraph_xml)
            .role
            .unwrap_or_default()
    }

    fn list_info_for_paragraph(&mut self, paragraph_xml: &str) -> Option<ListInfo> {
        let style = self.hwpx_paragraph_style_for_paragraph(paragraph_xml);

        match style.kind {
            Some(ListKind::Ordered) => {
                let key = (style.list_id.unwrap_or(0), style.level);
                let start = style
                    .list_id
                    .and_then(|id| self.numberings.get(&id))
                    .and_then(|numbering| numbering.levels.get(&style.level))
                    .map(|level| level.start)
                    .unwrap_or(1);
                let number = self
                    .ordered_counts
                    .entry(key)
                    .and_modify(|number| *number += 1)
                    .or_insert(start);
                let number = *number;
                self.ordered_counts
                    .retain(|(id, level), _| *id != key.0 || *level <= key.1);
                let marker = style.list_id.and_then(|id| {
                    self.numberings
                        .get(&id)
                        .and_then(|numbering| numbering.levels.get(&style.level))
                        .cloned()
                        .map(|level| self.render_ordered_marker(id, style.level, number, &level))
                });

                Some(ListInfo {
                    kind: ListKind::Ordered,
                    level: style.level,
                    marker,
                    marker_format: None,
                    number: Some(number),
                })
            }
            Some(ListKind::Unordered) => {
                let marker = style
                    .list_id
                    .and_then(|list_id| match self.bullet_markers.get(&list_id) {
                        Some(marker) => Some(marker.clone()),
                        None => {
                            self.add_warning_once(&format!(
                                "HWPX unordered list referenced missing bullet marker id {list_id}; hwp-convert used a default bullet marker."
                            ));
                            None
                        }
                    })
                    .unwrap_or_else(|| "•".to_string());
                Some(ListInfo {
                    kind: ListKind::Unordered,
                    level: style.level,
                    marker: Some(marker),
                    marker_format: None,
                    number: None,
                })
            }
            _ => None,
        }
    }

    fn render_ordered_marker(
        &mut self,
        numbering_id: u32,
        level: u8,
        number: u32,
        definition: &HwpxNumberingLevel,
    ) -> String {
        let Some(format) = hwpx_number_format(&definition.format) else {
            self.add_warning_once(&format!(
                "HWPX numbering id {numbering_id} level {level} referenced unsupported numFormat `{}`; hwp-convert used decimal digits.",
                definition.format
            ));
            return number.to_string();
        };
        let Ok(number) = u16::try_from(number) else {
            self.add_warning_once(&format!(
                "HWPX numbering id {numbering_id} level {level} exceeded the supported formatted-number range; hwp-convert used decimal digits."
            ));
            return number.to_string();
        };
        format_rhwp_number(number, format)
    }

    fn hwpx_paragraph_style_for_paragraph(&mut self, paragraph_xml: &str) -> HwpxParagraphStyle {
        let paragraph_style_id =
            root_xml_attribute_u32_any(paragraph_xml, "p", HWPX_PARAGRAPH_PR_ID_REF_ATTRIBUTES)
                .map(|id| id as usize);
        let mut style = paragraph_style_id
            .and_then(|para_pr_id| {
                let style = self.paragraph_styles.get(para_pr_id).cloned();
                if style.is_none() {
                    self.add_warning_once(&format!(
                        "HWPX paragraph referenced missing paraPr id {para_pr_id}; hwp-convert used direct paragraph properties or default paragraph style."
                    ));
                }
                style
            })
            .unwrap_or_default();
        let (direct_style, warnings) = extract_hwpx_direct_paragraph_style(paragraph_xml);
        for warning in warnings {
            self.add_warning_once(&warning);
        }
        merge_hwpx_paragraph_style(&mut style, direct_style);
        style.apply_layout_flags();
        self.resolve_hwpx_paragraph_border(&mut style);
        style
    }

    fn resolve_hwpx_paragraph_border(&mut self, style: &mut HwpxParagraphStyle) {
        let Some(border_fill_id) = style.border_fill_id.filter(|id| *id != 0) else {
            return;
        };

        style.style.background_color = self.border_fill_background_color(border_fill_id);
        let [border_left, border_right, border_top, border_bottom] =
            self.border_fill_borders(border_fill_id);
        style.style.border_top = border_top;
        style.style.border_right = border_right;
        style.style.border_bottom = border_bottom;
        style.style.border_left = border_left;
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
        let mut requested_id = decoded_xml_attribute_value_any(tag, HWPX_NOTE_ID_ATTRIBUTES);
        if let Some(raw_id) = requested_id.as_deref() {
            let requested_note_id = NoteId(format!("{note_prefix}-{raw_id}"));
            if self.notes.get(&requested_note_id).is_some() {
                self.add_warning_once(&format!(
                    "HWPX note referenced duplicate id `{}`; hwp-convert assigned a unique note id instead of dropping the note.",
                    requested_note_id.as_str()
                ));
            }
        }
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
        let Some(image_key) = self.resolve_image_item_key(binary_item_id_ref) else {
            self.add_warning_once(&format!(
                "HWPX picture referenced missing binary item `{}`; hwp-convert preserved available fallback text instead of silently dropping the image.",
                binary_item_id_ref.trim()
            ));
            return None;
        };
        if let Some(resource_id) = self.image_resource_ids.get(&image_key) {
            return Some(resource_id.clone());
        }

        let Some(item) = self.image_items.get(&image_key) else {
            self.add_warning_once(&format!(
                "HWPX picture referenced missing binary item `{image_key}`; hwp-convert preserved available fallback text instead of silently dropping the image."
            ));
            return None;
        };
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

#[derive(Clone, Debug, Default, PartialEq)]
struct HwpxBorderFill {
    background_color: Option<Color>,
    /// left, right, top, bottom
    borders: [Option<Border>; 4],
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
    role: Option<ParagraphRole>,
    border_fill_id: Option<u32>,
    widow_orphan: Option<bool>,
    keep_with_next: Option<bool>,
    keep_lines: Option<bool>,
    page_break_before: Option<bool>,
    style: ParagraphStyle,
}

impl HwpxParagraphStyle {
    fn apply_layout_flags(&mut self) {
        self.style.widow_orphan = self.widow_orphan.unwrap_or(false);
        self.style.keep_with_next = self.keep_with_next.unwrap_or(false);
        self.style.keep_lines = self.keep_lines.unwrap_or(false);
        self.style.page_break_before = self.page_break_before.unwrap_or(false);
    }
}

fn merge_hwpx_paragraph_style(base: &mut HwpxParagraphStyle, overlay: HwpxParagraphStyle) {
    if overlay.kind.is_some() {
        base.kind = overlay.kind;
        base.level = overlay.level;
        base.list_id = overlay.list_id;
    }
    if overlay.role.is_some() {
        base.role = overlay.role;
    }
    if overlay.border_fill_id.is_some() {
        base.border_fill_id = overlay.border_fill_id;
    }
    if overlay.widow_orphan.is_some() {
        base.widow_orphan = overlay.widow_orphan;
    }
    if overlay.keep_with_next.is_some() {
        base.keep_with_next = overlay.keep_with_next;
    }
    if overlay.keep_lines.is_some() {
        base.keep_lines = overlay.keep_lines;
    }
    if overlay.page_break_before.is_some() {
        base.page_break_before = overlay.page_break_before;
    }
    merge_paragraph_style(&mut base.style, overlay.style);
}

fn merge_paragraph_style(base: &mut ParagraphStyle, overlay: ParagraphStyle) {
    if overlay.alignment.is_some() {
        base.alignment = overlay.alignment;
    }
    if overlay.spacing.before_pt.is_some() {
        base.spacing.before_pt = overlay.spacing.before_pt;
    }
    if overlay.spacing.after_pt.is_some() {
        base.spacing.after_pt = overlay.spacing.after_pt;
    }
    if overlay.spacing.line_pt.is_some() {
        base.spacing.line_pt = overlay.spacing.line_pt;
    }
    if overlay.spacing.line_percent.is_some() {
        base.spacing.line_percent = overlay.spacing.line_percent;
        base.spacing.line_pt = None;
    }
    if overlay.indent.left_pt.is_some() {
        base.indent.left_pt = overlay.indent.left_pt;
    }
    if overlay.indent.right_pt.is_some() {
        base.indent.right_pt = overlay.indent.right_pt;
    }
    if overlay.indent.first_line_pt.is_some() {
        base.indent.first_line_pt = overlay.indent.first_line_pt;
    }
    if overlay.background_color.is_some() {
        base.background_color = overlay.background_color;
    }
    if overlay.padding_top_pt.is_some() {
        base.padding_top_pt = overlay.padding_top_pt;
    }
    if overlay.padding_right_pt.is_some() {
        base.padding_right_pt = overlay.padding_right_pt;
    }
    if overlay.padding_bottom_pt.is_some() {
        base.padding_bottom_pt = overlay.padding_bottom_pt;
    }
    if overlay.padding_left_pt.is_some() {
        base.padding_left_pt = overlay.padding_left_pt;
    }
    if overlay.border_top.is_some() {
        base.border_top = overlay.border_top;
    }
    if overlay.border_right.is_some() {
        base.border_right = overlay.border_right;
    }
    if overlay.border_bottom.is_some() {
        base.border_bottom = overlay.border_bottom;
    }
    if overlay.border_left.is_some() {
        base.border_left = overlay.border_left;
    }
}

fn read_hwpx_fallback_context<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
) -> io::Result<HwpxFallbackContext> {
    let mut context = match read_hwpx_header_xml(archive) {
        Ok(Some(header_xml)) => extract_hwpx_fallback_context(&header_xml),
        Ok(None) => HwpxFallbackContext::default(),
        Err(error) => return Err(error),
    };
    context.image_items = read_hwpx_image_items(archive)?;
    Ok(context)
}

fn read_hwpx_header_xml<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
) -> io::Result<Option<String>> {
    read_optional_zip_text_entry_case_insensitive(archive, HEADER_XML_PATH)
}

fn read_hwpx_content_hpf_xml<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
) -> io::Result<Option<String>> {
    read_optional_zip_text_entry_case_insensitive(archive, CONTENT_HPF_PATH)
}

fn read_optional_zip_text_entry_case_insensitive<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
) -> io::Result<Option<String>> {
    match read_zip_text_entry(archive, path) {
        Ok(header_xml) => Ok(Some(header_xml)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let Some(actual_path) = find_archive_entry_case_insensitive(archive, path)? else {
                return Ok(None);
            };
            read_zip_text_entry(archive, &actual_path).map(Some)
        }
        Err(error) => Err(error),
    }
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
                let fonts = extract_hwpx_font_face(fontface_xml);
                if let Some(group_index) =
                    xml_attribute_value_any(tag.raw, HWPX_FONT_FACE_LANG_ATTRIBUTES)
                        .and_then(hwpx_font_face_group_index)
                {
                    if context.font_faces.len() <= group_index {
                        context.font_faces.resize_with(group_index + 1, Vec::new);
                    }
                    context.font_faces[group_index] = fonts;
                } else {
                    context.font_faces.push(fonts);
                }
                cursor = fontface_end;
            }
            "charPr" => {
                let Some(id) = xml_attribute_value(tag.raw, "id").and_then(parse_trimmed::<usize>)
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
                let (style, warnings) =
                    extract_hwpx_text_style_with_warnings(tag.raw, char_xml, &context.font_faces);
                context.text_styles[id] = style;
                for warning in warnings {
                    context.add_warning_once(&warning);
                }
                cursor = char_end;
            }
            "borderFill" => {
                let Some(id) = xml_attribute_value(tag.raw, "id").and_then(parse_trimmed::<usize>)
                else {
                    cursor = tag.end;
                    continue;
                };
                let Some(border_end) = find_matching_element_end(header_xml, &tag) else {
                    cursor = tag.end;
                    continue;
                };

                if context.border_fills.len() <= id {
                    context
                        .border_fills
                        .resize_with(id + 1, HwpxBorderFill::default);
                }

                let border_xml = &header_xml[tag.start..border_end];
                let (border_fill, warnings) = extract_hwpx_border_fill(border_xml);
                context.border_fills[id] = border_fill;
                for warning in warnings {
                    context.add_warning_once(&warning);
                }
                cursor = border_end;
            }
            "bullet" => {
                let Some(id) = xml_attribute_value(tag.raw, "id").and_then(parse_trimmed::<u32>)
                else {
                    cursor = tag.end;
                    continue;
                };
                let bullet_end = if tag.is_self_closing {
                    tag.end
                } else {
                    find_matching_element_end(header_xml, &tag).unwrap_or(tag.end)
                };
                let bullet_xml = &header_xml[tag.start..bullet_end];
                if let Some(marker) = extract_hwpx_bullet_marker(tag.raw, bullet_xml) {
                    context.bullet_markers.insert(id, marker);
                }
                cursor = bullet_end;
            }
            "numbering" => {
                let Some(id) = xml_attribute_value(tag.raw, "id").and_then(parse_trimmed::<u32>)
                else {
                    cursor = tag.end;
                    continue;
                };
                let numbering_end = if tag.is_self_closing {
                    tag.end
                } else {
                    find_matching_element_end(header_xml, &tag).unwrap_or(tag.end)
                };
                let numbering_xml = &header_xml[tag.start..numbering_end];
                context
                    .numberings
                    .insert(id, extract_hwpx_numbering(numbering_xml));
                cursor = numbering_end;
            }
            "paraPr" => {
                let Some(id) = xml_attribute_value(tag.raw, "id").and_then(parse_trimmed::<usize>)
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
                let (paragraph_style, warnings) = extract_hwpx_paragraph_style(para_xml);
                context.paragraph_styles[id] = paragraph_style;
                for warning in warnings {
                    context.add_warning_once(&warning);
                }
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
    let Some(content_xml) = read_hwpx_content_hpf_xml(archive)? else {
        return Ok(BTreeMap::new());
    };

    let mut items = BTreeMap::new();
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(&content_xml, cursor) {
        if tag.name != "item" || tag.is_closing {
            cursor = tag.end;
            continue;
        }

        let id = decoded_xml_attribute_value_any(tag.raw, HWPX_MANIFEST_ID_ATTRIBUTES);
        let href = decoded_xml_attribute_value_any(tag.raw, HWPX_MANIFEST_HREF_ATTRIBUTES);
        let media_type =
            decoded_xml_attribute_value_any(tag.raw, HWPX_MANIFEST_MEDIA_TYPE_ATTRIBUTES);
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
        if let Some(actual_path) = find_archive_entry_case_insensitive(archive, &path)? {
            return read_zip_binary_entry(archive, &actual_path).map(Some);
        }
    }

    Ok(None)
}

fn hwpx_binary_entry_candidates(href: &str) -> Vec<String> {
    let Some(normalized) = normalize_hwpx_archive_path(href) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    push_unique_candidate(&mut candidates, normalized.clone());
    if !normalized.starts_with("Contents/")
        && let Some(candidate) = normalize_hwpx_archive_path(&format!("Contents/{normalized}"))
    {
        push_unique_candidate(&mut candidates, candidate);
    }
    candidates
}

fn normalize_hwpx_archive_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let mut parts = Vec::new();

    for part in normalized.trim_start_matches('/').split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn push_unique_candidate(candidates: &mut Vec<String>, candidate: String) {
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn is_hwpx_image_manifest_item(href: &str, media_type: Option<&str>) -> bool {
    media_type.is_some_and(is_hwpx_image_media_type)
        || href
            .replace('\\', "/")
            .to_ascii_lowercase()
            .contains("bindata/")
            && path_extension(href)
                .as_deref()
                .and_then(media_type_for_extension)
                .is_some_and(|media_type| media_type.starts_with("image/"))
}

fn is_hwpx_image_media_type(media_type: &str) -> bool {
    media_type_base(media_type)
        .to_ascii_lowercase()
        .starts_with("image/")
}

fn media_type_base(media_type: &str) -> &str {
    media_type.split(';').next().unwrap_or(media_type).trim()
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
            && let Some(id) = xml_attribute_value(tag.raw, "id").and_then(parse_trimmed::<usize>)
            && let Some(face) = decoded_xml_attribute_value_any(tag.raw, HWPX_FONT_FACE_ATTRIBUTES)
        {
            if fonts.len() <= id {
                fonts.resize(id + 1, String::new());
            }
            fonts[id] = face;
        }
        cursor = tag.end;
    }

    fonts
}

fn hwpx_font_face_group_index(lang: &str) -> Option<usize> {
    match lang.trim().to_ascii_uppercase().as_str() {
        "HANGUL" => Some(0),
        "LATIN" => Some(1),
        "HANJA" => Some(2),
        "JAPANESE" => Some(3),
        "OTHER" => Some(4),
        "SYMBOL" => Some(5),
        "USER" => Some(6),
        _ => None,
    }
}

fn extract_hwpx_text_style_with_warnings(
    char_pr_tag: &str,
    char_pr_xml: &str,
    font_faces: &[Vec<String>],
) -> (TextStyle, Vec<String>) {
    let mut style_warnings = Vec::new();
    let font_size_pt = match xml_attribute_value_any(char_pr_tag, HWPX_TEXT_FONT_SIZE_ATTRIBUTES) {
        Some(value) => parse_trimmed::<i32>(value)
            .and_then(hwp_units_to_pt_option)
            .or_else(|| {
                style_warnings.push(format!(
                    "HWPX charPr referenced unsupported font size `{}`; hwp-convert used the default text size.",
                    value.trim()
                ));
                None
            }),
        None => None,
    };
    let color = parse_hwpx_color_attribute_with_warning(
        char_pr_tag,
        HWPX_TEXT_COLOR_ATTRIBUTES,
        "text color",
        &mut style_warnings,
    );
    let background_color = parse_hwpx_color_attribute_with_warning(
        char_pr_tag,
        HWPX_TEXT_BACKGROUND_COLOR_ATTRIBUTES,
        "text background color",
        &mut style_warnings,
    );
    let mut style = TextStyle {
        emphasis_dot: xml_attribute_value_any(char_pr_tag, HWPX_TEXT_EMPHASIS_DOT_ATTRIBUTES)
            .is_some_and(hwpx_style_value_is_enabled),
        kerning: xml_attribute_value_any(char_pr_tag, &["useKerning", "kerning"])
            .is_some_and(hwpx_style_value_is_enabled),
        font_size_pt,
        color,
        background_color,
        ..Default::default()
    };
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(char_pr_xml, cursor) {
        if !tag.is_closing {
            match tag.name {
                "bold" => style.bold = true,
                "italic" => style.italic = true,
                "underline" => {
                    style.underline = hwpx_style_tag_is_enabled(tag.raw, &["type"]);
                    if style.underline {
                        style.underline_above = xml_attribute_value_any(
                            tag.raw,
                            &["type", "position"],
                        )
                        .is_some_and(|value| {
                            value.eq_ignore_ascii_case("TOP") || value.eq_ignore_ascii_case("ABOVE")
                        });
                        style.underline_style =
                            xml_attribute_value_any(tag.raw, &["shape", "style"]).and_then(
                                |value| {
                                    map_hwpx_text_decoration_style_with_warning(
                                        value,
                                        "underline",
                                        &mut style_warnings,
                                    )
                                },
                            );
                        style.underline_color = parse_hwpx_color_attribute_with_warning(
                            tag.raw,
                            HWPX_DECORATION_COLOR_ATTRIBUTES,
                            "underline color",
                            &mut style_warnings,
                        );
                    }
                }
                "strikeout" | "strikeOut" => {
                    style.strike = hwpx_style_tag_is_enabled(tag.raw, &["shape", "type"]);
                    if style.strike {
                        style.strike_style =
                            xml_attribute_value_any(tag.raw, &["shape", "style", "type"]).and_then(
                                |value| {
                                    map_hwpx_text_decoration_style_with_warning(
                                        value,
                                        "strikeout",
                                        &mut style_warnings,
                                    )
                                },
                            );
                        style.strike_color = parse_hwpx_color_attribute_with_warning(
                            tag.raw,
                            HWPX_DECORATION_COLOR_ATTRIBUTES,
                            "strikeout color",
                            &mut style_warnings,
                        );
                    }
                }
                "supscript" => style.superscript = true,
                "subscript" => style.subscript = true,
                "emboss" => style.emboss = true,
                "engrave" => style.engrave = true,
                "shadow" => style.shadow = hwpx_style_tag_is_enabled(tag.raw, &["type"]),
                "outline" => style.outline = hwpx_style_tag_is_enabled(tag.raw, &["type"]),
                "ratio" => {
                    style.font_width_percent = extract_uniform_hwpx_percent(
                        tag.raw,
                        100.0,
                        50.0..=200.0,
                        "horizontal ratio",
                        &mut style_warnings,
                    );
                }
                "spacing" => {
                    style.letter_spacing_percent = extract_uniform_hwpx_percent(
                        tag.raw,
                        0.0,
                        -50.0..=50.0,
                        "character spacing",
                        &mut style_warnings,
                    );
                }
                "relSz" => {
                    style.relative_size_percent = extract_uniform_hwpx_percent(
                        tag.raw,
                        100.0,
                        10.0..=250.0,
                        "relative size",
                        &mut style_warnings,
                    );
                }
                "offset" => {
                    style.vertical_offset_percent = extract_uniform_hwpx_percent(
                        tag.raw,
                        0.0,
                        -100.0..=100.0,
                        "character offset",
                        &mut style_warnings,
                    );
                }
                "fontRef" => {
                    let (font_family, warnings) =
                        font_ref_family_with_warnings(tag.raw, font_faces);
                    style.font_family = font_family;
                    style_warnings.extend(warnings);
                }
                _ => {}
            }
        }
        cursor = tag.end;
    }

    if let Some(relative_size) = style.relative_size_percent
        && let Some(font_size) = style.font_size_pt.as_mut()
    {
        font_size.0 *= relative_size.0 / 100.0;
    }

    (style, style_warnings)
}

fn extract_uniform_hwpx_percent(
    tag: &str,
    default: f32,
    valid_range: std::ops::RangeInclusive<f32>,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<Percent> {
    const SCRIPT_ATTRIBUTES: [&str; 7] = [
        "hangul", "latin", "hanja", "japanese", "other", "symbol", "user",
    ];

    let raw_values = SCRIPT_ATTRIBUTES.map(|attribute| xml_attribute_value(tag, attribute));
    if raw_values.iter().all(Option::is_none) {
        return None;
    }

    let mut values = [default; 7];
    for (index, raw_value) in raw_values.into_iter().enumerate() {
        let Some(raw_value) = raw_value else {
            continue;
        };
        let Some(value) = parse_trimmed::<f32>(raw_value) else {
            warnings.push(format!(
                "HWPX charPr used invalid {label} value `{}`; hwp-convert omitted that metric.",
                raw_value.trim()
            ));
            return None;
        };
        if !value.is_finite() || !valid_range.contains(&value) {
            warnings.push(format!(
                "HWPX charPr used out-of-range {label} value `{}`; hwp-convert omitted that metric.",
                raw_value.trim()
            ));
            return None;
        }
        values[index] = value;
    }

    let first = values[0];
    if values
        .iter()
        .any(|value| (*value - first).abs() > f32::EPSILON)
    {
        warnings.push(format!(
            "HWPX charPr used script-specific {label} values {values:?}; TextStyle omitted them because one value cannot represent every script."
        ));
        return None;
    }

    ((first - default).abs() > f32::EPSILON).then_some(Percent(first))
}

fn parse_hwpx_color_attribute_with_warning(
    tag: &str,
    attribute_names: &[&str],
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<Color> {
    let value = xml_attribute_value_any(tag, attribute_names)?;
    match parse_hwpx_hex_color(value) {
        Some(color) => Some(color),
        None => {
            warnings.push(format!(
                "HWPX charPr referenced unsupported {label} `{}`; hwp-convert used the default text style value.",
                value.trim()
            ));
            None
        }
    }
}

fn hwpx_style_tag_is_enabled(tag: &str, attribute_names: &[&str]) -> bool {
    attribute_names
        .iter()
        .find_map(|attribute| xml_attribute_value(tag, attribute))
        .is_none_or(hwpx_style_value_is_enabled)
}

fn hwpx_style_value_is_enabled(value: &str) -> bool {
    let value = value.trim();
    !value.eq_ignore_ascii_case("none") && value != "0"
}

fn map_hwpx_text_decoration_style_with_warning(
    value: &str,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<TextDecorationStyle> {
    let normalized = value.trim().to_ascii_uppercase().replace('-', "_");
    let (style, approximated) = match normalized.as_str() {
        "NONE" | "0" => return None,
        "SOLID" => (TextDecorationStyle::Solid, false),
        "DASH" => (TextDecorationStyle::Dashed, false),
        "DOT" => (TextDecorationStyle::Dotted, false),
        "DOUBLE" | "DOUBLE_SLIM" => (TextDecorationStyle::Double, false),
        "WAVE" => (TextDecorationStyle::Wavy, false),
        "DASH_DOT" | "DASH_DOT_DOT" | "LONG_DASH" => (TextDecorationStyle::Dashed, true),
        "CIRCLE" => (TextDecorationStyle::Dotted, true),
        "SLIM_THICK" | "THICK_SLIM" | "SLIM_THICK_SLIM" => (TextDecorationStyle::Double, true),
        "DOUBLE_WAVE" => (TextDecorationStyle::Wavy, true),
        _ => {
            warnings.push(format!(
                "HWPX {label} referenced unsupported decoration shape `{}`; hwp-convert used the default solid decoration style.",
                value.trim()
            ));
            return None;
        }
    };

    if approximated {
        warnings.push(format!(
            "HWPX {label} decoration shape `{}` has no exact CSS equivalent; hwp-convert used the closest {style:?} style.",
            value.trim()
        ));
    }
    Some(style)
}

fn extract_hwpx_border_fill(border_fill_xml: &str) -> (HwpxBorderFill, Vec<String>) {
    let mut warnings = Vec::new();
    let fill = HwpxBorderFill {
        background_color: extract_hwpx_border_fill_background_color(border_fill_xml),
        borders: [
            extract_hwpx_border(border_fill_xml, "leftBorder", &mut warnings),
            extract_hwpx_border(border_fill_xml, "rightBorder", &mut warnings),
            extract_hwpx_border(border_fill_xml, "topBorder", &mut warnings),
            extract_hwpx_border(border_fill_xml, "bottomBorder", &mut warnings),
        ],
    };

    (fill, warnings)
}

fn extract_hwpx_border_fill_background_color(border_fill_xml: &str) -> Option<Color> {
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(border_fill_xml, cursor) {
        if !tag.is_closing {
            if let Some(color) = xml_attribute_value_any(tag.raw, HWPX_FILL_COLOR_ATTRIBUTES)
                .and_then(parse_hwpx_hex_color)
            {
                return Some(color);
            }
            if matches!(tag.name, "winBrush" | "solidFill" | "fill" | "color")
                && let Some(color) =
                    xml_attribute_value(tag.raw, "color").and_then(parse_hwpx_hex_color)
            {
                return Some(color);
            }
        }
        cursor = tag.end;
    }

    None
}

fn extract_hwpx_border(
    border_fill_xml: &str,
    border_name: &str,
    warnings: &mut Vec<String>,
) -> Option<Border> {
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(border_fill_xml, cursor) {
        cursor = tag.end;
        if tag.is_closing || tag.name != border_name {
            continue;
        }

        let border_type = xml_attribute_value_any(tag.raw, HWPX_BORDER_STYLE_ATTRIBUTES)?.trim();
        if border_type.eq_ignore_ascii_case("none") {
            return None;
        }

        let width = match xml_attribute_value_any(tag.raw, HWPX_BORDER_WIDTH_ATTRIBUTES) {
            Some(value) => parse_hwpx_border_width(value).unwrap_or_else(|| {
                warnings.push(format!(
                    "HWPX borderFill referenced unsupported border width `{}`; hwp-convert used the default 1px border width.",
                    value.trim()
                ));
                LengthPx(1.0)
            }),
            None => LengthPx(1.0),
        };

        return Some(Border {
            width,
            style: map_hwpx_border_style_with_warning(border_type, "borderFill", warnings),
            color: xml_attribute_value_any(tag.raw, HWPX_BORDER_COLOR_ATTRIBUTES)
                .and_then(parse_hwpx_hex_color),
        });
    }

    None
}

fn parse_hwpx_border_width(value: &str) -> Option<LengthPx> {
    let mut parts = value.split_whitespace();
    let amount = parts.next()?.parse::<f32>().ok()?;
    if amount < 0.0 {
        return None;
    }

    let px = match parts.next().map(str::to_ascii_lowercase).as_deref() {
        Some("mm") | None => amount * 96.0 / 25.4,
        Some("pt") => amount * 96.0 / 72.0,
        Some("px") => amount,
        Some(_) => return None,
    };
    Some(LengthPx(px))
}

fn map_hwpx_border_style_with_warning(
    value: &str,
    source: &str,
    warnings: &mut Vec<String>,
) -> BorderStyle {
    let normalized = value.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "DASH" | "LONG_DASH" | "DASH_DOT" | "DASH_DOT_DOT" => BorderStyle::Dashed,
        "DOT" | "CIRCLE" => BorderStyle::Dotted,
        "DOUBLE" | "DOUBLE_SLIM" | "SLIM_THICK" | "THICK_SLIM" | "SLIM_THICK_SLIM"
        | "DOUBLE_WAVE" => BorderStyle::Double,
        "SOLID" => BorderStyle::Solid,
        _ => {
            warnings.push(format!(
                "HWPX {source} referenced unsupported border style `{}`; hwp-convert approximated it as solid.",
                value.trim()
            ));
            BorderStyle::Solid
        }
    }
}

fn extract_hwpx_bullet_marker(bullet_tag: &str, bullet_xml: &str) -> Option<String> {
    first_non_empty_string([
        decoded_xml_attribute_value_any(bullet_tag, HWPX_BULLET_MARKER_ATTRIBUTES),
        first_hwpx_child_element_text(bullet_xml, HWPX_BULLET_MARKER_ATTRIBUTES),
    ])
}

fn extract_hwpx_numbering(numbering_xml: &str) -> HwpxNumbering {
    let mut numbering = HwpxNumbering::default();
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(numbering_xml, cursor) {
        cursor = tag.end;
        if tag.is_closing || tag.name != "paraHead" {
            continue;
        }
        let Some(source_level) = xml_attribute_value_any(tag.raw, HWPX_LIST_LEVEL_ATTRIBUTES)
            .and_then(parse_trimmed::<u8>)
        else {
            continue;
        };
        let level = source_level.saturating_sub(1);
        let start = xml_attribute_value(tag.raw, "start")
            .and_then(parse_trimmed::<u32>)
            .filter(|start| *start > 0)
            .unwrap_or(1);
        let format = xml_attribute_value(tag.raw, "numFormat")
            .map(str::trim)
            .filter(|format| !format.is_empty())
            .unwrap_or("DIGIT")
            .to_string();
        numbering
            .levels
            .insert(level, HwpxNumberingLevel { start, format });
    }

    numbering
}

fn hwpx_number_format(value: &str) -> Option<RhwpNumberFormat> {
    match value.trim().to_ascii_uppercase().as_str() {
        "DIGIT" | "ARABIC" | "DECIMAL" => Some(RhwpNumberFormat::Digit),
        "CIRCLED_DIGIT" | "CIRCLE_DIGIT" => Some(RhwpNumberFormat::CircledDigit),
        "ROMAN_CAPITAL" | "ROMAN_UPPER" | "UPPER_ROMAN" => Some(RhwpNumberFormat::RomanUpper),
        "ROMAN_SMALL" | "ROMAN_LOWER" | "LOWER_ROMAN" => Some(RhwpNumberFormat::RomanLower),
        "LATIN_CAPITAL" | "LATIN_UPPER" | "UPPER_LATIN" | "UPPER_ALPHA" => {
            Some(RhwpNumberFormat::LatinUpper)
        }
        "LATIN_SMALL" | "LATIN_LOWER" | "LOWER_LATIN" | "LOWER_ALPHA" => {
            Some(RhwpNumberFormat::LatinLower)
        }
        "HANGUL_SYLLABLE" | "HANGUL_GANADA" => Some(RhwpNumberFormat::HangulGaNaDa),
        "HANGUL_DIGIT" | "HANGUL_NUMBER" => Some(RhwpNumberFormat::HangulNumber),
        "HANJA_DIGIT" | "HANJA_NUMBER" => Some(RhwpNumberFormat::HanjaNumber),
        _ => None,
    }
}

fn font_ref_family_with_warnings(
    font_ref_tag: &str,
    font_faces: &[Vec<String>],
) -> (Option<String>, Vec<String>) {
    let mut family = None;
    let mut warnings = Vec::new();

    for (group_index, attribute) in [
        "hangul", "latin", "hanja", "japanese", "other", "symbol", "user",
    ]
    .iter()
    .enumerate()
    {
        let Some(font_id) =
            xml_attribute_value(font_ref_tag, attribute).and_then(parse_trimmed::<usize>)
        else {
            continue;
        };
        let Some(group) = font_faces.get(group_index) else {
            warnings.push(format!(
                "HWPX charPr fontRef referenced missing {attribute} font id {font_id}; hwp-convert used an available fallback font family or default font style."
            ));
            continue;
        };
        let Some(face) = group.get(font_id).filter(|face| !face.is_empty()) else {
            warnings.push(format!(
                "HWPX charPr fontRef referenced missing {attribute} font id {font_id}; hwp-convert used an available fallback font family or default font style."
            ));
            continue;
        };
        if family.is_none() {
            family = Some(face.clone());
        }
    }

    (family, warnings)
}

fn extract_hwpx_paragraph_style(para_xml: &str) -> (HwpxParagraphStyle, Vec<String>) {
    let mut cursor = 0usize;
    let mut paragraph_style = HwpxParagraphStyle::default();
    let mut warnings = Vec::new();

    while let Some(tag) = next_xml_tag(para_xml, cursor) {
        if !tag.is_closing {
            match tag.name {
                "heading" => {
                    let heading_type = xml_attribute_value_any(tag.raw, HWPX_LIST_TYPE_ATTRIBUTES)
                        .map(str::to_ascii_uppercase);
                    paragraph_style.level =
                        xml_attribute_value_any(tag.raw, HWPX_LIST_LEVEL_ATTRIBUTES)
                            .and_then(parse_trimmed::<u8>)
                            .unwrap_or(0);
                    paragraph_style.kind = match heading_type.as_deref() {
                        Some("NUMBER") => Some(ListKind::Ordered),
                        Some("BULLET") => Some(ListKind::Unordered),
                        _ => None,
                    };
                    paragraph_style.role = match heading_type.as_deref() {
                        Some("OUTLINE" | "HEADING") => Some(ParagraphRole::Heading {
                            level: paragraph_style.level.saturating_add(1).clamp(1, 6),
                        }),
                        Some("TITLE") => Some(ParagraphRole::Title),
                        _ => None,
                    };
                    paragraph_style.list_id =
                        xml_attribute_value_any(tag.raw, HWPX_LIST_ID_REF_ATTRIBUTES)
                            .and_then(parse_trimmed);
                }
                "align" => {
                    paragraph_style.style.alignment = xml_attribute_value_any(
                        tag.raw,
                        HWPX_PARAGRAPH_HORIZONTAL_ALIGN_ATTRIBUTES,
                    )
                    .and_then(|value| map_hwpx_alignment_with_warning(value, &mut warnings));
                }
                "intent" | "indent" => {
                    paragraph_style.style.indent.first_line_pt =
                        hwpx_paragraph_hwp_units_to_pt_with_warning(
                            tag.raw,
                            "first-line indent",
                            &mut warnings,
                        );
                }
                "left" => {
                    paragraph_style.style.indent.left_pt =
                        hwpx_paragraph_hwp_units_to_pt_with_warning(
                            tag.raw,
                            "left indent",
                            &mut warnings,
                        );
                }
                "right" => {
                    paragraph_style.style.indent.right_pt =
                        hwpx_paragraph_hwp_units_to_pt_with_warning(
                            tag.raw,
                            "right indent",
                            &mut warnings,
                        );
                }
                "prev" => {
                    paragraph_style.style.spacing.before_pt =
                        hwpx_paragraph_hwp_units_to_pt_with_warning(
                            tag.raw,
                            "spacing before",
                            &mut warnings,
                        );
                }
                "next" => {
                    paragraph_style.style.spacing.after_pt =
                        hwpx_paragraph_hwp_units_to_pt_with_warning(
                            tag.raw,
                            "spacing after",
                            &mut warnings,
                        );
                }
                "lineSpacing" if !is_hwpx_percent_line_spacing(tag.raw) => {
                    paragraph_style.style.spacing.line_pt =
                        hwpx_paragraph_hwp_units_to_pt_with_warning(
                            tag.raw,
                            "line spacing",
                            &mut warnings,
                        );
                }
                "lineSpacing" => {
                    paragraph_style.style.spacing.line_percent =
                        hwpx_percent_line_spacing_with_warning(tag.raw, &mut warnings);
                }
                "breakSetting" => {
                    paragraph_style.widow_orphan = hwpx_paragraph_bool_with_warning(
                        tag.raw,
                        &["widowOrphan", "widow-orphan"],
                        "widow/orphan control",
                        &mut warnings,
                    );
                    paragraph_style.keep_with_next = hwpx_paragraph_bool_with_warning(
                        tag.raw,
                        &["keepWithNext", "keep-with-next"],
                        "keep-with-next",
                        &mut warnings,
                    );
                    paragraph_style.keep_lines = hwpx_paragraph_bool_with_warning(
                        tag.raw,
                        &["keepLines", "keep-lines"],
                        "keep-lines",
                        &mut warnings,
                    );
                    paragraph_style.page_break_before = hwpx_paragraph_bool_with_warning(
                        tag.raw,
                        &["pageBreakBefore", "page-break-before"],
                        "page-break-before",
                        &mut warnings,
                    );
                }
                "border" => {
                    if let Some(value) =
                        xml_attribute_value_any(tag.raw, HWPX_BORDER_FILL_ID_REF_ATTRIBUTES)
                    {
                        match parse_trimmed::<u32>(value) {
                            Some(border_fill_id) => {
                                paragraph_style.border_fill_id = Some(border_fill_id)
                            }
                            None => warnings.push(format!(
                                "HWPX paragraph border referenced unsupported borderFill id `{}`; hwp-convert omitted the paragraph border and background.",
                                value.trim()
                            )),
                        }
                    }
                    paragraph_style.style.padding_left_pt =
                        hwpx_paragraph_border_offset_to_pt_with_warning(
                            tag.raw,
                            &["offsetLeft", "leftOffset"],
                            "left",
                            &mut warnings,
                        );
                    paragraph_style.style.padding_right_pt =
                        hwpx_paragraph_border_offset_to_pt_with_warning(
                            tag.raw,
                            &["offsetRight", "rightOffset"],
                            "right",
                            &mut warnings,
                        );
                    paragraph_style.style.padding_top_pt =
                        hwpx_paragraph_border_offset_to_pt_with_warning(
                            tag.raw,
                            &["offsetTop", "topOffset"],
                            "top",
                            &mut warnings,
                        );
                    paragraph_style.style.padding_bottom_pt =
                        hwpx_paragraph_border_offset_to_pt_with_warning(
                            tag.raw,
                            &["offsetBottom", "bottomOffset"],
                            "bottom",
                            &mut warnings,
                        );
                }
                _ => {}
            }
        }

        cursor = tag.end;
    }

    (paragraph_style, warnings)
}

fn extract_hwpx_direct_paragraph_style(paragraph_xml: &str) -> (HwpxParagraphStyle, Vec<String>) {
    extract_hwpx_paragraph_style(hwpx_direct_paragraph_style_prefix(paragraph_xml))
}

fn hwpx_paragraph_hwp_units_to_pt_with_warning(
    tag: &str,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<LengthPt> {
    let value = xml_attribute_value_any(tag, HWPX_HWP_UNIT_VALUE_ATTRIBUTES)?;
    parse_trimmed::<i32>(value)
        .and_then(hwp_units_to_pt_option)
        .or_else(|| {
            warnings.push(format!(
                "HWPX paragraph style referenced unsupported {label} value `{}`; hwp-convert omitted that paragraph style value.",
                value.trim()
            ));
            None
        })
}

fn hwpx_paragraph_border_offset_to_pt_with_warning(
    tag: &str,
    attributes: &[&str],
    side: &str,
    warnings: &mut Vec<String>,
) -> Option<LengthPt> {
    let value = xml_attribute_value_any(tag, attributes)?;
    match parse_trimmed::<i32>(value) {
        Some(0) => None,
        Some(value) if value > 0 => Some(LengthPt(value as f32 / 100.0)),
        _ => {
            warnings.push(format!(
                "HWPX paragraph border referenced unsupported {side} offset `{}`; hwp-convert omitted that paragraph padding side.",
                value.trim()
            ));
            None
        }
    }
}

fn hwpx_paragraph_bool_with_warning(
    tag: &str,
    attributes: &[&str],
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<bool> {
    let value = xml_attribute_value_any(tag, attributes)?;
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" | "none" => Some(false),
        _ => {
            warnings.push(format!(
                "HWPX paragraph style referenced unsupported {label} value `{}`; hwp-convert used the inherited or default paragraph pagination setting.",
                value.trim()
            ));
            None
        }
    }
}

fn hwpx_direct_paragraph_style_prefix(paragraph_xml: &str) -> &str {
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(paragraph_xml, cursor) {
        if tag.is_closing {
            cursor = tag.end;
            continue;
        }
        if matches!(tag.name, "run" | "ctrl" | "tbl" | "subList") {
            return &paragraph_xml[..tag.start];
        }
        cursor = tag.end;
    }

    paragraph_xml
}

fn is_hwpx_percent_line_spacing(tag: &str) -> bool {
    xml_attribute_value_any(tag, HWPX_LINE_SPACING_TYPE_ATTRIBUTES)
        .is_some_and(|value| value.eq_ignore_ascii_case("PERCENT"))
}

fn hwpx_percent_line_spacing_with_warning(
    tag: &str,
    warnings: &mut Vec<String>,
) -> Option<Percent> {
    let Some(value) = xml_attribute_value_any(tag, HWPX_HWP_UNIT_VALUE_ATTRIBUTES) else {
        warnings.push(
            "HWPX percent line spacing omitted its numeric value; hwp-convert omitted that line height."
                .to_string(),
        );
        return None;
    };
    parse_trimmed::<f32>(value)
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(Percent)
        .or_else(|| {
            warnings.push(format!(
                "HWPX percent line spacing referenced unsupported value `{}`; hwp-convert omitted that line height.",
                value.trim()
            ));
            None
        })
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
                blocks.extend(extract_table_blocks_from_xml(table_xml, context));
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
    let table_padding = [
        hwpx_table_hwp_units_to_px_with_warning(
            table_xml,
            &["inMargin"],
            HWPX_MARGIN_TOP_ATTRIBUTES,
            "top padding",
            context,
        ),
        hwpx_table_hwp_units_to_px_with_warning(
            table_xml,
            &["inMargin"],
            HWPX_MARGIN_RIGHT_ATTRIBUTES,
            "right padding",
            context,
        ),
        hwpx_table_hwp_units_to_px_with_warning(
            table_xml,
            &["inMargin"],
            HWPX_MARGIN_BOTTOM_ATTRIBUTES,
            "bottom padding",
            context,
        ),
        hwpx_table_hwp_units_to_px_with_warning(
            table_xml,
            &["inMargin"],
            HWPX_MARGIN_LEFT_ATTRIBUTES,
            "left padding",
            context,
        ),
    ];
    let mut rows = Vec::new();
    let mut next_order = 0usize;
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
        let cells = extract_table_cells_from_row_xml(row_xml, context, &table_padding);
        if !cells.is_empty() {
            rows.push((
                hwpx_table_row_addr(row_xml),
                next_order,
                TableRow {
                    cells,
                    height: hwpx_table_row_hwp_units_to_px_with_warning(
                        row_xml,
                        HWPX_HEIGHT_ATTRIBUTES,
                        "height",
                        context,
                    ),
                },
            ));
            next_order += 1;
        }
        cursor = row_end;
    }

    if rows.is_empty() {
        return None;
    }
    if rows.iter().any(|(row_addr, _, _)| row_addr.is_some()) {
        rows.sort_by_key(|(row_addr, order, _)| (row_addr.unwrap_or(u32::MAX), *order));
    }
    let rows = rows.into_iter().map(|(_, _, row)| row).collect();

    let background_color = root_or_direct_child_xml_attribute_u32_any(
        table_xml,
        "tbl",
        &["tblPr"],
        HWPX_BORDER_FILL_ID_REF_ATTRIBUTES,
    )
    .and_then(|border_fill_id| context.border_fill_background_color(border_fill_id));
    let width = hwpx_table_hwp_units_to_px_with_warning(
        table_xml,
        &["sz", "size", "extent"],
        HWPX_WIDTH_ATTRIBUTES,
        "width",
        context,
    );
    let height = hwpx_table_hwp_units_to_px_with_warning(
        table_xml,
        &["sz", "size", "extent"],
        HWPX_HEIGHT_ATTRIBUTES,
        "height",
        context,
    );
    let margin_top = hwpx_table_hwp_units_to_px_with_warning(
        table_xml,
        &["outMargin"],
        HWPX_MARGIN_TOP_ATTRIBUTES,
        "top outer margin",
        context,
    );
    let margin_right = hwpx_table_hwp_units_to_px_with_warning(
        table_xml,
        &["outMargin"],
        HWPX_MARGIN_RIGHT_ATTRIBUTES,
        "right outer margin",
        context,
    );
    let margin_bottom = hwpx_table_hwp_units_to_px_with_warning(
        table_xml,
        &["outMargin"],
        HWPX_MARGIN_BOTTOM_ATTRIBUTES,
        "bottom outer margin",
        context,
    );
    let margin_left = hwpx_table_hwp_units_to_px_with_warning(
        table_xml,
        &["outMargin"],
        HWPX_MARGIN_LEFT_ATTRIBUTES,
        "left outer margin",
        context,
    );

    Some(Table {
        rows,
        style: TableStyle {
            background_color,
            width,
            height,
            margin_top,
            margin_right,
            margin_bottom,
            margin_left,
        },
    })
}

fn hwpx_table_row_addr(row_xml: &str) -> Option<u32> {
    root_xml_attribute_u32_any(row_xml, "tr", HWPX_TABLE_CELL_ROW_ADDR_ATTRIBUTES).or_else(|| {
        let mut cursor = 0usize;
        while let Some(tag) = next_xml_tag(row_xml, cursor) {
            if tag.name == "tc" && !tag.is_closing {
                let from_cell_attr =
                    xml_attribute_value_any(tag.raw, HWPX_TABLE_CELL_ROW_ADDR_ATTRIBUTES)
                        .and_then(parse_trimmed);
                if from_cell_attr.is_some() {
                    return from_cell_attr;
                }
                let cell_end = if tag.is_self_closing {
                    tag.end
                } else {
                    find_matching_element_end(row_xml, &tag).unwrap_or(tag.end)
                };
                return root_or_direct_child_xml_attribute_u32_any(
                    &row_xml[tag.start..cell_end],
                    "tc",
                    &["cellAddr"],
                    HWPX_TABLE_CELL_ROW_ADDR_ATTRIBUTES,
                );
            }
            cursor = tag.end;
        }
        None
    })
}

fn hwpx_table_row_hwp_units_to_px_with_warning(
    row_xml: &str,
    attribute_names: &[&str],
    label: &str,
    context: &mut HwpxFallbackContext,
) -> Option<LengthPx> {
    let value = root_or_direct_child_xml_attribute_value_any(
        row_xml,
        "tr",
        &["trPr", "sz", "size", "extent"],
        attribute_names,
    )?;
    parse_trimmed::<u32>(value)
        .and_then(hwp_units_to_px_option)
        .or_else(|| {
            context.add_warning_once(&format!(
                "HWPX table row referenced unsupported {label} value `{}`; hwp-convert omitted that row height.",
                value.trim()
            ));
            None
        })
}

fn extract_table_blocks_from_xml(table_xml: &str, context: &mut HwpxFallbackContext) -> Vec<Block> {
    let Some(table) = extract_table_from_xml(table_xml, context) else {
        return Vec::new();
    };
    let table_block = Block::Table(table);
    let Some(caption) = extract_hwpx_object_caption_blocks(table_xml, context) else {
        return vec![table_block];
    };

    match caption.placement {
        HwpxCaptionPlacement::Before => {
            let mut blocks = caption.blocks;
            blocks.push(table_block);
            blocks
        }
        HwpxCaptionPlacement::After => {
            let mut blocks = vec![table_block];
            blocks.extend(caption.blocks);
            blocks
        }
    }
}

fn extract_table_cells_from_row_xml(
    row_xml: &str,
    context: &mut HwpxFallbackContext,
    table_padding: &[Option<LengthPx>; 4],
) -> Vec<TableCell> {
    let mut cells = Vec::new();
    let mut next_order = 0usize;
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
        let col_addr =
            root_xml_attribute_u32_any(cell_xml, "tc", HWPX_TABLE_CELL_COL_ADDR_ATTRIBUTES)
                .or_else(|| {
                    root_or_direct_child_xml_attribute_u32_any(
                        cell_xml,
                        "tc",
                        &["cellAddr"],
                        HWPX_TABLE_CELL_COL_ADDR_ATTRIBUTES,
                    )
                });
        cells.push((
            col_addr,
            next_order,
            extract_table_cell_from_xml(cell_xml, context, table_padding),
        ));
        next_order += 1;
        cursor = cell_end;
    }

    if cells.iter().any(|(col_addr, _, _)| col_addr.is_some()) {
        cells.sort_by_key(|(col_addr, order, _)| (col_addr.unwrap_or(u32::MAX), *order));
    }

    cells.into_iter().map(|(_, _, cell)| cell).collect()
}

fn extract_table_cell_from_xml(
    cell_xml: &str,
    context: &mut HwpxFallbackContext,
    table_padding: &[Option<LengthPx>; 4],
) -> TableCell {
    let border_fill_id = root_or_direct_child_xml_attribute_u32_any(
        cell_xml,
        "tc",
        &["cellPr"],
        HWPX_BORDER_FILL_ID_REF_ATTRIBUTES,
    );
    let background_color = border_fill_id.and_then(|id| context.border_fill_background_color(id));
    let [border_left, border_right, border_top, border_bottom] = border_fill_id
        .map(|id| context.border_fill_borders(id))
        .unwrap_or_default();

    let is_header = root_or_direct_child_xml_attribute_value_any(
        cell_xml,
        "tc",
        &["cellPr"],
        HWPX_TABLE_CELL_HEADER_ATTRIBUTES,
    )
    .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
    let vertical_align = root_or_direct_child_xml_attribute_value_any(
        cell_xml,
        "tc",
        &["subList"],
        HWPX_VERTICAL_ALIGN_ATTRIBUTES,
    )
    .and_then(|value| map_hwpx_vertical_align_with_warning(value, context));
    let has_margin_attribute = root_xml_attribute_value(cell_xml, "hasMargin");
    let has_explicit_cell_margin = [
        HWPX_MARGIN_TOP_ATTRIBUTES,
        HWPX_MARGIN_RIGHT_ATTRIBUTES,
        HWPX_MARGIN_BOTTOM_ATTRIBUTES,
        HWPX_MARGIN_LEFT_ATTRIBUTES,
    ]
    .iter()
    .any(|attributes| {
        root_or_direct_child_xml_attribute_value_any(cell_xml, "tc", &["cellMargin"], attributes)
            .is_some()
    });
    let use_cell_margin = has_margin_attribute
        .map(xml_boolean_is_true)
        .unwrap_or(has_explicit_cell_margin);
    let padding = if use_cell_margin {
        [
            hwpx_table_cell_hwp_units_to_px_with_warning(
                cell_xml,
                &["cellMargin"],
                HWPX_MARGIN_TOP_ATTRIBUTES,
                "top padding",
                context,
            ),
            hwpx_table_cell_hwp_units_to_px_with_warning(
                cell_xml,
                &["cellMargin"],
                HWPX_MARGIN_RIGHT_ATTRIBUTES,
                "right padding",
                context,
            ),
            hwpx_table_cell_hwp_units_to_px_with_warning(
                cell_xml,
                &["cellMargin"],
                HWPX_MARGIN_BOTTOM_ATTRIBUTES,
                "bottom padding",
                context,
            ),
            hwpx_table_cell_hwp_units_to_px_with_warning(
                cell_xml,
                &["cellMargin"],
                HWPX_MARGIN_LEFT_ATTRIBUTES,
                "left padding",
                context,
            ),
        ]
    } else {
        *table_padding
    };
    let mut blocks = extract_section_xml_blocks(cell_xml, context);
    if let Some(field_name) = root_or_direct_child_xml_attribute_value_any(
        cell_xml,
        "tc",
        &["cellPr"],
        HWPX_TABLE_CELL_FIELD_NAME_ATTRIBUTES,
    )
    .map(decode_xml_text)
    .and_then(non_empty_string_owned)
    {
        blocks.insert(
            0,
            Block::Unknown(UnknownBlock {
                kind: "table_cell_field".to_string(),
                fallback_text: Some(format!("[cell field: {field_name}]")),
                message: Some(
                    "Table cell field name preserved as fallback text because Document IR does not yet model cell fields."
                        .to_string(),
                ),
                source: Some("hwpx".to_string()),
            }),
        );
        context.add_warning_once(
            "HWPX table cell field names were preserved as unknown block fallback text.",
        );
    }

    TableCell {
        row_span: hwpx_table_cell_span(cell_xml, HWPX_TABLE_CELL_ROW_SPAN_ATTRIBUTES),
        col_span: hwpx_table_cell_span(cell_xml, HWPX_TABLE_CELL_COL_SPAN_ATTRIBUTES),
        is_header,
        blocks,
        style: TableCellStyle {
            background_color,
            vertical_align,
            width: hwpx_table_cell_hwp_units_to_px_with_warning(
                cell_xml,
                &["cellSz"],
                HWPX_WIDTH_ATTRIBUTES,
                "width",
                context,
            ),
            height: hwpx_table_cell_hwp_units_to_px_with_warning(
                cell_xml,
                &["cellSz"],
                HWPX_HEIGHT_ATTRIBUTES,
                "height",
                context,
            ),
            padding_top: padding[0],
            padding_right: padding[1],
            padding_bottom: padding[2],
            padding_left: padding[3],
            border_top,
            border_right,
            border_bottom,
            border_left,
        },
    }
}

fn hwpx_table_cell_span(cell_xml: &str, attribute_names: &[&str]) -> u32 {
    attribute_names
        .iter()
        .find_map(|attribute_name| {
            root_or_direct_child_xml_attribute_u32(cell_xml, "tc", &["cellSpan"], attribute_name)
        })
        .filter(|span| *span > 0)
        .unwrap_or(1)
}

fn hwpx_table_cell_hwp_units_to_px_with_warning(
    cell_xml: &str,
    child_names: &[&str],
    attribute_names: &[&str],
    label: &str,
    context: &mut HwpxFallbackContext,
) -> Option<LengthPx> {
    let value =
        root_or_direct_child_xml_attribute_value_any(cell_xml, "tc", child_names, attribute_names)?;
    parse_trimmed::<u32>(value)
        .and_then(hwp_units_to_px_option)
        .or_else(|| {
            context.add_warning_once(&format!(
                "HWPX table cell referenced unsupported {label} value `{}`; hwp-convert omitted that table cell style value.",
                value.trim()
            ));
            None
        })
}

fn hwpx_table_hwp_units_to_px_with_warning(
    table_xml: &str,
    child_names: &[&str],
    attribute_names: &[&str],
    label: &str,
    context: &mut HwpxFallbackContext,
) -> Option<LengthPx> {
    let value = root_or_direct_child_xml_attribute_value_any(
        table_xml,
        "tbl",
        child_names,
        attribute_names,
    )?;
    parse_trimmed::<u32>(value)
        .and_then(hwp_units_to_px_option)
        .or_else(|| {
            context.add_warning_once(&format!(
                "HWPX table referenced unsupported {label} value `{}`; hwp-convert omitted that table style value.",
                value.trim()
            ));
            None
        })
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
    let paragraph_role = context.paragraph_role_for_paragraph(paragraph_xml);
    let mut pending_list = context.list_info_for_paragraph(paragraph_xml);

    while let Some(tag) = next_xml_tag(paragraph_xml, cursor) {
        if tag.is_closing {
            cursor = tag.end;
            continue;
        }

        if tag.name == "ctrl" && !hwpx_control_contains_supported_content(paragraph_xml, &tag) {
            let control_end = if tag.is_self_closing {
                tag.end
            } else {
                let Some(end) = find_matching_element_end(paragraph_xml, &tag) else {
                    cursor = tag.end;
                    continue;
                };
                end
            };
            let pushed_fragment = push_paragraph_text_fragment_as_block(
                &mut blocks,
                &paragraph_xml[fragment_start..tag.start],
                pending_list.clone(),
                paragraph_role.clone(),
                paragraph_style.clone(),
                context,
            );
            if pushed_fragment {
                pending_list = None;
            }

            let control_xml = &paragraph_xml[tag.start..control_end];
            if let Some(layout_kind) = hwpx_layout_only_control_kind(control_xml) {
                context.add_warning_once(&format!(
                    "HWPX {layout_kind} control is not modeled by Document IR; hwp-convert omitted its layout metadata."
                ));
            } else {
                blocks.push(Block::Unknown(unknown_hwpx_control_block(
                    control_xml,
                    context,
                )));
            }
            fragment_start = control_end;
            cursor = control_end;
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

        let pushed_fragment = push_paragraph_text_fragment_as_block(
            &mut blocks,
            &paragraph_xml[fragment_start..tag.start],
            pending_list.clone(),
            paragraph_role.clone(),
            paragraph_style.clone(),
            context,
        );
        if pushed_fragment {
            pending_list = None;
        }

        if object_kind == Some("table") {
            let table_xml = &paragraph_xml[tag.start..object_end];
            blocks.extend(extract_table_blocks_from_xml(table_xml, context));
        } else if object_kind == Some("image") {
            let object_xml = &paragraph_xml[tag.start..object_end];
            if let Some(image) = extract_hwpx_image_from_pic_xml(object_xml, context) {
                blocks.push(Block::Image(image));
            } else {
                blocks.push(Block::Unknown(unknown_hwpx_object_block(
                    "image", object_xml, context,
                )));
            }
        } else if object_kind == Some("equation") {
            let object_xml = &paragraph_xml[tag.start..object_end];
            blocks.push(Block::Equation(extract_hwpx_equation_from_xml(
                object_xml, context,
            )));
        } else if object_kind == Some("shape") {
            let object_xml = &paragraph_xml[tag.start..object_end];
            blocks.push(Block::Shape(extract_hwpx_shape_from_xml(
                tag.name, object_xml, context,
            )));
        } else if object_kind == Some("chart") {
            let object_xml = &paragraph_xml[tag.start..object_end];
            blocks.push(Block::Chart(extract_hwpx_chart_from_xml(
                object_xml, context,
            )));
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

    let final_list = pending_list.take();
    let pushed_final_fragment = push_paragraph_text_fragment_as_block(
        &mut blocks,
        &paragraph_xml[fragment_start..],
        final_list.clone(),
        paragraph_role.clone(),
        paragraph_style.clone(),
        context,
    );
    if !pushed_final_fragment
        && blocks.is_empty()
        && !hwpx_paragraph_has_block_level_content(paragraph_xml)
    {
        blocks.push(Block::Paragraph(Paragraph {
            role: paragraph_role,
            inlines: Vec::new(),
            style: paragraph_style,
            style_ref: None,
            list: final_list,
        }));
    }
    blocks
}

fn hwpx_paragraph_has_block_level_content(paragraph_xml: &str) -> bool {
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(paragraph_xml, cursor) {
        if tag.is_closing {
            cursor = tag.end;
            continue;
        }

        if tag.name == "ctrl"
            || hwpx_fallback_object_kind(tag.name).is_some()
            || hwpx_fallback_structural_control_kind(tag.name).is_some()
        {
            return true;
        }

        cursor = tag.end;
    }

    false
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

fn hwpx_control_contains_supported_content(xml: &str, control_tag: &XmlTag<'_>) -> bool {
    let control_end = if control_tag.is_self_closing {
        control_tag.end
    } else {
        find_matching_element_end(xml, control_tag).unwrap_or(control_tag.end)
    };
    let control_xml = &xml[control_tag.start..control_end];
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(control_xml, cursor) {
        if tag.is_closing || tag.name == "ctrl" {
            cursor = tag.end;
            continue;
        }

        if hwpx_fallback_object_kind(tag.name).is_some()
            || hwpx_fallback_structural_control_kind(tag.name).is_some()
            || is_hwpx_inline_control_tag(tag.name)
        {
            return true;
        }

        cursor = tag.end;
    }

    false
}

fn hwpx_layout_only_control_kind(control_xml: &str) -> Option<&'static str> {
    match first_hwpx_control_child_name(control_xml)?.as_str() {
        "colPr" => Some("column definition"),
        _ => None,
    }
}

fn is_hwpx_inline_control_tag(tag_name: &str) -> bool {
    matches!(
        tag_name,
        "bookmark"
            | "fieldBegin"
            | "fieldEnd"
            | "footNote"
            | "endNote"
            | "hyperlink"
            | "a"
            | "link"
    )
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
    let value =
        root_xml_attribute_value_any(control_xml, HWPX_HEADER_FOOTER_APPLY_PAGE_TYPE_ATTRIBUTES)
            .map(|value| value.trim().to_ascii_uppercase());

    match value.as_deref() {
        Some("EVEN") => HeaderFooterPlacement::EvenPage,
        Some("ODD") => HeaderFooterPlacement::OddPage,
        Some("FIRST" | "FIRST_PAGE") => HeaderFooterPlacement::FirstPage,
        _ => HeaderFooterPlacement::Default,
    }
}

fn extract_hwpx_equation_from_xml(
    equation_xml: &str,
    context: &mut HwpxFallbackContext,
) -> Equation {
    let content = first_non_empty_string([
        decoded_root_xml_attribute_value_any(equation_xml, HWPX_EQUATION_CONTENT_ATTRIBUTES),
        first_hwpx_direct_child_element_text(equation_xml, HWPX_EQUATION_CONTENT_ATTRIBUTES),
        non_empty_string_owned(inlines_to_plain_text(&extract_inlines_from_xml_fragment(
            equation_xml,
            context,
        ))),
    ]);

    Equation {
        kind: EquationKind::PlainText,
        fallback_text: content.clone().or_else(|| Some("[equation]".to_string())),
        content,
        resource_id: None,
        ..Default::default()
    }
}

fn extract_hwpx_shape_from_xml(
    tag_name: &str,
    shape_xml: &str,
    context: &mut HwpxFallbackContext,
) -> Shape {
    let description = decoded_root_xml_attribute_value_any(shape_xml, HWPX_DESCRIPTION_ATTRIBUTES);
    let shape_text = non_empty_string_owned(inlines_to_plain_text(
        &extract_inlines_from_xml_fragment(shape_xml, context),
    ));
    let fallback_text = first_non_empty_string([description.clone(), shape_text])
        .or_else(|| Some("[shape]".to_string()));

    Shape {
        kind: hwpx_shape_kind(tag_name),
        fallback_text,
        description,
        ..Default::default()
    }
}

fn hwpx_shape_kind(tag_name: &str) -> ShapeKind {
    match tag_name {
        "line" | "connectLine" => ShapeKind::Line,
        "rect" => ShapeKind::Rectangle,
        "ellipse" | "arc" => ShapeKind::Ellipse,
        "polygon" | "curve" => ShapeKind::Polygon,
        _ => ShapeKind::Unknown,
    }
}

fn extract_hwpx_chart_from_xml(chart_xml: &str, context: &mut HwpxFallbackContext) -> Chart {
    let title = first_non_empty_string([
        decoded_root_xml_attribute_value_any(chart_xml, HWPX_CHART_TITLE_ATTRIBUTES),
        first_hwpx_direct_child_element_text(chart_xml, &["title", "caption", "name"]),
    ]);
    let chart_text = non_empty_string_owned(inlines_to_plain_text(
        &extract_inlines_from_xml_fragment(chart_xml, context),
    ));
    let fallback_text =
        first_non_empty_string([title.clone(), chart_text]).or_else(|| Some("[chart]".to_string()));

    Chart {
        title,
        fallback_text,
        resource_id: None,
    }
}

fn extract_hwpx_image_from_pic_xml(
    pic_xml: &str,
    context: &mut HwpxFallbackContext,
) -> Option<Image> {
    let binary_item_id_ref = hwpx_pic_binary_item_id_ref(pic_xml)?;
    let resource_id = context.ensure_image_resource(&binary_item_id_ref)?;
    warn_hwpx_picture_transform(pic_xml, context);
    let image_attributes = hwpx_pic_image_attributes(pic_xml, &binary_item_id_ref);
    let image_effect = image_attributes.effect;
    if image_attributes.brightness != 0 || image_attributes.contrast != 0 {
        context.add_warning_once(&format!(
            "HWPX picture brightness/contrast (brightness:{},contrast:{}) is not modeled; hwp-convert preserved the original image bytes without applying the adjustment.",
            image_attributes.brightness, image_attributes.contrast
        ));
    }
    if image_attributes.alpha.abs() > f64::EPSILON {
        context.add_warning_once(&format!(
            "HWPX picture alpha {} is not modeled; hwp-convert preserved the original image bytes without applying transparency.",
            image_attributes.alpha
        ));
    }
    let grayscale = match image_effect.as_deref() {
        Some("GRAY_SCALE") => true,
        Some("BLACK_WHITE") => {
            context.add_warning_once(
                "HWPX picture BLACK_WHITE effect was represented as a grayscale approximation because Image IR does not distinguish threshold black-and-white.",
            );
            true
        }
        Some("REAL_PIC" | "NONE") | None => false,
        Some(effect) => {
            context.add_warning_once(&format!(
                "HWPX picture effect `{effect}` is not modeled; hwp-convert preserved the original image bytes without applying the effect."
            ));
            false
        }
    };

    Some(Image {
        resource_id,
        alt: first_non_empty_string([
            decoded_root_xml_attribute_value_any(pic_xml, HWPX_DESCRIPTION_ATTRIBUTES),
            first_hwpx_direct_child_element_text(
                pic_xml,
                &["altText", "description", "desc", "name", "title"],
            ),
            first_hwpx_direct_child_element_text(pic_xml, &["shapeComment"]),
        ]),
        caption: extract_hwpx_object_caption(pic_xml, context),
        width: hwpx_object_dimension_to_px_with_warning(pic_xml, &["width", "w"], "width", context),
        height: hwpx_object_dimension_to_px_with_warning(
            pic_xml,
            &["height", "h"],
            "height",
            context,
        ),
        border: hwpx_picture_border(pic_xml, context),
        grayscale,
    })
}

fn hwpx_picture_border(pic_xml: &str, context: &mut HwpxFallbackContext) -> Option<Border> {
    let tag = hwpx_picture_direct_child_tag(pic_xml, "lineShape")?;
    let style_name = xml_attribute_value_any(tag.raw, HWPX_IMAGE_BORDER_STYLE_ATTRIBUTES)
        .unwrap_or("SOLID")
        .trim()
        .to_ascii_uppercase();
    if style_name == "NONE" {
        return None;
    }
    let width_value = xml_attribute_value_any(tag.raw, HWPX_IMAGE_BORDER_WIDTH_ATTRIBUTES)?;
    let Some(width) = width_value
        .trim()
        .parse()
        .ok()
        .and_then(hwp_units_to_px_option)
    else {
        context.add_warning_once(&format!(
            "HWPX picture lineShape referenced unsupported border width `{}`; hwp-convert omitted the picture border.",
            width_value.trim()
        ));
        return None;
    };
    let mut warnings = Vec::new();
    let style = map_hwpx_border_style_with_warning(&style_name, "picture lineShape", &mut warnings);
    for warning in warnings {
        context.add_warning_once(&warning);
    }
    Some(Border {
        width,
        style,
        color: xml_attribute_value_any(tag.raw, HWPX_IMAGE_BORDER_COLOR_ATTRIBUTES)
            .and_then(parse_hwpx_hex_color),
    })
}

fn warn_hwpx_picture_transform(pic_xml: &str, context: &mut HwpxFallbackContext) {
    let flip = hwpx_picture_direct_child_tag(pic_xml, "flip");
    let horizontal_flip = flip
        .and_then(|tag| xml_attribute_value_any(tag.raw, HWPX_IMAGE_FLIP_HORIZONTAL_ATTRIBUTES))
        .is_some_and(xml_boolean_is_true);
    let vertical_flip = flip
        .and_then(|tag| xml_attribute_value_any(tag.raw, HWPX_IMAGE_FLIP_VERTICAL_ATTRIBUTES))
        .is_some_and(xml_boolean_is_true);
    let rotation = hwpx_picture_direct_child_tag(pic_xml, "rotationInfo")
        .and_then(|tag| xml_attribute_value_any(tag.raw, HWPX_IMAGE_ROTATION_ANGLE_ATTRIBUTES))
        .and_then(|value| value.trim().parse::<i32>().ok())
        .unwrap_or(0);

    if horizontal_flip || vertical_flip || rotation != 0 {
        context.add_warning_once(&format!(
            "HWPX picture transform (flip_h:{horizontal_flip},flip_v:{vertical_flip},rotation:{rotation}) is not modeled; hwp-convert preserved the original image bytes without applying the transform."
        ));
    }

    let root = next_xml_tag(pic_xml, 0);
    let text_wrap = root
        .and_then(|tag| xml_attribute_value_any(tag.raw, HWPX_PICTURE_TEXT_WRAP_ATTRIBUTES))
        .unwrap_or("SQUARE")
        .trim()
        .to_ascii_uppercase();
    let pos = hwpx_picture_direct_child_tag(pic_xml, "pos");
    if let Some(pos) = pos {
        let treat_as_char = xml_attribute_value_any(pos.raw, HWPX_PICTURE_TREAT_AS_CHAR_ATTRIBUTES)
            .is_some_and(xml_boolean_is_true);
        let vert_rel_to = xml_attribute_value_any(pos.raw, HWPX_PICTURE_VERTICAL_REL_TO_ATTRIBUTES)
            .unwrap_or("PAPER");
        let vert_align = xml_attribute_value_any(pos.raw, HWPX_PICTURE_VERTICAL_ALIGN_ATTRIBUTES)
            .unwrap_or("TOP");
        let vert_offset = xml_attribute_value_any(pos.raw, HWPX_PICTURE_VERTICAL_OFFSET_ATTRIBUTES)
            .unwrap_or("0");
        let horz_rel_to =
            xml_attribute_value_any(pos.raw, HWPX_PICTURE_HORIZONTAL_REL_TO_ATTRIBUTES)
                .unwrap_or("PAPER");
        let horz_align = xml_attribute_value_any(pos.raw, HWPX_PICTURE_HORIZONTAL_ALIGN_ATTRIBUTES)
            .unwrap_or("LEFT");
        let horz_offset =
            xml_attribute_value_any(pos.raw, HWPX_PICTURE_HORIZONTAL_OFFSET_ATTRIBUTES)
                .unwrap_or("0");
        let has_layout = !treat_as_char
            || text_wrap != "SQUARE"
            || vert_rel_to != "PAPER"
            || vert_align != "TOP"
            || vert_offset != "0"
            || horz_rel_to != "PAPER"
            || horz_align != "LEFT"
            || horz_offset != "0";
        if has_layout {
            context.add_warning_once(&format!(
                "HWPX picture layout (treat_as_char:{treat_as_char},wrap:{text_wrap},vertical:{vert_rel_to}/{vert_align}/{vert_offset},horizontal:{horz_rel_to}/{horz_align}/{horz_offset}) is not modeled; block order and image dimensions were preserved without floating placement."
            ));
        }
    } else if text_wrap != "SQUARE" {
        context.add_warning_once(&format!(
            "HWPX picture text wrap `{text_wrap}` is not modeled; block order and image dimensions were preserved without floating placement."
        ));
    }

    let clip = hwpx_picture_direct_child_tag(pic_xml, "imgClip").and_then(|tag| {
        Some([
            xml_attribute_value_any(tag.raw, HWPX_IMAGE_CROP_LEFT_ATTRIBUTES)?
                .trim()
                .parse::<i64>()
                .ok()?,
            xml_attribute_value_any(tag.raw, HWPX_IMAGE_CROP_RIGHT_ATTRIBUTES)?
                .trim()
                .parse::<i64>()
                .ok()?,
            xml_attribute_value_any(tag.raw, HWPX_IMAGE_CROP_TOP_ATTRIBUTES)?
                .trim()
                .parse::<i64>()
                .ok()?,
            xml_attribute_value_any(tag.raw, HWPX_IMAGE_CROP_BOTTOM_ATTRIBUTES)?
                .trim()
                .parse::<i64>()
                .ok()?,
        ])
    });
    let dimensions = hwpx_picture_direct_child_tag(pic_xml, "imgDim").and_then(|tag| {
        Some([
            xml_attribute_value_any(tag.raw, &["dimwidth", "dimWidth"])?
                .trim()
                .parse::<i64>()
                .ok()?,
            xml_attribute_value_any(tag.raw, &["dimheight", "dimHeight"])?
                .trim()
                .parse::<i64>()
                .ok()?,
        ])
    });
    if let (Some([left, right, top, bottom]), Some([width, height])) = (clip, dimensions)
        && (left != 0 || top != 0 || right != width || bottom != height)
    {
        context.add_warning_once(&format!(
            "HWPX picture crop (left:{left},right:{right},top:{top},bottom:{bottom},source:{width}x{height}) is not modeled; hwp-convert preserved the uncropped original image bytes."
        ));
    }

    let effect_names = hwpx_picture_effect_names(pic_xml);
    if !effect_names.is_empty() {
        context.add_warning_once(&format!(
            "HWPX picture visual effects ({}) are not modeled; hwp-convert preserved the original image bytes without applying the effects.",
            effect_names.join(",")
        ));
    }

    if let Some(margin) = hwpx_picture_direct_child_tag(pic_xml, "inMargin") {
        let values = [
            HWPX_MARGIN_LEFT_ATTRIBUTES,
            HWPX_MARGIN_RIGHT_ATTRIBUTES,
            HWPX_MARGIN_TOP_ATTRIBUTES,
            HWPX_MARGIN_BOTTOM_ATTRIBUTES,
        ]
        .map(|attributes| {
            xml_attribute_value_any(margin.raw, attributes)
                .and_then(|value| value.trim().parse::<u32>().ok())
                .unwrap_or(0)
        });
        if values.iter().any(|value| *value != 0) {
            context.add_warning_once(&format!(
                "HWPX picture inner margin ({}/{}/{}/{}) is not modeled; hwp-convert preserved the image without applying the margin.",
                values[0], values[1], values[2], values[3]
            ));
        }
    }
}

fn hwpx_picture_effect_names(pic_xml: &str) -> Vec<String> {
    let Some(effects) = hwpx_picture_direct_child_tag(pic_xml, "effects") else {
        return Vec::new();
    };
    if effects.is_self_closing {
        return Vec::new();
    }
    let Some(effects_end) = find_matching_element_end(pic_xml, &effects) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut cursor = effects.end;
    while let Some(tag) = next_xml_tag(pic_xml, cursor) {
        if tag.start >= effects_end {
            break;
        }
        if tag.is_closing || tag.name.is_empty() {
            cursor = tag.end;
            continue;
        }
        if !names.iter().any(|name| name == tag.name) {
            names.push(tag.name.to_string());
        }
        cursor = if tag.is_self_closing {
            tag.end
        } else {
            find_matching_element_end(pic_xml, &tag).unwrap_or(tag.end)
        };
    }
    names
}

fn xml_boolean_is_true(value: &str) -> bool {
    let value = value.trim();
    value == "1" || value.eq_ignore_ascii_case("true")
}

fn hwpx_picture_direct_child_tag<'a>(pic_xml: &'a str, child_name: &str) -> Option<XmlTag<'a>> {
    let root = next_xml_tag(pic_xml, 0)?;
    if root.name != "pic" || root.is_closing || root.is_self_closing {
        return None;
    }
    let root_end = find_matching_element_end(pic_xml, &root)?;
    let mut cursor = root.end;

    while let Some(tag) = next_xml_tag(pic_xml, cursor) {
        if tag.start >= root_end {
            break;
        }
        if tag.is_closing {
            cursor = tag.end;
            continue;
        }
        let tag_end = if tag.is_self_closing {
            tag.end
        } else {
            find_matching_element_end(pic_xml, &tag).unwrap_or(tag.end)
        };
        if tag.name == child_name {
            return Some(tag);
        }
        cursor = tag_end;
    }

    None
}

#[derive(Clone, Debug, Default, PartialEq)]
struct HwpxPictureImageAttributes {
    effect: Option<String>,
    brightness: i32,
    contrast: i32,
    alpha: f64,
}

fn hwpx_pic_image_attributes(
    pic_xml: &str,
    binary_item_id_ref: &str,
) -> HwpxPictureImageAttributes {
    let mut cursor = 0usize;
    while let Some(tag) = next_xml_tag(pic_xml, cursor) {
        cursor = tag.end;
        if tag.is_closing || !matches!(tag.name, "img" | "image") {
            continue;
        }
        let Some(tag_resource_ref) =
            decoded_xml_attribute_value_any(tag.raw, HWPX_BINARY_ITEM_ID_REF_ATTRIBUTES)
        else {
            continue;
        };
        if !tag_resource_ref.eq_ignore_ascii_case(binary_item_id_ref) {
            continue;
        }

        return HwpxPictureImageAttributes {
            effect: xml_attribute_value_any(tag.raw, HWPX_IMAGE_EFFECT_ATTRIBUTES)
                .and_then(|value| non_empty_string_owned(value.trim().to_ascii_uppercase())),
            brightness: xml_attribute_value_any(tag.raw, HWPX_IMAGE_BRIGHTNESS_ATTRIBUTES)
                .and_then(|value| value.trim().parse().ok())
                .unwrap_or(0),
            contrast: xml_attribute_value_any(tag.raw, HWPX_IMAGE_CONTRAST_ATTRIBUTES)
                .and_then(|value| value.trim().parse().ok())
                .unwrap_or(0),
            alpha: xml_attribute_value_any(tag.raw, HWPX_IMAGE_ALPHA_ATTRIBUTES)
                .and_then(|value| value.trim().parse().ok())
                .unwrap_or(0.0),
        };
    }
    HwpxPictureImageAttributes::default()
}

fn hwpx_pic_binary_item_id_ref(pic_xml: &str) -> Option<String> {
    let root = next_xml_tag(pic_xml, 0)?;
    if root.name != "pic" || root.is_closing {
        return None;
    }
    if let Some(value) =
        decoded_xml_attribute_value_any(root.raw, HWPX_BINARY_ITEM_ID_REF_ATTRIBUTES)
    {
        return Some(value);
    }
    if root.is_self_closing {
        return None;
    }

    let root_end = find_matching_element_end(pic_xml, &root)?;
    let mut cursor = root.end;

    while let Some(tag) = next_xml_tag(pic_xml, cursor) {
        if tag.start >= root_end {
            break;
        }
        if tag.is_closing {
            cursor = tag.end;
            continue;
        }
        let tag_end = if tag.is_self_closing {
            tag.end
        } else {
            find_matching_element_end(pic_xml, &tag).unwrap_or(tag.end)
        };
        if matches!(tag.name, "img" | "image")
            && let Some(image_xml) = pic_xml.get(tag.start..tag_end)
            && let Some(value) =
                first_decoded_xml_attribute_value_any(image_xml, HWPX_BINARY_ITEM_ID_REF_ATTRIBUTES)
        {
            return Some(value);
        }
        cursor = tag_end;
    }

    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HwpxCaptionPlacement {
    Before,
    After,
}

#[derive(Clone, Debug, PartialEq)]
struct HwpxObjectCaption {
    blocks: Vec<Block>,
    placement: HwpxCaptionPlacement,
}

fn extract_hwpx_object_caption(
    object_xml: &str,
    context: &mut HwpxFallbackContext,
) -> Option<String> {
    let caption = extract_hwpx_object_caption_blocks(object_xml, context)?;
    let text = crate::util::plain_text::blocks_to_plain_text(&caption.blocks);

    non_empty_string_owned(text)
}

fn extract_hwpx_object_caption_blocks(
    object_xml: &str,
    context: &mut HwpxFallbackContext,
) -> Option<HwpxObjectCaption> {
    let root = next_xml_tag(object_xml, 0)?;
    if root.is_closing || root.is_self_closing {
        return None;
    }
    let root_end = find_matching_element_end(object_xml, &root)?;
    let mut cursor = root.end;

    while let Some(tag) = next_xml_tag(object_xml, cursor) {
        if tag.start >= root_end {
            break;
        }
        if tag.is_closing {
            cursor = tag.end;
            continue;
        }
        let tag_end = if tag.is_self_closing {
            tag.end
        } else {
            find_matching_element_end(object_xml, &tag).unwrap_or(tag.end)
        };
        if matches!(tag.name, "caption" | "cap") {
            let caption_end = tag_end;
            let caption_xml = &object_xml[tag.start..caption_end];
            let mut blocks = extract_section_xml_blocks(caption_xml, context);
            mark_blocks_as_caption(&mut blocks);

            if !blocks.is_empty() {
                return Some(HwpxObjectCaption {
                    blocks,
                    placement: hwpx_caption_placement(tag.raw),
                });
            }
        }

        cursor = tag_end;
    }

    None
}

fn mark_blocks_as_caption(blocks: &mut [Block]) {
    for block in blocks {
        if let Block::Paragraph(paragraph) = block {
            paragraph.role = ParagraphRole::Caption;
        }
    }
}

fn hwpx_caption_placement(caption_tag: &str) -> HwpxCaptionPlacement {
    let value = decoded_xml_attribute_value_any(caption_tag, HWPX_CAPTION_PLACEMENT_ATTRIBUTES);

    let normalized = value.as_deref().map(str::to_ascii_uppercase);

    match normalized.as_deref() {
        Some("LEFT" | "TOP" | "L" | "T" | "BEFORE") => HwpxCaptionPlacement::Before,
        _ => HwpxCaptionPlacement::After,
    }
}

fn hwpx_object_dimension_to_px_with_warning(
    pic_xml: &str,
    attribute_names: &[&str],
    label: &str,
    context: &mut HwpxFallbackContext,
) -> Option<LengthPx> {
    let value = attribute_names.iter().find_map(|attribute_name| {
        root_or_direct_child_xml_attribute_value(
            pic_xml,
            "pic",
            &["sz", "imgRect", "size", "extent"],
            attribute_name,
        )
    })?;
    parse_trimmed::<u32>(value)
        .and_then(hwp_units_to_px_option)
        .or_else(|| {
            context.add_warning_once(&format!(
                "HWPX picture referenced unsupported {label} value `{}`; hwp-convert omitted that picture dimension.",
                value.trim()
            ));
            None
        })
}

fn unknown_hwpx_object_block(
    object_kind: &str,
    object_xml: &str,
    context: &mut HwpxFallbackContext,
) -> UnknownBlock {
    let object_text = first_non_empty_string([
        non_empty_string_owned(inlines_to_plain_text(&extract_inlines_from_xml_fragment(
            object_xml, context,
        ))),
        decoded_root_xml_attribute_value_any(object_xml, HWPX_FALLBACK_TEXT_ATTRIBUTES),
    ])
    .unwrap_or_default();
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

fn unknown_hwpx_control_block(
    control_xml: &str,
    context: &mut HwpxFallbackContext,
) -> UnknownBlock {
    let control_kind =
        first_hwpx_control_child_name(control_xml).unwrap_or_else(|| "unknown".to_string());
    let control_text = first_non_empty_string([
        non_empty_string_owned(inlines_to_plain_text(&extract_inlines_from_xml_fragment(
            control_xml,
            context,
        ))),
        first_hwpx_control_child_attribute_value(control_xml, HWPX_FALLBACK_TEXT_ATTRIBUTES),
    ])
    .unwrap_or_default();
    let fallback_text = if control_text.is_empty() {
        format!("[control: {control_kind}]")
    } else {
        format!("[control: {control_kind}]\n{control_text}")
    };

    UnknownBlock {
        kind: format!("hwpx:control:{control_kind}"),
        fallback_text: Some(fallback_text),
        message: Some(
            "HWPX section XML fallback preserved an unsupported control placeholder.".to_string(),
        ),
        source: Some("Contents/section*.xml".to_string()),
    }
}

fn first_hwpx_control_child_name(control_xml: &str) -> Option<String> {
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(control_xml, cursor) {
        if !tag.is_closing && tag.name != "ctrl" {
            return Some(tag.name.to_string());
        }
        cursor = tag.end;
    }

    None
}

fn first_hwpx_control_child_attribute_value(
    control_xml: &str,
    attribute_names: &[&str],
) -> Option<String> {
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(control_xml, cursor) {
        if !tag.is_closing && tag.name != "ctrl" {
            return decoded_xml_attribute_value_any(tag.raw, attribute_names);
        }
        cursor = tag.end;
    }

    None
}

fn push_paragraph_text_fragment_as_block(
    blocks: &mut Vec<Block>,
    xml: &str,
    list: Option<ListInfo>,
    role: ParagraphRole,
    style: ParagraphStyle,
    context: &mut HwpxFallbackContext,
) -> bool {
    let inlines = extract_inlines_from_xml_fragment(xml, context);
    if inlines.is_empty() {
        return false;
    }

    blocks.push(Block::Paragraph(Paragraph {
        role,
        inlines,
        style,
        style_ref: None,
        list,
    }));
    true
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
            "" if text_depth > 0 && is_xml_cdata_tag(tag.raw) => {
                text_buffer.push_str(xml_cdata_text(tag.raw));
            }
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
            "hyperlink" | "a" | "link"
                if !tag.is_closing && hwpx_direct_link_url(tag.raw).is_some() =>
            {
                push_text_buffer_to_hwpx_inline_target(
                    &mut inlines,
                    &mut active_field,
                    &mut text_buffer,
                    &current_style,
                );
                let link_end = if tag.is_self_closing {
                    tag.end
                } else {
                    find_matching_element_end(xml, &tag).unwrap_or(tag.end)
                };
                if let Some(link) = extract_hwpx_direct_link(
                    tag.raw,
                    xml_element_inner_xml(xml, &tag, link_end),
                    context,
                ) {
                    push_hwpx_inline(&mut inlines, &mut active_field, Inline::Link(link));
                }
                cursor = link_end;
                continue;
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
                    let begin_id = decoded_xml_attribute_value_any(
                        tag.raw,
                        HWPX_FIELD_BEGIN_ID_REF_ATTRIBUTES,
                    );
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

fn extract_hwpx_direct_link(
    tag: &str,
    inner_xml: &str,
    context: &mut HwpxFallbackContext,
) -> Option<Link> {
    let url = hwpx_direct_link_url(tag)?;
    let title = decoded_xml_attribute_value_any(tag, HWPX_LINK_TITLE_ATTRIBUTES)
        .filter(|value| value != &url);
    let inlines = extract_inlines_from_xml_fragment(inner_xml, context);
    let label = first_non_empty_string([
        non_empty_string_owned(inlines_to_plain_text(&inlines)),
        title.clone(),
        Some(url.clone()),
    ])
    .unwrap_or_else(|| url.clone());
    let inlines = if inlines.is_empty() {
        vec![Inline::Text(TextRun {
            text: label,
            style: TextStyle::default(),
            style_ref: None,
        })]
    } else {
        inlines
    };

    Some(Link {
        url,
        title,
        inlines,
    })
}

fn hwpx_direct_link_url(tag: &str) -> Option<String> {
    decoded_xml_attribute_value_any(tag, HWPX_LINK_URL_ATTRIBUTES)
}

fn xml_element_inner_xml<'a>(xml: &'a str, start_tag: &XmlTag<'_>, element_end: usize) -> &'a str {
    if start_tag.is_self_closing || start_tag.end >= element_end {
        return "";
    }

    let inner = &xml[start_tag.end..element_end];
    let inner_end = inner
        .rfind("</")
        .map(|relative_close_start| start_tag.end + relative_close_start)
        .unwrap_or(element_end);

    &xml[start_tag.end..inner_end]
}

fn extract_hwpx_field_begin(tag: &str, field_xml: &str) -> HwpxActiveField {
    let field_type = decoded_xml_attribute_value_any(tag, HWPX_FIELD_TYPE_ATTRIBUTES)
        .unwrap_or_else(|| "UNKNOWN".to_string());
    let name = decoded_xml_attribute_value_any(tag, HWPX_FIELD_NAME_ATTRIBUTES);
    let command = first_non_empty_string([
        decoded_xml_attribute_value_any(tag, HWPX_FIELD_COMMAND_ATTRIBUTES),
        hwpx_field_parameter_value(
            field_xml,
            &["command", "Command", "cmd", "hyperlink", "Hyperlink"],
        ),
    ]);
    let url = first_non_empty_string([
        decoded_xml_attribute_value_any(tag, HWPX_LINK_URL_ATTRIBUTES),
        command.clone().filter(|value| is_hwpx_url_like(value)),
        hwpx_field_parameter_value(
            field_xml,
            &[
                "url",
                "URL",
                "href",
                "HRef",
                "target",
                "Target",
                "address",
                "Address",
                "webAddress",
                "WebAddress",
            ],
        ),
        name.clone().filter(|value| is_hwpx_url_like(value)),
    ]);

    HwpxActiveField {
        id: decoded_xml_attribute_value_any(tag, HWPX_FIELD_ID_ATTRIBUTES),
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
    let name = decoded_xml_attribute_value_any(tag, HWPX_BOOKMARK_NAME_ATTRIBUTES)?;
    Some(Inline::Anchor {
        id: crate::util::plain_text::sanitize_anchor_id(&name),
    })
}

fn unknown_hwpx_field_end_inline(tag: &str) -> Inline {
    let fallback_text = decoded_xml_attribute_value_any(tag, HWPX_FIELD_BEGIN_ID_REF_ATTRIBUTES)
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

        let Some(parameter_name) =
            xml_attribute_value_any(tag.raw, HWPX_FIELD_PARAMETER_NAME_ATTRIBUTES)
        else {
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

        if let Some(value) =
            decoded_xml_attribute_value_any(tag.raw, HWPX_FIELD_PARAMETER_VALUE_ATTRIBUTES)
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
    xml_fragment_plain_text(text)
}

fn xml_fragment_plain_text(xml: &str) -> Option<String> {
    if xml_fragment_contains_text_node(xml) {
        hwpx_text_node_plain_text(xml)
    } else {
        direct_xml_text(xml)
    }
}

fn xml_fragment_contains_text_node(xml: &str) -> bool {
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(xml, cursor) {
        if tag.name == "t" && !tag.is_closing {
            return true;
        }
        cursor = tag.end;
    }

    false
}

fn hwpx_text_node_plain_text(xml: &str) -> Option<String> {
    let mut text = String::new();
    let mut cursor = 0usize;
    let mut text_depth = 0usize;

    while let Some(tag) = next_xml_tag(xml, cursor) {
        if text_depth > 0 && tag.start > cursor {
            text.push_str(&decode_xml_text(&xml[cursor..tag.start]));
        }

        match tag.name {
            "" if text_depth > 0 && is_xml_cdata_tag(tag.raw) => {
                text.push_str(xml_cdata_text(tag.raw));
            }
            "t" if tag.is_closing => text_depth = text_depth.saturating_sub(1),
            "t" if !tag.is_closing && !tag.is_self_closing => text_depth += 1,
            "lineBreak" if !tag.is_closing => text.push('\n'),
            "tab" if !tag.is_closing => text.push('\t'),
            _ => {}
        }

        cursor = tag.end;
    }

    if text_depth > 0 && cursor < xml.len() {
        text.push_str(&decode_xml_text(&xml[cursor..]));
    }

    non_empty_string_owned(text)
}

fn direct_xml_text(xml: &str) -> Option<String> {
    let mut text = String::new();
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(xml, cursor) {
        if tag.start > cursor {
            push_non_empty_xml_text_segment(&mut text, &xml[cursor..tag.start]);
        }
        match tag.name {
            "" if is_xml_cdata_tag(tag.raw) => text.push_str(xml_cdata_text(tag.raw)),
            "lineBreak" if !tag.is_closing => text.push('\n'),
            "tab" if !tag.is_closing => text.push('\t'),
            _ => {}
        }
        cursor = tag.end;
    }

    if cursor < xml.len() {
        push_non_empty_xml_text_segment(&mut text, &xml[cursor..]);
    }

    non_empty_string_owned(text)
}

fn push_non_empty_xml_text_segment(output: &mut String, segment: &str) {
    let decoded = decode_xml_text(segment);
    if decoded.trim().is_empty() {
        return;
    }
    output.push_str(&decoded);
}

fn first_hwpx_child_element_text(xml: &str, names: &[&str]) -> Option<String> {
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(xml, cursor) {
        if tag.is_closing || !names.contains(&tag.name) {
            cursor = tag.end;
            continue;
        }

        if tag.is_self_closing {
            cursor = tag.end;
            continue;
        }

        let Some(element_end) = find_matching_element_end(xml, &tag) else {
            cursor = tag.end;
            continue;
        };

        if let Some(text) = simple_xml_element_text(xml_element_inner_xml(xml, &tag, element_end)) {
            return Some(text);
        }

        cursor = tag.end;
    }

    None
}

fn first_hwpx_direct_child_element_text(xml: &str, names: &[&str]) -> Option<String> {
    let root = next_xml_tag(xml, 0)?;
    if root.is_closing || root.is_self_closing {
        return None;
    }
    let root_end = find_matching_element_end(xml, &root)?;
    let mut cursor = root.end;

    while let Some(tag) = next_xml_tag(xml, cursor) {
        if tag.start >= root_end {
            break;
        }
        if tag.is_closing {
            cursor = tag.end;
            continue;
        }
        let tag_end = if tag.is_self_closing {
            tag.end
        } else {
            find_matching_element_end(xml, &tag).unwrap_or(tag.end)
        };
        if names.contains(&tag.name)
            && let Some(text) = simple_xml_element_text(xml_element_inner_xml(xml, &tag, tag_end))
        {
            return Some(text);
        }
        cursor = tag_end;
    }

    None
}

fn decoded_xml_attribute_value(tag: &str, attribute_name: &str) -> Option<String> {
    xml_attribute_value(tag, attribute_name)
        .map(decode_xml_text)
        .and_then(non_empty_string_owned)
}

fn decoded_xml_attribute_value_any(tag: &str, attribute_names: &[&str]) -> Option<String> {
    attribute_names
        .iter()
        .find_map(|attribute_name| decoded_xml_attribute_value(tag, attribute_name))
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

fn parse_trimmed<T: std::str::FromStr>(value: &str) -> Option<T> {
    value.trim().parse().ok()
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

fn root_xml_attribute_u32(xml: &str, tag_name: &str, attribute_name: &str) -> Option<u32> {
    let tag = next_xml_tag(xml, 0)?;
    if tag.name == tag_name && !tag.is_closing {
        xml_attribute_value(tag.raw, attribute_name).and_then(parse_trimmed)
    } else {
        None
    }
}

fn root_xml_attribute_u32_any(xml: &str, tag_name: &str, attribute_names: &[&str]) -> Option<u32> {
    attribute_names
        .iter()
        .find_map(|attribute_name| root_xml_attribute_u32(xml, tag_name, attribute_name))
}

fn root_xml_attribute_value<'a>(xml: &'a str, attribute_name: &str) -> Option<&'a str> {
    let tag = next_xml_tag(xml, 0)?;
    if tag.is_closing {
        return None;
    }
    xml_attribute_value(tag.raw, attribute_name)
}

fn root_xml_attribute_value_any<'a>(xml: &'a str, attribute_names: &[&str]) -> Option<&'a str> {
    attribute_names
        .iter()
        .find_map(|attribute_name| root_xml_attribute_value(xml, attribute_name))
}

fn decoded_root_xml_attribute_value(xml: &str, attribute_name: &str) -> Option<String> {
    root_xml_attribute_value(xml, attribute_name)
        .map(decode_xml_text)
        .and_then(non_empty_string_owned)
}

fn decoded_root_xml_attribute_value_any(xml: &str, attribute_names: &[&str]) -> Option<String> {
    attribute_names
        .iter()
        .find_map(|attribute_name| decoded_root_xml_attribute_value(xml, attribute_name))
}

fn root_or_direct_child_xml_attribute_u32(
    xml: &str,
    root_name: &str,
    child_names: &[&str],
    attribute_name: &str,
) -> Option<u32> {
    root_or_direct_child_xml_attribute_value(xml, root_name, child_names, attribute_name)
        .and_then(parse_trimmed)
}

fn root_or_direct_child_xml_attribute_u32_any(
    xml: &str,
    root_name: &str,
    child_names: &[&str],
    attribute_names: &[&str],
) -> Option<u32> {
    attribute_names.iter().find_map(|attribute_name| {
        root_or_direct_child_xml_attribute_u32(xml, root_name, child_names, attribute_name)
    })
}

fn root_or_direct_child_xml_attribute_value_any<'a>(
    xml: &'a str,
    root_name: &str,
    child_names: &[&str],
    attribute_names: &[&str],
) -> Option<&'a str> {
    attribute_names.iter().find_map(|attribute_name| {
        root_or_direct_child_xml_attribute_value(xml, root_name, child_names, attribute_name)
    })
}

fn root_or_direct_child_xml_attribute_value<'a>(
    xml: &'a str,
    root_name: &str,
    child_names: &[&str],
    attribute_name: &str,
) -> Option<&'a str> {
    let root = next_xml_tag(xml, 0)?;
    if root.name != root_name || root.is_closing {
        return None;
    }
    if let Some(value) = xml_attribute_value(root.raw, attribute_name) {
        return Some(value);
    }
    if root.is_self_closing {
        return None;
    }

    let root_end = find_matching_element_end(xml, &root)?;
    let mut cursor = root.end;
    while let Some(tag) = next_xml_tag(xml, cursor) {
        if tag.start >= root_end {
            break;
        }
        if tag.is_closing {
            cursor = tag.end;
            continue;
        }
        if child_names.contains(&tag.name)
            && let Some(value) = xml_attribute_value(tag.raw, attribute_name)
        {
            return Some(value);
        }
        cursor = if tag.is_self_closing {
            tag.end
        } else {
            find_matching_element_end(xml, &tag).unwrap_or(tag.end)
        };
    }

    None
}

fn first_decoded_xml_attribute_value_any(xml: &str, attribute_names: &[&str]) -> Option<String> {
    let mut cursor = 0usize;

    while let Some(tag) = next_xml_tag(xml, cursor) {
        if !tag.is_closing
            && let Some(value) = decoded_xml_attribute_value_any(tag.raw, attribute_names)
        {
            return Some(value);
        }
        cursor = tag.end;
    }

    None
}

fn map_hwpx_alignment_with_warning(value: &str, warnings: &mut Vec<String>) -> Option<Alignment> {
    let normalized = value.trim().to_ascii_uppercase();
    Some(match normalized.as_str() {
        "LEFT" => Alignment::Left,
        "CENTER" => Alignment::Center,
        "RIGHT" => Alignment::Right,
        "JUSTIFY" | "DISTRIBUTE" | "DISTRIBUTE_SPACE" => Alignment::Justify,
        _ => {
            warnings.push(format!(
                "HWPX paragraph align referenced unsupported horizontal alignment `{}`; hwp-convert used the default paragraph alignment.",
                value.trim()
            ));
            return None;
        }
    })
}

fn map_hwpx_vertical_align_with_warning(
    value: &str,
    context: &mut HwpxFallbackContext,
) -> Option<VerticalAlign> {
    Some(match value.trim().to_ascii_uppercase().as_str() {
        "TOP" => VerticalAlign::Top,
        "CENTER" | "MIDDLE" => VerticalAlign::Middle,
        "BOTTOM" => VerticalAlign::Bottom,
        _ => {
            context.add_warning_once(&format!(
                "HWPX table cell referenced unsupported vertical alignment `{}`; hwp-convert used the default table cell vertical alignment.",
                value.trim()
            ));
            return None;
        }
    })
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
    let trimmed = value.trim();
    let hex = trimmed
        .strip_prefix('#')
        .or_else(|| trimmed.strip_prefix("0x"))
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
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
    let end = xml_tag_end_exclusive(xml, start)?;
    let raw = xml.get(start + 1..end - 1)?;
    let Some(name) = xml_tag_local_name(raw) else {
        return Some(XmlTag {
            start,
            end,
            raw,
            name: "",
            is_closing: false,
            is_self_closing: true,
        });
    };

    Some(XmlTag {
        start,
        end,
        raw,
        name,
        is_closing: is_xml_closing_tag(raw),
        is_self_closing: is_xml_self_closing_tag(raw),
    })
}

fn xml_tag_end_exclusive(xml: &str, start: usize) -> Option<usize> {
    let rest = xml.get(start..)?;
    if rest.starts_with("<![CDATA[") {
        return rest.find("]]>").map(|relative| start + relative + 3);
    }
    if rest.starts_with("<!--") {
        return rest.find("-->").map(|relative| start + relative + 3);
    }
    if rest.starts_with("<?") {
        return rest.find("?>").map(|relative| start + relative + 2);
    }
    if rest.starts_with("<!") {
        return xml_markup_declaration_end_exclusive(xml, start);
    }
    xml_normal_tag_end_exclusive(xml, start)
}

fn xml_normal_tag_end_exclusive(xml: &str, start: usize) -> Option<usize> {
    let rest = xml.get(start..)?;
    let mut quote = None;

    for (relative, ch) in rest.char_indices() {
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => quote = Some(ch),
            '>' => return Some(start + relative + ch.len_utf8()),
            _ => {}
        }
    }

    None
}

fn xml_markup_declaration_end_exclusive(xml: &str, start: usize) -> Option<usize> {
    let rest = xml.get(start + 2..)?;
    let mut bracket_depth = 0usize;
    let mut quote = None;

    for (relative, ch) in rest.char_indices() {
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => quote = Some(ch),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '>' if bracket_depth == 0 => return Some(start + 2 + relative + ch.len_utf8()),
            _ => {}
        }
    }

    None
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

fn xml_attribute_value_any<'a>(tag: &'a str, attribute_names: &[&str]) -> Option<&'a str> {
    attribute_names
        .iter()
        .find_map(|attribute_name| xml_attribute_value(tag, attribute_name))
}

fn is_xml_attribute_boundary(tag: &str, attr_start: usize, attr_end: usize) -> bool {
    let before_ok = attr_start == 0
        || tag
            .as_bytes()
            .get(attr_start.saturating_sub(1))
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b':');
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

    let preview_text =
        read_optional_zip_text_entry_case_insensitive(&mut archive, PREVIEW_TEXT_PATH)?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("{PREVIEW_TEXT_PATH} entry was not found"),
                )
            })?;
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

fn find_archive_entry_case_insensitive<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
) -> io::Result<Option<String>> {
    let Some(target) = normalize_hwpx_archive_path(path) else {
        return Ok(None);
    };

    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("HWPX archive entry could not be read: {error}"),
            )
        })?;
        let Some(entry_path) = normalize_hwpx_archive_path(entry.name()) else {
            continue;
        };
        if entry_path.eq_ignore_ascii_case(&target) {
            return Ok(Some(entry.name().to_string()));
        }
    }

    Ok(None)
}

fn is_section_xml_path(path: &str) -> bool {
    section_xml_index(path).is_some()
}

fn section_xml_index(path: &str) -> Option<u32> {
    let normalized = path.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    let file_name = lower.strip_prefix("contents/section")?;

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

        let Some(tag_end) = xml_tag_end_exclusive(xml, tag_start) else {
            break;
        };
        let tag = &xml[tag_start + 1..tag_end - 1];
        let tag_name = xml_tag_local_name(tag);
        let is_closing = is_xml_closing_tag(tag);
        let is_self_closing = is_xml_self_closing_tag(tag);

        match tag_name {
            None if paragraph_depth > 0 && text_depth > 0 && is_xml_cdata_tag(tag) => {
                current.push_str(xml_cdata_text(tag));
            }
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

        cursor = tag_end;
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

fn is_xml_cdata_tag(tag: &str) -> bool {
    tag.trim_start().starts_with("![CDATA[")
}

fn xml_cdata_text(tag: &str) -> &str {
    let trimmed = tag.trim();
    trimmed
        .strip_prefix("![CDATA[")
        .and_then(|value| value.strip_suffix("]]"))
        .unwrap_or("")
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
    fn skips_xml_declaration_and_comments_in_section_xml_blocks() {
        let xml = r#"
            <?xml version="1.0" encoding="UTF-8"?>
            <!DOCTYPE hs:sec [
              <!ENTITY sample "a > b">
            ]>
            <!-- section comment with > marker -->
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p>
                <?ignore value="a > b"?>
                <!-- paragraph comment with > marker -->
                <hp:run title="a > b"><hp:t>Hello</hp:t></hp:run>
              </hp:p>
            </hs:sec>
        "#;

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);

        assert_eq!(blocks.len(), 1);
        assert!(matches!(
            &blocks[0],
            Block::Paragraph(paragraph)
                if inlines_to_plain_text(&paragraph.inlines) == "Hello"
        ));
    }

    #[test]
    fn preserves_cdata_text_in_hwpx_text_fallbacks() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p>
                <hp:run><hp:t><![CDATA[<raw & text>]]></hp:t></hp:run>
              </hp:p>
            </hs:sec>
        "#;

        assert_eq!(
            extract_section_xml_paragraphs(xml),
            vec!["<raw & text>".to_string()]
        );

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);
        assert!(matches!(
            &blocks[0],
            Block::Paragraph(paragraph)
                if inlines_to_plain_text(&paragraph.inlines) == "<raw & text>"
        ));
    }

    #[test]
    fn preserves_empty_hwpx_paragraphs_in_section_fallback() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p><hp:run><hp:t>before</hp:t></hp:run></hp:p>
              <hp:p/>
              <hp:p>
                <hp:run><hp:t>after</hp:t></hp:run>
              </hp:p>
            </hs:sec>
        "#;

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);

        assert_eq!(blocks.len(), 3);
        assert!(matches!(
            &blocks[0],
            Block::Paragraph(paragraph)
                if inlines_to_plain_text(&paragraph.inlines) == "before"
        ));
        assert!(matches!(
            &blocks[1],
            Block::Paragraph(paragraph) if paragraph.inlines.is_empty()
        ));
        assert!(matches!(
            &blocks[2],
            Block::Paragraph(paragraph)
                if inlines_to_plain_text(&paragraph.inlines) == "after"
        ));
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
                    <hp:tr h="1500">
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
                assert_eq!(table.rows[0].height, Some(LengthPx(20.0)));
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
    fn extracts_table_cell_span_from_tc_attributes() {
        let xml = r#"
            <hp:tbl xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:tr>
                <hp:tc row-span=" 3 " col-span=" 2 ">
                  <hp:subList>
                    <hp:p><hp:run><hp:t>merged cell</hp:t></hp:run></hp:p>
                  </hp:subList>
                </hp:tc>
              </hp:tr>
            </hp:tbl>
        "#;

        let mut context = HwpxFallbackContext::default();
        let table = extract_table_from_xml(xml, &mut context).expect("table should be parsed");

        assert_eq!(table.rows[0].cells[0].row_span, 3);
        assert_eq!(table.rows[0].cells[0].col_span, 2);
    }

    #[test]
    fn extracts_table_cell_size_and_padding_from_tc() {
        let xml = r#"
            <hp:tbl xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:tr>
                <hp:tc>
                  <hp:cellPr isHeader="true"/>
                  <hp:cellSz w="7500" h="1500"/>
                  <hp:cellMargin l="150" r="150" t="75" b="75"/>
                  <hp:subList verticalAlign="CENTER">
                    <hp:p><hp:run><hp:t>cell</hp:t></hp:run></hp:p>
                  </hp:subList>
                </hp:tc>
              </hp:tr>
            </hp:tbl>
        "#;

        let mut context = HwpxFallbackContext::default();
        let table = extract_table_from_xml(xml, &mut context).expect("table should be parsed");
        let cell = &table.rows[0].cells[0];

        assert!(cell.is_header);
        assert_eq!(cell.style.width, Some(LengthPx(100.0)));
        assert_eq!(cell.style.height, Some(LengthPx(20.0)));
        assert_eq!(cell.style.padding_left, Some(LengthPx(2.0)));
        assert_eq!(cell.style.padding_top, Some(LengthPx(1.0)));
        assert_eq!(cell.style.vertical_align, Some(VerticalAlign::Middle));
    }

    #[test]
    fn warns_when_hwpx_table_cell_vertical_align_is_unsupported() {
        let xml = r#"
            <hp:tbl xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:tr>
                <hp:tc>
                  <hp:subList verticalAlign="BASELINE">
                    <hp:p><hp:run><hp:t>cell</hp:t></hp:run></hp:p>
                  </hp:subList>
                </hp:tc>
              </hp:tr>
            </hp:tbl>
        "#;

        let mut context = HwpxFallbackContext::default();
        let table = extract_table_from_xml(xml, &mut context).expect("table should be parsed");

        assert_eq!(table.rows[0].cells[0].style.vertical_align, None);
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported vertical alignment `BASELINE`")
        }));
    }

    #[test]
    fn warns_when_hwpx_table_cell_metrics_are_invalid() {
        let xml = r#"
            <hp:tbl xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:tr>
                <hp:tc hasMargin="1">
                  <hp:cellSz w="bad-width" h="bad-height"/>
                  <hp:cellMargin l="bad-left" t="bad-top"/>
                  <hp:subList>
                    <hp:p><hp:run><hp:t>cell</hp:t></hp:run></hp:p>
                  </hp:subList>
                </hp:tc>
              </hp:tr>
            </hp:tbl>
        "#;

        let mut context = HwpxFallbackContext::default();
        let table = extract_table_from_xml(xml, &mut context).expect("table should be parsed");
        let cell_style = &table.rows[0].cells[0].style;

        assert_eq!(cell_style.width, None);
        assert_eq!(cell_style.height, None);
        assert_eq!(cell_style.padding_left, None);
        assert_eq!(cell_style.padding_top, None);
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported width value `bad-width`")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported height value `bad-height`")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported left padding value `bad-left`")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported top padding value `bad-top`")
        }));
    }

    #[test]
    fn warns_when_hwpx_table_padding_metrics_are_invalid() {
        let xml = r#"
            <hp:tbl xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:inMargin l="bad-left" t="bad-top"/>
              <hp:tr>
                <hp:tc hasMargin="0">
                  <hp:subList><hp:p><hp:run><hp:t>cell</hp:t></hp:run></hp:p></hp:subList>
                </hp:tc>
              </hp:tr>
            </hp:tbl>
        "#;

        let mut context = HwpxFallbackContext::default();
        let table = extract_table_from_xml(xml, &mut context).expect("table should be parsed");
        let cell_style = &table.rows[0].cells[0].style;

        assert_eq!(cell_style.padding_left, None);
        assert_eq!(cell_style.padding_top, None);
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported left padding value `bad-left`")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported top padding value `bad-top`")
        }));
    }

    #[test]
    fn applies_hwpx_table_padding_unless_cell_margin_is_enabled() {
        let xml = r#"
            <hp:tbl xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:inMargin l="300" r="300" t="225" b="225"/>
              <hp:tr>
                <hp:tc hasMargin="0">
                  <hp:cellMargin l="900" r="900" t="900" b="900"/>
                  <hp:subList><hp:p><hp:run><hp:t>default</hp:t></hp:run></hp:p></hp:subList>
                </hp:tc>
                <hp:tc hasMargin="1">
                  <hp:cellMargin l="150" r="150" t="75" b="75"/>
                  <hp:subList><hp:p><hp:run><hp:t>custom</hp:t></hp:run></hp:p></hp:subList>
                </hp:tc>
              </hp:tr>
            </hp:tbl>
        "#;

        let mut context = HwpxFallbackContext::default();
        let table = extract_table_from_xml(xml, &mut context).expect("table should be parsed");
        let inherited = &table.rows[0].cells[0].style;
        let custom = &table.rows[0].cells[1].style;

        assert_eq!(inherited.padding_left, Some(LengthPx(4.0)));
        assert_eq!(inherited.padding_top, Some(LengthPx(3.0)));
        assert_eq!(custom.padding_left, Some(LengthPx(2.0)));
        assert_eq!(custom.padding_top, Some(LengthPx(1.0)));
    }

    #[test]
    fn preserves_hwpx_table_cell_field_name_as_unknown_block() {
        let xml = r#"
            <hp:tbl xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:tr>
                <hp:tc>
                  <hp:cellPr field-name="amount"/>
                  <hp:subList>
                    <hp:p><hp:run><hp:t>1000</hp:t></hp:run></hp:p>
                  </hp:subList>
                </hp:tc>
              </hp:tr>
            </hp:tbl>
        "#;

        let mut context = HwpxFallbackContext::default();
        let table = extract_table_from_xml(xml, &mut context).expect("table should be parsed");
        let cell = &table.rows[0].cells[0];

        assert!(matches!(
            &cell.blocks[0],
            Block::Unknown(unknown)
                if unknown.kind == "table_cell_field"
                    && unknown.fallback_text.as_deref() == Some("[cell field: amount]")
                    && unknown.source.as_deref() == Some("hwpx")
        ));
        assert!(
            context
                .warnings
                .iter()
                .any(|warning| warning.message.contains("table cell field names"))
        );
    }

    #[test]
    fn orders_hwpx_table_cells_by_col_addr_when_present() {
        let xml = r#"
            <hp:tbl xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:tr>
                <hp:tc>
                  <hp:cellAddr colIndex="1"/>
                  <hp:subList>
                    <hp:p><hp:run><hp:t>second cell</hp:t></hp:run></hp:p>
                  </hp:subList>
                </hp:tc>
                <hp:tc>
                  <hp:cellAddr colIndex="0"/>
                  <hp:subList>
                    <hp:p><hp:run><hp:t>first cell</hp:t></hp:run></hp:p>
                  </hp:subList>
                </hp:tc>
              </hp:tr>
            </hp:tbl>
        "#;

        let mut context = HwpxFallbackContext::default();
        let table = extract_table_from_xml(xml, &mut context).expect("table should be parsed");

        assert_eq!(
            blocks_first_paragraph_text(&table.rows[0].cells[0].blocks),
            Some("first cell".to_string())
        );
        assert_eq!(
            blocks_first_paragraph_text(&table.rows[0].cells[1].blocks),
            Some("second cell".to_string())
        );
    }

    #[test]
    fn orders_hwpx_table_rows_by_row_addr_when_present() {
        let xml = r#"
            <hp:tbl xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:tr>
                <hp:tc>
                  <hp:cellAddr rowIndex="1"/>
                  <hp:subList>
                    <hp:p><hp:run><hp:t>second row</hp:t></hp:run></hp:p>
                  </hp:subList>
                </hp:tc>
              </hp:tr>
              <hp:tr>
                <hp:tc>
                  <hp:cellAddr rowIndex="0"/>
                  <hp:subList>
                    <hp:p><hp:run><hp:t>first row</hp:t></hp:run></hp:p>
                  </hp:subList>
                </hp:tc>
              </hp:tr>
            </hp:tbl>
        "#;

        let mut context = HwpxFallbackContext::default();
        let table = extract_table_from_xml(xml, &mut context).expect("table should be parsed");

        assert_eq!(
            blocks_first_paragraph_text(&table.rows[0].cells[0].blocks),
            Some("first row".to_string())
        );
        assert_eq!(
            blocks_first_paragraph_text(&table.rows[1].cells[0].blocks),
            Some("second row".to_string())
        );
    }

    #[test]
    fn normalizes_zero_hwpx_table_cell_span_to_one() {
        let xml = r#"
            <hp:tbl xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:tr>
                <hp:tc rowSpan="0" colSpan="0">
                  <hp:subList>
                    <hp:p><hp:run><hp:t>cell</hp:t></hp:run></hp:p>
                  </hp:subList>
                </hp:tc>
              </hp:tr>
            </hp:tbl>
        "#;

        let mut context = HwpxFallbackContext::default();
        let table = extract_table_from_xml(xml, &mut context).expect("table should be parsed");

        assert_eq!(table.rows[0].cells[0].row_span, 1);
        assert_eq!(table.rows[0].cells[0].col_span, 1);
    }

    #[test]
    fn extracts_table_and_cell_background_from_property_tags() {
        let xml = r#"
            <hp:tbl xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:tblPr borderFillIDRef="1"/>
              <hp:tr>
                <hp:tc>
                  <hp:cellPr borderFillIDRef="2"/>
                  <hp:subList>
                    <hp:p><hp:run><hp:t>cell</hp:t></hp:run></hp:p>
                  </hp:subList>
                </hp:tc>
              </hp:tr>
            </hp:tbl>
        "#;
        let table_color = Color {
            r: 0x11,
            g: 0x22,
            b: 0x33,
            a: 255,
        };
        let cell_color = Color {
            r: 0x44,
            g: 0x55,
            b: 0x66,
            a: 255,
        };
        let mut context = HwpxFallbackContext {
            border_fills: vec![
                HwpxBorderFill::default(),
                HwpxBorderFill {
                    background_color: Some(table_color),
                    ..Default::default()
                },
                HwpxBorderFill {
                    background_color: Some(cell_color),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let table = extract_table_from_xml(xml, &mut context).expect("table should be parsed");

        assert_eq!(table.style.background_color, Some(table_color));
        assert_eq!(
            table.rows[0].cells[0].style.background_color,
            Some(cell_color)
        );
    }

    #[test]
    fn warns_when_hwpx_table_references_missing_border_fill() {
        let xml = r#"
            <hp:tbl xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:tr>
                <hp:tc>
                  <hp:cellPr borderFillIDRef="9"/>
                  <hp:subList>
                    <hp:p><hp:run><hp:t>cell</hp:t></hp:run></hp:p>
                  </hp:subList>
                </hp:tc>
              </hp:tr>
            </hp:tbl>
        "#;

        let mut context = HwpxFallbackContext::default();
        let table = extract_table_from_xml(xml, &mut context).expect("table should be parsed");

        assert_eq!(table.rows[0].cells[0].style.background_color, None);
        assert!(
            context
                .warnings
                .iter()
                .any(|warning| warning.message.contains("missing borderFill id 9"))
        );
    }

    #[test]
    fn does_not_leak_nested_hwpx_table_properties_to_outer_table() {
        let nested_color = Color {
            r: 0x44,
            g: 0x55,
            b: 0x66,
            a: 255,
        };
        let mut context = HwpxFallbackContext {
            border_fills: vec![
                HwpxBorderFill::default(),
                HwpxBorderFill {
                    background_color: Some(nested_color),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let xml = r#"
            <hp:tbl xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:tr>
                <hp:tc>
                  <hp:subList>
                    <hp:tbl borderFillIDRef="1">
                      <hp:tr>
                        <hp:tc rowSpan="2" colSpan="3" borderFillIDRef="1">
                          <hp:subList>
                            <hp:p><hp:run><hp:t>nested</hp:t></hp:run></hp:p>
                          </hp:subList>
                        </hp:tc>
                      </hp:tr>
                    </hp:tbl>
                  </hp:subList>
                </hp:tc>
              </hp:tr>
            </hp:tbl>
        "#;

        let table = extract_table_from_xml(xml, &mut context).expect("table should be parsed");

        assert_eq!(table.style.background_color, None);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].cells.len(), 1);
        let outer_cell = &table.rows[0].cells[0];
        assert_eq!(outer_cell.row_span, 1);
        assert_eq!(outer_cell.col_span, 1);
        assert_eq!(outer_cell.style.background_color, None);

        let nested_table = match &outer_cell.blocks[0] {
            Block::Table(table) => table,
            other => panic!("expected nested table block, got {other:?}"),
        };
        assert_eq!(nested_table.style.background_color, Some(nested_color));
        let nested_cell = &nested_table.rows[0].cells[0];
        assert_eq!(nested_cell.row_span, 2);
        assert_eq!(nested_cell.col_span, 3);
        assert_eq!(nested_cell.style.background_color, Some(nested_color));
    }

    #[test]
    fn preserves_hwpx_table_caption_as_adjacent_caption_block() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:tbl>
                <hp:caption placement="TOP">
                  <hp:subList>
                    <hp:p><hp:run><hp:t>table caption</hp:t></hp:run></hp:p>
                  </hp:subList>
                </hp:caption>
                <hp:tr>
                  <hp:tc>
                    <hp:subList>
                      <hp:p><hp:run><hp:t>cell</hp:t></hp:run></hp:p>
                    </hp:subList>
                  </hp:tc>
                </hp:tr>
              </hp:tbl>
            </hs:sec>
        "#;

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);

        assert_eq!(blocks.len(), 2);
        match &blocks[0] {
            Block::Paragraph(paragraph) => {
                assert_eq!(paragraph.role, ParagraphRole::Caption);
                assert_eq!(inlines_to_plain_text(&paragraph.inlines), "table caption");
            }
            other => panic!("expected caption paragraph block, got {other:?}"),
        }
        assert!(matches!(&blocks[1], Block::Table(_)));
    }

    #[test]
    fn does_not_leak_nested_hwpx_table_caption_to_outer_table() {
        let xml = r#"
            <hp:tbl xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:tr>
                <hp:tc>
                  <hp:subList>
                    <hp:tbl>
                      <hp:caption side="TOP">
                        <hp:subList>
                          <hp:p><hp:run><hp:t>nested caption</hp:t></hp:run></hp:p>
                        </hp:subList>
                      </hp:caption>
                      <hp:tr>
                        <hp:tc>
                          <hp:subList>
                            <hp:p><hp:run><hp:t>nested cell</hp:t></hp:run></hp:p>
                          </hp:subList>
                        </hp:tc>
                      </hp:tr>
                    </hp:tbl>
                  </hp:subList>
                </hp:tc>
              </hp:tr>
            </hp:tbl>
        "#;

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_table_blocks_from_xml(xml, &mut context);

        assert_eq!(blocks.len(), 1);
        let outer_table = match &blocks[0] {
            Block::Table(table) => table,
            other => panic!("expected outer table block, got {other:?}"),
        };
        assert!(matches!(
            &outer_table.rows[0].cells[0].blocks[0],
            Block::Paragraph(paragraph)
                if paragraph.role == ParagraphRole::Caption
                    && inlines_to_plain_text(&paragraph.inlines) == "nested caption"
        ));
        assert!(matches!(
            &outer_table.rows[0].cells[0].blocks[1],
            Block::Table(_)
        ));
    }

    #[test]
    fn preserves_hwpx_fallback_object_placeholders() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p>
                <hp:run><hp:t>before image</hp:t></hp:run>
                <hp:ctrl><hp:pic><hp:imgRect/></hp:pic></hp:ctrl>
                <hp:run><hp:t>after image</hp:t></hp:run>
                <hp:ctrl><hp:equation><hp:formula>x + y</hp:formula></hp:equation></hp:ctrl>
                <hp:ctrl><hp:chart chartTitle="Sales"/></hp:ctrl>
              </hp:p>
            </hs:sec>
        "#;

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);

        assert_eq!(blocks.len(), 5);
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
            Block::Equation(equation)
                if equation.kind == EquationKind::PlainText
                    && equation.content.as_deref() == Some("x + y")
                    && equation.fallback_text.as_deref() == Some("x + y")
        ));
        assert!(matches!(
            &blocks[4],
            Block::Chart(chart)
                if chart.title.as_deref() == Some("Sales")
                    && chart.fallback_text.as_deref() == Some("Sales")
        ));
    }

    #[test]
    fn preserves_missing_hwpx_image_alt_as_unknown_fallback() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"
                    xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core">
              <hp:p>
                <hp:ctrl>
                  <hp:pic altText="missing image alt">
                    <hc:img binaryItemIDRef="missing-image"/>
                  </hp:pic>
                </hp:ctrl>
              </hp:p>
            </hs:sec>
        "#;

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);

        assert!(matches!(
            &blocks[0],
            Block::Unknown(unknown)
                if unknown.kind == "hwpx:image"
                    && unknown.fallback_text.as_deref() == Some("[image]\nmissing image alt")
        ));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("missing binary item `missing-image`")
        }));
    }

    #[test]
    fn preserves_hwpx_shape_text_as_shape_fallback() {
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
            Block::Shape(shape)
                if shape.kind == ShapeKind::Rectangle
                    && shape.fallback_text.as_deref() == Some("shape text")
        ));
    }

    #[test]
    fn recovers_hwpx_shape_description_attribute_alias() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p>
                <hp:ctrl><hp:rect title="shape alt text"/></hp:ctrl>
              </hp:p>
            </hs:sec>
        "#;

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);

        match &blocks[0] {
            Block::Shape(shape) => {
                assert_eq!(shape.description.as_deref(), Some("shape alt text"));
                assert_eq!(shape.fallback_text.as_deref(), Some("shape alt text"));
            }
            other => panic!("expected shape block, got {other:?}"),
        }
    }

    #[test]
    fn recovers_nested_hwpx_chart_title_text_without_raw_xml() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p>
                <hp:ctrl>
                  <hp:chart>
                    <hp:title>
                      <hp:run><hp:t>Nested Sales</hp:t></hp:run>
                    </hp:title>
                  </hp:chart>
                </hp:ctrl>
              </hp:p>
            </hs:sec>
        "#;

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);

        assert!(matches!(
            &blocks[0],
            Block::Chart(chart)
                if chart.title.as_deref() == Some("Nested Sales")
                    && chart.fallback_text.as_deref() == Some("Nested Sales")
        ));
    }

    #[test]
    fn does_not_leak_nested_hwpx_object_attributes_to_root_metadata() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p>
                <hp:ctrl>
                  <hp:equation>
                    <hp:run text="wrong attribute"><hp:t>x + y</hp:t></hp:run>
                  </hp:equation>
                </hp:ctrl>
                <hp:ctrl>
                  <hp:chart>
                    <hp:series name="Wrong Series"/>
                    <hp:title><hp:run><hp:t>Right Title</hp:t></hp:run></hp:title>
                  </hp:chart>
                </hp:ctrl>
              </hp:p>
            </hs:sec>
        "#;

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);

        assert!(matches!(
            &blocks[0],
            Block::Equation(equation)
                if equation.content.as_deref() == Some("x + y")
                    && equation.fallback_text.as_deref() == Some("x + y")
        ));
        assert!(matches!(
            &blocks[1],
            Block::Chart(chart)
                if chart.title.as_deref() == Some("Right Title")
                    && chart.fallback_text.as_deref() == Some("Right Title")
        ));
    }

    #[test]
    fn does_not_leak_nested_hwpx_child_metadata_to_outer_object() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p>
                <hp:ctrl>
                  <hp:chart>
                    <hp:series><hp:title>Wrong Series Title</hp:title></hp:series>
                    <hp:run><hp:t>Chart Body</hp:t></hp:run>
                  </hp:chart>
                </hp:ctrl>
              </hp:p>
            </hs:sec>
        "#;

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);

        assert!(matches!(
            &blocks[0],
            Block::Chart(chart)
                if chart.title.is_none()
                    && chart.fallback_text.as_deref() == Some("Chart Body")
        ));
    }

    #[test]
    fn preserves_unsupported_hwpx_control_without_text_as_unknown_block() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p>
                <hp:ctrl><hp:unknownControl id="7" title="opaque control"/></hp:ctrl>
              </hp:p>
            </hs:sec>
        "#;

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);

        assert_eq!(blocks.len(), 1);
        assert!(matches!(
            &blocks[0],
            Block::Unknown(unknown)
                if unknown.kind == "hwpx:control:unknownControl"
                    && unknown.fallback_text.as_deref()
                        == Some("[control: unknownControl]\nopaque control")
        ));
    }

    #[test]
    fn omits_hwpx_column_definition_control_as_layout_metadata() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p>
                <hp:ctrl><hp:colPr type="NEWSPAPER" colCount="2"/></hp:ctrl>
              </hp:p>
            </hs:sec>
        "#;

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);

        assert!(blocks.is_empty());
        assert!(context.warnings.iter().any(|warning| {
            warning.message.contains("column definition")
                && warning.message.contains("omitted its layout metadata")
        }));
    }

    #[test]
    fn recovers_hwpx_hyperlink_field_as_link_inline() {
        let xml = r#"
            <hp:p xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:ctrl>
                <hp:fieldBegin instId="7" fieldType="HYPERLINK" title="Example">
                  <hp:parameters cnt="1">
                    <hp:stringParam paramName="URL" val="https://example.com"/>
                  </hp:parameters>
                </hp:fieldBegin>
              </hp:ctrl>
              <hp:run><hp:t>Example Site</hp:t></hp:run>
              <hp:ctrl><hp:fieldEnd beginIdRef="7"/></hp:ctrl>
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
    fn recovers_hwpx_field_parameter_from_nested_text_node() {
        let xml = r#"
            <hp:p xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:ctrl>
                <hp:fieldBegin id="8" type="HYPERLINK" name="Nested Example">
                  <hp:parameters cnt="1">
                    <hp:stringParam name="WebAddress">
                      <hp:run><hp:t>https://example.com/nested</hp:t></hp:run>
                    </hp:stringParam>
                  </hp:parameters>
                </hp:fieldBegin>
              </hp:ctrl>
              <hp:run><hp:t>Nested Site</hp:t></hp:run>
              <hp:ctrl><hp:fieldEnd beginIDRef="8"/></hp:ctrl>
            </hp:p>
        "#;

        let mut context = HwpxFallbackContext::default();
        let inlines = extract_inlines_from_xml_fragment(xml, &mut context);

        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            Inline::Link(link) => {
                assert_eq!(link.url, "https://example.com/nested");
                assert_eq!(link.title.as_deref(), Some("Nested Example"));
                assert_eq!(inlines_to_plain_text(&link.inlines), "Nested Site");
            }
            other => panic!("expected link inline, got {other:?}"),
        }
    }

    #[test]
    fn recovers_hwpx_direct_hyperlink_as_link_inline() {
        let xml = r#"
            <hp:p xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:hyperlink webAddress="https://example.com/direct" tooltip="Direct Example">
                <hp:run><hp:t>Direct Site</hp:t></hp:run>
              </hp:hyperlink>
            </hp:p>
        "#;

        let mut context = HwpxFallbackContext::default();
        let inlines = extract_inlines_from_xml_fragment(xml, &mut context);

        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            Inline::Link(link) => {
                assert_eq!(link.url, "https://example.com/direct");
                assert_eq!(link.title.as_deref(), Some("Direct Example"));
                assert_eq!(inlines_to_plain_text(&link.inlines), "Direct Site");
            }
            other => panic!("expected link inline, got {other:?}"),
        }
    }

    #[test]
    fn recovers_hwpx_namespaced_hyperlink_attribute() {
        let xml = r#"
            <hp:p xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"
                  xmlns:xlink="http://www.w3.org/1999/xlink">
              <hp:hyperlink xlink:href="https://example.com/namespaced">
                <hp:run><hp:t>Namespaced Site</hp:t></hp:run>
              </hp:hyperlink>
            </hp:p>
        "#;

        let mut context = HwpxFallbackContext::default();
        let inlines = extract_inlines_from_xml_fragment(xml, &mut context);

        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            Inline::Link(link) => {
                assert_eq!(link.url, "https://example.com/namespaced");
                assert_eq!(inlines_to_plain_text(&link.inlines), "Namespaced Site");
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
    fn preserves_hwpx_bookmark_id_as_anchor_inline() {
        let xml = r#"
            <hp:p xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:ctrl><hp:bookmark title="bookmark-7"/></hp:ctrl>
              <hp:run><hp:t>target text</hp:t></hp:run>
            </hp:p>
        "#;

        let mut context = HwpxFallbackContext::default();
        let inlines = extract_inlines_from_xml_fragment(xml, &mut context);

        assert!(matches!(
            &inlines[0],
            Inline::Anchor { id } if id == "bookmark-7"
        ));
    }

    #[test]
    fn preserves_hwpx_non_link_field_as_unknown_inline() {
        let xml = r#"
            <hp:p xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:ctrl>
                <hp:fieldBegin instId="9" fieldType="DATE" title="created date"/>
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
                    <hh:bullets>
                      <hh:bullet id="1" bulletMarker="*"/>
                    </hh:bullets>
                    <hh:numberings>
                      <hh:numbering id="1">
                        <hh:paraHead level="1" start="3" numFormat="LATIN_CAPITAL"/>
                      </hh:numbering>
                      <hh:numbering id="2">
                        <hh:paraHead level="1" start="5" numFormat="DIGIT"/>
                      </hh:numbering>
                    </hh:numberings>
                    <hh:paraProperties>
                      <hh:paraPr id="0"><hh:heading type="BULLET" idREF="1" level="0"/></hh:paraPr>
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
            Some("*")
        );
        assert_eq!(
            paragraphs[1].list.as_ref().map(|list| (
                &list.kind,
                list.number,
                list.marker.as_deref()
            )),
            Some((&ListKind::Ordered, Some(3), Some("C")))
        );
        assert_eq!(
            paragraphs[2].list.as_ref().map(|list| (
                &list.kind,
                list.number,
                list.marker.as_deref()
            )),
            Some((&ListKind::Ordered, Some(4), Some("D")))
        );
        assert_eq!(
            paragraphs[3].list.as_ref().map(|list| (
                &list.kind,
                list.number,
                list.marker.as_deref()
            )),
            Some((&ListKind::Ordered, Some(5), Some("5")))
        );

        Ok(())
    }

    #[test]
    fn recovers_unordered_marker_from_official_hwpx_list_fixture() -> Result<(), Box<dyn Error>> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/list/input.hwpx");
        let bytes = fs::read(path)?;
        let document = read_section_document_from_archive(&bytes)?;
        let paragraph = document.sections[0]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Paragraph(paragraph) => Some(paragraph),
                _ => None,
            })
            .expect("list fixture paragraph");

        assert_eq!(
            paragraph.list.as_ref().map(|list| &list.kind),
            Some(&ListKind::Unordered),
            "fallback paragraph: {paragraph:#?}"
        );
        assert_eq!(
            paragraph
                .list
                .as_ref()
                .and_then(|list| list.marker.as_deref()),
            Some("•")
        );
        Ok(())
    }

    #[test]
    fn recovers_heading_role_from_hwpx_header_paragraph_properties() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[
            (
                HEADER_XML_PATH,
                r#"
                <hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head">
                  <hh:refList>
                    <hh:paraProperties>
                      <hh:paraPr id="0"><hh:heading kind="OUTLINE" outlineLevel="2"/></hh:paraPr>
                    </hh:paraProperties>
                  </hh:refList>
                </hh:head>
                "#,
            ),
            (
                "Contents/section0.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p paraPrIDRef="0"><hp:run><hp:t>heading text</hp:t></hp:run></hp:p>
                </hs:sec>
                "#,
            ),
        ])?;

        let document = read_section_document_from_archive(&bytes)?;
        let paragraph = match &document.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => paragraph,
            other => panic!("expected paragraph block, got {other:?}"),
        };

        assert_eq!(paragraph.role, ParagraphRole::Heading { level: 3 });
        assert!(paragraph.list.is_none());

        Ok(())
    }

    #[test]
    fn warns_when_hwpx_style_references_are_missing() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p paraPrIDRef="9">
                <hp:run charPrIDRef="7"><hp:t>unstyled</hp:t></hp:run>
              </hp:p>
            </hs:sec>
        "#;

        let mut context = HwpxFallbackContext::default();
        let blocks = extract_section_xml_blocks(xml, &mut context);

        assert!(matches!(&blocks[0], Block::Paragraph(_)));
        assert!(
            context
                .warnings
                .iter()
                .any(|warning| warning.message.contains("missing paraPr id 9"))
        );
        assert!(
            context
                .warnings
                .iter()
                .any(|warning| warning.message.contains("missing charPr id 7"))
        );
    }

    #[test]
    fn warns_when_hwpx_unordered_list_marker_is_missing() {
        let xml = r#"
            <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
              <hp:p paraPrIDRef="0"><hp:run><hp:t>item</hp:t></hp:run></hp:p>
            </hs:sec>
        "#;
        let mut context = HwpxFallbackContext {
            paragraph_styles: vec![HwpxParagraphStyle {
                kind: Some(ListKind::Unordered),
                list_id: Some(99),
                ..Default::default()
            }],
            ..Default::default()
        };

        let blocks = extract_section_xml_blocks(xml, &mut context);
        let paragraph = match &blocks[0] {
            Block::Paragraph(paragraph) => paragraph,
            other => panic!("expected paragraph block, got {other:?}"),
        };

        assert_eq!(
            paragraph
                .list
                .as_ref()
                .and_then(|list| list.marker.as_deref()),
            Some("•")
        );
        assert!(
            context
                .warnings
                .iter()
                .any(|warning| warning.message.contains("missing bullet marker id 99"))
        );
    }

    #[test]
    fn recovers_paragraph_style_from_hwpx_header_paragraph_properties() -> Result<(), Box<dyn Error>>
    {
        let bytes = create_archive_bytes(&[
            (
                HEADER_XML_PATH,
                r##"
                <hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head"
                         xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core">
                  <hh:refList>
                    <hh:borderFills>
                      <hh:borderFill id="3">
                        <hh:leftBorder type="SOLID" width="1 px" color="#112233"/>
                        <hh:rightBorder type="DASH" width="1 px" color="#223344"/>
                        <hh:topBorder type="DOT" width="1 px" color="#334455"/>
                        <hh:bottomBorder type="DOUBLE_SLIM" width="1 px" color="#445566"/>
                        <hc:fillBrush><hc:winBrush faceColor="#AABBCC"/></hc:fillBrush>
                      </hh:borderFill>
                    </hh:borderFills>
                    <hh:paraProperties>
                      <hh:paraPr id="0">
                        <hh:align horizontalAlign="center"/>
                        <hh:margin>
                          <hh:indent unit="HWPUNIT" val="100"/>
                          <hh:left unit="HWPUNIT" val="200"/>
                          <hh:right unit="HWPUNIT" val="300"/>
                          <hh:prev unit="HWPUNIT" val="400"/>
                          <hh:next unit="HWPUNIT" val="500"/>
                        </hh:margin>
                        <hh:lineSpacing type="fixed" val="600" unit="HWPUNIT"/>
                        <hh:breakSetting widowOrphan="1" keepWithNext="true"
                                         keepLines="yes" pageBreakBefore="on"/>
                        <hh:border borderFillIDRef="3" offsetLeft="100" offsetRight="200"
                                   offsetTop="300" offsetBottom="400"/>
                      </hh:paraPr>
                    </hh:paraProperties>
                  </hh:refList>
                </hh:head>
                "##,
            ),
            (
                "Contents/section0.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p paraPrIDREF="0"><hp:run><hp:t>styled paragraph</hp:t></hp:run></hp:p>
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
        assert_eq!(
            paragraph.style.background_color,
            Some(Color {
                r: 0xAA,
                g: 0xBB,
                b: 0xCC,
                a: 255,
            })
        );
        assert_eq!(paragraph.style.padding_left_pt, Some(LengthPt(1.0)));
        assert_eq!(paragraph.style.padding_right_pt, Some(LengthPt(2.0)));
        assert_eq!(paragraph.style.padding_top_pt, Some(LengthPt(3.0)));
        assert_eq!(paragraph.style.padding_bottom_pt, Some(LengthPt(4.0)));
        assert_eq!(
            paragraph
                .style
                .border_left
                .as_ref()
                .map(|border| border.style),
            Some(BorderStyle::Solid)
        );
        assert_eq!(
            paragraph
                .style
                .border_right
                .as_ref()
                .map(|border| border.style),
            Some(BorderStyle::Dashed)
        );
        assert_eq!(
            paragraph
                .style
                .border_top
                .as_ref()
                .map(|border| border.style),
            Some(BorderStyle::Dotted)
        );
        assert_eq!(
            paragraph
                .style
                .border_bottom
                .as_ref()
                .map(|border| border.style),
            Some(BorderStyle::Double)
        );
        assert!(paragraph.style.widow_orphan);
        assert!(paragraph.style.keep_with_next);
        assert!(paragraph.style.keep_lines);
        assert!(paragraph.style.page_break_before);

        Ok(())
    }

    #[test]
    fn warns_when_hwpx_paragraph_alignment_is_unsupported() {
        let mut context = extract_hwpx_fallback_context(
            r#"
            <hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head">
              <hh:paraPr id="0">
                <hh:align horizontalAlign="UNKNOWN_ALIGN"/>
              </hh:paraPr>
            </hh:head>
            "#,
        );

        extract_section_xml_blocks(
            r#"
            <hp:p>
              <hp:align horizontal="SIDEWAYS"/>
              <hp:run><hp:t>text</hp:t></hp:run>
            </hp:p>
            "#,
            &mut context,
        );

        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported horizontal alignment `UNKNOWN_ALIGN`")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported horizontal alignment `SIDEWAYS`")
        }));
    }

    #[test]
    fn warns_when_hwpx_paragraph_metric_is_invalid() {
        let context = extract_hwpx_fallback_context(
            r#"
            <hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head">
              <hh:paraPr id="0">
                <hh:margin>
                  <hh:indent value="bad-indent"/>
                  <hh:left value="bad-left"/>
                </hh:margin>
                <hh:lineSpacing value="bad-line"/>
                <hh:breakSetting keepLines="maybe"/>
                <hh:border borderFillIDRef="bad-border" offsetLeft="-10" offsetRight="bad"/>
              </hh:paraPr>
            </hh:head>
            "#,
        );

        assert_eq!(context.paragraph_styles[0].style.indent.first_line_pt, None);
        assert_eq!(context.paragraph_styles[0].style.indent.left_pt, None);
        assert_eq!(context.paragraph_styles[0].style.spacing.line_pt, None);
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported first-line indent value `bad-indent`")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported left indent value `bad-left`")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported line spacing value `bad-line`")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported keep-lines value `maybe`")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported borderFill id `bad-border`")
        }));
        assert!(
            context
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("unsupported left offset `-10`") })
        );
        assert!(
            context
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("unsupported right offset `bad`") })
        );
    }

    #[test]
    fn preserves_hwpx_percent_line_spacing() {
        let mut context = extract_hwpx_fallback_context(
            r#"
            <hh:head>
              <hh:paraPr id="1">
                <hh:lineSpacing kind="PERCENT" value="160" unit="HWPUNIT"/>
              </hh:paraPr>
            </hh:head>
            "#,
        );

        assert_eq!(
            context.paragraph_styles[1].style.spacing.line_percent,
            Some(Percent(160.0))
        );
        assert!(context.warnings.is_empty());

        let blocks = extract_section_xml_blocks(
            r#"
            <hp:p>
              <hp:lineSpacing type="PERCENT" value="130" unit="HWPUNIT"/>
              <hp:run><hp:t>direct style</hp:t></hp:run>
            </hp:p>
            "#,
            &mut context,
        );

        let Block::Paragraph(paragraph) = &blocks[0] else {
            panic!("expected paragraph block");
        };
        assert_eq!(paragraph.style.spacing.line_percent, Some(Percent(130.0)));
        assert!(context.warnings.is_empty());
    }

    #[test]
    fn recovers_hwpx_header_context_from_case_variant_archive_entry() -> Result<(), Box<dyn Error>>
    {
        let bytes = create_archive_bytes(&[
            (
                "contents/HEADER.XML",
                r#"
                <hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head">
                  <hh:refList>
                    <hh:paraProperties>
                      <hh:paraPr id="0">
                        <hh:align horizontal="center"/>
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

        Ok(())
    }

    #[test]
    fn recovers_direct_hwpx_paragraph_style_without_para_pr_ref() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[
            (
                HEADER_XML_PATH,
                r#"
                <hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head">
                  <hh:refList>
                    <hh:bullets>
                      <hh:bullet id="7" char="*"/>
                    </hh:bullets>
                  </hh:refList>
                </hh:head>
                "#,
            ),
            (
                "Contents/section0.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p>
                    <hp:heading type="bullet" idref="7" level="1"/>
                    <hp:align horzAlign="right"/>
                    <hp:run><hp:t>direct style paragraph</hp:t></hp:run>
                  </hp:p>
                </hs:sec>
                "#,
            ),
        ])?;

        let document = read_section_document_from_archive(&bytes)?;
        let paragraph = match &document.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => paragraph,
            other => panic!("expected paragraph block, got {other:?}"),
        };

        assert_eq!(paragraph.style.alignment, Some(Alignment::Right));
        assert_eq!(
            paragraph
                .list
                .as_ref()
                .map(|list| (&list.kind, list.level, list.marker.as_deref())),
            Some((&ListKind::Unordered, 1, Some("*")))
        );

        Ok(())
    }

    #[test]
    fn does_not_leak_nested_hwpx_paragraph_style_to_outer_paragraph() -> Result<(), Box<dyn Error>>
    {
        let bytes = create_archive_bytes(&[
            (
                HEADER_XML_PATH,
                r#"
                <hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head">
                  <hh:refList>
                    <hh:bullets>
                      <hh:bullet id="7" char="*"/>
                    </hh:bullets>
                    <hh:paraProperties>
                      <hh:paraPr id="0">
                        <hh:heading type="bullet" idRef="7" level="1"/>
                        <hh:align horizontal="right"/>
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
                  <hp:p>
                    <hp:run><hp:t>outer before</hp:t></hp:run>
                    <hp:ctrl>
                      <hp:tbl>
                        <hp:tr>
                          <hp:tc>
                            <hp:subList>
                              <hp:p paraPrIDRef="0"><hp:run><hp:t>styled cell</hp:t></hp:run></hp:p>
                            </hp:subList>
                          </hp:tc>
                        </hp:tr>
                      </hp:tbl>
                    </hp:ctrl>
                    <hp:run><hp:t>outer after</hp:t></hp:run>
                  </hp:p>
                </hs:sec>
                "#,
            ),
        ])?;

        let document = read_section_document_from_archive(&bytes)?;
        assert_eq!(document.sections[0].blocks.len(), 3);

        for index in [0, 2] {
            let paragraph = match &document.sections[0].blocks[index] {
                Block::Paragraph(paragraph) => paragraph,
                other => panic!("expected outer paragraph fragment, got {other:?}"),
            };
            assert_eq!(paragraph.style.alignment, None);
            assert!(paragraph.list.is_none());
        }

        let cell_paragraph = match &document.sections[0].blocks[1] {
            Block::Table(table) => match &table.rows[0].cells[0].blocks[0] {
                Block::Paragraph(paragraph) => paragraph,
                other => panic!("expected styled cell paragraph, got {other:?}"),
            },
            other => panic!("expected table block, got {other:?}"),
        };
        assert_eq!(cell_paragraph.style.alignment, Some(Alignment::Right));
        assert_eq!(
            cell_paragraph.list.as_ref().map(|list| (
                &list.kind,
                list.level,
                list.marker.as_deref()
            )),
            Some((&ListKind::Unordered, 1, Some("*")))
        );

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
                      <hh:fontface lang="LATIN"><hh:font id="0" face="Wrong Latin"/></hh:fontface>
                      <hh:fontface script="HANGUL"><hh:font id="0" fontName="Noto Sans KR"/></hh:fontface>
                    </hh:fontfaces>
                    <hh:charProperties>
                      <hh:charPr id="7" fontSize="1200" color="010203" backgroundColor="0x040506" symbolMark="DOT_ABOVE">
                        <hh:fontRef hangul="0"/>
                        <hh:bold/>
                        <hh:italic/>
                        <hh:underline type="TOP" shape="WAVE" lineColor="#070809"/>
                        <hh:strikeout shape="DOUBLE_SLIM" lineColor="#0A0B0C"/>
                        <hh:outline type="SOLID"/>
                        <hh:shadow type="DROP"/>
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
                  <hp:p><hp:run charPrIDREF="7"><hp:t>styled text</hp:t></hp:run></hp:p>
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
        assert!(text_run.style.underline_above);
        assert_eq!(
            text_run.style.underline_style,
            Some(TextDecorationStyle::Wavy)
        );
        assert_eq!(
            text_run.style.strike_style,
            Some(TextDecorationStyle::Double)
        );
        assert!(text_run.style.emphasis_dot);
        assert!(text_run.style.outline);
        assert!(text_run.style.shadow);
        assert_eq!(
            text_run.style.underline_color,
            Some(Color {
                r: 7,
                g: 8,
                b: 9,
                a: 255,
            })
        );
        assert_eq!(
            text_run.style.strike_color,
            Some(Color {
                r: 10,
                g: 11,
                b: 12,
                a: 255,
            })
        );

        Ok(())
    }

    #[test]
    fn ignores_disabled_hwpx_text_effect_tags() {
        let (style, warnings) = extract_hwpx_text_style_with_warnings(
            r#"hh:charPr id="0" symMark="NONE""#,
            r##"
              <hh:charPr id="0" symMark="NONE">
                <hh:underline type="NONE" shape="SOLID" color="#010203"/>
                <hh:strikeout shape="NONE" color="#040506"/>
                <hh:outline type="NONE"/>
                <hh:shadow type="NONE" color="#070809"/>
              </hh:charPr>
            "##,
            &[],
        );

        assert!(warnings.is_empty());
        assert!(!style.underline);
        assert!(!style.strike);
        assert!(!style.emphasis_dot);
        assert!(!style.outline);
        assert!(!style.shadow);
        assert_eq!(style.underline_color, None);
        assert_eq!(style.strike_color, None);
        assert_eq!(style.underline_style, None);
        assert_eq!(style.strike_style, None);
        assert!(!style.underline_above);
    }

    #[test]
    fn recovers_uniform_hwpx_text_metrics() {
        let (style, warnings) = extract_hwpx_text_style_with_warnings(
            r#"hh:charPr id="0" height="1200" useKerning="1""#,
            r#"
              <hh:charPr id="0" height="1200" useKerning="1">
                <hh:ratio hangul="95" latin="95" hanja="95" japanese="95" other="95" symbol="95" user="95"/>
                <hh:spacing hangul="-5" latin="-5" hanja="-5" japanese="-5" other="-5" symbol="-5" user="-5"/>
                <hh:relSz hangul="80" latin="80" hanja="80" japanese="80" other="80" symbol="80" user="80"/>
                <hh:offset hangul="10" latin="10" hanja="10" japanese="10" other="10" symbol="10" user="10"/>
              </hh:charPr>
            "#,
            &[],
        );

        assert!(warnings.is_empty(), "unexpected warnings: {warnings:#?}");
        assert_eq!(style.font_width_percent, Some(Percent(95.0)));
        assert_eq!(style.letter_spacing_percent, Some(Percent(-5.0)));
        assert_eq!(style.relative_size_percent, Some(Percent(80.0)));
        assert_eq!(style.vertical_offset_percent, Some(Percent(10.0)));
        assert_eq!(style.font_size_pt, Some(LengthPt(9.6)));
        assert!(style.kerning);
    }

    #[test]
    fn warns_and_omits_script_specific_hwpx_text_metrics() {
        let (style, warnings) = extract_hwpx_text_style_with_warnings(
            r#"hh:charPr id="0""#,
            r#"
              <hh:charPr id="0">
                <hh:ratio hangul="95" latin="100"/>
                <hh:spacing hangul="-5" latin="0"/>
              </hh:charPr>
            "#,
            &[],
        );

        assert_eq!(style.font_width_percent, None);
        assert_eq!(style.letter_spacing_percent, None);
        assert!(
            warnings
                .iter()
                .any(|warning| { warning.contains("script-specific horizontal ratio values") })
        );
        assert!(
            warnings
                .iter()
                .any(|warning| { warning.contains("script-specific character spacing values") })
        );
    }

    #[test]
    fn warns_when_hwpx_font_ref_references_missing_face() {
        let context = extract_hwpx_fallback_context(
            r#"
            <hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head">
              <hh:fontface lang="LATIN">
                <hh:font id="0" face="Latin Font"/>
              </hh:fontface>
              <hh:charPr id="0">
                <hh:fontRef hangul="9" latin="0"/>
              </hh:charPr>
            </hh:head>
            "#,
        );

        assert_eq!(
            context.text_styles[0].font_family.as_deref(),
            Some("Latin Font")
        );
        assert!(
            context
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("missing hangul font id 9") })
        );
    }

    #[test]
    fn warns_when_hwpx_char_pr_values_are_invalid() {
        let context = extract_hwpx_fallback_context(
            r##"
            <hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head">
              <hh:charPr id="0" fontSize="large" color="not-a-color" backgroundColor="#xyz">
                <hh:underline type="BOTTOM" color="bad-underline"/>
                <hh:strikeout shape="SOLID" color="bad-strike"/>
              </hh:charPr>
            </hh:head>
            "##,
        );

        assert_eq!(context.text_styles[0].font_size_pt, None);
        assert_eq!(context.text_styles[0].color, None);
        assert_eq!(context.text_styles[0].background_color, None);
        assert!(
            context
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("unsupported font size `large`") })
        );
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported text color `not-a-color`")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported text background color `#xyz`")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported underline color `bad-underline`")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported strikeout color `bad-strike`")
        }));
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
                      <hh:borderFill id="3"><hc:fillBrush><hc:winBrush faceColor="112233"/></hc:fillBrush></hh:borderFill>
                      <hh:borderFill id="4">
                        <hh:leftBorder type="SOLID" width="0.12 mm" color="#010203"/>
                        <hh:rightBorder type="DASH" w="1 pt" lineColor="#040506"/>
                        <hh:topBorder type="DOT" w="2 px" lineColor="#070809"/>
                        <hh:bottomBorder style="DOUBLE_SLIM" width="0.2 mm" color="#0A0B0C"/>
                        <hc:fillBrush><hc:winBrush fillColor="0X445566"/></hc:fillBrush>
                      </hh:borderFill>
                    </hh:borderFills>
                  </hh:refList>
                </hh:head>
                "##,
            ),
            (
                "Contents/section0.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:tbl>
                    <hp:tblPr borderFillIdRef="3"/>
                    <hp:tr>
                      <hp:tc>
                        <hp:cellPr borderFillIDREF="4"/>
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
        let cell_style = &table.rows[0].cells[0].style;
        let left = cell_style.border_left.as_ref().expect("left border");
        assert!((left.width.0 - (0.12 * 96.0 / 25.4)).abs() < 0.001);
        assert_eq!(left.style, BorderStyle::Solid);
        assert_eq!(
            left.color,
            Some(Color {
                r: 1,
                g: 2,
                b: 3,
                a: 255,
            })
        );
        assert_eq!(
            cell_style.border_right.as_ref().map(|border| border.style),
            Some(BorderStyle::Dashed)
        );
        assert_eq!(
            cell_style.border_top.as_ref().map(|border| border.style),
            Some(BorderStyle::Dotted)
        );
        assert_eq!(
            cell_style.border_bottom.as_ref().map(|border| border.style),
            Some(BorderStyle::Double)
        );

        Ok(())
    }

    #[test]
    fn warns_when_hwpx_border_fill_style_is_approximated() {
        let context = extract_hwpx_fallback_context(
            r##"
            <hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head">
              <hh:borderFill id="0">
                <hh:leftBorder type="WAVE" width="1 px" color="#010203"/>
              </hh:borderFill>
            </hh:head>
            "##,
        );

        assert_eq!(
            context.border_fills[0].borders[0]
                .as_ref()
                .map(|border| border.style),
            Some(BorderStyle::Solid)
        );
        assert!(
            context
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("unsupported border style `WAVE`") })
        );
    }

    #[test]
    fn warns_when_hwpx_border_fill_width_is_approximated() {
        let context = extract_hwpx_fallback_context(
            r##"
            <hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head">
              <hh:borderFill id="0">
                <hh:leftBorder type="SOLID" width="wide" color="#010203"/>
              </hh:borderFill>
            </hh:head>
            "##,
        );

        assert_eq!(
            context.border_fills[0].borders[0]
                .as_ref()
                .map(|border| border.width),
            Some(LengthPx(1.0))
        );
        assert!(
            context
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("unsupported border width `wide`") })
        );
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
                  <opf:spine><opf:itemref idRef="section0"/></opf:spine>
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
                      <hp:pic>
                        <hp:altText><hp:run><hp:t>sample image</hp:t></hp:run></hp:altText>
                        <hp:sz w="7500" h="3750"/>
                        <hp:img><hc:img binItem="image1" effect="GRAY_SCALE"/></hp:img>
                        <hp:caption>
                          <hp:subList>
                            <hp:p><hp:run><hp:t>image caption</hp:t></hp:run></hp:p>
                          </hp:subList>
                        </hp:caption>
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
        assert_eq!(image.caption.as_deref(), Some("image caption"));
        assert_eq!(image.width, Some(LengthPx(100.0)));
        assert_eq!(image.height, Some(LengthPx(50.0)));
        assert!(image.grayscale);

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
    fn warns_when_hwpx_picture_dimensions_are_invalid() {
        let mut context = HwpxFallbackContext::default();
        context.image_items.insert(
            "image1".to_string(),
            HwpxImageItem {
                id: "image1".to_string(),
                media_type: Some("image/png".to_string()),
                extension: Some("png".to_string()),
                bytes: b"image-bytes".to_vec(),
            },
        );
        let image = extract_hwpx_image_from_pic_xml(
            r#"
            <hp:pic>
              <hp:sz w="bad-width" h="bad-height"/>
              <hc:img binaryItemIDRef="image1"/>
            </hp:pic>
            "#,
            &mut context,
        )
        .expect("image should parse");

        assert_eq!(image.width, None);
        assert_eq!(image.height, None);
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported width value `bad-width`")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("unsupported height value `bad-height`")
        }));
    }

    #[test]
    fn warns_when_hwpx_black_white_image_effect_is_approximated() {
        let mut context = HwpxFallbackContext::default();
        context.image_items.insert(
            "image1".to_string(),
            HwpxImageItem {
                id: "image1".to_string(),
                media_type: Some("image/png".to_string()),
                extension: Some("png".to_string()),
                bytes: b"image-bytes".to_vec(),
            },
        );
        let image = extract_hwpx_image_from_pic_xml(
            r#"<hp:pic><hc:img binaryItemIDRef="image1" effect="BLACK_WHITE"/></hp:pic>"#,
            &mut context,
        )
        .expect("image should be recovered");

        assert!(image.grayscale);
        assert!(context.warnings.iter().any(|warning| {
            warning.message.contains("BLACK_WHITE effect")
                && warning.message.contains("grayscale approximation")
        }));
    }

    #[test]
    fn warns_when_hwpx_image_brightness_or_contrast_is_not_applied() {
        let mut context = HwpxFallbackContext::default();
        context.image_items.insert(
            "image1".to_string(),
            HwpxImageItem {
                id: "image1".to_string(),
                media_type: Some("image/png".to_string()),
                extension: Some("png".to_string()),
                bytes: b"image-bytes".to_vec(),
            },
        );
        let image = extract_hwpx_image_from_pic_xml(
            r#"<hp:pic><hp:img><hc:img binaryItemIDRef="image1" brightness="12" contrastValue="-4" pictureEffect="REAL_PIC"/></hp:img></hp:pic>"#,
            &mut context,
        )
        .expect("image should be recovered");

        assert!(!image.grayscale);
        assert!(context.warnings.iter().any(|warning| {
            warning.message.contains("brightness:12,contrast:-4")
                && warning.message.contains("without applying the adjustment")
        }));
    }

    #[test]
    fn warns_when_hwpx_image_alpha_or_inner_margin_is_not_applied() {
        let mut context = HwpxFallbackContext::default();
        context.image_items.insert(
            "image1".to_string(),
            HwpxImageItem {
                id: "image1".to_string(),
                media_type: Some("image/png".to_string()),
                extension: Some("png".to_string()),
                bytes: b"image-bytes".to_vec(),
            },
        );
        let xml = r#"
            <hp:pic>
              <hp:inMargin l="10" r="20" t="30" b="40"/>
              <hc:img binaryItemIDRef="image1" opacity="0.5"/>
            </hp:pic>
        "#;

        extract_hwpx_image_from_pic_xml(xml, &mut context).expect("image should parse");

        assert!(context.warnings.iter().any(|warning| {
            warning.message.contains("alpha 0.5")
                && warning.message.contains("without applying transparency")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning.message.contains("inner margin (10/20/30/40)")
                && warning.message.contains("without applying the margin")
        }));
    }

    #[test]
    fn reads_hwpx_effect_from_the_outer_picture_resource() {
        let mut context = HwpxFallbackContext::default();
        for id in ["outer-image", "caption-image"] {
            context.image_items.insert(
                id.to_string(),
                HwpxImageItem {
                    id: id.to_string(),
                    media_type: Some("image/png".to_string()),
                    extension: Some("png".to_string()),
                    bytes: b"image-bytes".to_vec(),
                },
            );
        }
        let xml = r#"
            <hp:pic>
              <hp:caption><hp:pic><hc:img binaryItemIDRef="caption-image" effect="GRAY_SCALE"/></hp:pic></hp:caption>
              <hp:img><hc:img binaryItemIDRef="outer-image" effect="REAL_PIC"/></hp:img>
            </hp:pic>
        "#;

        let image = extract_hwpx_image_from_pic_xml(xml, &mut context).expect("image should parse");

        assert_eq!(image.resource_id.as_str(), "outer-image");
        assert!(!image.grayscale);
    }

    #[test]
    fn recovers_hwpx_shape_comment_as_fallback_image_alt() {
        let mut context = HwpxFallbackContext::default();
        context.image_items.insert(
            "image1".to_string(),
            HwpxImageItem {
                id: "image1".to_string(),
                media_type: Some("image/png".to_string()),
                extension: Some("png".to_string()),
                bytes: b"image-bytes".to_vec(),
            },
        );
        let xml = r#"
            <hp:pic altText="explicit alt">
              <hc:img binaryItemIDRef="image1"/>
              <hp:shapeComment>fallback description</hp:shapeComment>
            </hp:pic>
        "#;
        let fallback_xml = r#"
            <hp:pic>
              <hc:img binaryItemIDRef="image1"/>
              <hp:shapeComment>fallback description</hp:shapeComment>
            </hp:pic>
        "#;

        let explicit = extract_hwpx_image_from_pic_xml(xml, &mut context).expect("image");
        let fallback = extract_hwpx_image_from_pic_xml(fallback_xml, &mut context).expect("image");

        assert_eq!(explicit.alt.as_deref(), Some("explicit alt"));
        assert_eq!(fallback.alt.as_deref(), Some("fallback description"));
    }

    #[test]
    fn recovers_hwpx_picture_line_shape_as_image_border() {
        let mut context = HwpxFallbackContext::default();
        context.image_items.insert(
            "image1".to_string(),
            HwpxImageItem {
                id: "image1".to_string(),
                media_type: Some("image/png".to_string()),
                extension: Some("png".to_string()),
                bytes: b"image-bytes".to_vec(),
            },
        );
        let xml = r##"
            <hp:pic>
              <hp:lineShape lineColor="#123456" w="150" type="DASH_DOT"/>
              <hc:img binaryItemIDRef="image1" effect="REAL_PIC"/>
            </hp:pic>
        "##;

        let image = extract_hwpx_image_from_pic_xml(xml, &mut context).expect("image should parse");
        let border = image.border.expect("picture border should be recovered");

        assert_eq!(border.width, LengthPx(2.0));
        assert_eq!(border.style, BorderStyle::Dashed);
        assert_eq!(
            border.color,
            Some(Color {
                r: 0x12,
                g: 0x34,
                b: 0x56,
                a: 255,
            })
        );
    }

    #[test]
    fn ignores_disabled_or_nested_hwpx_picture_line_shape() {
        let disabled =
            r##"<hp:pic><hp:lineShape color="#123456" width="150" style="NONE"/></hp:pic>"##;
        let nested = r##"
            <hp:pic>
              <hp:caption><hp:pic><hp:lineShape color="#123456" width="150" style="SOLID"/></hp:pic></hp:caption>
            </hp:pic>
        "##;
        let mut context = HwpxFallbackContext::default();

        assert_eq!(hwpx_picture_border(disabled, &mut context), None);
        assert_eq!(hwpx_picture_border(nested, &mut context), None);
    }

    #[test]
    fn warns_when_hwpx_picture_border_style_is_approximated() {
        let mut context = HwpxFallbackContext::default();
        let border = hwpx_picture_border(
            r##"<hp:pic><hp:lineShape color="#123456" width="150" style="WAVE"/></hp:pic>"##,
            &mut context,
        )
        .expect("picture border should be recovered");

        assert_eq!(border.style, BorderStyle::Solid);
        assert!(
            context
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("unsupported border style `WAVE`") })
        );
    }

    #[test]
    fn warns_when_hwpx_picture_border_width_is_invalid() {
        let mut context = HwpxFallbackContext::default();
        let border = hwpx_picture_border(
            r##"<hp:pic><hp:lineShape color="#123456" width="-1" style="SOLID"/></hp:pic>"##,
            &mut context,
        );

        assert_eq!(border, None);
        assert!(
            context
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("unsupported border width `-1`") })
        );
    }

    #[test]
    fn warns_for_hwpx_picture_flip_and_rotation_without_nested_leakage() {
        let mut context = HwpxFallbackContext::default();
        context.image_items.insert(
            "image1".to_string(),
            HwpxImageItem {
                id: "image1".to_string(),
                media_type: Some("image/png".to_string()),
                extension: Some("png".to_string()),
                bytes: b"image-bytes".to_vec(),
            },
        );
        let xml = r#"
            <hp:pic wrap="TOP_AND_BOTTOM">
              <hp:caption><hp:pic><hp:rotationInfo angle="1234"/></hp:pic></hp:caption>
              <hp:flip horizontalFlip="1" verticalFlip="false"/>
              <hp:rotationInfo rotateAngle="9000"/>
              <hc:img binaryItemIDRef="image1"/>
              <hp:imgClip l="10" r="900" t="20" b="700"/>
              <hp:imgDim dimWidth="1000" dimHeight="800"/>
              <hp:effects><hp:shadow/><hp:glow/><hp:shadow/></hp:effects>
              <hp:pos treat-as-char="0" verticalRelTo="PAGE" verticalAlign="CENTER" verticalOffset="120"
                      horizontalRelTo="COLUMN" horizontalAlign="RIGHT" horizontalOffset="240"/>
            </hp:pic>
        "#;

        extract_hwpx_image_from_pic_xml(xml, &mut context).expect("image should parse");

        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("flip_h:true,flip_v:false,rotation:9000")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("treat_as_char:false,wrap:TOP_AND_BOTTOM")
                && warning.message.contains("vertical:PAGE/CENTER/120")
                && warning.message.contains("horizontal:COLUMN/RIGHT/240")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning
                .message
                .contains("left:10,right:900,top:20,bottom:700,source:1000x800")
                && warning.message.contains("uncropped original image bytes")
        }));
        assert!(context.warnings.iter().any(|warning| {
            warning.message.contains("visual effects (shadow,glow)")
                && warning.message.contains("without applying the effects")
        }));
        assert!(
            context
                .warnings
                .iter()
                .all(|warning| !warning.message.contains("1234"))
        );

        let mut full_frame_context = HwpxFallbackContext::default();
        warn_hwpx_picture_transform(
            r#"<hp:pic><hp:imgClip left="0" right="1000" top="0" bottom="800"/><hp:imgDim dimwidth="1000" dimheight="800"/><hp:effects/></hp:pic>"#,
            &mut full_frame_context,
        );
        assert!(
            full_frame_context
                .warnings
                .iter()
                .all(|warning| !warning.message.contains("picture crop"))
        );
        assert!(
            full_frame_context
                .warnings
                .iter()
                .all(|warning| !warning.message.contains("visual effects"))
        );
    }

    #[test]
    fn does_not_leak_caption_dimensions_to_hwpx_image_size() {
        let mut context = HwpxFallbackContext::default();
        context.image_items.insert(
            "image1".to_string(),
            HwpxImageItem {
                id: "image1".to_string(),
                media_type: Some("image/png".to_string()),
                extension: Some("png".to_string()),
                bytes: b"image-bytes".to_vec(),
            },
        );
        let xml = r#"
            <hp:pic xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"
                    xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core">
              <hp:img><hc:img binaryItemIDRef="image1"/></hp:img>
              <hp:caption>
                <hp:subList>
                  <hp:p>
                    <hp:run w="7500" h="3750"><hp:t>caption</hp:t></hp:run>
                  </hp:p>
                </hp:subList>
              </hp:caption>
            </hp:pic>
        "#;

        let image = extract_hwpx_image_from_pic_xml(xml, &mut context).expect("image should parse");

        assert_eq!(image.width, None);
        assert_eq!(image.height, None);
        assert_eq!(image.caption.as_deref(), Some("caption"));
    }

    #[test]
    fn recovers_self_closing_hwpx_pic_root_image_ref() {
        let mut context = HwpxFallbackContext::default();
        context.image_items.insert(
            "image1".to_string(),
            HwpxImageItem {
                id: "image1".to_string(),
                media_type: Some("image/png".to_string()),
                extension: Some("png".to_string()),
                bytes: b"image-bytes".to_vec(),
            },
        );
        let xml = r#"
            <hp:pic xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"
                    binaryItemIdRef="image1"/>
        "#;

        let image = extract_hwpx_image_from_pic_xml(xml, &mut context).expect("image should parse");

        assert_eq!(image.resource_id.as_str(), "image1");
    }

    #[test]
    fn does_not_use_caption_image_resource_as_outer_hwpx_image_resource() {
        let mut context = HwpxFallbackContext::default();
        context.image_items.insert(
            "caption-image".to_string(),
            HwpxImageItem {
                id: "caption-image".to_string(),
                media_type: Some("image/png".to_string()),
                extension: Some("png".to_string()),
                bytes: b"caption-image-bytes".to_vec(),
            },
        );
        let xml = r#"
            <hp:pic xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"
                    xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core">
              <hp:caption>
                <hp:subList>
                  <hp:p>
                    <hp:ctrl>
                      <hp:pic><hp:img><hc:img binaryItemIDRef="caption-image"/></hp:img></hp:pic>
                    </hp:ctrl>
                    <hp:run><hp:t>caption</hp:t></hp:run>
                  </hp:p>
                </hp:subList>
              </hp:caption>
            </hp:pic>
        "#;

        assert!(extract_hwpx_image_from_pic_xml(xml, &mut context).is_none());
    }

    #[test]
    fn recovers_hwpx_image_resource_from_parent_relative_manifest_path()
    -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[
            (
                "Contents/content.hpf",
                r#"
                <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
                  <opf:manifest>
                    <opf:item id="image1" href="../BinData/image1.png" media-type="image/png"/>
                    <opf:item id="section0" href="section0.xml" media-type="application/xml"/>
                  </opf:manifest>
                  <opf:spine><opf:itemref idREF="section0"/></opf:spine>
                </opf:package>
                "#,
            ),
            (
                "Contents/section0.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"
                        xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core">
                  <hp:p>
                    <hp:ctrl>
                      <hp:pic>
                        <hp:img><hc:img binaryItemIDRef="image1"/></hp:img>
                      </hp:pic>
                    </hp:ctrl>
                  </hp:p>
                </hs:sec>
                "#,
            ),
            ("BinData/image1.png", "relative-png-bytes"),
        ])?;

        let document = read_section_document_from_archive(&bytes)?;

        let image = match &document.sections[0].blocks[0] {
            Block::Image(image) => image,
            other => panic!("expected image block, got {other:?}"),
        };
        assert_eq!(image.resource_id.as_str(), "image1");
        match document.resources.get(&ResourceId("image1".to_string())) {
            Some(Resource::Image(resource)) => {
                assert_eq!(resource.bytes, b"relative-png-bytes");
            }
            other => panic!("expected image resource, got {other:?}"),
        }

        Ok(())
    }

    #[test]
    fn recovers_hwpx_image_resource_from_case_variant_archive_entry() -> Result<(), Box<dyn Error>>
    {
        let bytes = create_archive_bytes(&[
            (
                "Contents/content.hpf",
                r#"
                <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
                  <opf:manifest>
                    <opf:item id="image1" href="bindata/IMAGE1.PNG"/>
                    <opf:item id="section0" href="section0.xml" media-type="application/xml"/>
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
                    <hp:ctrl>
                      <hp:pic>
                        <hp:img><hc:img binaryItemIDRef="image1"/></hp:img>
                      </hp:pic>
                    </hp:ctrl>
                  </hp:p>
                </hs:sec>
                "#,
            ),
            ("BinData/Image1.PNG", "case-png-bytes"),
        ])?;

        let document = read_section_document_from_archive(&bytes)?;

        match document.resources.get(&ResourceId("image1".to_string())) {
            Some(Resource::Image(resource)) => {
                assert_eq!(resource.extension.as_deref(), Some("png"));
                assert_eq!(resource.bytes, b"case-png-bytes");
            }
            other => panic!("expected image resource, got {other:?}"),
        }

        Ok(())
    }

    #[test]
    fn recovers_hwpx_image_resource_from_manifest_attribute_aliases() -> Result<(), Box<dyn Error>>
    {
        let bytes = create_archive_bytes(&[
            (
                "Contents/content.hpf",
                r#"
                <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
                  <opf:manifest>
                    <opf:item itemId="image1" full-path="BinData/image1.png" mediaType="image/png"/>
                    <opf:item itemId="section0" full-path="section0.xml" mediaType="application/xml"/>
                  </opf:manifest>
                  <opf:spine><opf:itemref itemIdRef="section0"/></opf:spine>
                </opf:package>
                "#,
            ),
            (
                "Contents/section0.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"
                        xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core">
                  <hp:p>
                    <hp:ctrl>
                      <hp:pic>
                        <hp:img><hc:img binaryItemIDRef="image1"/></hp:img>
                      </hp:pic>
                    </hp:ctrl>
                  </hp:p>
                </hs:sec>
                "#,
            ),
            ("BinData/image1.png", "alias-png-bytes"),
        ])?;

        let document = read_section_document_from_archive(&bytes)?;

        match document.resources.get(&ResourceId("image1".to_string())) {
            Some(Resource::Image(resource)) => {
                assert_eq!(resource.media_type.as_deref(), Some("image/png"));
                assert_eq!(resource.bytes, b"alias-png-bytes");
            }
            other => panic!("expected image resource, got {other:?}"),
        }

        Ok(())
    }

    #[test]
    fn recovers_hwpx_image_resource_from_case_variant_content_hpf() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[
            (
                "contents/CONTENT.HPF",
                r#"
                <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
                  <opf:manifest>
                    <opf:item id="image1" href="BinData/image1.png" media-type="image/png"/>
                    <opf:item id="section0" href="section0.xml" media-type="application/xml"/>
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
                    <hp:ctrl>
                      <hp:pic>
                        <hp:img><hc:img binaryItemIDRef="image1"/></hp:img>
                      </hp:pic>
                    </hp:ctrl>
                  </hp:p>
                </hs:sec>
                "#,
            ),
            ("BinData/image1.png", "case-content-png-bytes"),
        ])?;

        let document = read_section_document_from_archive(&bytes)?;

        assert!(matches!(
            document.resources.get(&ResourceId("image1".to_string())),
            Some(Resource::Image(resource)) if resource.bytes == b"case-content-png-bytes"
        ));

        Ok(())
    }

    #[test]
    fn recognizes_hwpx_manifest_media_type_parameters() {
        assert!(is_hwpx_section_manifest_item(
            "Contents/section0.xml",
            Some("text/xml; charset=utf-8")
        ));
        assert!(is_hwpx_section_manifest_item(
            "Contents/Section1.XML",
            Some("APPLICATION/XML")
        ));
        assert!(is_hwpx_image_manifest_item(
            "Media/image.bin",
            Some("IMAGE/PNG; charset=binary")
        ));
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
    fn recovers_case_variant_section_xml_archive_entries() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[(
            "Contents/Section0.XML",
            r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p><hp:run><hp:t>case variant section</hp:t></hp:run></hp:p>
                </hs:sec>
                "#,
        )])?;

        let document = read_section_document_from_archive(&bytes)?;

        assert_eq!(
            section_first_paragraph_text(&document.sections[0]),
            Some("case variant section".to_string())
        );

        Ok(())
    }

    #[test]
    fn uses_content_hpf_spine_order_for_hwpx_sections() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[
            (
                "Contents/content.hpf",
                r#"
                <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
                  <opf:manifest>
                    <opf:item id="section0" href="section0.xml" media-type="application/xml; charset=utf-8"/>
                    <opf:item id="section1" href="section1.xml" media-type="TEXT/XML"/>
                  </opf:manifest>
                  <opf:spine>
                    <opf:itemref idref="section1"/>
                    <opf:itemref idref="section0"/>
                  </opf:spine>
                </opf:package>
                "#,
            ),
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
            Some("second section".to_string())
        );
        assert_eq!(
            section_first_paragraph_text(&document.sections[1]),
            Some("first section".to_string())
        );

        Ok(())
    }

    #[test]
    fn uses_content_hpf_attribute_aliases_for_hwpx_sections() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[
            (
                "Contents/content.hpf",
                r#"
                <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
                  <opf:manifest>
                    <opf:item itemID="section0" full-path="section0.xml" mediaType="application/xml"/>
                  </opf:manifest>
                  <opf:spine><opf:itemref itemIDRef="section0"/></opf:spine>
                </opf:package>
                "#,
            ),
            (
                "Contents/section0.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p><hp:run><hp:t>alias section</hp:t></hp:run></hp:p>
                </hs:sec>
                "#,
            ),
        ])?;

        let document = read_section_document_from_archive(&bytes)?;

        assert_eq!(
            section_first_paragraph_text(&document.sections[0]),
            Some("alias section".to_string())
        );

        Ok(())
    }

    #[test]
    fn uses_case_variant_content_hpf_for_hwpx_section_order() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[
            (
                "contents/CONTENT.HPF",
                r#"
                <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
                  <opf:manifest>
                    <opf:item id="section0" href="section0.xml" media-type="application/xml"/>
                    <opf:item id="section1" href="section1.xml" media-type="application/xml"/>
                  </opf:manifest>
                  <opf:spine>
                    <opf:itemref idref="section1"/>
                    <opf:itemref idref="section0"/>
                  </opf:spine>
                </opf:package>
                "#,
            ),
            (
                "Contents/section0.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p><hp:run><hp:t>first in archive</hp:t></hp:run></hp:p>
                </hs:sec>
                "#,
            ),
            (
                "Contents/section1.xml",
                r#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p><hp:run><hp:t>first in spine</hp:t></hp:run></hp:p>
                </hs:sec>
                "#,
            ),
        ])?;

        let document = read_section_document_from_archive(&bytes)?;

        assert_eq!(
            section_first_paragraph_text(&document.sections[0]),
            Some("first in spine".to_string())
        );
        assert_eq!(
            section_first_paragraph_text(&document.sections[1]),
            Some("first in archive".to_string())
        );

        Ok(())
    }

    #[test]
    fn text_fallback_uses_content_hpf_spine_order() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_bytes(&[
            (
                "Contents/content.hpf",
                r#"
                <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
                  <opf:manifest>
                    <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
                    <opf:item id="section1" href="Contents/section1.xml" media-type="application/xml"/>
                  </opf:manifest>
                  <opf:spine>
                    <opf:itemref idref="section1"/>
                    <opf:itemref idref="section0"/>
                  </opf:spine>
                </opf:package>
                "#,
            ),
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

        let fallback = read_text_fallback_from_archive(&bytes)?;

        assert_eq!(fallback.source, HwpxTextFallbackSource::SectionXml);
        assert_eq!(
            fallback.paragraphs,
            vec!["second section".to_string(), "first section".to_string()]
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
                      <hp:header pageType="odd">
                        <hp:subList>
                          <hp:p><hp:run><hp:t>header text</hp:t></hp:run></hp:p>
                        </hp:subList>
                      </hp:header>
                    </hp:ctrl>
                    <hp:ctrl>
                      <hp:footer applyTo="even">
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
                      <hp:endNote id="4">
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
        assert!(
            document
                .warnings
                .iter()
                .any(|warning| { warning.message.contains("duplicate id `footnote-3`") })
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
    fn falls_back_to_case_variant_preview_archive_entry_for_hwpx_parse_failure()
    -> Result<(), Box<dyn Error>> {
        let path = temp_fixture_path("preview-fallback-case", "hwpx");
        let bytes = create_archive_bytes(&[("preview/PRVTEXT.TXT", "first line\r\nsecond line")])?;
        fs::write(&path, bytes)?;

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

    #[test]
    fn rejects_invalid_hwpx_header_xml_in_document_fallback() -> Result<(), Box<dyn Error>> {
        let bytes = create_archive_binary_bytes(&[
            (HEADER_XML_PATH, &[0xff, 0xfe]),
            (
                "Contents/section0.xml",
                br#"
                <hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                  <hp:p><hp:run><hp:t>body</hp:t></hp:run></hp:p>
                </hs:sec>
                "#,
            ),
        ])?;

        let error = read_section_document_from_archive(&bytes).expect_err("header should fail");
        let message = error.to_string();

        assert!(message.contains(HEADER_XML_PATH));
        assert!(message.contains("UTF-8"));

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

    fn create_archive_binary_bytes(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, Box<dyn Error>> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);

        for (path, content) in entries {
            writer.start_file(*path, SimpleFileOptions::default())?;
            writer.write_all(content)?;
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
