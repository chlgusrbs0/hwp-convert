# rHWP 기반 파일 변환 로드맵 (Roadmap)

이 문서는 `hwp-convert`가 rHWP 위에서 쓸만한 파일 변환기가 되기 위한 장기 작업 기준이다.

목표는 rHWP를 다시 만드는 것이 아니다. 목표는 rHWP가 읽어낸 HWP/HWPX 문서 정보를 `hwp-convert`의 `Document IR`로 안정적으로 옮기고, 각 출력 형식에 맞게 가능한 한 많이 보존하는 변환 계층을 완성하는 것이다.

현재 crate 버전, rHWP pin, IR_VERSION, 테스트 수, fixture 목록 같은 기준값은 `AGENTS.md`의 "현재 프로젝트 사실" 블록을 따른다. 현재 구현 상태(요소별 지원, HWPX 현황, 리스크)는 `docs/STATUS.md`를 본다. 이 문서는 "어디로 가는가"에 집중한다.

## 제품 목표

최종 목표는 Windows COM, 유료 오피스 프로그램, GUI 자동화 없이 HWP/HWPX 파일을 신뢰 가능한 중간 표현과 여러 출력 형식으로 변환하는 CLI 도구다.

우선순위:

1. 변환 정확도
2. 회귀 테스트 가능성
3. 조용한 데이터 손실 방지
4. 출력 형식별 현실적인 보존 정책
5. 지원 형식 확장

많은 출력 형식을 얕게 지원하는 것보다, 적은 문서 요소라도 실제 HWP/HWPX에서 정확하게 보존하는 것이 더 중요하다.

## 비목표

- rHWP를 대체하지 않는다.
- rHWP 공개 API에 없는 HWP/HWPX 기능을 자체 파싱하거나 역공학하지 않는다.
- raw HWP record, unknown control bytes, HWPX XML 속성에서 신규 semantic 의미를 만들지 않는다.
- HWP/HWPX 편집기를 만들지 않는다.
- 모든 출력 형식에서 원본 HWP의 시각적 배치를 동일하게 재현한다고 주장하지 않는다.
- 기본 `--to svg`는 semantic/plain-text 기반 SVG exporter다. `src/render`의 RenderSnapshot visual path는 별도 진단 경로이며 기본 SVG exporter와 섞지 않는다.

## 핵심 아키텍처

레이어 경계의 정확한 정의는 `docs/ARCHITECTURE.md`에 있다. 요약하면:

기본 변환 경로:

```text
HWP/HWPX file -> rHWP parser -> src/bridge/rhwp.rs -> Document IR -> src/exporter.rs -> txt/json/markdown/html/svg
```

Legacy HWPX 폴백 경로(rHWP가 실패하거나 빈 결과일 때의 호환성 안전망):

```text
HWPX file -> src/hwpx.rs (Contents/section*.xml 또는 Preview/PrvText.txt) -> Document IR -> exporters
```

실험적 visual 진단 경로:

```text
HWP/HWPX file -> rHWP DocumentCore / renderer query API -> src/render/mod.rs -> RenderSnapshot / 진단 SVG
```

경계 규칙: exporter는 rHWP 타입에 직접 의존하지 않는다. rHWP 타입은 `src/bridge`와 `src/render` 뒤에 둔다. semantic IR에 페이지 좌표를 섞지 않는다. bridge-only 개선은 보통 `IR_VERSION`을 올리지 않고, serialized JSON shape가 바뀌면 bump를 검토한다. 신규 지원은 현재 pin의 rHWP 공개 typed model/query가 제공하는 정보로만 시작한다. `src/hwpx.rs`는 신규 기능 경로가 아니다.

## 쓸만함의 정의

"rHWP 수준의 쓸만한 변환기"는 다음을 뜻한다.

1. rHWP가 읽어낸 주요 문서 요소를 버리지 않고 받는다.
2. 받은 요소를 `Document IR`에 명시적 구조로 보존한다.
3. 각 exporter가 자기 형식에서 표현 가능한 만큼 보존한다.
4. 표현할 수 없는 정보는 조용히 버리지 않고 fallback text, placeholder, `Unknown`, `ConversionWarning` 중 하나로 남긴다.
5. 실제 HWP/HWPX fixture로 parse, bridge, export 결과를 검증한다.
6. rHWP pin을 올릴 때 회귀를 감지할 수 있다.

여기서 "rHWP가 읽어낸다"는 tag 이름이나 raw bytes가 존재한다는 뜻이 아니다. 현재 고정 revision의 공개 typed model 또는 renderer query에서 의미가 확인되는 경우만 해당한다. `Unknown`/warning 또는 legacy HWPX 폴백으로 발견되는 정보는 지원 완료에 포함하지 않는다.

형식별 기대치:

| 출력 형식 | 목표 |
| --- | --- |
| JSON | `Document IR`의 최대 보존. 디버깅/구조 검증 기준 형식. |
| HTML | 구조, 기본 스타일, 표, 이미지 asset, 링크, note를 사람이 읽을 수 있게 보존. |
| Markdown | 의미 중심 보존. 표, 링크, 목록, 강조 등 Markdown 표현 가능 범위 우선. |
| TXT | 텍스트 추출과 읽기 순서 우선. 표/control은 평문 fallback 허용. |
| semantic SVG | 현재 평문 기반. visual fidelity를 대표하지 않는다. |
| visual SVG/PDF | 별도 renderer-assisted 단계. 아직 기본 변환 목표 아님. |

## 정확도 계층

1. Parse survival: 파일을 열 수 있는가.
2. Semantic capture: 문서 요소가 IR에 들어오는가.
3. Structural fidelity: 문단, 표, 셀, 목록, note, resource 관계가 유지되는가.
4. Text fidelity: 문자, 줄바꿈, 탭, 공백, 한글/영문/숫자가 보존되는가.
5. Style fidelity: 글자/문단/표 스타일이 보존되는가.
6. Asset fidelity: 이미지/binary resource가 참조 가능한 파일로 남는가.
7. Warning fidelity: 미지원/손실 요소가 추적 가능한가.
8. Visual fidelity: 좌표, 페이지, 배치, wrap이 원본에 가까운가.

현재 주력 단계는 2-7이다. 8은 `src/render` 기반으로 별도 진행한다.

## 확정된 기능 경계

### `src/hwpx.rs` 자작 HWPX 파서의 방향

현재 `src/hwpx.rs`는 rHWP의 HWPX 약점을 보완하는 폴백 파서지만 점점 커져 `src` 전체의 약 1/3에 이르고, 정규 XML 파서가 아닌 자작 문자열 스캐너다 (`docs/STATUS.md`의 "지속가능성 리스크" 참고). "rHWP를 다시 만들지 않는다"는 비목표와 긴장 관계다.

2026-07-18 결정: **폴백을 동결하고 rHWP upstream 개선에 따라 축소한다.** 이전 선택지 중 (c)를 채택했으며, `quick-xml` 기반 재작성이나 현행 스캐너의 기능 확장은 하지 않는다.

동결의 정확한 의미:

- 현재 사용자에게 발생할 수 있는 데이터 손실을 피하기 위해 기존 코드를 즉시 삭제하지 않는다.
- 기존 fixture가 보장하는 동작의 회귀, 보안 문제, panic/손상 입력, 기존 복구 데이터의 silent drop만 수정할 수 있다.
- 새 element/control/attribute alias, 새 style/layout 정보, 새 semantic node 복구는 추가하지 않는다.
- rHWP가 같은 정보를 공개 API로 제공하면 먼저 bridge로 옮기고 대응하는 폴백 코드를 줄인다.
- 폴백 결과는 기능 지원의 근거가 아니며 `docs/STATUS.md`에서 별도 관찰로만 기록한다.

### rHWP 공개 surface 소진

신규 정확도 작업은 현재 pin의 공개 surface를 다음 상태 중 하나로 분류하는 것에서 시작한다.

| 상태 | 의미 | 지원 완료 계산 |
| --- | --- | --- |
| `mapped` | bridge/render가 typed 값을 받아 IR/query 결과에 보존 | exporter와 fixture까지 갖추면 포함 |
| `normalized` | 명시적이고 검증 가능한 규칙으로 정규화하여 보존 | 정규화 근거가 문서화되면 포함 |
| `warning/unknown` | 존재만 추적하고 의미 구조는 보존하지 못함 | 미포함 |
| `render-only` | semantic IR 대상이 아니며 공개 renderer query로만 사용 | visual 경로에서 별도 계산 |
| `unmapped` | rHWP가 공개하지만 현재 bridge/render가 받지 않음 | 최우선 후보 |
| `upstream-needed` | 현재 공개 API가 값을 제공하지 않음 | hwp-convert에서 구현 금지 |

완성도 수치는 이 분류표와 실제 fixture 결과 없이 감으로 제시하지 않는다. field 수가 같아도 텍스트 본문과 희귀 플래그의 영향도가 다르므로, 단순 field 개수와 사용자 체감 정확도도 별도로 보고한다.

## 마일스톤

Document IR 자체의 단계별 마일스톤(v0-v7.4)은 `docs/ARCHITECTURE.md`에 있다. 아래는 변환기 제품 관점의 마일스톤이다.

### M0: 기준선 정리 — 기초 완료

완료: `src/lib.rs`로 integration test에서 crate module 사용 가능, fixture smoke harness, P0 fixture skeleton, 핵심 docs(`ARCHITECTURE`, `STATUS`, `FIXTURES`), rHWP git revision 고정.

남은 것: 나머지 P0 fixture의 실제 `input.hwp`/`input.hwpx` 확정.

### M1: P0 fixture corpus 구축

목표: P0 fixture 5개를 실제 HWP/HWPX 쌍으로 준비한다. 각 fixture는 `bridge smoke`, `feature assertion`, `export smoke`를 가진다. deterministic할 때만 golden output을 추가한다.

P0: `basic_text`, `style`, `table`, `merged_table`, `image`.

완료 기준: 각 fixture에 `input.hwp`, `input.hwpx`, `notes.md`가 있고, 두 입력 형식 모두 parse/export smoke를 통과하며, fixture별 핵심 field assertion이 있고, `docs/STATUS.md`가 실제 관찰 결과에 맞다.

금지: 검증하지 않은 이상적 미래 동작을 expected로 쓰기. fixture 없는 요소를 "지원한다"고 말하기.

현재 진행: HWP fixture는 위 P0 + `header_footer`, `footnote`, `list`, `equation`, `shape`까지 존재한다. 채택된 HWPX 쌍은 `basic_text`, `list`뿐이다. (상세: `docs/STATUS.md`)

### M2: P0 bridge mapping 강화

목표: P0 fixture와 rHWP 공개 surface 점검에서 드러난 손실을 bridge에서 줄인다. IR이 부족하면 먼저 호환성 정책을 검토한 뒤 확장한다. rHWP 공개 API에 없는 값은 이 마일스톤의 구현 대상이 아니다.

작업: text(줄바꿈/탭/공백/혼합), paragraph(빈 문단 정책 확정), style(`TextStyle`/`ParagraphStyle`/style ref), table(span/nested/배경색), image(resource id/extension/media type/bytes/alt/caption/dimensions), exporter(asset 출력 정책 고정).

완료 기준: P0 전체 통과, silent drop 의심 요소 없음(또는 warning/unknown 추적), JSON/HTML에서 P0 구조를 사람이 검토 가능.

### M3: P1 semantic coverage 확장

P1: `link_list`, `note_header_footer`.

작업: hyperlink field range와 trailing hyperlink control 분리 검증, list kind/level/marker/number/restart, header/footer placement와 block contents, footnote/endnote store와 inline ref fallback warning.

완료 기준: P1 전체 통과, note/link 위치 한계가 warning으로 남음, HTML/Markdown/TXT에서 읽을 수 있는 결과.

### M4: P2 coverage와 누락 감시

P2: `equation_shape_chart`, `kitchen_sink`.

목표: 어려운 요소를 과장하지 않고 현재 보존 수준을 정확히 기록한다. chart, shape, equation, unknown의 손실 지점을 숨기지 않는다.

작업: equation script/fallback 고정, shape kind/description/fallback 보존, chart는 bridge가 실제 `Block::Chart`를 만들 수 있는지 먼저 확인(안 되면 "미지원 관찰" fixture), known-but-unmapped control의 warning/unknown 정책, `kitchen_sink`는 끝까지 parse/export 되는지와 핵심 smoke.

완료 기준: P2가 현재 한계를 드러내는 회귀 테스트 역할을 하고, 미지원 요소가 `docs/STATUS.md`에 문서화된다.

### M5: rHWP revision update discipline

목표: rHWP upstream 발전을 활용하되 변환기 안정성을 잃지 않는다.

정책: rHWP는 git revision 고정. fixture coverage가 충분해지기 전엔 자주 올리지 않는다. 완성본에 가까워질수록 의도적으로 올린다. 절차는 `AGENTS.md`의 "rHWP dependency 정책"을 따른다. revision update 시 `src/hwpx.rs`에서 대체 가능한 폴백 범위를 찾아 축소한다(위 "확정된 기능 경계" 참고).

### M6: exporter fidelity 강화

목표: IR에 들어온 정보를 각 형식에서 합리적으로 보존한다. 형식별 방향은 `AGENTS.md`의 "exporter 정책"을 따른다.

완료 기준: 각 exporter의 손실 정책이 문서화되고, fixture가 exporter별 핵심 보존을 확인하며, HTML/Markdown image asset이 다중 문서 변환에서 충돌하지 않는다.

### M7: renderer-assisted visual conversion

목표: 원본처럼 보이는 출력이 필요한 형식에서 rHWP renderer query 결과를 활용한다.

주의: semantic conversion과 분리한다. RenderSnapshot은 진단용 시작점이다. visual output을 기본 `--to svg`에 바로 덮어쓰지 않는다. PDF는 별도 설계가 필요하다.

가능한 방향: `--to visual-svg` 같은 별도 형식, semantic/visual path 옵션 분리, rHWP native SVG export API 조사, PDF backend 결정.

완료 기준: visual fixture와 semantic fixture 분리, page count/bounds/text/controls/images/tables에 대한 visual smoke, semantic SVG와 visual SVG 혼동 방지.

## 기능별 작업표

| 요소 | 현재 단계 | 남은 핵심 작업 |
| --- | --- | --- |
| text | P0 | 실제 fixture로 문자/줄바꿈/탭/공백 검증 |
| paragraph | P0 | 빈 문단/role 정책 확정 |
| style | P0 | style ref, spacing, indent, colors, font metadata fixture 고정 |
| table | P0 | border, padding, width, cell style, caption 확장 |
| merged table | P0 | span과 exporter별 fallback/golden 고정 |
| image | P0 | asset, resource, caption, dimensions, missing bin data warning |
| resource | P0/P1 | image 외 binary resource 정책 설계 |
| link | P1 | field range와 control fallback 분리 검증 |
| list | P1 | nested/restart/list container 표현 |
| header/footer | P1 | placement, repeated page semantics, exporter representation |
| footnote/endnote | P1 | inline position 한계, warning, note body export |
| equation | P2 | script kind, fallback, LaTeX/MathML 판별 |
| shape | P2 | geometry, border/fill, text box, group/caption 보존 수준 결정 |
| chart | P2 | rHWP model에서 bridge-visible chart path 확인 |
| unknown | P2 | known-but-unmapped control의 silent drop 방지 |
| HWPX 폴백 | 동결 | 기존 회귀·보안·silent-drop 수정만 허용하고 rHWP 지원 확대에 따라 축소 |
| visual layout | Future | RenderSnapshot 또는 rHWP native renderer 연동 |

## 회귀 지표

작업 중 추적할 숫자: rHWP 공개 surface 분류 수(`mapped`/`normalized`/`warning/unknown`/`render-only`/`unmapped`/`upstream-needed`), official fixture count, fixture input pair count, parse success count, format별 export success count, code별 warning count, unknown block/inline count, silent drop 의심 count, image asset output count, golden comparison count, rHWP revision update regression count.

최소 목표: P0 완료 시 HWP/HWPX pair 5개 이상. Useful converter 단계에서 실제 문서 fixture 30-50개. unsupported element는 적어도 warning/unknown/docs 중 하나에 남는다.

## silent drop 방지 규칙

가장 위험한 실패는 변환이 성공했는데 데이터가 사라지는 것이다. (운영 규칙은 `AGENTS.md`의 "silent drop 대응" 참고.)

- rHWP가 control을 제공했는데 bridge가 못 옮기면 `UnknownBlock`/`UnknownInline`/`ConversionWarning`을 남긴다.
- exporter가 표현 못 하는 구조는 fallback text를 남긴다.
- fallback도 못 만들면 warning을 남긴다.
- warning이 너무 많아지면 code를 세분화한다.
- fixture에 "현재 미지원이지만 발견되어야 하는 요소"를 넣는다.

## 시간 추정

2026-06-06 기준 작업량 추정. 실제 fixture 확보 난이도, rHWP upstream 변화, 기대 시각 정확도에 따라 달라진다.

| 단계 | 목표 | 예상 기간 |
| --- | --- | --- |
| Alpha | 기본 fixture와 핵심 semantic 변환을 실제 문서로 검증 | 3-6주 |
| Beta | rHWP 주요 semantic 요소를 대부분 bridge/exporter로 연결 | 2-3개월 |
| Useful converter | 30-50개 실제 fixture 기반 실사용 가능한 안정성 | 3-5개월 |
| Product-grade | 넓은 문서군, 시각 출력, packaging, CI 품질 관문 강화 | 6개월 이상 |

이는 "rHWP 수준 parser/renderer를 만드는 시간"이 아니라 "rHWP가 주는 정보를 변환기로 충분히 활용하는 시간"이다.

## 다음 작업 순서

1. 현재 pin의 rHWP 공개 model/query field를 위 여섯 상태로 전수 분류한다.
2. `unmapped` 중 데이터 손실 영향이 큰 항목을 골라 `bridge -> IR -> exporter` 수직 단위로 구현한다.
3. 해당 값을 가진 HWP fixture 또는 unit test로 회귀를 고정한다.
4. 나머지 P0 fixture(`table`, `merged_table`, `style`, `image`)의 실제 HWP/HWPX 입력을 확정하되, HWPX 폴백 확장으로 통과시키지 않는다.
5. 실패를 rHWP parse, bridge mapping, exporter 표현 문제로 분리한다. `upstream-needed`는 로컬 parser 구현 대신 기록한다.
6. 공개 surface 분류와 P0가 안정되면 rHWP revision update rehearsal을 한 번 한다.
7. P1(`link_list`, `note_header_footer`)로 넘어간다.

각 단계의 commit은 작게 나눈다 (`AGENTS.md`의 Git 규칙 참고).

## 의사결정 기록

2026-06-06:

- 변환 정확도를 최우선으로 둔다.
- 많은 출력 형식 추가보다 실제 문서 fixture와 semantic 보존을 우선한다.
- rHWP는 upstream을 무작정 따라가지 않고 revision pin을 유지하며, 완성도에 따라 차근차근 올린다.
- 기본 SVG는 semantic exporter로 유지하고, visual output은 별도 renderer-assisted 단계로 다룬다.

2026-06-16:

- 문서를 통합 정리했다: `COMPATIBILITY.md` → `STATUS.md`, `RHWP_CONVERSION_ROADMAP.md` → `ROADMAP.md`, `HWPX_FIXTURE_FINDINGS.md`는 `STATUS.md`에 병합. addendum 패턴 제거, stale 사실(HEAD 해시, 테스트 수)은 `AGENTS.md` 단일 출처로 이동.
- `src/hwpx.rs` 자작 파서의 비대화를 당시 전략 검토 항목으로 등록했다.
- 글꼴 fidelity 1차 확장: rhwp `CharShape`가 주는데 버려지던 위/아래첨자, 강조점, 양각/음각, 외곽선, 그림자를 `TextStyle`로 끌어왔다. bridge 매핑 + HTML(CSS) + Markdown(sup/sub) + 테스트 포함, `IR_VERSION` 7 → 8. 남은 글꼴 항목(밑줄 색/모양, 취소선 색, 장평/자간/커닝)은 후속.
- 표 셀 fidelity 1차 확장: rhwp `Cell`의 `is_header`(→ HTML `<th>`)와 vertical align(→ `TableCellStyle.vertical_align`, CSS `vertical-align`)을 끌어왔다. HWPX 폴백은 `header` 속성으로 헤더 여부를 복구한다. `IR_VERSION` 8 → 9. 남은 표 항목(셀 폭/높이, padding, 경계선, 열 폭)은 후속.
- 밑줄/취소선 색 + 표 셀 폭/높이: `TextStyle.{underline_color, strike_color}`(→ CSS `text-decoration-color`)와 `TableCellStyle.{width, height}`(→ CSS, 기존 hwp-units→px 변환 재사용)를 끌어왔다. `IR_VERSION` 9 → 10. 남은 항목(밑줄 모양, 장평/자간, 셀 padding/경계선)은 후속.
- 표 셀 padding: `TableCellStyle.{padding_top, right, bottom, left}`(→ CSS padding-*, hwp-units(i16)→px 변환)를 끌어왔다. `IR_VERSION` 10 → 11. 셀 박스모델(폭/높이/padding) 완료. 남은 표 항목: 셀/표 경계선, 표 전체 폭, 표 outer margin.
- 표 셀 테두리: `TableCellStyle`에 4면 `Border{width, style, color}`(`BorderStyle` enum)를 추가, BorderFill의 `borders[4]`(좌우상하)를 매핑해 CSS border-*로 출력. `IR_VERSION` 11 → 12. **주의(근사 포함):** 굵기 인덱스→px는 rhwp 자체 함수 `css_border_width_to_hwp`의 임계값(0–7)을 역산하고 8–15는 표준 HWP 표를 썼다. 선종류는 solid/dashed/dotted/double로 매핑하고 wave/3D는 solid로 근사했다. HWPX 폴백도 borderFill 기반 셀 테두리를 복구한다. 이 근사는 실제 테두리 포함 문서 fixture로 검증해야 하며, 그게 이 기능의 다음 할 일이다.
- HWPX 폴백 파리티(새 IR 없음): section XML `<hp:tc>`의 `cellSz`(폭/높이), `cellMargin`(padding), `subList@vertAlign`, `borderFillIDRef` 계열 속성을 읽어 rhwp 경로와 동일한 `TableCellStyle` 필드를 복구한다. 결정적 XML 속성 파싱(근사 없음). 표 전체 폭과 outer margin은 후속.
- 이미지 테두리 + 흑백 효과: `Image.{border, grayscale}` 추가(`CellBorder`를 일반 `Border`로 리네임해 셀·이미지 공유). border_width(raw hwp단위→px)·border_color 매핑(선종류는 solid 가정), `ImageEffect::{GrayScale, BlackWhite}`→`grayscale`(→ CSS `filter: grayscale`). `IR_VERSION` 12 → 13. 이제 미지원으로 warning되는 것은 crop, 밝기/대비, opacity, 내부 padding, Pattern8x8 효과. BlackWhite는 grayscale로 근사. 실문서 fixture 검증 권장.
- HWPX 속성 alias/fallback 보강(새 IR 없음): 이미지·링크·필드·주석·스타일 참조·글꼴명·list marker·manifest·borderFill·margin·수식/도형/차트의 생성기별 속성명 차이를 일부 흡수한다. missing image와 unsupported control/object는 내부 텍스트가 없어도 alt/title/name/description/value 계열 속성을 `UnknownBlock.fallback_text`로 남긴다. exporter는 multiline unknown fallback을 HTML/Markdown/TXT에서 읽을 수 있게 출력한다.

- 문서 의미 보존 IR 확장: `IR_VERSION` 13 → 14. 글자 장평·자간·상대크기·기준선 위치·커닝과 밑줄·취소선 위치/선 종류, 문자권별 HWP 실행 구간, 문단 테두리·배경·안쪽 여백·페이지 나눔 속성, 실제 다단계 번호 표식을 보존한다. HWPX 폴백도 균일 글자 메트릭과 문단 border/breakSetting 및 장식 선 종류를 복구하고 HTML은 대응 CSS를 출력한다.
- 표 배치 IR 확장: `IR_VERSION` 14 → 15. HWP와 HWPX 표의 전체 너비·높이와 4방향 바깥 여백을 `TableStyle`에 보존하고 HTML CSS로 출력한다.
- 표 행 배치 IR 확장: `IR_VERSION` 15 → 16. HWP `row_sizes`와 HWPX 행 높이를 `TableRow.height`에 보존하고 HTML 행 스타일로 출력한다.
- 방정식 표시 IR 확장: `IR_VERSION` 16 → 17. rHWP가 제공하는 방정식 글꼴·크기·색·기준선·크기·오프셋·버전 정보를 보존하고 HTML 표시 스타일에 반영한다.
- 도형 기본 배치 IR 확장: `IR_VERSION` 17 → 18. HWP 도형의 기본 너비·높이·X/Y 오프셋을 보존하고 HTML fallback placeholder에 반영한다.
- 이미지 transform IR 확장: `IR_VERSION` 18 → 19. HWP/HWPX 이미지의 회전과 가로·세로 반전을 보존하고 HTML CSS transform으로 출력한다.
- 표 반복 머리글 IR 확장: `IR_VERSION` 19 → 20. HWP `repeat_header`를 보존하고 HTML 첫 행을 `<thead>`로 출력한다.
- 표 페이지 분할 IR 확장: `IR_VERSION` 20 → 21. HWP 표의 `CellBreak`/`RowBreak` 규칙을 보존하고 `RowBreak`는 HTML `break-inside: avoid`로 출력한다.
- 도형 단순 스타일 IR 확장: `IR_VERSION` 21 → 22. HWP 도형의 표준 선종류·굵기·색상과 패턴 없는 단색 채우기를 보존하고 HTML CSS로 출력한다. 패턴·이미지·그라데이션 채우기와 그림자는 계속 warning으로 남긴다.
- 도형 변환 IR 확장: `IR_VERSION` 22 → 23. HWP/HWPX 도형의 회전과 가로·세로 반전을 보존하고 HTML CSS transform으로 출력한다. 이 과정에서 HWP 그림 회전각도 rHWP renderer 기준의 도 단위로 바로잡았다.
- 도형 텍스트 상자 IR 확장: `IR_VERSION` 23 → 24. HWP 도형 텍스트 상자의 안쪽 여백과 가운데·아래 세로 정렬을 보존하고 HTML box CSS로 출력한다. HWPX 도형 텍스트 상자 속성은 fixture 검증 전까지 미매핑이다.
- HWPX 도형 기본 표시 보강: OWPML `AbstractShapeObjectType`의 `sz`/`pos`를 기준으로 도형 너비·높이와 X/Y 오프셋을 복구하고, `AbstractDrawingObjectType`의 `lineShape`, `fillBrush`, `drawText`에서 표준 테두리·단색 채우기·텍스트 상자 여백과 세로 정렬을 복구한다. 기존 Shape IR 필드 매핑이므로 `IR_VERSION`은 유지한다.
- HWPX 표 페이지 속성 보강: 공식 OWPML 값 `repeatHeader`와 `pageBreak=TABLE/CELL/NONE`을 기존 Table IR에 매핑한다. `TABLE`은 행 단위 분할로 보존하며 호환 별칭 `ROW/ROW_BREAK`도 허용한다.
- 표 셀 간격 IR 확장: `IR_VERSION` 24 → 25. HWP/HWPX `cell_spacing`/`cellSpacing`을 보존하고 HTML `border-spacing`으로 출력한다.
- 이미지 안쪽 여백 IR 확장: `IR_VERSION` 25 → 26. HWP `Picture.padding`과 HWPX `inMargin`을 보존하고 HTML 이미지 padding으로 출력한다.
- 이미지 캡션 배치 IR 확장: `IR_VERSION` 26 → 27. HWP/HWPX 이미지 캡션의 왼쪽·오른쪽·위·아래 방향을 보존하고 HTML 순서와 좌우 배치에 반영한다.
- 이미지 crop IR 확장: `IR_VERSION` 27 → 28. HWP/HWPX 원본 이미지의 자르기 사각형과 HWPX 원본 크기를 보존한다. 원본 크기가 명확한 HWPX crop은 HTML에 반영하고, 적용할 수 없는 출력은 원본 바이트와 warning을 유지한다.
- 이미지 조정값 IR 확장: `IR_VERSION` 28 → 29. HWP/HWPX 밝기·대비 원시 값을 보존한다. exporter별 임의 근사는 하지 않고 미적용 warning을 유지한다.
- HWPX 이미지 투명도 IR 확장: `IR_VERSION` 29 → 30. 표준 `alpha`와 호환 `opacity`의 반대 의미를 구분해 불투명도로 정규화하고 HTML에 반영한다.
- 이미지 효과 IR 확장: `IR_VERSION` 30 → 31. 일반 회색조·임계 흑백·Pattern8x8을 구분해 보존하고, exporter의 근사 또는 미적용 여부를 warning으로 남긴다.
- 이미지 배치 IR 확장: `IR_VERSION` 31 → 32. HWP/HWPX의 글자처럼 취급, 텍스트 감싸기, 가로·세로 기준·정렬·오프셋을 보존한다. semantic exporter는 페이지 좌표계를 임의로 근사하지 않고 선형화 warning을 유지한다.
- 이미지 배치 부가 정보 확장: `IR_VERSION` 32 → 33. HWP/HWPX Z-order와 바깥 여백, HWPX `flowWithText`·`allowOverlap`, HWP 쪽 나눔 방지를 보존한다.
- 이미지 변형 크기 IR 확장: `IR_VERSION` 33 → 34. HWP/HWPX 원본 크기와 현재 변형 크기를 표시 크기와 별도로 보존해 crop·변환 계산 근거를 유지한다.
- HWP 그룹 도형 caption silent drop 제거: 자식 도형을 순차 블록으로 펼치는 현재 fallback에서도 그룹 caption 문단을 방향에 따라 앞뒤 인접 블록으로 보존한다.
- HWP 미참조 BinData 보존: 이미지로 소비되지 않은 로드된 임베디드 BinData를 `BinaryResource`로 매핑해 JSON 변환에서 원본 바이트가 조용히 사라지지 않게 한다.
- HWP BinData 종류·링크 경로 IR 확장: `IR_VERSION` 34 → 35. `Link`·`Embedding`·`Storage`를 구분하고 외부 링크의 절대·상대 경로를 `BinaryResource`에 보존한다.
- resource bytes JSON 압축 개선: `IR_VERSION` 35 → 36. 이미지·첨부 바이트를 숫자 배열 대신 Base64 문자열로 직렬화하고, 구형 배열 표현도 계속 역직렬화한다. JSON exporter는 전체 결과 문자열을 메모리에 만들지 않고 스트리밍한다.
- 표 객체 배치 IR 확장: `IR_VERSION` 36 → 37. 이미지 전용이던 배치 구조를 `ObjectPlacement`로 일반화하고 HWP 표의 글자처럼 취급, 감싸기, 기준·정렬·오프셋, Z-order, 여백, 쪽 나눔 방지를 보존한다. semantic exporter의 페이지 좌표 선형화는 문서당 한 번 경고한다.
- 문단 탭 정의 IR 확장: `IR_VERSION` 37 → 38. HWP 문단이 참조하는 사용자 정의 탭의 위치·정렬·리더 원시 값과 자동 좌우 탭 플래그를 보존한다. HTML은 탭 문자를 전용 span으로 출력하되 사용자 정의 탭 위치는 근사 경고를 유지한다.

2026-07-18:

- 신규 기능 범위를 현재 pin의 rHWP 공개 typed model/query가 제공하는 정보로 확정했다.
- HWP 도형 그림자 IR 확장: `IR_VERSION` 49 → 50. rHWP 공개 `DrawingObjAttr`의 그림자 종류·원시 색상·X/Y 오프셋·투명도를 구조화하고 HTML은 공개된 색상과 오프셋으로 CSS `box-shadow`를 근사한다. rHWP가 제공하지 않는 blur 값이나 방향 규칙은 추정하지 않는다.
- HWP 바탕쪽 IR 확장: `IR_VERSION` 50 → 51. rHWP 공개 `MasterPage`의 적용 대상·확장/겹침 플래그·텍스트 영역·참조 마스크·원시 list header와 내부 문단 블록을 구역에 보존한다. HTML/Markdown/TXT는 내용을 명시적으로 선형화하고 반복 페이지 배경 배치는 재현하지 않는다고 warning으로 남긴다. 실제 바탕쪽 fixture 검증은 후속이다.
- HWP 목록 표식 메타데이터 IR 확장: `IR_VERSION` 51 → 52. rHWP 공개 bullet/numbering 정의 ID, 속성 비트, 너비 보정·본문 거리 원시 값, 번호 표식 글자 모양 참조, 이미지 글머리표 ID/메타데이터와 체크 표식을 구조화한다. HTML은 `data-*`로 보존하고, 공개 API에서 실제 이미지 resource 연결을 확인할 수 없는 이미지 글머리표와 정확한 표식 배치·단위는 추정하지 않는다.
- HWP 글자 그림자 IR 확장: `IR_VERSION` 52 → 53. rHWP 공개 `CharShape`의 그림자 종류·X/Y 비율 오프셋·색상과 원시 색상 값을 구조화한다. HTML은 공개 비율을 `em`으로 적용하고 원본 값을 `data-*`로 남기며, 공개되지 않은 blur와 종류별 렌더링 규칙은 추정하지 않는다. 양각·음각은 별도 boolean을 유지하고 기존 일반 그림자 근사를 사용한다.
- HWP 글자 강조점 IR 확장: `IR_VERSION` 53 → 54. rHWP 공개 `CharShape.emphasis_dot`의 원시 종류를 보존하고, rHWP가 문서화한 1~6 값을 HTML의 ●·○·ˇ·˜·･·: 기호로 구분한다. 알 수 없는 값은 원시 타입과 warning을 유지하고 generic dot으로 대체한다.
- HWP 글자 테두리·배경 IR 확장: `IR_VERSION` 54 → 55. rHWP 공개 `CharShape.border_fill_id`가 참조하는 BorderFill의 원본 ID, 4면 테두리, 단색·그라데이션·이미지 채우기를 구조화한다. HTML은 기존 border/fill CSS 경로로 출력하고 원본 참조 ID를 `data-*`로 남긴다.
- HWP 원본 스타일 정의 IR 확장: `IR_VERSION` 55 → 56. rHWP 공개 `Style`의 문단/글자 종류, 한글·영문 이름, 다음 스타일 ID, 원본 문단·글자 모양 ID와 변환된 스타일 참조를 `StyleSheet.source_styles`에 보존한다. 기존 text/paragraph style 배열은 호환성을 위해 유지한다.
- HWP 글꼴 fallback IR 확장: `IR_VERSION` 56 → 57. rHWP 공개 `Font`의 대체 글꼴 유형·대체 이름·기본 이름을 `TextStyle`에 보존한다. HTML은 원본 글꼴 뒤에 대체·기본 글꼴을 순서대로 추가하고, 미지 유형은 원시값과 warning을 유지한다.
- HWP BorderFill 대각선 IR 확장: `IR_VERSION` 57 → 58. rHWP 공개 `BorderFill.attr`과 `DiagonalLine`의 종류·굵기 인덱스·색상 원시값을 글자, 표 셀, table zone에 보존한다. HTML은 임의로 선 방향을 재구성하지 않고 표 셀 `data-*` 메타데이터와 warning을 남긴다.
- HWP 표 셀 글자 방향 IR 확장: `IR_VERSION` 58 → 59. rHWP 공개 `Cell.text_direction`의 가로, 세로/영문 눕힘, 세로/영문 세움 값을 구조화한다. HTML은 세로쓰기와 영문 방향을 CSS로 반영하고 원본 값을 `data-text-direction`으로 남기며, 미지 값은 원시값과 warning만 보존한다.
- HWP 표 셀 보호 IR 확장: `IR_VERSION` 59 → 60. rHWP 공개 `Cell.list_header_width_ref` 원시값을 보존하고, rHWP query가 `cellProtect`로 공개하는 bit 1만 셀 보호 여부로 정규화한다. HTML은 두 값을 `data-*`로 남기며 정적 출력에서 편집 보호를 동작처럼 가장하지 않는다.
- HWP 표 BorderFill IR 확장: `IR_VERSION` 60 → 61. rHWP 공개 `Table.border_fill_id`의 원본 참조, 단색·그라데이션·이미지 채우기, 4면 테두리와 대각선 정보를 표 스타일에 보존한다. HTML은 채우기와 테두리를 CSS로 출력하고 원본 참조·대각선은 `data-*`로 남긴다.
- HWP 이미지 캡션 레이아웃 IR 확장: `IR_VERSION` 61 → 62. rHWP 공개 `Caption`의 좌우 캡션 수직정렬, 폭, 개체와의 간격, 최대 폭, 마진 포함 여부를 이미지에 보존한다. HTML은 figure/figcaption flex 배치로 근사하고 원본 boolean을 `data-*`로 남긴다.
- HWP 표 캡션 구조 IR 확장: `IR_VERSION` 62 → 63. rHWP 공개 `Table.caption`의 문단 블록, 방향, 수직정렬, 폭, 개체와의 간격, 최대 폭, 마진 포함 여부를 표 내부의 `TableCaption`으로 보존한다. HTML은 figure/figcaption으로 근사하고 Markdown/TXT는 원본 앞뒤 순서로 캡션 내용을 선형화한다. 동결된 HWPX 폴백의 기존 인접 캡션 복구는 변경하지 않는다.
- HWP 도형 캡션 구조 IR 확장: `IR_VERSION` 63 → 64. rHWP 공개 일반 도형과 그룹 도형의 `Caption` 문단 블록·방향·수직정렬·폭·간격·최대 폭·마진 포함 여부를 공통 `ObjectCaption`으로 보존한다. 그룹 캡션도 더 이상 도형 바깥 단락으로 분리하지 않으며 HTML은 figure/figcaption, Markdown/TXT는 원본 앞뒤 순서로 출력한다. 동결된 HWPX 폴백에는 신규 도형 캡션 구조화를 추가하지 않는다.
- HWP 표 원본 크기 메타데이터 IR 확장: `IR_VERSION` 64 → 65. rHWP 공개 표의 선언 행·열 수와 HWP `HWPTAG_TABLE` 원본 속성값을 보존하고 HTML `data-*`로 남긴다. 한컴 HWP 5.0 revision 1.3 사양의 `HWPUNIT16 Row Size` 정의에 따라 rHWP `row_sizes`는 행 높이로 매핑하며, 값이 없거나 유효하지 않을 때만 단일 행 셀 높이의 최댓값으로 보강하고 warning을 남긴다.
- HWP 이미지 캡션 내용 IR 확장: `IR_VERSION` 65 → 66. rHWP 공개 이미지 캡션 문단을 `ObjectCaption` 블록으로 보존해 링크·필드·스타일을 평문으로 축약하지 않는다. 기존 `Image.caption` 문자열과 배치·레이아웃 필드는 JSON/Rust 호환성을 위해 함께 유지하며 exporter는 구조화된 내용을 우선한다. 동결된 HWPX 폴백은 기존 평문 캡션 경로를 유지한다.
- HWP 덧말·글자 겹침 inline IR 확장: `IR_VERSION` 66 → 67. rHWP 공개 `Ruby`의 덧말과 정렬 원시값, `CharOverlap`의 문자·테두리 종류·내부 글자 크기·확장·글자 모양 참조를 문단 안에 구조화한다. rHWP가 덧말의 기준 글자 범위는 공개하지 않으므로 이를 추정하지 않으며, semantic exporter는 덧말을 명시적 fallback으로, 겹친 문자를 읽기 순서대로 선형화한다. 동결된 HWPX 폴백에는 신규 해석을 추가하지 않는다.
- HWP 양식 개체 IR 확장: `IR_VERSION` 67 → 68. rHWP 공개 `FormObject`의 종류·이름·캡션·텍스트·크기·전경/배경색 원시값·선택값·활성 상태·임의 속성을 `DocumentControl::Form`으로 보존한다. HTML은 정적 span과 `data-*` 메타데이터로 출력하고 Markdown/TXT/SVG는 기존 fallback을 유지하며, 입력·선택 같은 상호작용은 재현한다고 주장하지 않는다.
- HWP 표 셀 필드명 IR 확장: `IR_VERSION` 68 → 69. rHWP 공개 `Cell.field_name`을 `TableCell` 메타데이터로 보존하고 HTML `data-field-name`으로 출력한다. 원문에 없던 `[cell field: ...]` 가시 텍스트를 삽입하던 기존 HWP 경로를 제거한다. 동결된 HWPX 폴백은 기존 UnknownBlock 호환 경로를 유지한다.
- HWP 문단 나누기 IR 확장: `IR_VERSION` 69 → 70. rHWP 공개 `Paragraph.column_type`의 구역·다단·쪽·단 나누기와 원본 `raw_break_type`을 문단 스타일 메타데이터로 보존한다. HTML은 page/column `break-before`로 근사하고 원본 종류를 `data-*`로 남기며, Markdown/TXT/SVG는 페이지 구조를 추정하지 않고 읽기 순서를 유지한다.
- HWP 숨은 설명글 IR 확장: `IR_VERSION` 70 → 71. rHWP 공개 `HiddenComment.paragraphs`를 전용 block 내부에 보존해 링크·필드·스타일을 평문 fallback으로 축약하지 않는다. HTML/Markdown/TXT는 명시적인 설명글 레이블과 함께 내용을 선형화하며, 원본의 숨김 상태나 페이지 배치를 재현한다고 주장하지 않는다.
- HWP 문단 줄 간격 모드 IR 확장: `IR_VERSION` 71 → 72. rHWP 공개 `LineSpacingType`의 백분율·고정값·글자에 따라·최소 모드를 수치와 함께 보존한다. HTML은 원본 모드를 `data-*`로 남기고, CSS가 동일 의미를 제공하지 않는 `space_only`와 `minimum`은 기존 고정 line-height 근사와 warning을 유지한다. 동결된 HWPX 폴백에는 신규 해석을 추가하지 않는다.
- HWP 문단 특수 정렬 IR 확장: `IR_VERSION` 72 → 73. rHWP 공개 `Alignment::Distribute`와 `Alignment::Split`을 일반 `Justify`와 구분해 보존한다. HTML은 원본 종류를 `data-*`로 남기고 CSS `justify`로 근사하며, 동결된 HWPX 폴백의 기존 정렬 복구는 변경하지 않는다.
- HWP 도형 원본 변환 IR 확장: `IR_VERSION` 73 → 74. rHWP 공개 `ShapeComponentAttr`의 원본·현재 크기, 그룹 내부 오프셋, 회전 중심과 합성 affine 행렬을 `ShapeTransform`에 보존한다. HTML은 정확한 후속 처리를 위한 `data-*` 메타데이터를 출력하되 전체 그룹 좌표계를 임의로 재현하지 않는다. 비정상 비유한 실수 행렬은 JSON 직렬화 실패를 막기 위해 warning 후 제외하며, 동결된 HWPX 폴백에는 신규 해석을 추가하지 않는다.
- HWP 도형 연결선 IR 확장: `IR_VERSION` 74 → 75. rHWP 공개 `LineShape`와 `ConnectorData`의 시작 방향, 직선·꺾은선·곡선 및 화살표 조합 9종, 시작·끝 대상 ID/인덱스와 제어점을 `ShapeLineMetadata`에 보존한다. HTML은 구조를 `data-*`로 출력하고, semantic exporter가 페이지 좌표계에서 연결 대상을 임의로 재배치하지 않도록 warning을 유지한다. 동결된 HWPX 폴백에는 신규 해석을 추가하지 않는다.
- HWP 도형 텍스트 상자 최대 폭 IR 확장: `IR_VERSION` 75 → 76. rHWP 공개 `TextBox.max_width`를 `Shape.text_box_max_width`에 보존하고 HTML `max-width`로 근사한다. 동결된 HWPX 폴백에는 신규 해석을 추가하지 않는다.
- HWP 문단 BorderFill IR 확장: `IR_VERSION` 76 → 77. rHWP 공개 `ParaShape.border_fill_id`가 참조하는 원본 ID, 단색·gradient·image 채우기와 대각선 정보를 문단 스타일에 보존한다. HTML 문단과 목록 항목은 기존 공용 fill 렌더링 경로와 `data-*` 메타데이터를 사용하며, 기존 단색 `background_color`는 호환성을 위해 유지한다. 동결된 HWPX 폴백에는 신규 해석을 추가하지 않는다.
- HWP 이미지 원본 변환 IR 확장: `IR_VERSION` 77 → 78. rHWP 공개 `Picture.shape_attr`의 원본·현재 크기, 그룹 내부 오프셋, 회전 중심과 합성 affine 행렬을 공용 `ShapeTransform`으로 보존한다. 기존 이미지 크기 필드는 호환성을 위해 유지하고, HTML은 전체 페이지 좌표 변환을 임의 적용하지 않은 채 `data-*` 메타데이터를 출력한다. 동결된 HWPX 폴백에는 신규 해석을 추가하지 않는다.
- HWP 이미지·도형 테두리 원본 메타데이터 IR 확장: `IR_VERSION` 78 → 79. rHWP 공개 `ShapeBorderLine.outline_style`의 normal·outer·inner·미지 값을 공용 `ObjectBorderMetadata`로 보존하고, 이미지의 `border_opacity` 원시값도 같은 구조에 남긴다. rHWP renderer가 투명도 의미를 적용하지 않으므로 HTML은 두 값을 `data-*`로만 출력하며 CSS 의미를 추정하지 않는다. 동결된 HWPX 폴백에는 신규 해석을 추가하지 않는다.
- HWP 호·곡선 도형 종류 IR 확장: `IR_VERSION` 79 → 80. rHWP 공개 `ShapeObject::Arc`와 `ShapeObject::Curve`를 각각 `ShapeKind::Arc`, `ShapeKind::Curve`로 보존해 타원·다각형과 구분한다. 기존 구조화 `ShapeGeometry::Arc/Curve`와 일치시키고 HTML이 호를 완전한 타원처럼 표시하지 않게 한다. 동결된 HWPX 폴백의 기존 분류는 변경하지 않는다.
- HWP 개체 원본 인스턴스 ID IR 확장: `IR_VERSION` 80 → 81. rHWP 공개 `CommonObjAttr.instance_id`, 도형 `DrawingObjAttr.inst_id`, 그림 `Picture.instance_id`를 도형·이미지에 보존한다. rHWP가 연결선 `SubjectID`를 도형 component ID와 공통 ID에 대조하므로, 기존 연결선 대상 참조를 실제 대상 개체와 다시 연결할 수 있게 한다. HTML은 이 값을 `data-*` 메타데이터로 남기며 동결된 HWPX 폴백에는 신규 해석을 추가하지 않는다.
- raw HWP record, unknown control bytes, HWPX XML을 직접 해석한 독자 기능 구현을 금지했다.
- `src/hwpx.rs`는 즉시 삭제하지 않되 legacy 호환성 안전망으로 동결하고, 회귀·보안·기존 silent-drop 수정만 허용하기로 결정했다.
- 신규 정확도 작업은 rHWP 공개 surface를 `mapped`, `normalized`, `warning/unknown`, `render-only`, `unmapped`, `upstream-needed`로 분류한 뒤 진행한다.
- 근거표와 fixture 없이 직관적인 완성도 퍼센트를 확정값처럼 제시하지 않기로 했다.

## 완료 선언 기준

다음 전에는 "rHWP 수준의 쓸만한 변환기"라고 말하지 않는다.

- 실제 HWP/HWPX fixture 30개 이상.
- P0/P1 fixture가 모두 HWP/HWPX 쌍으로 존재.
- JSON/HTML에서 주요 구조 보존.
- Markdown/TXT에서 일관된 읽을 수 있는 fallback.
- image asset이 안정적으로 출력.
- unsupported/missing 요소가 warning 또는 unknown으로 추적.
- rHWP revision update를 최소 1회 fixture 기반으로 성공.
- `cargo test` 전체 통과.
- `docs/STATUS.md`가 현재 코드와 일치.
