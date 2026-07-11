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
- HWP/HWPX 편집기를 만들지 않는다.
- 모든 출력 형식에서 원본 HWP의 시각적 배치를 동일하게 재현한다고 주장하지 않는다.
- 기본 `--to svg`는 semantic/plain-text 기반 SVG exporter다. `src/render`의 RenderSnapshot visual path는 별도 진단 경로이며 기본 SVG exporter와 섞지 않는다.

## 핵심 아키텍처

레이어 경계의 정확한 정의는 `docs/ARCHITECTURE.md`에 있다. 요약하면:

기본 변환 경로:

```text
HWP/HWPX file -> rHWP parser -> src/bridge/rhwp.rs -> Document IR -> src/exporter.rs -> txt/json/markdown/html/svg
```

HWPX 폴백 경로(rHWP가 실패하거나 빈 결과일 때):

```text
HWPX file -> src/hwpx.rs (Contents/section*.xml 또는 Preview/PrvText.txt) -> Document IR -> exporters
```

실험적 visual 진단 경로:

```text
HWP/HWPX file -> rHWP DocumentCore / renderer query API -> src/render/mod.rs -> RenderSnapshot / 진단 SVG
```

경계 규칙: exporter는 rHWP 타입에 직접 의존하지 않는다. rHWP 타입은 `src/bridge`와 `src/render` 뒤에 둔다. semantic IR에 페이지 좌표를 섞지 않는다. bridge-only 개선은 보통 `IR_VERSION`을 올리지 않고, serialized JSON shape가 바뀌면 bump를 검토한다.

## 쓸만함의 정의

"rHWP 수준의 쓸만한 변환기"는 다음을 뜻한다.

1. rHWP가 읽어낸 주요 문서 요소를 버리지 않고 받는다.
2. 받은 요소를 `Document IR`에 명시적 구조로 보존한다.
3. 각 exporter가 자기 형식에서 표현 가능한 만큼 보존한다.
4. 표현할 수 없는 정보는 조용히 버리지 않고 fallback text, placeholder, `Unknown`, `ConversionWarning` 중 하나로 남긴다.
5. 실제 HWP/HWPX fixture로 parse, bridge, export 결과를 검증한다.
6. rHWP pin을 올릴 때 회귀를 감지할 수 있다.

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

## 전략적 결정 사항

### `src/hwpx.rs` 자작 HWPX 파서의 방향

현재 `src/hwpx.rs`는 rHWP의 HWPX 약점을 보완하는 폴백 파서지만 점점 커져 `src` 전체의 약 1/3에 이르고, 정규 XML 파서가 아닌 자작 문자열 스캐너다 (`docs/STATUS.md`의 "지속가능성 리스크" 참고). "rHWP를 다시 만들지 않는다"는 비목표와 긴장 관계다.

선택지:

- (a) 현행 유지하며 계속 확장한다. 단기적으로 HWPX 복구 능력은 늘지만 스캐너 엣지케이스 유지보수 부담이 누적된다.
- (b) `quick-xml` 같은 검증된 XML 크레이트 기반으로 리팩터링한다. 최근 `fix(hwpx):` 류 버그의 상당수가 사라질 종류다. IR/exporter 경계는 그대로 두고 파서 내부만 교체한다.
- (c) rHWP upstream의 HWPX 지원 개선을 기다리며 폴백을 동결하고, revision update 시 재평가한다.

권장: 폴백이 더 커지기 전에 (b)를 검토한다. 단, 실문서 fixture로 회귀를 먼저 고정한 뒤 리팩터링하는 것이 안전하다.

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

목표: P0 fixture에서 드러난 손실을 bridge에서 줄인다. IR이 부족하면 먼저 호환성 정책을 검토한 뒤 확장한다.

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

정책: rHWP는 git revision 고정. fixture coverage가 충분해지기 전엔 자주 올리지 않는다. 완성본에 가까워질수록 의도적으로 올린다. 절차는 `AGENTS.md`의 "rHWP dependency 정책"을 따른다. revision update 시 `src/hwpx.rs` 폴백의 필요성도 재평가한다(위 전략적 결정 참고).

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
| HWPX 폴백 | 전략 결정 | `src/hwpx.rs` 방향 결정 (위 "전략적 결정 사항") |
| visual layout | Future | RenderSnapshot 또는 rHWP native renderer 연동 |

## 회귀 지표

작업 중 추적할 숫자: official fixture count, fixture input pair count, parse success count, format별 export success count, code별 warning count, unknown block/inline count, silent drop 의심 count, image asset output count, golden comparison count, rHWP revision update regression count.

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

1. 나머지 P0 fixture(`table`, `merged_table`, `style`, `image`)의 실제 입력 파일을 확정한다.
2. `cargo test --test fixture_smoke`가 실제 파일에서 통과하는지 확인한다.
3. 실패하면 rHWP parse 문제인지 bridge mapping 문제인지 분리한다.
4. P0가 모두 통과하면 rHWP revision update rehearsal을 한 번 한다.
5. P1(`link_list`, `note_header_footer`)로 넘어간다.

각 단계의 commit은 작게 나눈다 (`AGENTS.md`의 Git 규칙 참고).

## 의사결정 기록

2026-06-06:

- 변환 정확도를 최우선으로 둔다.
- 많은 출력 형식 추가보다 실제 문서 fixture와 semantic 보존을 우선한다.
- rHWP는 upstream을 무작정 따라가지 않고 revision pin을 유지하며, 완성도에 따라 차근차근 올린다.
- 기본 SVG는 semantic exporter로 유지하고, visual output은 별도 renderer-assisted 단계로 다룬다.

2026-06-16:

- 문서를 통합 정리했다: `COMPATIBILITY.md` → `STATUS.md`, `RHWP_CONVERSION_ROADMAP.md` → `ROADMAP.md`, `HWPX_FIXTURE_FINDINGS.md`는 `STATUS.md`에 병합. addendum 패턴 제거, stale 사실(HEAD 해시, 테스트 수)은 `AGENTS.md` 단일 출처로 이동.
- `src/hwpx.rs` 자작 파서의 비대화를 명시적 전략 결정 사항으로 등록했다 (위 "전략적 결정 사항").
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
