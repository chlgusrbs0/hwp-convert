# 현재 지원 상태 (Status)

이 문서는 현재 코드 기준의 bridge/exporter 지원 상태와 HWPX fixture 현황을 한곳에 정리한다. 새 기능을 지원한다고 말하기 전에 반드시 이 문서를 확인하고, 코드가 바뀌면 함께 갱신한다.

기준값(crate 버전, rHWP pin, IR_VERSION, 테스트 수, fixture 목록)은 `AGENTS.md`의 "현재 프로젝트 사실" 블록을 따른다. 최종 검증: 2026-07-11.

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
| paragraph | 예 | 부분 | 예 | 부분 (heading/title/caption 구분 제한적) | 빈 문단, 정렬·간격·들여쓰기, 문단 테두리·배경·안쪽 여백, 외톨이줄 보호·다음 문단과 함께·분할 금지·문단 앞 쪽 나눔을 보존한다. HWP 사용자 정의 탭의 위치·정렬·리더 값과 자동 탭 플래그를 IR에 보존하고 HTML은 탭 문자를 축약되지 않는 span으로 출력하지만 정확한 탭 위치는 근사한다. outline 기반 heading은 일부 매핑되지만 title/caption 등 전체 role 추론은 아직 제한적. |
| style | 부분 | 부분 | 부분 | 부분 (JSON 보존, HTML CSS, Markdown은 일부 장식, TXT/SVG 시각 스타일 소실) | 글자 장식과 밑줄·취소선의 위치/선 종류, 글꼴명/크기, 전경/배경색, 장평·자간·상대 크기·기준선 위치·커닝과 고정/퍼센트 줄 간격을 매핑한다. HWP 혼합 문자권은 실행 구간을 나눠 문자권별 값을 보존한다. HWPX 폴백은 균일한 문자권 메트릭과 문단 border/breakSetting을 복구한다. 서로 다른 밑줄·취소선 모양을 동시에 쓰는 HTML, table style ref, 전체 paragraph role 추론은 아직 제한적이다. |
| table | 예 | 예 | 예 | 부분 (JSON/HTML 구조 유지, 반복 머리글은 `<thead>`, 헤더셀은 `<th>`, 셀 수직정렬 CSS, TXT/SVG 평문, Markdown 단순 표만) | 셀 `is_header`, 수직정렬, 폭/높이/padding, 4면 테두리, 표 전체 너비·높이/바깥 여백, 행 높이, 셀 간격, HWP/HWPX 반복 머리글과 페이지 분할 규칙은 매핑됨. HWP 표의 글자처럼 취급·wrap·가로/세로 기준·정렬·오프셋·Z-order·개체 여백·쪽 나눔 방지도 `ObjectPlacement`로 보존한다. HWPX 폴백은 borderFill, 셀 margin, 표 `sz/outMargin`, 행 높이와 `cellSpacing/repeatHeader/pageBreak`를 복구한다. borderFill zone은 아직 경고 후 생략한다. 페이지 좌표 기반 표 배치는 semantic exporter에서 선형화한다. 테두리 굵기와 wave/3D 선종류는 근사이므로 실문서 fixture 검증이 필요하다. |
| merged table cell | 예 | 예 | 예 | 부분 (JSON/HTML `row_span`/`col_span`, Markdown fallback, TXT/SVG 평문) | 병합 셀 시각 배치/너비 계산 없음. Markdown 병합 표현 없음. |
| image | 예 | 부분 | 예 | 부분 (JSON Base64 bytes 포함, HTML/Markdown asset 파일, TXT/SVG 대체 텍스트) | 이미지 테두리(색·굵기, 선종류는 solid 가정), 회색조·임계 흑백·Pattern8x8 효과 구분, 회전·가로/세로 반전, 표시·원본·현재 변형 크기, 내부 padding, 캡션 배치, crop 좌표, 밝기·대비 원시 값, 글자처럼 취급·wrap·가로/세로 기준·정렬·오프셋·Z-order·바깥 여백은 IR에 매핑됨. HWPX `flowWithText`·`allowOverlap`과 HWP 쪽 나눔 방지도 보존한다. 원본·표시 크기가 명확한 HWP/HWPX crop과 HWPX 이미지 opacity는 HTML에 적용한다. 임계 흑백은 회색조로 근사하고 Pattern8x8·밝기·대비는 warning과 원본 바이트를 유지한다. 페이지 좌표 기반 본문 배치는 semantic exporter에서 선형화한다. HWPX 폴백은 이미지 참조/설명/테두리/배치/crop/`inMargin`·`outMargin` 관련 속성 alias 일부를 복구한다. bin data 없으면 `UnknownBlock`으로 남기며 alt/description 계열 속성은 fallback text로 보존한다. `Resource::Binary`는 그림 참조에 사용하지 않는다. |
| resource | 예 | 부분 | 부분 | 부분 (JSON store 보존, HTML/Markdown image·binary 파일 출력) | HWP에서 이미지가 참조한 BinData는 `ImageResource`, 나머지는 `Link`·`Embedded`·`Storage` 종류와 외부 절대·상대 경로를 포함한 `BinaryResource`로 보존한다. 로드된 미참조 바이트와 스토리지 누락 metadata도 버리지 않는다. HWPX manifest의 embedded·external binary resource와 누락 entry metadata를 IR에 보존하며, 참조된 이미지는 `ImageResource`로 승격한다. HTML/Markdown은 embedded·storage binary bytes를 별도 asset 파일로 쓰고 외부 link는 경로 metadata만 유지한다. |
| header/footer | 예 | 예 | 예 | 부분 (모두 선형화 출력) | 페이지 반복 레이아웃이 아니라 본문 앞뒤 block 묶음. HWPX 폴백은 `FirstPage`/odd/even placement와 관련 속성 alias 일부를 복구한다. |
| footnote/endnote | 예 | 부분 | 예 | 부분 (note ref + body 출력) | paragraph offset으로 위치를 증명할 수 있으면 note ref를 해당 위치에 배치하고, 복구 불가할 때만 문단 끝에 append하며 warning을 남긴다. 페이지 하단 배치/separator 없음. |
| link | 부분 | 부분 | 예 | 부분 (JSON/HTML/Markdown URL 보존, TXT/SVG 라벨 fallback) | hyperlink field range와 복구 가능한 control offset을 inline 위치로 사용한다. 위치가 없으면 유일한 라벨 일치 또는 문단 끝 fallback과 warning을 사용한다. HWPX 폴백은 직접 link/field link의 URL, title, parameter 이름 alias 일부를 복구한다. |
| list | 부분 | 부분 | 예 | 부분 (JSON/TXT/Markdown prefix, HTML `<ul>/<ol>`, SVG 평문) | bullet/number/outline만 `ListInfo`로. HWPX 폴백은 list type/level/idRef, bullet marker와 numbering의 레벨별 시작값/숫자 형식을 복구하며, 동일 문단으로 확인된 rHWP 결과의 빈 marker를 보강한다. explicit list container 구조 없음. nested/restart fixture 없음. |
| equation | 예 | 부분 | 예 | 부분 (JSON 보존, HTML 표시 스타일, 나머지 `[equation: ...]`, Markdown은 `Latex`일 때만 `$$`) | bridge가 `EquationKind::PlainText`를 생성하며 rHWP의 글꼴·크기·색·기준선·크기·오프셋·버전을 보존한다. LaTeX/MathML 판별, numbering, resource 연결 없음. |
| shape | 예 | 부분 | 부분 | 부분 (모두 `[shape: ...]` placeholder) | `kind`, `fallback_text`, `description`과 HWP 도형의 기본 너비·높이·X/Y 오프셋, 회전·반전, 표준 테두리와 패턴 없는 단색 채우기, 텍스트 상자 안쪽 여백·세로 정렬을 보존한다. HWP 그룹 자식은 순차 블록으로 펼치며 그룹 caption은 방향에 따라 인접 caption 문단으로 보존한다. HWPX도 `sz`/`pos` 크기·오프셋, 회전·반전, `lineShape` 테두리, `fillBrush` 단색 채우기와 `drawText` 텍스트 상자 스타일을 복구한다. pattern/image/gradient fill, shadow와 그룹 layout은 제한적이다. |
| chart | 부분 | 아니오 | 예 | 부분 (exporter는 `[chart: ...]` 가능하나 bridge가 block을 못 만듦) | 로컬 rhwp에 chart tag 흔적은 있으나 bridge-visible model 없음. 현재 경로에서 직접 매핑 불가. |
| unknown element | 부분 | 부분 | 예 | 부분 (`fallback_text` 우선, 없으면 `[unknown: kind]`) | `Control::Unknown`은 `UnknownBlock`으로 감싼다. 일부 known-but-unmapped control은 아직 구조적 보존이 제한적이다. HWPX unsupported control/object는 내부 텍스트가 없을 때도 title/name/description/value 계열 속성을 fallback text로 보존한다. `UnknownInline`은 거의 미사용. |
| render snapshot | 예 | — | — | — (기본 `--to svg`는 RenderSnapshot이 아님) | experimental visual path (`src/render`). 기본 사용자 경로에 노출 안 됨. fidelity 낮고 이미지/표/도형은 placeholder. |

### 핵심 관찰

1. 가장 안정적인 경로: `text -> paragraph -> simple table/list/link -> JSON/HTML/Markdown/TXT/SVG`.
2. 이미지/resource는 IR까지 들어오며, HTML/Markdown exporter는 `Resource::Image` bytes를 출력 파일 stem 기준 `<stem>_assets/images/`에 저장하고 `<stem>_assets/images/...`로 참조한다. embedded·storage `Resource::Binary` bytes는 `<stem>_assets/files/`에 저장한다. 예: `out/sample.html`/`out/sample.md`는 `out/sample_assets/images/image-1.png`와 `out/sample_assets/files/attachment.bin`을 쓴다. TXT/SVG와 RenderSnapshot path의 asset 처리는 별도다.
3. chart는 bridge 기준 사실상 미지원이다.
4. unknown element 처리는 제한적이다. 모든 unsupported 정보가 구조적으로 보존되지는 않는다.

### 미지원 control warning 동작

`src/bridge/rhwp.rs`는 parser가 노출하지만 아직 완전히 매핑하지 못한 known control에 대해 `ConversionWarning`을 기록한다. 현재 대상: auto number, new number, page number position, page hide, hidden comment, non-hyperlink fields, form objects. 이름 있는 bookmark는 `Anchor` inline으로 보존하고, 복구 가능한 command string이 있는 non-hyperlink field는 `UnknownInline` fallback text로 남긴다. 자동번호·쪽번호 fallback에는 형식과 장식 문자를, ruby·글자겹침 fallback에는 정렬·테두리·크기·글자속성 참조를 함께 남긴다. 복구 가능한 텍스트가 있는 visible unsupported control과 paragraph 내용이 있는 hidden comment는 `UnknownBlock` fallback text로 남긴다.

### HTML list 렌더링

HTML export는 연속 list 문단을 semantic `<ul>`/`<ol>`로 묶고, ordered list 번호를 `<li value="...">`로 쓴다. HWP `^1`~`^7` 템플릿은 실제 다단계 표식으로 계산해 `data-marker`와 CSS `::marker`로 표시하며 원본 템플릿도 `data-marker-format`에 남긴다. nested list fidelity는 IR이 list 메타데이터를 문단 단위로 저장하므로 아직 제한적이다.

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

rHWP 파싱이 실패하거나 HWPX를 빈 semantic document로 매핑하면, hwp-convert는 `Preview/PrvText.txt`로 떨어지기 전에 구조적 `Contents/section*.xml` 폴백을 시도한다. 이 section XML 폴백은 현재 문단 텍스트, inline line break/tab, sections, 표(셀 크기·여백·헤더·수직정렬·borderFill 기반 배경/테두리 포함), caption, image resource, list 메타데이터, link, field/bookmark, header/footer, note, equation, shape, chart, unsupported-control/object placeholder, 일부 basic style을 복구한다. 여러 HWPX 생성기에서 달라지는 주요 속성 alias도 일부 허용하고, unsupported fallback은 텍스트가 없을 때 일부 설명 속성을 대체 텍스트로 남긴다. preview text 폴백은 평문만 복구한다.

> 주의 (정직성): 이 폴백 파서의 *복구 능력*은 위처럼 넓지만, 그것이 곧 HWPX *지원*을 뜻하지 않는다. 위 "채택된 paired fixture"가 보여주듯 회귀 테스트로 parity가 검증된 HWPX 요소는 아직 `basic_text`, `list`뿐이다. 폴백이 복구한다고 fixture 없이 "지원"이라 쓰지 않는다.

## 지속가능성 리스크 (sustainability notes)

정직하게 추적해 둘 구조적 리스크:

- **`src/hwpx.rs`가 자작 HWPX 파서로 커지는 중.** 현재 `src` 전체의 약 1/3 규모이며, 정규 XML 파서가 아니라 손으로 만든 문자열 스캐너다. 최근 다수의 `fix(hwpx):` 커밋이 이 스캐너의 엣지케이스(DOCTYPE, CDATA, self-closing, attribute alias 등) 대응이다. 프로젝트 원칙인 "rHWP를 다시 만들지 않는다"와 긴장 관계에 있다. 향후 결정 필요: (a) 계속 확장, (b) 검증된 XML 크레이트(`quick-xml` 등) 기반 리팩터링, (c) rHWP upstream의 HWPX 지원 개선을 기다리며 동결. 자세한 논의는 `docs/ROADMAP.md`.
- **실제 문서 fixture corpus 부재.** 현재 fixture는 대부분 합성/단일 기능이며 HWPX 쌍은 2개뿐이다. 변환 정확도를 입증할 실문서가 없어 "쓸만한 변환기" 주장은 아직 불가하다. 이것이 최대 병목이다 (`docs/ROADMAP.md` 완료 기준 참고).

## 우선순위

- P0: `basic_text`, `style`, `table`, `merged_table`, `image`
- P1: `link_list`, `note_header_footer`
- P2: `equation_shape_chart`, `kitchen_sink`
