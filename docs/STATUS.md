# 현재 지원 상태 (Status)

이 문서는 현재 코드 기준의 bridge/exporter 지원 상태와 HWPX fixture 현황을 한곳에 정리한다. 새 기능을 지원한다고 말하기 전에 반드시 이 문서를 확인하고, 코드가 바뀌면 함께 갱신한다.

기준값(crate 버전, rHWP pin, IR_VERSION, 테스트 수, fixture 목록)은 `AGENTS.md`의 "현재 프로젝트 사실" 블록을 따른다. 최종 검증: 2026-06-16.

근거 코드:

- `src/bridge/rhwp.rs` — rHWP 파싱 결과를 Document IR로 매핑
- `src/hwpx.rs` — HWPX section XML 폴백 파서
- `src/exporter.rs` — Document IR를 출력 형식으로 변환
- `src/util/plain_text.rs` — 평문 fallback
- 로컬 dependency `rhwp` `src/model/control.rs`

판정 기준:

- `예`: 현재 코드 경로에 구현이 있고 기본 동작이 확인된다.
- `부분`: 구현은 있지만 구조/위치 손실, 형식별 fallback, 누락된 메타데이터가 있다.
- `아니오`: 현재 코드 경로에 구현이 없다.

## 지원 행렬

| 요소 | rhwp parse | bridge mapping | Document IR | exporter 지원 | 현재 한계 |
| --- | --- | --- | --- | --- | --- |
| text | 예 | 예 | 예 | 예 (TXT/JSON/HTML/Markdown/SVG 모두) | 좌표/페이지 단위 정보 없음. unsupported control 내부 텍스트는 보존 안 될 수 있음. |
| paragraph | 예 | 부분 | 예 | 부분 (heading/title/caption 구분 없음) | bridge가 빈 문단을 drop하고 `ParagraphRole`을 항상 `Body`로 둠. |
| style | 부분 | 부분 | 부분 | 부분 (JSON 보존, HTML CSS, Markdown은 bold/italic/strike/sup/sub/link, TXT/SVG 소실) | 글자 장식은 굵기/기울임/밑줄/취소선/위·아래첨자/강조점/양각/음각/외곽선/그림자, 글꼴명/크기, 전경/배경색, 밑줄·취소선 색까지 매핑. 밑줄 모양, 장평/자간/커닝, table style ref, border, padding, percent line spacing, paragraph role 추론은 아직 없음. |
| table | 예 | 예 | 예 | 부분 (JSON/HTML 구조 유지, 헤더셀은 `<th>`, 셀 수직정렬 CSS, TXT/SVG 평문, Markdown 단순 표만) | 셀 `is_header`, 수직정렬, 폭/높이/padding, 4면 테두리(색·선종류·굵기)는 매핑됨. 표 전체 폭은 아직. 테두리 굵기 인덱스→px는 rhwp 임계값(0–7)+표준 HWP 표(8–15), 선종류 wave/3D는 solid로 근사(실문서 fixture로 검증 필요). |
| merged table cell | 예 | 예 | 예 | 부분 (JSON/HTML `row_span`/`col_span`, Markdown fallback, TXT/SVG 평문) | 병합 셀 시각 배치/너비 계산 없음. Markdown 병합 표현 없음. |
| image | 예 | 부분 | 예 | 부분 (JSON bytes 포함, HTML/Markdown asset 파일, TXT/SVG 대체 텍스트) | 배치/wrap/crop/anchor 없음. bin data 없으면 `UnknownBlock`. `Resource::Binary`는 asset으로 안 씀. |
| resource | 부분 | 부분 | 부분 | 부분 (JSON store 보존, HTML/Markdown `Resource::Image`만 파일로) | 현재 image bin data만 `ImageResource`로. `BinaryResource` 미사용. |
| header/footer | 예 | 예 | 예 | 부분 (모두 선형화 출력) | 페이지 반복 레이아웃이 아니라 본문 앞뒤 block 묶음. `FirstPage` 미생성. |
| footnote/endnote | 예 | 부분 | 예 | 부분 (note ref + body 출력) | rHWP가 정확한 inline 위치를 안 줘 note ref가 문단 끝에 append. 페이지 하단 배치/separator 없음. |
| link | 부분 | 부분 | 예 | 부분 (JSON/HTML/Markdown URL 보존, TXT/SVG 라벨 fallback) | hyperlink field range는 inline으로, 일부 hyperlink control은 문단 끝 append. `title` 미설정. |
| list | 부분 | 부분 | 예 | 부분 (JSON/TXT/Markdown prefix, HTML `<ul>/<ol>`, SVG 평문) | bullet/number/outline만 `ListInfo`로. explicit list container 구조 없음. nested/restart fixture 없음. |
| equation | 예 | 부분 | 예 | 부분 (JSON 보존, 나머지 `[equation: ...]`, Markdown은 `Latex`일 때만 `$$`) | bridge가 `EquationKind::PlainText`만 생성. LaTeX/MathML 판별, numbering, resource 연결 없음. |
| shape | 예 | 부분 | 부분 | 부분 (모두 `[shape: ...]` placeholder) | `kind`, `fallback_text`, `description`만 남김. geometry/border/fill/text box/caption/child shape 소실. |
| chart | 부분 | 아니오 | 예 | 부분 (exporter는 `[chart: ...]` 가능하나 bridge가 block을 못 만듦) | 로컬 rhwp에 chart tag 흔적은 있으나 bridge-visible model 없음. 현재 경로에서 직접 매핑 불가. |
| unknown element | 부분 | 부분 | 예 | 부분 (`fallback_text` 우선, 없으면 `[unknown: kind]`) | `Control::Unknown`은 `UnknownBlock`으로 감싸지만 일부 known-but-unmapped control은 drop. `UnknownInline`은 거의 미사용. |
| render snapshot | 예 | — | — | — (기본 `--to svg`는 RenderSnapshot이 아님) | experimental visual path (`src/render`). 기본 사용자 경로에 노출 안 됨. fidelity 낮고 이미지/표/도형은 placeholder. |

### 핵심 관찰

1. 가장 안정적인 경로: `text -> paragraph -> simple table/list/link -> JSON/HTML/Markdown/TXT/SVG`.
2. 이미지/resource는 IR까지 들어오며, HTML/Markdown exporter는 `Resource::Image` bytes를 출력 파일 stem 기준 `<stem>_assets/images/`에 저장하고 `<stem>_assets/images/...`로 참조한다. 예: `out/sample.html`/`out/sample.md`는 `out/sample_assets/images/image-1.png`를 쓴다. TXT/SVG와 RenderSnapshot path의 asset 처리는 별도다.
3. chart는 bridge 기준 사실상 미지원이다.
4. unknown element 처리는 제한적이다. 모든 unsupported 정보가 구조적으로 보존되지는 않는다.

### 미지원 control warning 동작

`src/bridge/rhwp.rs`는 parser가 노출하지만 아직 완전히 매핑하지 못한 known control에 대해 `ConversionWarning`을 기록한다. 현재 대상: auto number, new number, page number position, page hide, hidden comment, non-hyperlink fields, form objects. 이름 있는 bookmark는 `Anchor` inline으로 보존하고, 복구 가능한 command string이 있는 non-hyperlink field는 `UnknownInline` fallback text로 남긴다. 복구 가능한 텍스트가 있는 visible unsupported control(ruby, character overlap)과 paragraph 내용이 있는 hidden comment는 `UnknownBlock` fallback text로 남긴다.

### HTML list 렌더링

HTML export는 연속 list 문단을 semantic `<ul>`/`<ol>`로 묶고, ordered list 번호를 `<li value="...">`로 쓴다. nested list fidelity는 IR이 list 메타데이터를 문단 단위로 저장하므로 아직 제한적이다.

## HWPX fixture 현황

HWPX paired fixture는 매칭되는 HWP fixture와 같은 feature-level assertion을 통과할 때만 추가한다. parse만 된다고 받지 않는다. 다음을 모두 통과해야 한다: `bridge::rhwp::read_document` → `Document IR` → fixture feature assertions → exporter smoke → bridge stats.

### 채택된 paired fixture

| Fixture | HWP | HWPX | 비고 |
| --- | --- | --- | --- |
| `basic_text` | yes | yes | 문단 텍스트, line break, tab, styled run, bridge stats 보존. |
| `list` | yes | yes | list 문단 메타데이터와 읽기 순서 보존. bullet glyph 정확도는 assertion 대상 아님. |

### 거부된 synthetic HWPX 시도

기존 HWP fixture를 rHWP parse + HWPX serialization으로 만든 HWPX 파일들. 매칭 HWP fixture assertion을 통과하지 못해 제거함 (현재 rHWP pin 기준 관찰).

| Fixture | 관찰된 실패 | 해석 |
| --- | --- | --- |
| `table` | 빈 semantic content. `Contents/section0.xml`에 표 없음, preview 비어 있음. | synthetic 경로가 표 control을 충분히 직렬화하지 못함. |
| `style` | 텍스트는 살아남았으나 paragraph spacing 손실. HWPX XML엔 margin 값 존재. | rHWP HWPX parser/model이 bridge가 쓰는 paragraph style 데이터를 다 노출하지 않음. |
| `equation` | 빈 semantic content + preview fallback warning. | equation block coverage에 사용 불가. |
| `shape` | 빈 semantic content + preview fallback warning. | shape block coverage에 사용 불가. |
| `footnote` | 본문은 parse되나 footnote store 미보존. | note coverage에 사용 불가. |
| `header_footer` | 빈 semantic content + preview fallback warning. | header/footer coverage에 사용 불가. |
| `image` | 빈 semantic content + preview fallback warning. | image/resource coverage에 사용 불가. |

이는 HWP 지원과 HWPX 지원을 동일하다고 설명하면 안 된다는 뜻이다. 특정 요소에 passing HWPX fixture 또는 별도 test가 있을 때만 HWPX 지원을 주장한다.

### HWPX 폴백 파서 (`src/hwpx.rs`)

rHWP 파싱이 실패하거나 HWPX를 빈 semantic document로 매핑하면, hwp-convert는 `Preview/PrvText.txt`로 떨어지기 전에 구조적 `Contents/section*.xml` 폴백을 시도한다. 이 section XML 폴백은 현재 문단 텍스트, inline line break/tab, sections, 표, caption, image resource, list 메타데이터, link, field/bookmark, header/footer, note, equation, shape, chart, unsupported-control placeholder, 일부 basic style을 복구한다. preview text 폴백은 평문만 복구한다.

> 주의 (정직성): 이 폴백 파서의 *복구 능력*은 위처럼 넓지만, 그것이 곧 HWPX *지원*을 뜻하지 않는다. 위 "채택된 paired fixture"가 보여주듯 회귀 테스트로 parity가 검증된 HWPX 요소는 아직 `basic_text`, `list`뿐이다. 폴백이 복구한다고 fixture 없이 "지원"이라 쓰지 않는다.

## 지속가능성 리스크 (sustainability notes)

정직하게 추적해 둘 구조적 리스크:

- **`src/hwpx.rs`가 자작 HWPX 파서로 커지는 중.** 현재 `src` 전체의 약 1/3 규모이며, 정규 XML 파서가 아니라 손으로 만든 문자열 스캐너다. 최근 다수의 `fix(hwpx):` 커밋이 이 스캐너의 엣지케이스(DOCTYPE, CDATA, self-closing, attribute alias 등) 대응이다. 프로젝트 원칙인 "rHWP를 다시 만들지 않는다"와 긴장 관계에 있다. 향후 결정 필요: (a) 계속 확장, (b) 검증된 XML 크레이트(`quick-xml` 등) 기반 리팩터링, (c) rHWP upstream의 HWPX 지원 개선을 기다리며 동결. 자세한 논의는 `docs/ROADMAP.md`.
- **실제 문서 fixture corpus 부재.** 현재 fixture는 대부분 합성/단일 기능이며 HWPX 쌍은 2개뿐이다. 변환 정확도를 입증할 실문서가 없어 "쓸만한 변환기" 주장은 아직 불가하다. 이것이 최대 병목이다 (`docs/ROADMAP.md` 완료 기준 참고).

## 우선순위

- P0: `basic_text`, `style`, `table`, `merged_table`, `image`
- P1: `link_list`, `note_header_footer`
- P2: `equation_shape_chart`, `kitchen_sink`
