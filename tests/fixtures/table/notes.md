# table fixture

이 fixture는 가장 단순한 표 구조가 실제 HWP 입력에서 `Document IR`의 `Block::Table`로 보존되는지 검증한다.

현재 `input.hwp`가 준비되어 있다. `input.hwpx`는 아직 추가하지 않았다.

문서 내용:

- 2행 2열 표 1개
- 각 셀에는 단일 문단 텍스트 1개
- 셀 텍스트는 row-major 순서로 `cell 1-1`, `cell 1-2`, `cell 2-1`, `cell 2-2`

현재 기대 동작:

- `bridge::rhwp::read_document`가 준비된 입력 파일에서 성공한다.
- 본문 block으로 `Block::Table` 1개가 남는다.
- 표는 row 2개와 cell 4개를 가진다.
- 각 cell의 nested paragraph text가 row-major 순서로 보존된다.
- `txt`, `json`, `markdown`, `html`, `svg` export가 모두 성공한다.

`expected/bridge-stats.hwp.json`은 현재 HWP 관찰값을 회귀 기준으로 고정한다.
