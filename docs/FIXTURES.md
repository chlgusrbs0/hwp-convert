# Fixture Test Plan

이 문서는 HWP/HWPX bridge coverage를 올리기 위한 실제 fixture 테스트 계획이다.
목표는 새 기능을 먼저 만드는 것이 아니라, 현재 동작과 현재 한계를 문서화하고 회귀 테스트 기반을 만드는 것이다.

## 원칙

1. 모든 fixture는 가능하면 `HWP`와 `HWPX`를 같은 의미 내용으로 한 쌍으로 둔다.
2. 기대값은 "이상적인 미래 상태"가 아니라 "현재 코드가 보장해야 하는 상태"를 먼저 고정한다.
3. 이미지 bytes 전체, HWP 내부 ID, warning 순서처럼 흔들릴 수 있는 값은 전체 비교보다 부분 비교를 우선한다.
4. 기본 SVG fixture는 현재 CLI의 `--to svg` 결과, 즉 semantic/plain-text 기반 SVG exporter 출력을 기준으로 다룬다.
5. RenderSnapshot 기반 visual SVG와 visual-check artifact는 기본 SVG golden과 섞지 않고 별도 fixture 또는 diagnostics smoke로 분리한다.
6. `equation_shape_chart`와 `kitchen_sink`는 누락 영역을 드러내는 smoke/regression fixture 역할도 해야 한다.

## 권장 fixture 구조

```text
tests/fixtures/<fixture_name>/
  input.hwp
  input.hwpx
  notes.md
  expected/
    bridge.json
    txt.txt
    markdown.md
    html.html
    svg.svg
  diagnostics/
    render-snapshot.svg
    render-snapshot-summary.json
```

`diagnostics/`는 RenderSnapshot 경로를 검증하는 fixture에만 둔다. 일반 exporter golden의 `svg.svg`는 계속 CLI `--to svg` 결과를 의미한다.

권장 비교 레이어:

1. `bridge smoke`
   - 두 입력 형식 모두 `bridge::rhwp::read_document`가 성공하는지 확인
2. `bridge assert`
   - 전체 JSON dump 비교보다 필요한 필드 subset 비교
3. `export smoke`
   - `txt/json/markdown/html/svg` export가 모두 성공하는지 확인
4. `export golden`
   - 결정적인 fixture만 golden file 비교

## Fixture별 계획

| Fixture | 우선순위 | 포함 요소 | 문서 내용 | bridge 핵심 assert | exporter 핵심 assert | 비고 |
| --- | --- | --- | --- | --- | --- | --- |
| `basic_text` | P0 | text, paragraph | 3~5개 문단, 빈 문단 1개, 문단 내부 줄바꿈, 탭, 한글/영문/숫자 혼합 | section 1개, 비어 있지 않은 문단만 block으로 남는 현재 동작, `Inline::Text/LineBreak/Tab`, warning 없음 | TXT/Markdown/HTML/SVG에 텍스트가 모두 보이는지 확인 | 가장 먼저 만들 fixture. 테스트 harness 기준점 역할 |
| `style` | P0 | style | bold, italic, underline, strike, font family, font size, text color, background color, 정렬, before/after spacing, indent | `TextStyle`, `ParagraphStyle`, `style_ref`, `StyleSheet.text_styles`, `StyleSheet.paragraph_styles` | JSON/HTML golden, Markdown에는 bold/italic만 유지되는 현재 동작 확인 | table/cell 배경색은 table fixture와 중복되지 않게 최소만 포함 |
| `table` | P0 | table | 2x2 또는 3x2 단순 표, 각 셀은 단일 문단 텍스트 | `Block::Table`, row/cell count, 각 cell의 nested paragraph | HTML `<table>` 구조, Markdown simple table 유지, TXT/SVG fallback 텍스트 확인 | Markdown 표 경로를 살리려면 병합/복합 block 없이 단순 셀 유지 |
| `merged_table` | P0 | merged table cell | row span 1개, col span 1개가 모두 있는 표 | 해당 cell의 `row_span`/`col_span` 값 고정 | HTML `rowspan`/`colspan` 확인, Markdown은 plain text fallback으로 내려가는 현재 동작 확인 | 표 병합 지원이 깨지면 바로 잡을 수 있는 fixture |
| `image` | P0 | image, resource | png 또는 jpg 1개, description 기반 alt, 가능하면 caption 1개 | `Block::Image`, `ImageResource`, resource id, extension/media type, bytes 비어 있지 않음, width/height 힌트 | JSON resource 보존, HTML/Markdown `images/<id>.<ext>` 참조 문자열, TXT/SVG fallback 문구 확인 | 현재 asset bundle writer가 없다는 한계를 드러내는 fixture |
| `link_list` | P1 | link, list | hyperlink field 1개, hyperlink control 1개, bullet list, ordered list, nested list, numbering restart | `Inline::Link`, URL/label, `ListInfo.kind/level/number`, bullet marker 비어 있지 않음 | HTML/Markdown URL 유지, TXT/SVG prefix/fallback 확인 | list와 link가 같은 문서에서 같이 깨지기 쉬워 묶는 편이 효율적 |
| `note_header_footer` | P1 | header/footer, footnote/endnote | header 1개, footer 1개, 본문 문단 안 footnote/endnote 각각 1개 | `Section.headers/footers`, `NoteStore`, note kind/id, trailing note ref append warning 발생 | HTML/Markdown note section, TXT/SVG 선형화 결과 확인 | note ref 위치가 정확하지 않다는 현재 제약을 fixture에 명시 |
| `equation_shape_chart` | P2 | equation, shape, chart | equation 1개, 대표 shape 2~3개, chart 1개 | equation은 `Block::Equation`과 script/fallback 확인, shape는 placeholder 수준 정보 확인, chart는 우선 smoke/current-behavior 기록 | HTML/Markdown/TXT/SVG placeholder 출력 또는 누락 여부를 현재 상태 그대로 기록 | chart는 bridge 미지원이라 초기에는 "지원 안 됨"을 드러내는 fixture로 사용 |
| `kitchen_sink` | P2 | unknown element 포함 전체 | text/style/table/image/link/list/header/footer/note/equation/shape/unknown을 한 문서에 혼합 | block 종류 존재 여부, note/resource/warning 존재 여부, silent drop 후보가 있는지 점검 | 모든 exporter smoke, 일부 핵심 문자열만 부분 assert | 세부 golden보다 통합 회귀 감시용 |

## Fixture별 세부 메모

### `basic_text`

- 반드시 `HWP`와 `HWPX`를 같은 내용으로 저장한다.
- 빈 문단은 "현재는 bridge에서 drop된다"는 사실을 테스트 이름과 기대값에 명시한다.
- preview fallback fixture와 목적이 다르므로 정상 parse 경로만 다룬다.

### `style`

- 글꼴명은 테스트 환경에서 저장 시 흔들리지 않는 기본 글꼴을 쓴다.
- 색상은 전경/배경이 분명히 다른 값으로 둔다.
- HTML golden은 inline style 문자열 전체 비교보다 핵심 declaration 포함 여부 비교가 더 안정적이다.

### `table`

- 첫 표는 Markdown path를 검증하려고 단순 구조로 유지한다.
- 셀 안 다중 문단이나 이미지 같은 복합 내용은 `kitchen_sink`로 미룬다.

### `merged_table`

- row span, col span을 각각 최소 1회씩 포함한다.
- Markdown은 "표 렌더링"이 아니라 "fallback 문자열"이 현재 기대값이다.

### `image`

- resource bytes 전체를 golden file로 박제하지 말고, `resource_id`, `extension`, `media_type`, `bytes.len() > 0` 정도만 확인한다.
- HTML/Markdown export는 현재 image asset 파일이 실제로 써지지 않으므로, 참조 문자열 생성까지만 현재 기대값으로 둔다.

### `link_list`

- hyperlink는 field-range 기반 1개와 trailing control 기반 1개를 둘 다 넣는다.
- ordered list는 restart가 보이는 최소 케이스 3개 정도로 만든다.
- bullet glyph는 저작 도구가 private-use 문자로 저장할 수 있으므로 exact char 전체 비교보다 non-empty marker 또는 normalized marker 비교가 안전하다.

### `note_header_footer`

- note ref는 현재 문단 끝 append 경고가 핵심이므로 warning assert를 꼭 넣는다.
- header/footer는 odd/even placement가 있으면 둘 다 포함한다.

### `equation_shape_chart`

- equation은 bridge가 `PlainText`로만 넣는 현재 상태를 그대로 고정한다.
- shape는 description 또는 fallback text가 있는 대표 예제를 사용한다.
- chart는 현재 bridge 미지원이므로, 처음부터 강한 구조 assert를 걸지 말고 "smoke + 현재 관찰 결과 기록"으로 시작한다.

### `kitchen_sink`

- 이 fixture는 한 요소씩 엄격히 비교하기보다 "전체 문서가 끝까지 parse/export 되는가"를 보는 통합 회귀 용도다.
- unknown/ignored control 후보가 포함되면 `notes.md`에 현재 관찰된 손실을 적어 둔다.

## 추천 구현 순서

1. `basic_text`
2. `table`
3. `merged_table`
4. `style`
5. `image`
6. `link_list`
7. `note_header_footer`
8. `equation_shape_chart`
9. `kitchen_sink`

## 완료 기준

- 최소 P0 fixture 다섯 개가 `HWP`/`HWPX` 쌍으로 준비된다.
- 각 fixture마다 bridge assert 1세트와 exporter smoke 1세트가 있다.
- `equation_shape_chart`와 `kitchen_sink`는 미지원 영역을 숨기지 않고 현재 동작을 기록한다.
