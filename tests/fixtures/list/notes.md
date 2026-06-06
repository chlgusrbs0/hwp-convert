# list fixture

이 fixture는 HWP/HWPX 입력에서 기본 list metadata와 읽기 순서가 `Document IR`로 보존되는지 검증한다.

입력 파일:

- `input.hwp`: rHWP model로 구성한 synthetic HWP fixture.
- `input.hwpx`: `input.hwp`를 rHWP로 parse한 뒤 rHWP HWPX serializer로 생성한 paired fixture.

문서 내용:

- bullet list paragraph: `bullet item`
- ordered list paragraph 1: `first item`
- ordered list paragraph 2: `second item`

현재 기대 동작:

- `bridge::rhwp::read_document`가 HWP와 HWPX 입력 모두에서 성공한다.
- 세 문단의 텍스트와 읽기 순서가 유지된다.
- 첫 문단은 unordered list로 매핑된다.
- 둘째/셋째 문단은 ordered list로 매핑된다.
- ordered list numbering은 같은 list item series 안에서 이어진다.
- `txt`, `json`, `markdown`, `html`, `svg` export가 모두 성공한다.

현재 한계:

- synthetic HWP reparse에서 bullet marker 문자가 안정적으로 복원되지 않아 marker glyph 자체는 fixture 기준으로 삼지 않는다.
- 이 fixture는 list metadata 보존을 확인하지만, exporter별 list rendering fidelity 전체를 golden 비교하지는 않는다.

Expected files:

- `expected/bridge-stats.hwp.json`
- `expected/bridge-stats.hwpx.json`
