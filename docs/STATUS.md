# 현재 지원 상태 (Status)

이 문서는 현재 코드 기준의 bridge/exporter 지원 상태와 HWPX fixture 현황을 한곳에 정리한다. 새 기능을 지원한다고 말하기 전에 반드시 이 문서를 확인하고, 코드가 바뀌면 함께 갱신한다.

기준값(crate 버전, rHWP pin, IR_VERSION, 테스트 수, fixture 목록)은 `AGENTS.md`의 "현재 프로젝트 사실" 블록을 따른다. 최종 검증: 2026-07-18.

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
| section/page layout | 예 | 부분 | 예 | JSON 보존, 나머지는 선형화 | HWP 구역의 용지 크기·방향·제본, 모든 여백, 번호 시작값, 기본 탭/단 간격, 숨김 플래그, 각주·미주 모양, 페이지 테두리 참조와 불투명 확장 레코드를 보존한다. 문단 중간의 다단 전환은 순서가 있는 `ColumnLayout` 블록으로 너비·간격·방향·구분선 원시 값을 보존한다. rHWP 공개 `MasterPage`의 적용 대상·확장/겹침 플래그·텍스트 영역·참조 마스크·원시 list header와 내부 문단 블록도 HWP에서 보존한다. semantic exporter는 다단과 바탕쪽 내용을 선형화하며 반복 페이지 배경 배치는 재현하지 않는다. 바탕쪽은 synthetic unit test만 있고 실제 fixture 검증은 아직 없으며, 동결된 HWPX 폴백에는 새 복구를 추가하지 않았다. HWPX 구역 레이아웃 복구는 미구현이다. |
| page/number control | 예 | 예 | 예 | JSON 구조 보존, 나머지는 fallback text | HWP 자동 번호·새 번호 시작·쪽 번호 위치·현재 쪽 숨김 플래그를 순서가 있는 `DocumentControl` 블록으로 보존한다. semantic exporter는 fallback text를 유지하지만 실제 페이지 위치와 반복 동작은 재현하지 않는다. |
| style | 부분 | 부분 | 부분 | 부분 (JSON 보존, HTML CSS, Markdown은 일부 장식, TXT/SVG 시각 스타일 소실) | 글자 장식과 밑줄·취소선의 위치/선 종류, 글꼴명/크기, 전경/배경색, 장평·자간·상대 크기·기준선 위치·커닝과 고정/퍼센트 줄 간격을 매핑한다. rHWP 글꼴의 대체 유형·대체 이름·기본 이름도 보존하고 HTML fallback 순서에 반영한다. HWP 글자 그림자의 rHWP 공개 종류·X/Y 비율 오프셋·색상도 구조화하고 HTML은 blur 없이 `em` 오프셋으로 적용하며 원본 값을 `data-*`로 남긴다. 종류별 별도 시각 규칙과 blur는 추정하지 않고, 양각·음각은 기존 일반 그림자로 근사한다. rHWP가 정의한 HWP 강조점 종류 1~6도 원시 타입을 보존하고 HTML에서 ●·○·ˇ·˜·･·: 기호로 구분한다. `CharShape.border_fill_id`의 원본 참조와 4면 테두리·단색/gradient/image 채우기도 구조화하며 HTML은 기존 CSS 채우기 경로와 `data-border-fill-id`로 출력한다. rHWP 원본 스타일의 문단/글자 종류, 한글·영문 이름, 다음 스타일 ID와 원본 shape 참조도 별도 정의 목록으로 보존한다. HWP 혼합 문자권은 실행 구간을 나눠 문자권별 값을 보존하며, 이름 있는 글자 스타일도 한글·영문·한자·일어·기타·기호·사용자별 `TextStyle`을 함께 보존한다. HWPX 폴백은 균일한 문자권 메트릭과 문단 border/breakSetting을 복구하며 새 그림자·강조점·글자 borderFill 상세 복구는 추가하지 않는다. 서로 다른 밑줄·취소선 모양을 동시에 쓰는 HTML, table style ref, 전체 paragraph role 추론은 아직 제한적이다. |
| table | 예 | 예 | 예 | 부분 (JSON/HTML 구조 유지, 반복 머리글은 `<thead>`, 헤더셀은 `<th>`, 셀 수직정렬·글자 방향·채우기 CSS, TXT/SVG 평문, Markdown 단순 표만) | 셀 `is_header`, 원본 행·열 좌표, 수직정렬, 폭/높이/padding, 4면 테두리, 표 전체 너비·높이/바깥 여백, 행 높이, 셀 간격, HWP/HWPX 반복 머리글과 페이지 분할 규칙은 매핑됨. HWP 셀 글자 방향의 가로·세로/영문 눕힘·세로/영문 세움 값은 IR에 보존하고 HTML `writing-mode`·`text-orientation`과 `data-text-direction`으로 출력한다. 이 글자 방향 경로는 synthetic unit test로만 검증됐고 현재 공식 fixture에는 세로쓰기 셀이 없다. rHWP의 원본 `list_header_width_ref`와 query에서 확정된 셀 보호 bit도 IR 및 HTML `data-*`에 보존하지만 정적 출력은 편집 보호를 강제하지 않는다. 셀 보호 경로 역시 synthetic unit test만 있고 현재 공식 fixture에서 보호 셀은 확인되지 않았다. HWP 표 자체와 셀의 `border_fill_id` 원본 참조, 단색·무늬·gradient·image fill 원시 값, 이미지 resource 참조와 4면 테두리를 구조화하며 HTML은 채우기와 테두리를 CSS로 근사한다. HWP BorderFill의 대각선 속성·종류·굵기 인덱스·색상은 표·셀·zone에 보존하고 HTML `data-*`로 남기지만 대각선 자체는 그리지 않는다. HWP borderFill zone의 포함 좌표·원본 참조·채우기·4면 테두리도 구조화하고, HTML은 셀 고유 채우기가 없을 때 zone 채우기를 적용한다. HWP 표의 글자처럼 취급·wrap·가로/세로 기준·정렬·오프셋·Z-order·개체 여백·쪽 나눔 방지도 `ObjectPlacement`로 보존한다. HWPX 폴백은 셀 원본 좌표, borderFill, 셀 margin, 표 `sz/outMargin`, 행 높이와 `cellSpacing/repeatHeader/pageBreak`를 복구하지만 cellzone·글자 방향·보호 속성 및 표 전체 상세 BorderFill은 신규 복구하지 않는다. 페이지 좌표 기반 표 배치는 semantic exporter에서 선형화한다. 무늬·gradient·image fill과 테두리 굵기·wave/3D 선종류는 시각적 근사이므로 실문서 fixture 검증이 필요하다. |
| merged table cell | 예 | 예 | 예 | 부분 (JSON/HTML `row_span`/`col_span`, Markdown fallback, TXT/SVG 평문) | 병합 셀 시각 배치/너비 계산 없음. Markdown 병합 표현 없음. |
| image | 예 | 부분 | 예 | 부분 (JSON Base64 bytes 포함, HTML/Markdown asset 파일, TXT/SVG 대체 텍스트) | 이미지 테두리(색·굵기, 선종류는 solid 가정), 회색조·임계 흑백·Pattern8x8 효과 구분, 회전·가로/세로 반전, 표시·원본·현재 변형 크기, 내부 padding, 캡션 배치, crop 좌표, 밝기·대비 원시 값, 글자처럼 취급·wrap·가로/세로 기준·정렬·오프셋·Z-order·바깥 여백은 IR에 매핑됨. HWPX `flowWithText`·`allowOverlap`과 HWP 쪽 나눔 방지도 보존한다. 원본·표시 크기가 명확한 HWP/HWPX crop과 HWPX 이미지 opacity는 HTML에 적용한다. 임계 흑백은 회색조로 근사하고 Pattern8x8·밝기·대비는 warning과 원본 바이트를 유지한다. 페이지 좌표 기반 본문 배치는 semantic exporter에서 선형화한다. HWPX 폴백은 이미지 참조/설명/테두리/배치/crop/`inMargin`·`outMargin` 관련 속성 alias 일부를 복구한다. bin data 없으면 `UnknownBlock`으로 남기며 alt/description 계열 속성은 fallback text로 보존한다. `Resource::Binary`는 그림 참조에 사용하지 않는다. |
| resource | 예 | 부분 | 부분 | 부분 (JSON store 보존, HTML/Markdown image·binary 파일 출력) | HWP에서 이미지가 참조한 BinData는 `ImageResource`, 나머지는 `Link`·`Embedded`·`Storage` 종류와 외부 절대·상대 경로를 포함한 `BinaryResource`로 보존한다. 로드된 미참조 바이트와 스토리지 누락 metadata도 버리지 않는다. HWPX manifest의 embedded·external binary resource와 누락 entry metadata를 IR에 보존하며, 참조된 이미지는 `ImageResource`로 승격한다. HTML/Markdown은 embedded·storage binary bytes를 별도 asset 파일로 쓰고 외부 link는 경로 metadata만 유지한다. |
| header/footer | 예 | 예 | 예 | 부분 (모두 선형화 출력) | 페이지 반복 레이아웃이 아니라 본문 앞뒤 block 묶음. HWPX 폴백은 `FirstPage`/odd/even placement와 관련 속성 alias 일부를 복구한다. |
| footnote/endnote | 예 | 부분 | 예 | 부분 (note ref + body 출력) | paragraph offset으로 위치를 증명할 수 있으면 note ref를 해당 위치에 배치하고, 복구 불가할 때만 문단 끝에 append하며 warning을 남긴다. 페이지 하단 배치/separator 없음. |
| link | 부분 | 부분 | 예 | 부분 (JSON/HTML/Markdown URL 보존, TXT/SVG 라벨 fallback) | hyperlink field range와 복구 가능한 control offset을 inline 위치로 사용한다. 위치가 없으면 유일한 라벨 일치 또는 문단 끝 fallback과 warning을 사용한다. HWPX 폴백은 직접 link/field link의 URL, title, parameter 이름 alias 일부를 복구한다. |
| field | 예 | 부분 | 예 | JSON 구조 보존, 나머지는 fallback text | HWP의 Formula·날짜·문서 날짜·경로·책갈피·메일머지·상호참조·누름틀·요약·사용자 정보·메모·개인정보·목차 필드를 `DocumentField` inline으로 보존한다. 명령, 속성 비트, 식별자, control data 이름과 memo index를 유지하지만 필드 계산과 동적 갱신은 하지 않는다. URL형 hyperlink는 `Link`로 변환한다. HWPX 비링크 필드는 아직 Unknown fallback이다. |
| list | 부분 | 부분 | 예 | 부분 (JSON/TXT/Markdown prefix, HTML `<ul>/<ol>` + 원본 메타데이터, SVG 평문) | bullet/number/outline을 `ListInfo`로 보존한다. HWP는 rHWP 공개 정의 ID, 표식 속성 비트, 너비 보정·본문 거리 원시 값, 번호 표식 글자 모양 참조, 이미지 글머리표 ID/4바이트 메타데이터, 체크 표식을 추가로 구조화한다. HTML은 이를 `data-*` 속성으로 남기지만 정확한 표식 배치·글자 스타일과 이미지 글머리표 resource는 재현하지 않는다. HWPX 폴백은 동결 전부터 있던 list type/level/idRef, bullet marker와 numbering의 레벨별 시작값/숫자 형식을 복구하며, 동일 문단으로 확인된 rHWP 결과의 빈 marker를 보강한다. 새 목록 메타데이터를 HWPX 폴백에서 독자 복구하지 않는다. explicit list container 구조 없음. nested/restart fixture 없음. |
| equation | 예 | 부분 | 예 | 부분 (JSON 보존, HTML 표시 스타일, 나머지 `[equation: ...]`, Markdown은 `Latex`일 때만 `$$`) | bridge가 `EquationKind::PlainText`를 생성하며 rHWP의 글꼴·크기·색·기준선·크기·오프셋·버전을 보존한다. LaTeX/MathML 판별, numbering, resource 연결 없음. |
| shape | 예 | 부분 | 부분 | 부분 (모두 `[shape: ...]` placeholder) | `kind`, `fallback_text`, `description`과 HWP 도형의 기본 너비·높이·X/Y 오프셋, 회전·반전, 객체 배치, 표준 테두리, 단색·무늬·gradient·image fill 원시 값과 이미지 resource 참조, 그림자 종류·원시 색상·오프셋·투명도, 텍스트 상자 안쪽 여백·세로 정렬을 보존한다. HWP 텍스트 상자 문단·인라인은 재귀 `content` 블록으로 유지하고 HTML은 실제 내부 내용으로 출력한다. HTML은 채우기와 둥근 사각형·타원 외형, 그림자를 CSS로 근사한다. HWP의 선 시작/끝점, 사각형 꼭짓점·둥글기, 타원/호 중심·축, 다각형/곡선 점과 곡선 segment 종류도 구조화해 보존한다. HWP 그룹은 `ShapeKind::Group`과 재귀 `children`으로 경계·크기·변환·배치·자식을 보존하며 그룹 caption은 방향에 따라 인접 caption 문단으로 유지한다. semantic exporter는 텍스트 상자와 그룹 경계를 유지하지만 내부 좌표 배치는 아직 순차화한다. HWP 그림자는 현재 synthetic unit test로 검증했으며 실제 그림자 포함 fixture는 아직 없다. HWPX도 `sz`/`pos` 크기·오프셋, 회전·반전, `lineShape` 테두리, `fillBrush` 단색 채우기와 `drawText` 텍스트 상자 스타일을 복구하지만 상세 기하·배치와 무늬/image/gradient fill, 그림자는 신규 복구하지 않는다. HWPX 그룹 layout도 제한적이다. |
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

> 기능 경계 (2026-07-18 확정): 이 폴백은 legacy 호환성 안전망으로 동결했다. 새 element/control/attribute alias, style, layout 복구를 추가하지 않는다. 기존 회귀·보안·손상 입력·기존 silent-drop 수정만 허용하며, rHWP 공개 API가 같은 정보를 제공하면 bridge로 이전하고 해당 폴백을 축소한다. 자세한 강제 규칙은 `AGENTS.md`의 "rHWP 기능 경계"를 따른다.

## 지속가능성 리스크 (sustainability notes)

정직하게 추적해 둘 구조적 리스크:

- **`src/hwpx.rs`가 큰 legacy HWPX 파서로 남아 있음.** 현재 `src` 전체의 약 1/3 규모이며, 정규 XML 파서가 아니라 손으로 만든 문자열 스캐너다. 과거 다수의 `fix(hwpx):` 커밋이 이 스캐너의 엣지케이스(DOCTYPE, CDATA, self-closing, attribute alias 등)를 넓혔다. 2026-07-18부터 기능 확장과 parser 리팩터링을 중단하고, 기존 회귀·보안·silent-drop 유지보수만 허용한다. rHWP upstream 지원이 늘면 해당 경로를 축소한다. 자세한 결정은 `docs/ROADMAP.md`.
- **실제 문서 fixture corpus 부재.** 현재 fixture는 대부분 합성/단일 기능이며 HWPX 쌍은 2개뿐이다. 변환 정확도를 입증할 실문서가 없어 "쓸만한 변환기" 주장은 아직 불가하다. 이것이 최대 병목이다 (`docs/ROADMAP.md` 완료 기준 참고).

## 우선순위

- P0: `basic_text`, `style`, `table`, `merged_table`, `image`
- P1: `link_list`, `note_header_footer`
- P2: `equation_shape_chart`, `kitchen_sink`
